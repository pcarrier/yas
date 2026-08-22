//! `@muster` — one typed native YAS frame loop servicing CLI invocations,
//! terminal and filesystem state, panel Channels, probes, and deadlines.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use yas_ext_muster::config::{
    self, ConfigError, InstanceFile, ReadyWhen, StackFile, TopLevel, UnitFile, UnitType,
    WorktreeSourceFile,
};
use yas_ext_muster::envfile::{self, EnvFile, Origin};
use yas_ext_muster::journal::{Cause, Event, Journal, Record};
use yas_ext_muster::supervisor::{self, DependentAction, Phase, Run, Unit};
use yas_ext_muster::worktrees::{self, PortLedger};
use yas_guest::{
    Client, MonotonicInstant,
    channel::Listener,
    command::{CommandProvider, ProviderEvent},
    fs::{Root as FsRoot, Watch as FsWatch},
    kv::Namespace as KvNamespace,
    surface::{StateChange as SurfaceStateChange, Watch as SurfaceWatch},
    terminal::{
        PendingQuery, StateChange as TerminalStateChange, Watch as TerminalWatch, state_exit,
        state_resource_tag,
    },
};
use yas_wire::{
    Class, Extensions, Frame, family, fs as fs_wire, kv as kv_wire, state::Phase as StatePhase,
    surface as surface_wire, terminal as terminal_wire,
};

const DESCRIPTOR: &str = r#"{
  "protocol":"yas.cli.v1",
  "summary":"Supervise units that run in terminals",
  "commands":[
    {"path":["list"],"summary":"Every unit and instance, and what it is doing",
     "usage":"yas @muster list [--json]"},
    {"path":["status"],"summary":"One unit or instance, with its retained runs",
     "usage":"yas @muster status <name> [--json]"},
    {"path":["start"],"summary":"Start a unit or an instance now",
     "usage":"yas @muster start <name>"},
    {"path":["stop"],"summary":"Stop a unit or an instance and hold it",
     "usage":"yas @muster stop <name>"},
    {"path":["restart"],"summary":"Stop and start, in a new terminal",
     "usage":"yas @muster restart <name>"},
    {"path":["instantiate"],"summary":"Write an instance of a stack, and start it",
     "usage":"yas @muster instantiate <stack> <name> [VAR=VALUE ...] [--no-start] [--force] [--json]"},
    {"path":["reload"],"summary":"Ask a unit to re-read its own configuration",
     "usage":"yas @muster reload <name>"},
    {"path":["rewatch"],"summary":"Retry the directories whose watch the server refused",
     "usage":"yas @muster rewatch"},
    {"path":["ready"],"summary":"Declare a readyWhen:manual unit ready",
     "usage":"yas @muster ready <unit>"},
    {"path":["log"],"summary":"The supervision journal",
     "usage":"yas @muster log [-n N] [-u NAME] [--since SEQ] [--json]"},
    {"path":["cat"],"summary":"The file behind a unit or instance",
     "usage":"yas @muster cat <name>"},
    {"path":["env"],"summary":"The environment a start would resolve",
     "usage":"yas @muster env <unit> [--values] [--json]"},
    {"path":["stacks"],"summary":"Stacks and their parameters",
     "usage":"yas @muster stacks [--json]"},
    {"path":["doctor"],"summary":"Everything wrong with the directory",
     "usage":"yas @muster doctor [--json]"},
    {"path":["schema"],"summary":"The JSON Schema for a unit file",
     "usage":"yas @muster schema"}
  ]
}"#;

/// Terminals are created at a fixed size: nothing subscribes to them here, and
/// a client that attaches resizes to its own pane.
const ROWS: u16 = 40;
const COLS: u16 = 120;

/// How long the filesystem watch coalesces changes before reporting them.
/// Enough that saving a file is one event rather than one per write.
const SETTLE_MS: u16 = 200;

/// How soon a directory that could not be watched is tried again, and the
/// ceiling that backoff climbs to.
///
/// The common cause is a pointer written before its target exists — a stack in
/// a worktree that has not been created yet — and the person who is about to
/// create it is standing right there, so the first retry is quick. A pointer at
/// a directory that will never exist should not cost a sync every five seconds
/// forever, hence the climb.
const REWATCH_MS: u64 = 5_000;
const REWATCH_MAX_MS: u64 = 60_000;

/// How often a `path`/`tcp`/`http` probe is retried while activating.
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
/// What a `tcp`/`http` probe invites the peer to send.
///
/// A probe reads a status line and hangs up, but the window it opens with is
/// a standing invitation for the whole of it: at the SDK default a probe
/// against a dev server that answers with its bundle parks megabytes in the
/// client's pending queue, and a few of those in one activation storm are
/// enough that the next blocking request cannot read past them. One TCP
/// segment's worth is all a greeting needs.
const PROBE_WINDOW: u64 = 8 * 1024;
/// `log` polls faster: it is racing a ring buffer, not a listening socket.
const LOG_PROBE_INTERVAL: Duration = Duration::from_millis(100);
/// An idle tick, so a directory that changed without an event is still noticed.
const IDLE_TICK: Duration = Duration::from_secs(30);

/// One durable record is enough: muster is its only writer, and a single CAS
/// keeps allocations across every configured repository consistent.
/// Mirror only Git's linked-worktree pointers. Watching all of `.git` would
/// index object storage and every ref merely to learn when one tiny `gitdir`
/// file appears or disappears.
const GIT_WORKTREES_EXCLUDE: &str = "*\n!worktrees/\n!worktrees/*/\n!worktrees/*/gitdir\n";

fn main() {}

/// Publish one line as this attempt's stderr.
///
/// A wasmi guest is given five host functions — send, recv, wait, clock,
/// random — and none of them is a file descriptor. `eprintln!` compiles here
/// and writes nowhere, which is why muster has been exiting silently: the
/// reason was logged, into a void. The wire is the only way out.
pub(crate) fn ext_log(client: &mut Client, msg: &str) {
    let _ = client.attempt_stderr(msg.as_bytes());
}

yas_guest::entry!(run);

/// Everything the supervisor owns.
struct Muster {
    /// Absolute, `~` already expanded: the FS family does not expand it.
    dir: String,
    /// Derived from `dir` once, because `~` expansion happens per env file and
    /// per probe and the answer never changes.
    home: String,
    /// Resolver-derived template for each named automatic local server socket.
    /// This becomes `${YAS_SOCKET}` while expanding a stack instance.
    local_sockets: LocalSockets,
    /// Watched directories. Root 0 is `dir`; pointer files in it name every
    /// external stack/include and every filtered Git worktree root.
    roots: Vec<Root>,
    /// Roots the server refused, kept so `doctor` can say so on every run
    /// rather than only in the journal at the moment it happened.
    unwatchable: BTreeMap<String, u8>,
    /// When to try the directories in `unwatchable` again, and how long to wait
    /// after that. A watch that was refused is the one failure the watch cannot
    /// report its way out of — nothing is watching a directory that is not
    /// being watched — so it is the only thing here that polls.
    rewatch_at_ms: u64,
    rewatch_delay_ms: u64,
    units: BTreeMap<String, Unit>,
    stacks: BTreeMap<String, StackFile>,
    instances: BTreeMap<String, Instance>,
    port_ledger: PortLedger,
    port_ledger_hash: Option<[u8; 32]>,
    port_kv: Option<KvNamespace>,
    port_ledger_loaded: bool,
    journal: Journal,
    /// Everything `doctor` should say, rebuilt on every load.
    findings: Vec<ConfigError>,
    /// `log:` readiness cursors, keyed by unit.
    log_cursor: BTreeMap<String, (u64, u32)>,
    panel_listener: Option<Listener>,
    panel_conns: Vec<panel::Conn>,
    /// Units whose row a panel has not been told about yet.
    dirty: BTreeSet<String>,
    /// When the oldest unflushed change arrived.
    dirty_since: Option<u64>,
    pending_events: Vec<Record>,
    /// Surfaces this supervisor can account for, by surface id.
    surfaces: BTreeMap<u64, Surface>,
    /// Stamped `app_id` back to the unit that owns it, rebuilt on every load.
    /// A surface names its origin by `app_id`; this is the way back.
    surface_owners: BTreeMap<String, String>,
    /// Opaque native application endpoint handle to exact unit run.
    app_owners: BTreeMap<u64, (String, u64)>,
    terminal_apps: BTreeMap<u64, u64>,
    log_waits: BTreeMap<String, LogWait>,
    /// When each activating unit is next probed.
    next_probe_ms: BTreeMap<String, u64>,
    terminal_watch: TerminalWatch,
    terminal_generations: BTreeMap<u64, u32>,
    surface_watch: SurfaceWatch,
    adoptable: Vec<(u64, String)>,
    exited: BTreeMap<u64, i32>,
    command_provider: Option<CommandProvider>,
}

#[derive(Clone, Debug, PartialEq)]
struct Instance {
    stack: String,
    /// The port block this instance occupies, as `expand` resolved it.
    ports: Option<(i64, u32)>,
    members: Vec<String>,
}

/// The stable named local-socket candidate computed by the server's own
/// automatic resolver before the socket itself exists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LocalSockets {
    template: Option<String>,
}

impl LocalSockets {
    fn from_environment(get: &impl Fn(&str) -> Option<String>) -> Self {
        Self {
            template: get("YAS_SOCKET_TEMPLATE").filter(|template| valid_socket_template(template)),
        }
    }

    fn for_name(&self, name: &str) -> Option<String> {
        if !portable_server_name(name) {
            return None;
        }
        let template = self.template.as_deref()?;
        let socket = template.replace("{name}", name);
        if template.starts_with('/') && socket.len() > 103 {
            return None;
        }
        if template.starts_with(r"\\.\pipe\") && socket.encode_utf16().count() >= 256 {
            return None;
        }
        Some(socket)
    }
}

fn valid_socket_template(template: &str) -> bool {
    if template.chars().any(char::is_control)
        || template.match_indices("{name}").count() != 1
        || template.matches('{').count() != 1
        || template.matches('}').count() != 1
    {
        return false;
    }

    if let Some(parent) = template.strip_suffix("/yas-{name}.sock") {
        return parent.starts_with('/')
            && parent
                .split('/')
                .skip(1)
                .all(|component| !component.is_empty() && !matches!(component, "." | ".."));
    }

    let Some(stem) = template
        .strip_prefix(r"\\.\pipe\yas-")
        .and_then(|rest| rest.strip_suffix("{name}"))
    else {
        return false;
    };
    let user = stem.strip_suffix('-').unwrap_or(stem);
    (stem.is_empty() || stem.ends_with('-'))
        && user.len() <= 64
        && user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn portable_server_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 64
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return false;
    }
    let windows_stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    !matches!(windows_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !windows_stem
            .strip_prefix("COM")
            .or_else(|| windows_stem.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}

/// One watched directory and the mirror of its contents.
struct Root {
    path: String,
    kind: RootKind,
    root: FsRoot,
    watch: FsWatch,
    snapshot_done: bool,
    mirror: BTreeMap<String, MirrorNode>,
}

#[derive(Clone)]
struct MirrorNode {
    hash: [u8; 32],
    content: Option<Vec<u8>>,
}

struct LogWait {
    terminal_handle: u64,
    query: PendingQuery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootKind {
    /// Configuration, stack, or include directory: recursive JSON content.
    Files,
    /// A filtered `.git` tree containing only worktree `gitdir` pointers.
    GitWorktrees,
}

/// Report before leaving.
///
/// `serve` borrows the client rather than consuming it precisely so that this
/// can still speak after it returns an error — an extension that dies without
/// saying why costs more to diagnose than everything it does when it works.
/// Exit codes, because an error string cannot always get out.
///
/// A guest that hits a protocol violation *poisons its own client*, so the
/// obvious "log the error before returning" reports nothing precisely when
/// something went wrong. The code survives that: it is the return value of
/// `yas_main`, not a message that needs a working session.
const EXIT_FRAME_LOOP: i32 = 2;
const EXIT_STARTUP: i32 = 3;
const EXIT_GOAWAY: i32 = 4;
const EXIT_PROTOCOL: i32 = 5;
const EXIT_BUDGET: i32 = 6;
const EXIT_WIRE: i32 = 7;

fn run(mut client: Client) -> i32 {
    match serve(&mut client) {
        Ok(()) => 0,
        Err(error) => {
            // Best effort: this is silently dropped on a poisoned client,
            // which is exactly the case the exit code is here to cover.
            ext_log(&mut client, &format!("muster: {error}"));
            match () {
                _ if error.contains("[goaway]") => EXIT_GOAWAY,
                _ if error.contains("[protocol]") => EXIT_PROTOCOL,
                _ if error.contains("[budget]") => EXIT_BUDGET,
                _ if error.contains("[wire]") => EXIT_WIRE,
                _ if error.starts_with("native frame loop") => EXIT_FRAME_LOOP,
                _ => EXIT_STARTUP,
            }
        }
    }
}

fn serve(client: &mut Client) -> Result<(), String> {
    ext_log(client, "muster: entered serve");
    for (family_id, kind, name) in [
        (family::FS, fs_wire::request_kind::OPEN, "FS"),
        (family::ENV, yas_wire::env::request_kind::GET, "Env"),
        (
            family::TERMINAL,
            terminal_wire::request_kind::WATCH,
            "Terminal",
        ),
        (
            family::SURFACE,
            surface_wire::request_kind::WATCH,
            "Surface",
        ),
    ] {
        if !client.supports(family_id, Class::Request, kind) {
            return Err(format!("server does not support native {name}"));
        }
    }
    let environment = read_environment(client)?;
    let get = |key: &str| environment_string(&environment, key);
    let dir = resolve_dir_from(get)?;
    let local_sockets = LocalSockets::from_environment(&get);
    // The configuration directory is derived from HOME, so it carries it.
    let home = match dir.find("/.config/") {
        Some(at) => dir[..at].to_string(),
        None => String::from("/"),
    };
    let mut muster = Muster {
        dir,
        home,
        local_sockets,
        roots: Vec::new(),
        unwatchable: BTreeMap::new(),
        rewatch_at_ms: 0,
        rewatch_delay_ms: REWATCH_MS,
        units: BTreeMap::new(),
        stacks: BTreeMap::new(),
        instances: BTreeMap::new(),
        port_ledger: PortLedger::default(),
        port_ledger_hash: None,
        port_kv: None,
        port_ledger_loaded: false,
        journal: Journal::new(1),
        findings: Vec::new(),
        log_cursor: BTreeMap::new(),
        log_waits: BTreeMap::new(),
        panel_listener: None,
        panel_conns: Vec::new(),
        dirty: BTreeSet::new(),
        dirty_since: None,
        pending_events: Vec::new(),
        surfaces: BTreeMap::new(),
        surface_owners: BTreeMap::new(),
        app_owners: BTreeMap::new(),
        terminal_apps: BTreeMap::new(),
        next_probe_ms: BTreeMap::new(),
        terminal_watch: client
            .watch_terminals(None)
            .map_err(|error| format!("terminal watch: {error}"))?,
        terminal_generations: BTreeMap::new(),
        surface_watch: client
            .watch_surfaces(None)
            .map_err(|error| format!("surface watch: {error}"))?,
        adoptable: Vec::new(),
        exited: BTreeMap::new(),
        command_provider: None,
    };

    let dir = muster.dir.clone();
    muster.watch(client, &dir, RootKind::Files);
    muster.open_panel(client);

    let listener_name = format!(
        "yas.cli.{:016x}.{}",
        client.context().extension_handle,
        client.context().attempt
    );
    ext_log(
        client,
        &format!(
            "muster: starting dir={} listener={listener_name}",
            muster.dir
        ),
    );
    // Two steps rather than a chain: the listener has to stop borrowing the
    // client before the registration can borrow it again.
    let listener = client
        .listen_channel(&listener_name, b"")
        .map_err(|error| format!("cli listener: {error}"));
    muster.command_provider = match listener.and_then(|listener| {
        CommandProvider::register(client, listener, DESCRIPTOR)
            .map_err(|error| format!("command registration: {error}"))
    }) {
        Ok(provider) => Some(provider),
        Err(error) => {
            ext_log(client, &format!("muster: {error}"));
            None
        }
    };

    loop {
        let now = muster.now_ms(client);
        let deadline = muster.next_deadline(client, now);
        match client.next_frame_until(deadline) {
            Ok(Some(frame)) => muster.route(client, &frame),
            Ok(None) => {}
            Err(yas_guest::Error::EndpointClosed) => break,
            Err(error) => {
                let class = match &error {
                    yas_guest::Error::GoAway(_) => "goaway",
                    yas_guest::Error::Protocol(_) => "protocol",
                    yas_guest::Error::ReceiveBudgetExhausted { .. } => "budget",
                    yas_guest::Error::Wire(_) => "wire",
                    yas_guest::Error::Host(_) => "host",
                    _ => "other",
                };
                return Err(format!("native frame loop [{class}]: {error}"));
            }
        }
        muster.reconcile(client);
        let now = muster.now_ms(client);
        muster.flush_panel(client, now);
    }
    Ok(())
}

/// Read once: the extension has no ambient host environment, and both its
/// configuration directory and named local-socket paths derive from this.
fn read_environment(client: &mut Client) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, String> {
    client
        .get_environment()
        .map_err(|error| format!("environment: {error}"))
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.key, entry.value))
                .collect()
        })
}

fn environment_string(environment: &BTreeMap<Vec<u8>, Vec<u8>>, key: &str) -> Option<String> {
    environment
        .get(key.as_bytes())
        .and_then(|value| String::from_utf8(value.clone()).ok())
}

fn resolve_dir_from(get: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    if let Some(explicit) = get("YAS_MUSTER_DIR") {
        return Ok(explicit);
    }
    let name = get("YAS_SERVER_NAME")
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "default".to_owned());
    let valid = name.len() <= 64
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid {
        return Err("YAS_SERVER_NAME is not a portable server name".to_owned());
    }
    if let Some(xdg) = get("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Ok(format!("{xdg}/yas/instances/{name}/muster"));
    }
    let home = get("HOME")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/root".into());
    Ok(format!("{home}/.config/yas/instances/{name}/muster"))
}

impl Muster {
    fn now_ms(&self, client: &Client) -> u64 {
        (client.realtime_now().unix_timestamp_nanos() / 1_000_000) as u64
    }

    /// Start watching one directory. Recursive, because a stack is a
    /// subdirectory; the second level is dropped when the mirror is read.
    ///
    /// The configuration directory is root 0 and is never dropped. Every other
    /// root is named by a pointer file in it — an instance whose `stack` is a
    /// path, an include, or an explicit worktree source — so discovery always
    /// begins somewhere the user deliberately wrote, never in a checkout that
    /// merely happens to be there.
    fn watch(&mut self, client: &mut Client, path: &str, kind: RootKind) {
        if self.roots.iter().any(|root| root.path == path) {
            return;
        }
        let opened = client.open_fs(
            fs_wire::RootSource::PlatformPath(path.as_bytes().to_vec()),
            0,
        );
        let mut root = match opened {
            Ok(root) => root,
            Err(error) => {
                self.note_unwatchable(path, format!("open failed: {error}"));
                return;
            }
        };
        let flags = (yas_wire::schema::fs::WATCH_RECURSIVE
            | yas_wire::schema::fs::WATCH_CONTENT
            | yas_wire::schema::fs::WATCH_INCLUDE_HIDDEN) as u16;
        let ignore = match kind {
            RootKind::Files => String::new(),
            RootKind::GitWorktrees => GIT_WORKTREES_EXCLUDE.to_string(),
        };
        let watch = match root.watch(
            client,
            flags,
            SETTLE_MS,
            fs_wire::MAX_INLINE_BYTES as u32,
            ignore,
            None,
        ) {
            Ok(watch) => watch,
            Err(error) => {
                let _ = root.close(client);
                self.note_unwatchable(path, format!("watch failed: {error}"));
                return;
            }
        };
        self.roots.push(Root {
            path: path.to_string(),
            kind,
            root,
            watch,
            snapshot_done: false,
            mirror: BTreeMap::new(),
        });
    }

    fn note_unwatchable(&mut self, path: &str, detail: String) {
        self.unwatchable.insert(path.to_string(), 1);
        self.findings
            .push(ConfigError::new(path.to_string(), detail));
    }

    /// Ask again for the directories whose `FS_SYNC` was refused.
    ///
    /// `watch` returns early for a path already in `roots`, and a refused sync
    /// leaves the root there with no `sync_id` — so without dropping it first,
    /// a directory that did not exist when its pointer was written stays
    /// unwatched for the life of the supervisor. Nothing else can catch this:
    /// the watch is how muster hears about the world, and there is no watch on
    /// a directory that is not being watched.
    ///
    /// `now` forces the retry from `@muster reload`, which is the only thing
    /// bare `reload` is for.
    pub(crate) fn retry_unwatchable(&mut self, client: &mut Client, now: u64, immediate: bool) {
        let stuck: BTreeSet<String> = self.unwatchable.keys().cloned().collect();
        if stuck.is_empty() {
            return;
        }
        self.roots.retain(|root| !stuck.contains(&root.path));
        self.unwatchable.clear();
        self.rewatch_delay_ms = if immediate {
            REWATCH_MS
        } else {
            (self.rewatch_delay_ms * 2).min(REWATCH_MAX_MS)
        };
        self.rewatch_at_ms = now + self.rewatch_delay_ms;
        // `load` is what re-issues the syncs, because it is what decides which
        // directories are wanted in the first place.
        self.load(client);
    }

    /// Stop watching the roots nothing names any more.
    ///
    /// `wanted` includes the configuration directory, so there is no positional
    /// "root 0 is special" rule to remember. An earlier version exempted the
    /// first root by index, and the exemption then had to be re-remembered
    /// everywhere the same set was reused — `unwatchable` promptly forgot it and
    /// discarded the configuration directory's own status on every load.
    fn prune_roots(&mut self, client: &mut Client, wanted: &BTreeMap<String, RootKind>) {
        let mut kept = Vec::with_capacity(self.roots.len());
        for mut root in std::mem::take(&mut self.roots) {
            if wanted.get(&root.path) == Some(&root.kind) {
                kept.push(root);
                continue;
            }
            let _ = root.watch.close(client);
            let _ = root.root.close(client);
        }
        self.roots = kept;
    }

    /// A `stack`/`include`/`worktrees` value as an absolute directory.
    ///
    /// A bare word is a subdirectory of the configuration directory; anything
    /// else is a path, with `~` expanded here because the FS family does not
    /// expand it.
    pub(crate) fn resolve_path(&self, value: &str) -> String {
        if config::is_path(value) {
            let expanded = expand_tilde(value, &self.home);
            // Relative paths would resolve against the *server's* cwd, which is
            // never what a pointer file meant.
            if config::is_absolute_path(&expanded) {
                expanded
            } else {
                format!("{}/{expanded}", self.dir)
            }
        } else {
            format!("{}/{value}", self.dir)
        }
    }

    /// One file from one watched root, by its path relative to that root.
    pub(crate) fn file_at(&self, root: &str, relative: &str) -> Option<Vec<u8>> {
        self.roots
            .iter()
            .find(|r| r.path == root)?
            .mirror
            .get(relative)?
            .content
            .clone()
    }

    /// The configuration directory's own files, as `load` reads them.
    ///
    /// Nothing below the second level: a stack is a subdirectory, and anything
    /// deeper is yours.
    pub(crate) fn config_files(&self) -> BTreeMap<String, Vec<u8>> {
        self.files_in(&self.dir.clone())
            .into_iter()
            .filter(|(path, _)| path.matches('/').count() <= 1)
            .collect()
    }

    /// Write a file into the configuration directory.
    ///
    /// `exclusive` is a create-or-fail: a zero CAS base means "there must be
    /// nothing here", which is what keeps `instantiate` from silently
    /// replacing an instance somebody is running.
    ///
    /// Only the configuration directory is writable, and only at its top
    /// level. A stack directory outside it is a repository this supervisor was
    /// pointed at — cloning one must not let it be edited, and the same rule
    /// that keeps discovery inside the configuration directory keeps writes
    /// there too.
    pub(crate) fn write_config(
        &mut self,
        client: &mut Client,
        relative: &str,
        content: &[u8],
        exclusive: bool,
    ) -> Result<(), String> {
        if relative.contains('/') || relative.starts_with('.') {
            return Err(format!("{relative:?} is not a top-level file"));
        }
        let dir = self.dir.clone();
        let Some(index) = self.roots.iter().position(|root| root.path == dir) else {
            return Err(format!("{dir} is not being watched yet"));
        };
        // The CAS base is the hash of what is there now, so a replacement has
        // to name the thing it replaces.
        let base = if exclusive {
            fs_wire::Precondition::Absent
        } else {
            self.roots
                .iter()
                .find(|root| root.path == dir)
                .and_then(|root| root.mirror.get(relative))
                .map_or(fs_wire::Precondition::Any, |node| {
                    fs_wire::Precondition::Hash(node.hash)
                })
        };
        let path = fs_path(relative).ok_or_else(|| format!("invalid FS path {relative:?}"))?;
        let mut staged = self.roots[index]
            .root
            .stage_write(
                client,
                path,
                base,
                yas_wire::schema::fs::STAGE_CREATE_PARENTS as u16,
                0o600,
                content,
            )
            .map_err(|error| format!("stage {relative}: {error}"))?;
        let committed = staged
            .commit(client, 0)
            .map_err(|error| format!("commit {relative}: {error}"))?;

        // Put the bytes in the mirror ourselves.
        //
        // The write does come back as an `FS_UPDATE`, but a *metadata-only*
        // one: the server primes the echo by marking this client as already
        // holding the content, so its own upsert carries no copy. `files_in`
        // reads content, so without this the file muster just wrote is a node
        // it can see and not a file it can parse — and since a metadata-only
        // upsert preserves whatever content is already there, seeding here is
        // also what makes the echo harmless when it lands.
        if let Some(root) = self.roots.get_mut(index) {
            root.mirror.insert(
                relative.to_string(),
                MirrorNode {
                    hash: committed.content_hash,
                    content: Some(content.to_vec()),
                },
            );
        }
        Ok(())
    }

    fn files_in(&self, path: &str) -> BTreeMap<String, Vec<u8>> {
        let Some(root) = self.roots.iter().find(|root| root.path == path) else {
            return BTreeMap::new();
        };
        root.mirror
            .iter()
            .filter(|(path, _)| !path.starts_with('.') && !path.contains("/."))
            .filter(|(path, _)| path.ends_with(".json"))
            .filter_map(|(path, node)| node.content.as_ref().map(|c| (path.clone(), c.clone())))
            .collect()
    }

    fn content_in(&self, path: &str) -> BTreeMap<String, Vec<u8>> {
        let Some(root) = self.roots.iter().find(|root| root.path == path) else {
            return BTreeMap::new();
        };
        root.mirror
            .iter()
            .filter_map(|(path, node)| node.content.as_ref().map(|c| (path.clone(), c.clone())))
            .collect()
    }

    fn worktrees_for(&self, source: &WorktreeSourceFile) -> Vec<worktrees::Worktree> {
        let main = self.resolve_path(&source.worktrees);
        let git = worktrees::stack_path(&main, ".git");
        worktrees::discover(&main, &self.content_in(&git))
    }

    fn root_ready(&self, path: &str) -> bool {
        self.roots
            .iter()
            .find(|root| root.path == path)
            .is_some_and(|root| root.snapshot_done)
    }

    fn next_deadline(&self, client: &Client, now: u64) -> MonotonicInstant {
        let mut soonest: Option<u64> = None;
        for unit in self.units.values() {
            if let Some(at) = unit.next_deadline_ms() {
                soonest = Some(soonest.map_or(at, |s: u64| s.min(at)));
            }
        }
        for at in self.next_probe_ms.values() {
            soonest = Some(soonest.map_or(*at, |s: u64| s.min(*at)));
        }
        // A pending flush is a deadline like any other: without it the loop
        // would sleep through the coalescing window and a panel would see the
        // change only when something else happened to wake it.
        if let Some(at) = self.flush_due_ms(now) {
            soonest = Some(soonest.map_or(at, |s: u64| s.min(at)));
        }
        if !self.unwatchable.is_empty() {
            let at = self.rewatch_at_ms;
            soonest = Some(soonest.map_or(at, |s: u64| s.min(at)));
        }
        let idle = client.monotonic_now() + IDLE_TICK;
        match soonest {
            Some(at) => client.monotonic_now() + Duration::from_millis(at.saturating_sub(now)),
            None => idle,
        }
    }

    fn route(&mut self, client: &mut Client, frame: &Frame) {
        let now = self.now_ms(client);
        let provider_event = self
            .command_provider
            .as_mut()
            .and_then(|provider| provider.offer_frame(client, frame).ok().flatten());
        if let Some(event) = provider_event {
            match event {
                ProviderEvent::Invocation(mut invocation) => {
                    if let Err(error) = self.serve(client, &mut invocation) {
                        ext_log(client, &format!("muster: command failed: {error}"));
                        let _ = invocation.cancel(client);
                    }
                }
                ProviderEvent::Closed(_) => self.command_provider = None,
            }
            return;
        }
        if self.route_panel(client, frame, now) {
            return;
        }

        match self.terminal_watch.offer_frame(client, frame) {
            Ok(Some(update)) => {
                self.apply_terminal_update(client, update.phase, update.changes);
                return;
            }
            Err(error) => ext_log(client, &format!("muster: terminal state: {error}")),
            Ok(None) => {}
        }
        match self.surface_watch.offer_frame(client, frame) {
            Ok(Some(update)) => {
                self.apply_surface_update(update.changes, now);
                return;
            }
            Err(error) => ext_log(client, &format!("muster: surface state: {error}")),
            Ok(None) => {}
        }

        for index in 0..self.roots.len() {
            let update = {
                let root = &mut self.roots[index];
                root.watch.offer_frame(client, frame)
            };
            match update {
                Ok(Some(update)) => {
                    if let Err(error) = self.apply_fs_update(client, index, update) {
                        ext_log(client, &format!("muster: FS state: {error}"));
                    } else {
                        self.load(client);
                    }
                    return;
                }
                Err(error) => {
                    ext_log(client, &format!("muster: FS watch: {error}"));
                    return;
                }
                Ok(None) => {}
            }
        }

        let _ = self.note_log_wait(client, frame);
    }

    fn apply_terminal_update(
        &mut self,
        client: &mut Client,
        phase: StatePhase,
        changes: Vec<TerminalStateChange>,
    ) {
        if matches!(phase, StatePhase::SnapshotBegin | StatePhase::Reset) {
            self.adoptable.clear();
            self.exited.clear();
            self.terminal_generations.clear();
        }
        let snapshot = matches!(phase, StatePhase::SnapshotRecords | StatePhase::SnapshotEnd);
        for change in changes {
            match change {
                TerminalStateChange::Upsert(record) => {
                    let handle = record.terminal_handle;
                    self.terminal_generations.insert(handle, record.generation);
                    if snapshot
                        && let Ok(Some(tag)) = state_resource_tag(&record)
                        && tag.starts_with(supervisor::TAG_PREFIX)
                        && !self
                            .adoptable
                            .iter()
                            .any(|(existing, _)| *existing == handle)
                    {
                        self.adoptable.push((handle, tag.to_string()));
                    }
                    if let Ok(Some(exit)) = state_exit(&record) {
                        let status = terminal_exit_status(&exit);
                        let first = self.exited.insert(handle, status).is_none();
                        if first && self.units.values().any(|unit| unit.pty == Some(handle)) {
                            self.note_exit(client, handle, status);
                        }
                    } else {
                        self.exited.remove(&handle);
                    }
                }
                TerminalStateChange::Patch(_) => {}
                TerminalStateChange::Remove(handle) => {
                    self.terminal_generations.remove(&handle);
                    self.exited.remove(&handle);
                    self.adoptable.retain(|(pty, _)| *pty != handle);
                    let changed: Vec<String> = self
                        .units
                        .iter_mut()
                        .filter_map(|(name, unit)| unit.forget_run(handle).then(|| name.clone()))
                        .collect();
                    for name in changed {
                        self.touch(&name, self.now_ms(client));
                    }
                }
            }
        }
        if phase == StatePhase::SnapshotEnd && !self.units.is_empty() && !self.adoptable.is_empty()
        {
            self.adopt(client);
        }
    }

    fn apply_fs_update(
        &mut self,
        client: &mut Client,
        index: usize,
        update: yas_guest::fs::StateUpdate,
    ) -> Result<(), String> {
        if matches!(update.phase, StatePhase::SnapshotBegin | StatePhase::Reset) {
            self.roots[index].mirror.clear();
            self.roots[index].snapshot_done = false;
        }
        for mutation in update.changes {
            match mutation {
                fs_wire::StateMutation::Complete(record) => {
                    self.upsert_fs_record(client, index, record)?;
                }
                fs_wire::StateMutation::Patch(patch) => {
                    self.upsert_fs_record(client, index, patch.replacement)?;
                }
                fs_wire::StateMutation::Remove(removed) => {
                    if let Some(path) = fs_path_string(&removed.path) {
                        self.roots[index].mirror.remove(&path);
                    }
                }
                fs_wire::StateMutation::Move(moved) => {
                    if let (Some(from), Some(to)) =
                        (fs_path_string(&moved.from), fs_path_string(&moved.to))
                        && let Some(node) = self.roots[index].mirror.remove(&from)
                    {
                        self.roots[index].mirror.insert(to, node);
                    }
                }
            }
        }
        if update.phase == StatePhase::SnapshotEnd {
            self.roots[index].snapshot_done = true;
            self.rewatch_delay_ms = REWATCH_MS;
        }
        Ok(())
    }

    fn upsert_fs_record(
        &mut self,
        client: &mut Client,
        index: usize,
        record: fs_wire::EntryRecord,
    ) -> Result<(), String> {
        let path = fs_path_string(&record.path).ok_or("FS state path is not UTF-8")?;
        let node = match record.body {
            fs_wire::EntryBody::File {
                content_hash,
                inline_content,
                ..
            } => {
                let content = match inline_content {
                    Some(content) => Some(content),
                    None => Some(
                        self.roots[index]
                            .root
                            .fetch(client, record.path, Some(content_hash), 16 * 1024 * 1024)
                            .map_err(|error| format!("fetch {path}: {error}"))?
                            .bytes,
                    ),
                };
                MirrorNode {
                    hash: content_hash,
                    content,
                }
            }
            fs_wire::EntryBody::Symlink {
                content_hash,
                target,
            } => MirrorNode {
                hash: content_hash,
                content: Some(target),
            },
            fs_wire::EntryBody::Directory => MirrorNode {
                hash: [0; 32],
                content: None,
            },
        };
        self.roots[index].mirror.insert(path, node);
        Ok(())
    }

    // ---------------------------------------------------------------- loading

    /// Rebuild the unit table from the mirror.
    ///
    /// A file that does not parse never displaces the one that did: the running
    /// unit keeps running, the failure is journaled, and `doctor` lists it.
    fn load(&mut self, client: &mut Client) {
        let now = self.now_ms(client);
        self.findings.clear();
        for (path, status) in &self.unwatchable {
            self.findings.push(ConfigError::new(
                path.clone(),
                format!("cannot watch this directory (status {status})"),
            ));
        }

        let dir = self.dir.clone();
        let files = self.files_in(&dir);
        // Nothing below the second level of the configuration directory is
        // read: a stack is a subdirectory, and anything deeper is yours.
        let files: BTreeMap<String, Vec<u8>> = files
            .into_iter()
            .filter(|(path, _)| path.matches('/').count() <= 1)
            .collect();

        // Pass one: sort the top level into units, instances, includes, and
        // worktree sources, and learn which directories are named. The
        // configuration directory is in the set from the start — it is a
        // watched root like any other, and making it one removes every "except
        // root 0" caveat downstream.
        let mut pointers: Vec<(String, TopLevel)> = Vec::new();
        let mut wanted_roots: BTreeMap<String, RootKind> =
            BTreeMap::from([(dir.clone(), RootKind::Files)]);
        for (path, bytes) in &files {
            if path.contains('/') {
                continue;
            }
            let name = path.trim_end_matches(".json").to_string();
            match config::parse_top_level(path, bytes) {
                Ok(top) => {
                    match &top {
                        TopLevel::Instance(instance) if config::is_path(&instance.stack) => {
                            wanted_roots
                                .insert(self.resolve_path(&instance.stack), RootKind::Files);
                        }
                        TopLevel::WorktreeSource(source) => {
                            let main = self.resolve_path(&source.worktrees);
                            wanted_roots.insert(
                                worktrees::stack_path(&main, ".git"),
                                RootKind::GitWorktrees,
                            );
                            for worktree in self.worktrees_for(source) {
                                wanted_roots.insert(
                                    worktrees::stack_path(&worktree.path, &source.stack),
                                    RootKind::Files,
                                );
                            }
                        }
                        TopLevel::Include(include) => {
                            wanted_roots
                                .insert(self.resolve_path(&include.include), RootKind::Files);
                        }
                        _ => {}
                    }
                    pointers.push((name, top));
                }
                Err(err) => {
                    // A file that does not parse never displaces the one that
                    // did, but the failure is a decision worth recording next
                    // to the ones it prevented.
                    self.record(
                        Record::new(name, Event::Invalid, "stopped")
                            .cause(Cause::File)
                            .detail(err.detail.clone()),
                        now,
                    );
                    self.findings.push(err);
                }
            }
        }

        // Adjust the watch set before reading anything from it. A root added
        // here is empty until its own updates arrive, which triggers another
        // load — so a new pointer costs one extra pass, not a missing stack.
        self.prune_roots(client, &wanted_roots);
        self.unwatchable
            .retain(|path, _| wanted_roots.contains_key(path));
        for (path, kind) in &wanted_roots {
            self.watch(client, path, *kind);
        }

        // Stacks declared inside the configuration directory. External ones are
        // resolved per instance, since their declarations live beside them.
        self.stacks.clear();
        for (path, bytes) in &files {
            let Some((sub, base)) = path.rsplit_once('/') else {
                continue;
            };
            if base == "stack.json" {
                match config::parse_json(path, bytes).and_then(|v| {
                    serde_json::from_value(v).map_err(|e| ConfigError::new(path, e.to_string()))
                }) {
                    Ok(stack) => {
                        self.stacks.insert(sub.to_string(), stack);
                    }
                    Err(err) => self.findings.push(err),
                }
            } else {
                // A stack directory with templates but no stack.json still
                // works: it simply declares no parameters.
                self.stacks.entry(sub.to_string()).or_default();
            }
        }

        let mut wanted: BTreeMap<String, Unit> = BTreeMap::new();
        let mut instances: BTreeMap<String, Instance> = BTreeMap::new();
        // Which pointer contributed each name, so a collision can name both.
        let mut provenance: BTreeMap<String, String> = BTreeMap::new();
        let mut worktree_sources: Vec<(String, String, WorktreeSourceFile)> = Vec::new();

        for (name, top) in pointers {
            let file = format!("{name}.json");
            match top {
                TopLevel::Unit(unit) => {
                    provenance.insert(name.clone(), file);
                    wanted.insert(name.clone(), Unit::new(name, None, *unit));
                }
                TopLevel::Instance(instance) => match self.expand(&name, &instance, &files) {
                    Ok(expansion) => {
                        for unit in expansion.units {
                            provenance.insert(unit.name.clone(), file.clone());
                            wanted.insert(unit.name.clone(), unit);
                        }
                        instances.insert(
                            name.clone(),
                            Instance {
                                stack: instance.stack.clone(),
                                ports: expansion.ports,
                                members: expansion.members,
                            },
                        );
                    }
                    Err(err) => self.findings.push(ConfigError::new(file, err)),
                },
                TopLevel::WorktreeSource(source) => {
                    worktree_sources.push((name, file, *source));
                }
                TopLevel::Include(include) => {
                    let root = self.resolve_path(&include.include);
                    for (template, bytes) in self.files_in(&root) {
                        // An include contributes ordinary units only. Its
                        // subdirectories are not stacks — an instance names a
                        // stack by path, which is a different pointer.
                        if template.contains('/') {
                            continue;
                        }
                        let unit_name = template.trim_end_matches(".json").to_string();
                        if include.omit.contains(&unit_name) {
                            continue;
                        }
                        let where_ = format!("{root}/{template}");
                        match config::parse_top_level(&where_, &bytes) {
                            Ok(TopLevel::Unit(mut unit)) => {
                                // An include adds no suffix, so two of them
                                // offering one name is ambiguous rather than
                                // mergeable. First writer wins, and both are
                                // named, so the fix is obvious.
                                if let Some(first) = provenance.get(&unit_name) {
                                    self.findings.push(ConfigError::new(
                                        file.clone(),
                                        format!(
                                            "{unit_name:?} is already provided by {first}; \
                                             omit it in one of them"
                                        ),
                                    ));
                                    continue;
                                }
                                if !include.autostart {
                                    unit.autostart = false;
                                }
                                // A relative path in an included unit means the
                                // directory it came from, exactly as it does in
                                // a stack template. Without this an included
                                // `"envFile": ".env"` silently resolves against
                                // the unit's cwd instead.
                                rebase_unit_paths(&mut unit, &root);
                                provenance.insert(unit_name.clone(), file.clone());
                                wanted.insert(unit_name.clone(), Unit::new(unit_name, None, *unit));
                            }
                            Ok(_) => self.findings.push(ConfigError::new(
                                where_,
                                "an included directory holds units, not stacks, instances, or worktree sources",
                            )),
                            Err(err) => self.findings.push(err),
                        }
                    }
                }
            }
        }

        for (name, file, source) in worktree_sources {
            self.expand_worktree_source(
                client,
                &name,
                &file,
                &source,
                &files,
                &mut wanted,
                &mut instances,
                &mut provenance,
            );
        }

        // Rebuilt every load so an adopted unit re-claims the surfaces its
        // previous run stamped: the id is derived from the name, and the
        // initial burst replays every live surface's origin.
        self.surface_owners = wanted
            .keys()
            .map(|name| (supervisor::app_id_for(name), name.clone()))
            .collect();
        for surface in self.surfaces.values_mut() {
            let owner = self
                .app_owners
                .get(&surface.app_handle)
                .cloned()
                .or_else(|| {
                    self.surface_owners
                        .get(&surface.application_id)
                        .cloned()
                        .map(|unit| (unit, 0))
                });
            surface.unit = owner.as_ref().map(|(unit, _)| unit.clone());
            surface.seq = owner.and_then(|(_, seq)| (seq != 0).then_some(seq));
        }

        self.check_ports(&instances);
        self.reconcile_table(client, wanted, instances, now);

        // Adoption has to wait for the first load: matching a tag needs the
        // unit it names. It cannot hang off the initial Terminal State fence
        // either: bootstrap consumes that before this loop begins.
        if !self.adoptable.is_empty() {
            self.adopt(client);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_worktree_source(
        &mut self,
        client: &mut Client,
        source_name: &str,
        source_file: &str,
        source: &WorktreeSourceFile,
        files: &BTreeMap<String, Vec<u8>>,
        wanted: &mut BTreeMap<String, Unit>,
        instances: &mut BTreeMap<String, Instance>,
        provenance: &mut BTreeMap<String, String>,
    ) {
        let worktree_set: Vec<_> = self
            .worktrees_for(source)
            .into_iter()
            .filter(|worktree| {
                self.root_ready(&worktrees::stack_path(&worktree.path, &source.stack))
            })
            .collect();
        let Some(main) = worktree_set.iter().find(|worktree| worktree.is_main) else {
            // The main stack watch has been requested but its initial snapshot
            // has not landed yet. That update calls `load` again; reporting a
            // transient empty mirror as a broken source would be noise.
            return;
        };
        let main_stack = worktrees::stack_path(&main.path, &source.stack);
        let declarations = match self.declarations_of(&main_stack) {
            Ok(declarations) => declarations,
            Err(err) => {
                self.findings.push(ConfigError::new(source_file, err));
                return;
            }
        };

        let mut port_assignment: Option<(String, BTreeMap<String, i64>)> = None;
        if let Some((port_name, declaration)) = declarations
            .vars
            .iter()
            .find(|(_, declaration)| declaration.is_ports())
        {
            if source
                .vars
                .get(port_name)
                .and_then(serde_json::Value::as_str)
                != Some("auto")
            {
                self.findings.push(ConfigError::new(
                    source_file,
                    format!("worktree source must bind port parameter {port_name:?} to \"auto\""),
                ));
                return;
            }
            let Some(start) = declaration.start else {
                self.findings.push(ConfigError::new(
                    source_file,
                    format!("port parameter {port_name:?} needs start for a worktree source"),
                ));
                return;
            };
            if !client.supports(family::KV, Class::Request, kv_wire::request_kind::OPEN) {
                self.findings.push(ConfigError::new(
                    source_file,
                    "worktree port allocation needs server KV support",
                ));
                return;
            }
            let explicit: Vec<(i64, u32)> = instances
                .values()
                .filter_map(|instance| instance.ports)
                .collect();
            let assigned = match self.assign_worktree_ports(
                client,
                source_name,
                &worktree_set,
                start,
                declaration.span,
                &explicit,
            ) {
                Ok(assigned) => assigned,
                Err(err) => {
                    self.findings.push(ConfigError::new(source_file, err));
                    return;
                }
            };
            port_assignment = Some((port_name.clone(), assigned));
        }

        for worktree in &worktree_set {
            let instance_name = worktrees::instance_name(source_name, worktree);
            if let Some(first) = provenance.get(&instance_name) {
                self.findings.push(ConfigError::new(
                    source_file,
                    format!("instance {instance_name:?} collides with a unit from {first}"),
                ));
                continue;
            }
            if let Some(first) = instances.get(&instance_name) {
                self.findings.push(ConfigError::new(
                    source_file,
                    format!(
                        "instance {instance_name:?} is already provided by stack {:?}",
                        first.stack
                    ),
                ));
                continue;
            }
            let mut vars = source.vars.clone();
            if let Some((port_name, assigned)) = &port_assignment
                && let Some(base) = assigned.get(&worktree.id)
            {
                vars.insert(port_name.clone(), serde_json::json!(*base));
            }
            let instance = InstanceFile {
                stack: worktrees::stack_path(&worktree.path, &source.stack),
                vars,
                omit: source.omit.clone(),
                autostart: source.autostart,
            };
            let expansion = match self.expand(&instance_name, &instance, files) {
                Ok(expansion) => expansion,
                Err(err) => {
                    self.findings.push(ConfigError::new(source_file, err));
                    continue;
                }
            };
            if let Some((unit, first)) = expansion
                .units
                .iter()
                .find_map(|unit| provenance.get(&unit.name).map(|first| (&unit.name, first)))
            {
                self.findings.push(ConfigError::new(
                    source_file,
                    format!("{unit:?} is already provided by {first}"),
                ));
                continue;
            }
            for unit in expansion.units {
                provenance.insert(unit.name.clone(), source_file.to_string());
                wanted.insert(unit.name.clone(), unit);
            }
            instances.insert(
                instance_name,
                Instance {
                    stack: instance.stack,
                    ports: expansion.ports,
                    members: expansion.members,
                },
            );
        }
    }

    fn assign_worktree_ports(
        &mut self,
        client: &mut Client,
        source_name: &str,
        worktrees: &[worktrees::Worktree],
        start: i64,
        span: u32,
        explicit: &[(i64, u32)],
    ) -> Result<BTreeMap<String, i64>, String> {
        // An extension update briefly overlaps the retiring and replacement
        // attempts. Both can read the same ledger revision, so the loser must
        // merge from the winner instead of leaving this source empty until an
        // unrelated filesystem event happens to reload it.
        const CAS_ATTEMPTS: usize = 3;
        for attempt in 0..CAS_ATTEMPTS {
            if !self.port_ledger_loaded {
                self.load_port_ledger(client)?;
            }
            let before = self.port_ledger.clone();
            let assigned = worktrees::assign_ports(
                source_name,
                worktrees,
                start,
                span,
                &mut self.port_ledger,
                explicit,
            )?;
            if self.port_ledger == before {
                return Ok(assigned);
            }
            match self.persist_port_ledger(client) {
                Ok(()) => return Ok(assigned),
                Err(_) if !self.port_ledger_loaded && attempt + 1 < CAS_ATTEMPTS => {
                    self.port_ledger = before;
                }
                Err(err) => {
                    self.port_ledger = before;
                    return Err(err);
                }
            }
        }
        unreachable!("the bounded port-ledger retry always returns")
    }

    fn load_port_ledger(&mut self, client: &mut Client) -> Result<(), String> {
        if self.port_kv.is_none() {
            self.port_kv = Some(
                client
                    .open_kv(b"ext/muster/")
                    .map_err(|error| format!("open worktree port ledger: {error}"))?,
            );
        }
        let value = self
            .port_kv
            .as_mut()
            .expect("opened above")
            .get(client, b"worktree-ports/v1")
            .map_err(|error| format!("read worktree port ledger: {error}"))?;
        match value {
            None => {
                self.port_ledger = PortLedger::default();
                self.port_ledger_hash = None;
            }
            Some(value) => {
                self.port_ledger = serde_json::from_slice(&value.bytes)
                    .map_err(|err| format!("invalid worktree port ledger: {err}"))?;
                self.port_ledger_hash = Some(value.content_hash);
            }
        }
        self.port_ledger_loaded = true;
        Ok(())
    }

    fn persist_port_ledger(&mut self, client: &mut Client) -> Result<(), String> {
        let value = serde_json::to_vec(&self.port_ledger)
            .map_err(|err| format!("serialize worktree port ledger: {err}"))?;
        let precondition = self
            .port_ledger_hash
            .map_or(kv_wire::Precondition::Absent, kv_wire::Precondition::Hash);
        let result = self
            .port_kv
            .as_mut()
            .ok_or("worktree port ledger namespace is not open")?
            .put(client, b"worktree-ports/v1", &value, precondition, true)
            .map_err(|error| format!("write worktree port ledger: {error}"))?;
        if !result.status.is_ok() {
            if result.status == yas_wire::core::Status::Conflict {
                self.port_ledger_loaded = false;
            }
            return Err(format!(
                "cannot write worktree port ledger: {:?}",
                result.status
            ));
        }
        self.port_ledger_hash = Some(result.content_hash);
        Ok(())
    }

    /// What a stack declares, without expanding anything.
    ///
    /// Split out of [`Self::expand`] because `instantiate` has to know which
    /// parameter is the port block before it has an instance to expand.
    pub(crate) fn declarations_of(&self, stack: &str) -> Result<StackFile, String> {
        if !config::is_path(stack) {
            return self
                .stacks
                .get(stack)
                .cloned()
                .ok_or_else(|| format!("no stack named {stack:?}"));
        }
        let dir = self.resolve_path(stack);
        if !self.roots.iter().any(|root| root.path == dir) {
            return Err(format!("{dir} is not being watched yet"));
        }
        match self.files_in(&dir).get("stack.json") {
            // A stack directory with templates but no `stack.json` works: it
            // simply declares no parameters.
            None => Ok(StackFile::default()),
            Some(bytes) => config::parse_json("stack.json", bytes)
                .and_then(|value| {
                    serde_json::from_value(value)
                        .map_err(|e| ConfigError::new("stack.json", e.to_string()))
                })
                .map_err(|e| e.detail),
        }
    }

    /// Turn one instance into its units.
    fn expand(
        &self,
        instance_name: &str,
        instance: &InstanceFile,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Expansion, String> {
        let stack_dir = self.resolve_path(&instance.stack);
        // A stack in the configuration directory declares itself in a
        // subdirectory; one outside declares itself beside its templates. Both
        // reduce to a directory and a `stack.json` inside it.
        let (declarations, templates) = if config::is_path(&instance.stack) {
            let outside = self.files_in(&stack_dir);
            if outside.is_empty() && !self.roots.iter().any(|r| r.path == stack_dir) {
                return Err(format!("{stack_dir} is not being watched yet"));
            }
            let declared = outside
                .get("stack.json")
                .map(|bytes| {
                    config::parse_json("stack.json", bytes).and_then(|v| {
                        serde_json::from_value(v)
                            .map_err(|e| ConfigError::new("stack.json", e.to_string()))
                    })
                })
                .transpose()
                .map_err(|e| e.detail)?
                .unwrap_or_default();
            let templates: BTreeMap<String, Vec<u8>> = outside
                .into_iter()
                .filter(|(name, _)| !name.contains('/') && name != "stack.json")
                .collect();
            (declared, templates)
        } else {
            let declared = self
                .stacks
                .get(&instance.stack)
                .ok_or_else(|| format!("no stack named {:?}", instance.stack))?
                .clone();
            let prefix = format!("{}/", instance.stack);
            let templates: BTreeMap<String, Vec<u8>> = files
                .iter()
                .filter_map(|(path, bytes)| {
                    path.strip_prefix(&prefix)
                        .filter(|base| *base != "stack.json" && !base.contains('/'))
                        .map(|base| (base.to_string(), bytes.clone()))
                })
                .collect();
            (declared, templates)
        };

        let mut vars = config::bind_vars(
            instance_name,
            &instance.stack,
            &stack_dir,
            &declarations,
            &instance.vars,
        )?;
        if let Some(socket) = self.local_sockets.for_name(instance_name) {
            vars.insert("YAS_SOCKET".into(), serde_json::Value::String(socket));
        }

        let mut members = Vec::new();
        let mut units = Vec::new();
        for (base, bytes) in &templates {
            let template = base.trim_end_matches(".json");
            if instance.omit.iter().any(|o| o == template) {
                continue;
            }
            let path = format!("{stack_dir}/{base}");
            let mut value = config::parse_json(&path, bytes).map_err(|e| e.detail)?;
            config::substitute(&mut value, &vars)?;
            let mut file: UnitFile =
                serde_json::from_value(value).map_err(|e| format!("{path}: {e}"))?;
            rebase_unit_paths(&mut file, &stack_dir);
            file.validate(&path).map_err(|e| e.detail)?;
            let name = supervisor::qualified(instance_name, template);
            members.push(name.clone());

            // Inside a stack, dependencies name templates and always resolve
            // within the same instance.
            let qualify = |names: &mut Vec<String>| {
                for n in names.iter_mut() {
                    *n = supervisor::qualified(instance_name, n);
                }
            };
            qualify(&mut file.requires);
            qualify(&mut file.wants);
            qualify(&mut file.after);

            let mut unit = Unit::new(name, Some(instance_name.to_string()), file);
            if !instance.autostart {
                unit.file.autostart = false;
            }
            units.push(unit);
        }
        // A dependency on an omitted template is a mistake worth naming.
        for unit in &units {
            for dep in unit.file.requires.iter() {
                if !members.contains(dep) {
                    return Err(format!(
                        "{} requires {dep}, which this instance omits or does not have",
                        unit.name
                    ));
                }
            }
        }
        Ok(Expansion {
            members,
            units,
            ports: config::port_span(&declarations, &vars),
        })
    }

    /// Two instances whose port blocks overlap is the failure mode of running
    /// several dev stacks, and it presents as EADDRINUSE in whichever lost.
    ///
    /// Takes the freshly parsed map: reading `self.instances` here would
    /// inspect the *previous* generation, which is empty on the first load and
    /// stale on every one after it.
    fn check_ports(&mut self, instances: &BTreeMap<String, Instance>) {
        // The span is whatever `expand` resolved. Re-deriving it here from
        // `self.stacks` looked equivalent and was not: that map is keyed by
        // subdirectory name, so an instance naming a stack by path never
        // matched, and overlap detection was blind to exactly the case port
        // blocks exist for — one stack running once per worktree.
        let blocks: Vec<(String, i64, u32)> = instances
            .iter()
            .filter_map(|(name, instance)| {
                instance
                    .ports
                    .map(|(base, span)| (name.clone(), base, span))
            })
            .collect();
        for (i, (a, a_base, a_span)) in blocks.iter().enumerate() {
            for (b, b_base, b_span) in blocks.iter().skip(i + 1) {
                let overlap =
                    *a_base < b_base + i64::from(*b_span) && *b_base < a_base + i64::from(*a_span);
                if overlap {
                    self.findings.push(ConfigError::new(
                        format!("{a}.json"),
                        format!("port block {a_base}+{a_span} overlaps {b}'s {b_base}+{b_span}"),
                    ));
                }
            }
        }
    }

    /// Fold a freshly parsed table into the live one, keeping running units.
    fn reconcile_table(
        &mut self,
        client: &mut Client,
        wanted: BTreeMap<String, Unit>,
        instances: BTreeMap<String, Instance>,
        now: u64,
    ) {
        let mut restart: Vec<String> = Vec::new();
        let mut reaped: Vec<(String, &'static str, Option<String>, Run)> = Vec::new();
        for (name, fresh) in &wanted {
            match self.units.get_mut(name) {
                None => {
                    let unit = fresh.clone();
                    let instance = unit.instance.clone();
                    self.units.insert(name.clone(), unit);
                    self.record(
                        Record::new(name.clone(), Event::Loaded, "stopped").instance(instance),
                        now,
                    );
                }
                Some(existing) => {
                    let changed = !same_spec(&existing.file, &fresh.file);
                    existing.file = fresh.file.clone();
                    if changed {
                        existing.stale = true;
                    }
                    let instance = existing.instance.clone();
                    let phase = existing.phase;
                    let restart_after_change = changed && existing.restarts_after_change();
                    reaped.extend(
                        existing
                            .reap()
                            .into_iter()
                            .map(|run| (name.clone(), phase.as_str(), instance.clone(), run)),
                    );
                    if changed {
                        self.record(
                            Record::new(name.clone(), Event::Changed, phase.as_str())
                                .instance(instance.clone()),
                            now,
                        );
                    }
                    if restart_after_change {
                        restart.push(name.clone());
                    }
                }
            }
        }

        // `keep` is live policy. In particular, lowering it must close the
        // excess terminals now rather than waiting for another run to exit.
        for (name, phase, instance, run) in reaped {
            let _ = client.close_terminal(run.pty);
            self.record(
                Record::new(name, Event::Reaped, phase)
                    .pty(run.pty)
                    .exit_code(run.exit_code)
                    .instance(instance),
                now,
            );
        }

        let gone: Vec<String> = self
            .units
            .keys()
            .filter(|name| !wanted.contains_key(*name))
            .cloned()
            .collect();
        for name in gone {
            self.close_all(client, &name);
            if let Some(unit) = self.units.remove(&name) {
                self.record(
                    Record::new(name, Event::Unloaded, "stopped").instance(unit.instance),
                    now,
                );
            }
        }

        // A partial frame carries units, never the tree they hang under, so an
        // instance appearing or losing a member has to be a whole frame.
        if self.instances != instances {
            self.touch_all(now);
        }
        self.instances = instances;
        for name in restart {
            self.restart(client, &name, Cause::File);
        }
    }

    // -------------------------------------------------------------- lifecycle

    /// Whether every directory a pointer named has answered, one way or the
    /// other.
    ///
    /// Until then the unit table is incomplete by construction: a root added
    /// during a load is empty until its own updates arrive.
    fn roots_settled(&self) -> bool {
        self.roots
            .iter()
            .all(|root| root.snapshot_done || self.unwatchable.contains_key(&root.path))
    }

    /// Adopt the terminals a previous supervisor left running.
    ///
    /// Runs on every load, not just the first, because a unit whose definition
    /// lives outside the configuration directory does not exist yet on the load
    /// that discovers its pointer. A tag naming a unit that is not in the table
    /// *yet* stays pending; it is only closed once every root has reported, at
    /// which point "not in the table" really does mean gone. Closing eagerly
    /// killed and respawned exactly the units an external stack or an include
    /// contributed — the restart storm adoption exists to prevent, arriving by
    /// a different door.
    fn adopt(&mut self, client: &mut Client) {
        let now = self.now_ms(client);
        let tags = std::mem::take(&mut self.adoptable);
        if tags.is_empty() {
            return;
        }
        let settled = self.roots_settled();
        // Per unit, sort by sequence: the highest is the live run.
        let mut by_unit: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
        for (pty, tag) in tags {
            let Some((name, seq)) = supervisor::parse_tag(&tag) else {
                continue;
            };
            by_unit
                .entry(name.to_string())
                .or_default()
                .push((seq, pty));
        }
        for (name, mut runs) in by_unit {
            runs.sort_unstable();
            if !self.units.contains_key(&name) {
                if !settled {
                    // Its definition may still be arriving from a root that has
                    // not reported. Keep the tags and try again next load.
                    self.adoptable.extend(
                        runs.into_iter()
                            .map(|(seq, pty)| (pty, supervisor::tag_for(&name, seq))),
                    );
                    continue;
                }
                // Every root has reported, so this really is gone. It takes its
                // history with it.
                for (_, pty) in runs {
                    let _ = client.close_terminal(pty);
                }
                continue;
            }
            let present: BTreeSet<u64> = runs.iter().map(|(_, pty)| *pty).collect();
            let (policy, retained, history_changed) = {
                let unit = self.units.get_mut(&name).expect("checked");
                let changed = unit.retain_present_runs(&present);
                let retained: BTreeSet<u64> = unit.runs.iter().map(|run| run.pty).collect();
                (adoption_policy(unit.phase, unit.pty), retained, changed)
            };
            if history_changed {
                self.touch(&name, now);
            }
            match policy {
                AdoptionPolicy::Observe(owned) => {
                    // A state snapshot is allowed to describe the run we
                    // already own and its retained history. Any other live run
                    // with this unit's tag is an orphan from an older race.
                    for (_, pty) in runs {
                        if pty == owned
                            || (retained.contains(&pty) && self.exited.contains_key(&pty))
                        {
                            continue;
                        }
                        if self.exited.contains_key(&pty) {
                            let _ = client.close_terminal(pty);
                        } else {
                            let _ = client.signal_terminal(pty, terminal_wire::SignalKind::Kill);
                        }
                    }
                    continue;
                }
                AdoptionPolicy::Reject => {
                    // `Held` is durable user intent. In particular, once its
                    // tracked PTY exits, a duplicate terminal from a racing
                    // snapshot must not resurrect the unit. Its already-known
                    // exited history remains addressable.
                    for (_, pty) in runs {
                        if retained.contains(&pty) && self.exited.contains_key(&pty) {
                            continue;
                        }
                        if self.exited.contains_key(&pty) {
                            let _ = client.close_terminal(pty);
                        } else {
                            let _ = client.signal_terminal(pty, terminal_wire::SignalKind::Kill);
                        }
                    }
                    continue;
                }
                AdoptionPolicy::Adopt => {}
            }

            let unit = self.units.get_mut(&name).expect("checked");
            let highest = runs.last().expect("non-empty").0;
            unit.seq = highest + 1;

            // The highest sequence that has *not* exited is the live run.
            // Everything else is history, newest first.
            let live = runs
                .iter()
                .rev()
                .find(|(_, pty)| !self.exited.contains_key(pty))
                .copied();

            for (seq, pty) in runs.iter().rev() {
                if live.is_some_and(|(_, live_pty)| live_pty == *pty) {
                    continue;
                }
                if unit.runs.iter().any(|run| run.pty == *pty) {
                    continue;
                }
                unit.runs.push(Run {
                    pty: *pty,
                    seq: *seq,
                    exit_code: self.exited.get(pty).copied().unwrap_or(0),
                    started_ms: 0,
                    ended_ms: now,
                });
            }
            unit.runs.sort_by_key(|run| std::cmp::Reverse(run.seq));

            let phase = match live {
                Some((_, pty)) => {
                    unit.pty = Some(pty);
                    unit.started_ms = now;
                    unit.failures = 0;
                    // Only a probe that describes *current* state can be
                    // re-run against an adopted terminal. `path`, `tcp` and
                    // `http` ask the world a question and get today's answer.
                    // `log`, `delay` and `spawn` describe something that
                    // already happened, and the evidence may have scrolled out
                    // of the ring — re-running one stalls a healthy unit until
                    // `timeoutStart` and then replaces it, which is the restart
                    // storm adoption exists to prevent. A live terminal is the
                    // evidence for those.
                    if unit.file.ready_when.is_stateless() {
                        unit.deadline_ms = now + unit.file.timeout_start.ms();
                        Phase::Activating
                    } else {
                        unit.deadline_ms = 0;
                        Phase::Running
                    }
                }
                None => {
                    // Every terminal this unit left behind is dead. A oneshot
                    // that succeeded is still ready; anything else is stopped,
                    // and the next start takes a fresh sequence.
                    let succeeded = unit.runs.first().is_some_and(|r| r.exit_code == 0);
                    unit.last_exit = unit.runs.first().map(|r| r.exit_code);
                    if unit.file.unit_type == UnitType::Oneshot && succeeded {
                        Phase::Exited
                    } else {
                        Phase::Stopped
                    }
                }
            };
            unit.phase = phase;

            let stale = unit.reap();
            let instance = unit.instance.clone();
            for run in stale {
                let _ = client.close_terminal(run.pty);
            }
            let mut record = Record::new(name, Event::Adopted, phase.as_str())
                .cause(Cause::Adopt)
                .instance(instance);
            if let Some((_, pty)) = live {
                record = record.pty(pty);
            }
            self.record(record, now);
        }
    }

    /// Start whatever is due, probe whatever is activating, kill whatever
    /// outstayed its stop grace.
    fn reconcile(&mut self, client: &mut Client) {
        let now = self.now_ms(client);

        if !self.unwatchable.is_empty() && now >= self.rewatch_at_ms {
            self.retry_unwatchable(client, now, false);
        }

        // Autostart: a unit that has never run and says so.
        let names: Vec<String> = self.units.keys().cloned().collect();
        // Resolve every readiness transition before trying waiting units.
        // Unit names are lexical, not topological: a dependent such as
        // `edge` can sort before `server`. If edge is checked first and
        // server becomes ready later in this same pass, edge otherwise has
        // no deadline to wake the loop and may sit idle until unrelated I/O.
        for name in &names {
            let Some(unit) = self.units.get(name) else {
                continue;
            };
            if unit.phase == Phase::Stopped && unit.file.autostart && unit.pty.is_none() {
                self.want(client, name, Cause::Autostart);
            }
        }

        for name in &names {
            let Some(unit) = self.units.get(name) else {
                continue;
            };
            // A stop that did not take.
            if unit.kill_at_ms > 0 && now >= unit.kill_at_ms {
                if let Some(pty) = unit.pty {
                    let _ = client.signal_terminal(pty, terminal_wire::SignalKind::Kill);
                }
                if let Some(unit) = self.units.get_mut(name) {
                    unit.kill_at_ms = 0;
                }
                continue;
            }
            if unit.phase == Phase::Activating {
                if now >= unit.deadline_ms && unit.deadline_ms > 0 {
                    self.fail_start(client, name, "timeout");
                } else if self.next_probe_ms.get(name).is_none_or(|at| now >= *at) {
                    self.probe(client, name, now);
                }
                continue;
            }
        }

        // Readiness is now settled for the whole table, so start everything
        // whose dependencies became ready in the pass above.
        for name in &names {
            let Some(unit) = self.units.get(name) else {
                continue;
            };
            if unit.pty.is_none() && unit.attempt_due(now) && self.deps_ready(name) {
                let cause = if unit.phase == Phase::Backoff {
                    Cause::Crash
                } else {
                    Cause::Policy
                };
                self.spawn(client, name, cause);
            }
        }
    }

    fn deps_ready(&self, name: &str) -> bool {
        let Some(unit) = self.units.get(name) else {
            return false;
        };
        unit.file.requires.iter().all(|dep| {
            self.units
                .get(dep)
                .is_some_and(Unit::is_ready_for_dependents)
        })
    }

    /// Record the intent to run something, pulling in what it needs.
    fn want(&mut self, client: &mut Client, name: &str, cause: Cause) {
        let reset_root = cause == Cause::Command;
        let closure = supervisor::start_closure(&self.units, name);
        let order = match supervisor::start_order(&self.units, &closure) {
            Ok(order) => order,
            Err(supervisor::Cycle(ring)) => {
                let now = self.now_ms(client);
                for member in &ring {
                    if let Some(unit) = self.units.get_mut(member) {
                        unit.phase = Phase::Failed;
                    }
                }
                self.record(
                    Record::new(name.to_string(), Event::Cycle, "failed").detail(ring.join(" -> ")),
                    now,
                );
                return;
            }
        };
        let now = self.now_ms(client);
        for member in order {
            // `start_order` walks `after` too, because ordering has to see it.
            // Only the closure says what to *start*: an `after` dependency
            // orders a unit that is already coming up and must not be brought
            // up by it.
            if !closure.contains(&member) {
                continue;
            }
            let is_root = member == name;
            let Some(unit) = self.units.get_mut(&member) else {
                continue;
            };
            if unit.phase.is_live() || unit.phase == Phase::Exited {
                continue;
            }
            if unit.phase == Phase::Held && !is_root {
                continue;
            }
            // A dependency that is already backing off from a previous failure
            // must keep its retry timer. Resetting it to Waiting would let an
            // autostarted dependent respawn the dependency immediately and
            // create a terminal storm.
            if unit.phase == Phase::Backoff && !is_root {
                continue;
            }
            // A dependency that has given up is also not to be pulled back by a
            // dependent. Leave it Failed until someone explicitly starts it.
            if unit.phase == Phase::Failed && !is_root {
                continue;
            }
            if is_root && reset_root {
                unit.reset_failure_budget();
            }
            unit.phase = Phase::Waiting;
            unit.next_attempt_ms = 0;
            let instance = unit.instance.clone();
            let cause = if is_root {
                cause.clone()
            } else {
                Cause::Dependency(name.to_string())
            };
            self.record(
                Record::new(member.clone(), Event::Start, "waiting")
                    .cause(cause)
                    .instance(instance),
                now,
            );
        }
    }

    /// Build the environment, then create the terminal.
    fn spawn(&mut self, client: &mut Client, name: &str, cause: Cause) {
        let now = self.now_ms(client);
        let Some(unit) = self.units.get(name) else {
            return;
        };
        let file = unit.file.clone();
        let instance = unit.instance.clone();
        let seq = unit.seq;

        let cwd = expand_tilde(file.cwd.as_deref().unwrap_or("~"), &self.home);
        let resolved = self.resolve_env(client, name, &cwd);
        let ResolvedEnv { vars: env, sources } = match resolved {
            Ok(resolved) => resolved,
            Err(failure) => {
                let phase = self.note_failed_start(name, now);
                self.record(
                    Record::new(name.to_string(), Event::Failed, phase.as_str())
                        .detail(failure)
                        .instance(instance.clone()),
                    now,
                );
                return;
            }
        };

        let argv: Vec<String> = file.command.clone().unwrap_or_default();
        let app_handle = self.app_endpoint(client, name, seq);
        let shell = file.shell.clone().unwrap_or_default();
        let tag = supervisor::tag_for(name, seq);
        let launch = terminal_launch(file.command.as_ref(), &shell, &cwd, &env, app_handle);
        let detail = if shell.is_empty() {
            argv.join(" ")
        } else {
            shell.clone()
        };
        let extensions = match yas_guest::terminal::resource_tag_extension(&tag) {
            Ok(extension) => Extensions(vec![extension]),
            Err(error) => {
                ext_log(client, &format!("muster: invalid resource tag: {error}"));
                return;
            }
        };
        match client.create_terminal_with_extensions(ROWS, COLS, launch, extensions) {
            Ok(created) => {
                let pty = created.terminal_handle;
                self.terminal_generations.insert(pty, created.generation);
                if let Some(app_handle) = app_handle {
                    self.terminal_apps.insert(pty, app_handle);
                }
                if let Some(unit) = self.units.get_mut(name) {
                    unit.pty = Some(pty);
                    unit.seq += 1;
                    unit.started_ms = now;
                    unit.stale = false;
                    unit.phase = Phase::Activating;
                    unit.deadline_ms = now + unit.file.timeout_start.ms();
                }
                self.log_cursor.remove(name);
                self.next_probe_ms.insert(name.to_string(), now);
                let event = if matches!(cause, Cause::Crash) {
                    Event::Restart
                } else {
                    Event::Spawn
                };
                self.record(
                    Record::new(name.to_string(), event, "activating")
                        .pty(pty)
                        .cause(cause)
                        .detail(detail)
                        .instance(instance)
                        .env(sources, env.len()),
                    now,
                );
            }
            Err(err) => {
                if let Some(app_handle) = app_handle {
                    self.app_owners.remove(&app_handle);
                    let _ = client.release_app_endpoint(app_handle);
                }
                // A refused create is a failed start, never a running unit:
                // the server resolves the program before forking, so this is
                // where "no such binary" surfaces.
                let phase = self.note_failed_start(name, now);
                self.record(
                    Record::new(name.to_string(), Event::Exit, phase.as_str())
                        .detail(format!("create refused: {err:?}"))
                        .instance(instance),
                    now,
                );
            }
        }
    }

    fn note_failed_start(&mut self, name: &str, now: u64) -> Phase {
        let random = random();
        if let Some(unit) = self.units.get_mut(name) {
            unit.note_failed_start(now, random);
            unit.phase
        } else {
            Phase::Failed
        }
    }

    fn fail_start(&mut self, client: &mut Client, name: &str, why: &str) {
        let now = self.now_ms(client);
        let random = random();
        let (pty, instance, phase) = match self.units.get_mut(name) {
            Some(unit) => {
                unit.note_failed_activation(now, random);
                let pty = unit.pty;
                if pty.is_some() {
                    unit.kill_at_ms = now + unit.file.timeout_stop.ms();
                }
                (pty, unit.instance.clone(), unit.phase)
            }
            None => return,
        };
        self.record(
            Record::new(name.to_string(), Event::Failed, phase.as_str())
                .detail(why.to_string())
                .instance(instance),
            now,
        );
        if let Some(pty) = pty {
            let _ = client.signal_terminal(pty, terminal_wire::SignalKind::Terminate);
        }
        self.next_probe_ms.remove(name);
    }

    fn note_exit(&mut self, client: &mut Client, pty: u64, exit_status: i32) {
        let now = self.now_ms(client);
        let random = random();
        if let Some(app_handle) = self.terminal_apps.remove(&pty) {
            self.app_owners.remove(&app_handle);
            let _ = client.release_app_endpoint(app_handle);
        }
        let Some(name) = self
            .units
            .iter()
            .find(|(_, u)| u.pty == Some(pty))
            .map(|(n, _)| n.clone())
        else {
            return;
        };
        let (stale, phase, instance, dependent_action) = {
            let unit = self.units.get_mut(&name).expect("just found");
            let completed_attempt = unit.phase.is_live();
            let stale = unit.note_exit(exit_status, now, random);
            let dependent_action = unit.dependent_action_after_exit(exit_status, completed_attempt);
            (stale, unit.phase, unit.instance.clone(), dependent_action)
        };
        for run in &stale {
            let _ = client.close_terminal(run.pty);
            self.record(
                Record::new(name.clone(), Event::Reaped, phase.as_str())
                    .pty(run.pty)
                    .exit_code(run.exit_code)
                    .instance(instance.clone()),
                now,
            );
        }
        self.next_probe_ms.remove(&name);
        self.log_cursor.remove(&name);
        self.record(
            Record::new(name.clone(), Event::Exit, phase.as_str())
                .pty(pty)
                .exit_code(exit_status)
                .instance(instance),
            now,
        );

        // A normal dependency exit stops its dependents. Re-running a
        // successful oneshot is a staged replacement instead: a failure keeps
        // the old result in service, and success restarts dependents so they
        // consume the new one.
        match dependent_action {
            DependentAction::None => {}
            DependentAction::Stop | DependentAction::Restart => {
                self.stop_dependents(client, &name);
            }
        }

        // Dependency recovery asked for a new terminal once this one died.
        if let Some(unit) = self.units.get_mut(&name)
            && std::mem::take(&mut unit.restart_pending)
        {
            unit.phase = Phase::Waiting;
            unit.next_attempt_ms = 0;
        }
    }

    fn ready(&mut self, client: &mut Client, name: &str, how: &str) {
        let now = self.now_ms(client);
        let instance = self.units.get(name).and_then(|u| u.instance.clone());
        let pty = self.units.get(name).and_then(|u| u.pty);
        if let Some(unit) = self.units.get_mut(name) {
            unit.phase = Phase::Running;
            unit.deadline_ms = 0;
        }
        self.next_probe_ms.remove(name);
        let mut record = Record::new(name.to_string(), Event::Ready, "running")
            .detail(how.to_string())
            .instance(instance);
        if let Some(pty) = pty {
            record = record.pty(pty);
        }
        self.record(record, now);
    }

    /// Stop a unit and everything that requires it.
    ///
    /// `dependents` is already the transitive set, so this sweeps it flat.
    /// Recursing into it re-derives the same closure per member and stops a
    /// chain of depth *k* 2^(k-1) times, with a duplicate kill and a duplicate
    /// journal record each time.
    fn stop(&mut self, client: &mut Client, name: &str, cause: Cause, hold: bool) {
        self.stop_dependents(client, name);
        if let Some(unit) = self.units.get_mut(name) {
            unit.cancel_refresh();
        }
        self.stop_unit(client, name, cause, hold);
    }

    /// Stop dependents that are currently wanted, and leave them waiting for
    /// this dependency to become ready again. Held and idle dependents retain
    /// their intent rather than being accidentally autostarted.
    fn stop_dependents(&mut self, client: &mut Client, name: &str) {
        let dependents: Vec<String> = supervisor::dependents(&self.units, name)
            .into_iter()
            .filter(|dependent| {
                self.units
                    .get(dependent)
                    .is_some_and(Unit::wants_dependency_recovery)
            })
            .collect();
        for dependent in dependents {
            if let Some(unit) = self.units.get_mut(&dependent) {
                unit.cancel_refresh();
            }
            self.stop_unit(
                client,
                &dependent,
                Cause::Dependency(name.to_string()),
                false,
            );
            if let Some(unit) = self.units.get_mut(&dependent) {
                unit.resume_after_stop();
            }
        }
    }

    /// Stop exactly one unit. Cascading is [`Muster::stop`]'s job.
    fn stop_unit(&mut self, client: &mut Client, name: &str, cause: Cause, hold: bool) {
        let now = self.now_ms(client);
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        let instance = unit.instance.clone();
        let pty = unit.pty;
        let stop_command = unit.file.stop_command.clone();
        unit.phase = if hold { Phase::Held } else { Phase::Stopped };
        unit.next_attempt_ms = 0;
        unit.deadline_ms = 0;
        if pty.is_some() {
            unit.kill_at_ms = now + unit.file.timeout_stop.ms();
        }
        let signal = signal_number(&unit.file.stop_signal);
        match (&stop_command, pty) {
            // A `stopCommand` replaces the signal, not the deadline: the
            // SIGKILL at `timeoutStop` still comes, because a stop command that
            // does not stop the unit is the case it exists to survive.
            (Some(argv), Some(_)) => self.run_side_command(client, name, "stop", argv.clone()),
            (_, Some(pty)) => {
                let _ = client.signal_terminal(pty, terminal_signal(signal));
            }
            (_, None) => {}
        }
        self.next_probe_ms.remove(name);
        self.record(
            Record::new(
                name.to_string(),
                Event::Stop,
                if hold { "held" } else { "stopped" },
            )
            .cause(cause)
            .instance(instance),
            now,
        );
    }

    /// Every restart is a new terminal: Terminal RESTART replays the launch
    /// spec the terminal was created with, so it cannot apply an edited spec.
    fn restart(&mut self, client: &mut Client, name: &str, cause: Cause) {
        let reset_failure_budget = matches!(cause, Cause::Command | Cause::File);
        let staged_refresh = self.units.get(name).is_some_and(Unit::can_stage_refresh);
        if staged_refresh {
            // A completed oneshot is still a usable dependency result. Keep
            // its consumers alive while producing a replacement; note_exit
            // commits the replacement only if it succeeds.
            if let Some(unit) = self.units.get_mut(name) {
                unit.begin_refresh();
            }
            self.stop_unit(client, name, cause.clone(), false);
        } else {
            self.stop(client, name, cause.clone(), false);
        }
        if let Some(unit) = self.units.get_mut(name)
            && reset_failure_budget
        {
            unit.reset_failure_budget();
        }
        // Starting through the ordinary path pulls in newly added dependencies
        // and checks the updated graph for cycles. The PTY guard in reconcile
        // keeps the replacement from spawning before the old terminal exits.
        self.want(client, name, cause);
    }

    /// Run a unit's `stopCommand` or `reloadCommand` in a terminal of its own.
    ///
    /// Not a run of the unit: it gets no sequence number, is never adopted, and
    /// is not retained. It is tagged all the same, so a supervisor that is
    /// replaced mid-stop can see what is still executing on its behalf — and so
    /// the terminal is identifiable rather than anonymous in `yas client
    /// list`.
    ///
    /// It inherits the unit's `cwd` and resolved environment, because a stop
    /// command that cannot see `DOCKER_HOST` or `.env` is a stop command that
    /// talks to a different machine than the one it is stopping.
    pub(crate) fn run_side_command(
        &mut self,
        client: &mut Client,
        name: &str,
        kind: &str,
        argv: Vec<String>,
    ) {
        let now = self.now_ms(client);
        let Some(unit) = self.units.get(name) else {
            return;
        };
        let instance = unit.instance.clone();
        let cwd = expand_tilde(unit.file.cwd.as_deref().unwrap_or("~"), &self.home);
        let env = match self.resolve_env(client, name, &cwd) {
            Ok(resolved) => resolved.vars,
            // The unit is being stopped, not started: a `.env` that has since
            // gone missing must not keep the stop from happening, so this falls
            // back to the bare environment and says so.
            Err(failure) => {
                self.record(
                    Record::new(name.to_string(), Event::Ran, "stopped")
                        .detail(format!("{kind}Command without its environment: {failure}"))
                        .instance(instance.clone()),
                    now,
                );
                Vec::new()
            }
        };
        let tag = format!("{}{name}/{kind}", supervisor::TAG_PREFIX);
        let launch = terminal_launch(Some(&argv), "", &cwd, &env, None);
        let phase = self
            .units
            .get(name)
            .map_or("stopped", |unit| unit.phase.as_str());
        let detail = match yas_guest::terminal::resource_tag_extension(&tag)
            .map(|extension| Extensions(vec![extension]))
            .and_then(|extensions| {
                client.create_terminal_with_extensions(ROWS, COLS, launch, extensions)
            }) {
            Ok(_) => format!("{kind}Command: {}", argv.join(" ")),
            Err(err) => format!("{kind}Command failed to start: {err:?}"),
        };
        self.record(
            Record::new(name.to_string(), Event::Ran, phase)
                .detail(detail)
                .instance(instance),
            now,
        );
    }

    fn close_all(&mut self, client: &mut Client, name: &str) {
        if let Some(unit) = self.units.get(name) {
            if let Some(pty) = unit.pty {
                let _ = client.signal_terminal(pty, terminal_wire::SignalKind::Terminate);
                let _ = client.close_terminal(pty);
            }
            for run in &unit.runs {
                let _ = client.close_terminal(run.pty);
            }
        }
    }

    // ------------------------------------------------------------- surfaces

    fn apply_surface_update(&mut self, changes: Vec<SurfaceStateChange>, now: u64) {
        for change in changes {
            let touched = match change {
                SurfaceStateChange::Upsert(record) => {
                    let owner = self
                        .app_owners
                        .get(&record.app_handle)
                        .cloned()
                        .or_else(|| {
                            self.surface_owners
                                .get(&record.application_id)
                                .cloned()
                                .map(|unit| (unit, 0))
                        });
                    let unit = owner.as_ref().map(|(unit, _)| unit.clone());
                    self.surfaces.insert(
                        record.surface_handle,
                        Surface {
                            unit: unit.clone(),
                            seq: owner.and_then(|(_, seq)| (seq != 0).then_some(seq)),
                            app_handle: record.app_handle,
                            application_id: record.application_id,
                            title: record.title,
                            width: fixed_dimension(record.logical_width_32_32),
                            height: fixed_dimension(record.logical_height_32_32),
                        },
                    );
                    unit
                }
                // Current v1 patches carry only revision/extensions. Geometry,
                // title, and ownership changes arrive as complete replacement
                // records, so there is no lossy packet-shaped patch synthesis.
                SurfaceStateChange::Patch(patch) => self
                    .surfaces
                    .get(&patch.surface_handle)
                    .and_then(|surface| surface.unit.clone()),
                SurfaceStateChange::Remove(removed) => self
                    .surfaces
                    .remove(&removed.surface_handle)
                    .and_then(|surface| surface.unit),
            };
            if let Some(unit) = touched {
                self.touch(&unit, now);
            }
        }
    }

    /// The surfaces a unit's live run has open, newest id last.
    fn surfaces_of(&self, name: &str) -> Vec<(u64, &Surface)> {
        let seq = self.units.get(name).map(Unit::current_seq);
        self.surfaces
            .iter()
            .filter(|(_, surface)| surface.unit.as_deref() == Some(name))
            .filter(|(_, surface)| seq.is_none() || surface.seq.is_none() || surface.seq == seq)
            .map(|(id, surface)| (*id, surface))
            .collect()
    }

    /// Mint a typed native application endpoint. The server applies its exact
    /// environment overrides when the app handle is carried by Terminal
    /// LAUNCH, and Surface state reports the same opaque handle back.
    fn app_endpoint(&mut self, client: &mut Client, name: &str, seq: u64) -> Option<u64> {
        let app_id = supervisor::app_id_for(name);
        let endpoint = client.create_app_endpoint(app_id.clone()).ok()?;
        self.surface_owners.insert(app_id, name.to_string());
        self.app_owners
            .insert(endpoint.app_handle, (name.to_string(), seq));
        Some(endpoint.app_handle)
    }
    // ----------------------------------------------------------- environment

    /// Read every `envFile` and merge it with `env`.
    fn resolve_env(
        &mut self,
        client: &mut Client,
        name: &str,
        cwd: &str,
    ) -> Result<ResolvedEnv, String> {
        let Some(unit) = self.units.get(name) else {
            return Ok(ResolvedEnv::default());
        };
        // Only the two env fields are needed, and cloning the whole UnitFile
        // per spawn is the expensive way to borrow them.
        let entries = unit.file.env_file.clone();
        let inline = unit.file.env.clone();
        let mut loaded: Vec<(String, EnvFile)> = Vec::new();
        let mut sources = Vec::new();
        for entry in &entries {
            let path = rebase(&expand_tilde(&entry.path, &self.home), cwd);
            match self.read_file(client, &path) {
                Some(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    loaded.push((path.clone(), envfile::parse(&text)));
                    sources.push(path);
                }
                None if entry.optional => {}
                None => return Err(format!("envFile {path} is missing")),
            }
        }
        Ok(ResolvedEnv {
            vars: envfile::merge(&loaded, &inline),
            sources,
        })
    }

    /// One-shot read of an absolute path. `FS_READ` needs no sync.
    fn read_file(&mut self, client: &mut Client, path: &str) -> Option<Vec<u8>> {
        let (parent, relative) = split_absolute_file(path)?;
        let mut root = client
            .open_fs(
                fs_wire::RootSource::PlatformPath(parent.as_bytes().to_vec()),
                yas_wire::schema::fs::OPEN_READ_ONLY as u16,
            )
            .ok()?;
        let result = root
            .read(
                client,
                vec![fs_wire::ReadQuestion {
                    kind: yas_wire::schema::fs::READ_CONTENT as u16,
                    flags: 0,
                    path: fs_path(relative)?,
                }],
            )
            .ok();
        let _ = root.close(client);
        match result?.records.into_iter().next()? {
            fs_wire::QueryRecord::Read(record)
                if record.status == yas_wire::core::Status::Ok.code() =>
            {
                Some(record.content)
            }
            _ => None,
        }
    }

    // -------------------------------------------------------------- readiness

    fn probe(&mut self, client: &mut Client, name: &str, now: u64) {
        let Some(unit) = self.units.get(name) else {
            return;
        };
        let ready_when = unit.file.ready_when.clone();
        let unit_type = unit.file.unit_type;
        let pty = unit.pty;
        let started = unit.started_ms;

        // A oneshot is ready when its Terminal State exit record reports 0.
        if unit_type == UnitType::Oneshot {
            self.next_probe_ms
                .insert(name.to_string(), now + PROBE_INTERVAL.as_millis() as u64);
            return;
        }

        let satisfied = match &ready_when {
            ReadyWhen::Spawn => true,
            ReadyWhen::Manual => false,
            ReadyWhen::Delay(d) => now.saturating_sub(started) >= d.ms(),
            ReadyWhen::Path(path) => {
                let path = expand_tilde(path, &self.home);
                self.path_exists(client, &path)
            }
            ReadyWhen::Tcp(target) => self.tcp_connects(client, target),
            ReadyWhen::Http(url) => self.http_answers(client, url),
            ReadyWhen::Log(needle) => {
                // Not polled: the server holds the wait and answers through
                // the loop. Arm it once and take no further deadline.
                if let Some(pty) = pty {
                    self.arm_log_wait(client, name, pty, needle, now);
                }
                self.next_probe_ms.remove(name);
                return;
            }
        };
        if satisfied {
            let how = describe_ready(&ready_when);
            self.ready(client, name, &how);
        } else {
            let interval = if matches!(ready_when, ReadyWhen::Log(_)) {
                LOG_PROBE_INTERVAL
            } else {
                PROBE_INTERVAL
            };
            self.next_probe_ms
                .insert(name.to_string(), now + interval.as_millis() as u64);
        }
    }

    fn path_exists(&mut self, client: &mut Client, path: &str) -> bool {
        let Some((parent, relative)) = split_absolute_file(path) else {
            return false;
        };
        let Ok(mut root) = client.open_fs(
            fs_wire::RootSource::PlatformPath(parent.as_bytes().to_vec()),
            yas_wire::schema::fs::OPEN_READ_ONLY as u16,
        ) else {
            return false;
        };
        let exists = root
            .read(
                client,
                vec![fs_wire::ReadQuestion {
                    kind: yas_wire::schema::fs::READ_STAT as u16,
                    flags: 0,
                    path: match fs_path(relative) {
                        Some(path) => path,
                        None => return false,
                    },
                }],
            )
            .ok()
            .and_then(|page| page.records.into_iter().next())
            .is_some_and(|record| {
                matches!(record, fs_wire::QueryRecord::Read(value) if value.status == yas_wire::core::Status::Ok.code())
            });
        let _ = root.close(client);
        exists
    }

    fn tcp_connects(&mut self, client: &mut Client, target: &str) -> bool {
        let Some((host, port)) = split_host_port(target) else {
            return false;
        };
        let deadline = client.monotonic_now() + PROBE_INTERVAL;
        let Ok(mut flow) = client.open_byte_flow_window_until(
            yas_wire::net::Address::Tcp { host, port },
            None,
            Vec::new(),
            deadline,
            PROBE_WINDOW,
        ) else {
            return false;
        };
        let _ = flow.close(client);
        true
    }

    /// The dumbest possible HTTP: connect, GET, read the status line. No TLS,
    /// no redirects, no body — a probe, not a client.
    fn http_answers(&mut self, client: &mut Client, url: &str) -> bool {
        let rest = match url.strip_prefix("http://") {
            Some(rest) => rest,
            None => return false,
        };
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let Some((host, port)) = split_host_port_default(authority, 80) else {
            return false;
        };
        let request =
            format!("GET {path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
        let deadline = client.monotonic_now() + PROBE_INTERVAL;
        let Ok(mut flow) = client.open_byte_flow_window_until(
            yas_wire::net::Address::Tcp { host, port },
            None,
            request.into_bytes(),
            deadline,
            PROBE_WINDOW,
        ) else {
            return false;
        };
        let mut body = Vec::new();
        while body.len() < 64 {
            match flow.next_event_until(client, deadline) {
                Ok(Some(yas_guest::net::Event::Read(delivery))) => {
                    let Ok(bytes) = flow.consume(client, delivery) else {
                        break;
                    };
                    body.extend_from_slice(&bytes[..bytes.len().min(64 - body.len())]);
                }
                Ok(Some(yas_guest::net::Event::WriteCredit { .. })) => {}
                Ok(Some(yas_guest::net::Event::ReadClosed { .. }))
                | Ok(Some(yas_guest::net::Event::Reset { .. }))
                | Ok(None)
                | Err(_) => break,
            }
        }
        let _ = flow.close(client);
        let head = String::from_utf8_lossy(&body);
        head.split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .is_some_and(|code| code < 500)
    }

    /// Arm one native Terminal WAIT and let the Result return through the loop.
    ///
    /// The server blocks; muster does not. Waiting on the reply here would park
    /// the single receive loop for the whole of `timeoutStart` — every other
    /// unit's exit and every CLI invocation behind it — which is worse than the
    /// poll this replaces. So the wait is armed once, its nonce remembered, and
    /// `route` turns the reply into a readiness decision whenever it lands.
    ///
    /// The cursor comes from a `SINCE_PROBE` taken now, so the match is text
    /// that arrives *after* the unit started rather than whatever was already
    /// on screen. That one round trip is bounded and happens once per start.
    fn arm_log_wait(&mut self, client: &mut Client, name: &str, pty: u64, needle: &str, now: u64) {
        if self.log_waits.contains_key(name) {
            return;
        }
        let Some(generation) = self.terminal_generations.get(&pty).copied() else {
            return;
        };
        let (from_seq, from_col) = match self.log_cursor.get(name) {
            Some(cursor) => *cursor,
            None => {
                let request = terminal_wire::Output {
                    terminal_handle: pty,
                    generation,
                    cursor_kind: yas_wire::schema::terminal::OUTPUT_CURSOR_PROBE as u8,
                    flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
                    cursor_a: 0,
                    cursor_b: 0,
                    max_bytes: 1,
                    initial_receive_credit: 64 * 1024,
                    extensions: Extensions::default(),
                };
                let Ok(result) = client.query_terminal_output(request, 64 * 1024) else {
                    return;
                };
                let Ok(reply) = result.decode::<terminal_wire::OutputResult>() else {
                    return;
                };
                let cursor = (reply.next_seq, reply.next_col);
                self.log_cursor.insert(name.to_string(), cursor);
                cursor
            }
        };
        let remaining = self
            .units
            .get(name)
            .map(|unit| unit.deadline_ms.saturating_sub(now))
            .unwrap_or(0);
        let request = terminal_wire::Wait {
            terminal_handle: pty,
            generation,
            wait_kind: yas_wire::schema::terminal::WAIT_OUTPUT as u8,
            flags: yas_wire::schema::terminal::WAIT_FLAGS as u8,
            cursor_a: from_seq,
            cursor_b: from_col,
            max_bytes: 64 * 1024,
            timeout_ns: remaining.saturating_mul(1_000_000).max(1),
            needle: needle.as_bytes().to_vec(),
            initial_receive_credit: 64 * 1024,
            extensions: Extensions::default(),
        };
        if let Ok(query) = client.begin_terminal_wait(request, 64 * 1024) {
            self.log_waits.insert(
                name.to_string(),
                LogWait {
                    terminal_handle: pty,
                    query,
                },
            );
        }
    }

    /// A Terminal WAIT Result arrived. Matched means ready; anything else means
    /// the wait timed out, and `timeoutStart` decides what that costs.
    fn note_log_wait(&mut self, client: &mut Client, frame: &Frame) -> bool {
        let Some(name) = self
            .log_waits
            .iter()
            .find_map(|(name, wait)| wait.query.owns_frame(frame).then(|| name.clone()))
        else {
            return false;
        };
        let Some(mut wait) = self.log_waits.remove(&name) else {
            return true;
        };
        let result = match wait.query.offer_frame(client, frame) {
            Ok(Some(result)) => result,
            Ok(None) => {
                self.log_waits.insert(name, wait);
                return true;
            }
            Err(error) => {
                ext_log(client, &format!("muster: log wait: {error}"));
                return true;
            }
        };
        let Ok(reply) = result.decode::<terminal_wire::OutputResult>() else {
            return true;
        };
        self.log_cursor
            .insert(name.clone(), (reply.next_seq, reply.next_col));
        // A wait armed for a run that has since been replaced must not declare
        // its successor ready: the reply describes a terminal that is gone.
        let current = self.units.get(&name).is_some_and(|unit| {
            unit.phase == Phase::Activating && unit.pty == Some(wait.terminal_handle)
        });
        if current && reply.flags & yas_wire::schema::terminal::OUTPUT_MATCHED as u16 != 0 {
            self.ready(client, &name, "log");
        }
        true
    }

    fn record(&mut self, record: Record, now: u64) {
        let stored = self.journal.push(record, now).clone();
        self.touch(&stored.unit, now);
        self.publish_event(&stored, now);
    }

    /// A verb a panel sent, with the same meaning the CLI gives it.
    fn panel_command(&mut self, client: &mut Client, verb: &str, name: &str, now: u64) {
        match verb {
            // Not "re-read the directory": the watch did that already. This is
            // the retry for a directory whose watch was refused, which is the
            // only thing a panel could ask for that it does not already have.
            "rewatch" => self.retry_unwatchable(client, now, true),
            "resync" => self.touch_all(now),
            "start" | "stop" | "restart" if !name.is_empty() => {
                for member in self.resolve_name(name) {
                    match verb {
                        "start" => self.want(client, &member, Cause::Command),
                        "stop" => self.stop(client, &member, Cause::Command, true),
                        _ => self.restart(client, &member, Cause::Command),
                    }
                }
            }
            _ => {}
        }
    }

    /// A name is a unit, or an instance standing for its members.
    pub(crate) fn resolve_name(&self, name: &str) -> Vec<String> {
        if self.units.contains_key(name) {
            return vec![name.to_string()];
        }
        self.instances
            .get(name)
            .map(|instance| instance.members.clone())
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------ helpers

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdoptionPolicy {
    Adopt,
    Observe(u64),
    Reject,
}

fn adoption_policy(phase: Phase, current: Option<u64>) -> AdoptionPolicy {
    match current {
        Some(pty) => AdoptionPolicy::Observe(pty),
        None if phase == Phase::Held => AdoptionPolicy::Reject,
        None => AdoptionPolicy::Adopt,
    }
}

fn same_spec(a: &UnitFile, b: &UnitFile) -> bool {
    a.requires == b.requires
        && a.wants == b.wants
        && a.after == b.after
        && a.command == b.command
        && a.shell == b.shell
        && a.cwd == b.cwd
        && a.env == b.env
        && a.env_file == b.env_file
        && a.unit_type == b.unit_type
        && a.ready_when == b.ready_when
}

fn terminal_launch(
    argv: Option<&Vec<String>>,
    shell: &str,
    cwd: &str,
    environment: &[(String, String, Origin)],
    app_handle: Option<u64>,
) -> terminal_wire::Launch {
    let command = match (argv, shell.is_empty()) {
        (Some(argv), _) => terminal_wire::Command::Argv(
            argv.iter()
                .map(|argument| argument.as_bytes().to_vec())
                .collect(),
        ),
        (None, false) => terminal_wire::Command::ShellCommand(shell.to_string()),
        (None, true) => terminal_wire::Command::DefaultShell,
    };
    let mut environment = environment
        .iter()
        .map(|(key, value, _)| terminal_wire::EnvironmentEntry {
            key: key.as_bytes().to_vec(),
            value: terminal_wire::EnvironmentValue::Set(value.as_bytes().to_vec()),
        })
        .collect::<Vec<_>>();
    environment.sort_by(|left, right| left.key.cmp(&right.key));
    environment.dedup_by(|left, right| left.key == right.key);
    let extensions = app_handle
        .and_then(|handle| yas_guest::terminal::app_handle_extension(handle).ok())
        .map_or_else(Extensions::default, |extension| Extensions(vec![extension]));
    terminal_wire::Launch {
        command,
        cwd: terminal_wire::Cwd::Path(cwd.as_bytes().to_vec()),
        environment_base: terminal_wire::EnvironmentBase::Server,
        environment,
        extensions,
    }
}

fn terminal_signal(signal: i32) -> terminal_wire::SignalKind {
    match signal {
        1 => terminal_wire::SignalKind::Hangup,
        2 => terminal_wire::SignalKind::Interrupt,
        9 => terminal_wire::SignalKind::Kill,
        _ => terminal_wire::SignalKind::Terminate,
    }
}

fn terminal_exit_status(exit: &terminal_wire::ExitRecord) -> i32 {
    match exit {
        terminal_wire::ExitRecord::Code { code, .. } => *code,
        terminal_wire::ExitRecord::Signal { native_signal, .. } => {
            native_signal.saturating_abs().saturating_neg()
        }
        terminal_wire::ExitRecord::Other { .. } => -1,
    }
}

fn fs_path(path: &str) -> Option<fs_wire::Path> {
    let components = path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(|component| component.as_bytes().to_vec())
        .collect::<Vec<_>>();
    (!components.is_empty()).then_some(fs_wire::Path { components })
}

fn fs_path_string(path: &fs_wire::Path) -> Option<String> {
    path.components
        .iter()
        .map(|component| core::str::from_utf8(component).ok())
        .collect::<Option<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn split_absolute_file(path: &str) -> Option<(&str, &str)> {
    let (parent, relative) = path.rsplit_once('/')?;
    if relative.is_empty() {
        return None;
    }
    Some((if parent.is_empty() { "/" } else { parent }, relative))
}

fn fixed_dimension(value: i64) -> u16 {
    u16::try_from((value >> 32).max(1)).unwrap_or(u16::MAX)
}

fn expand_tilde(path: &str, home: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) => format!("{home}{rest}"),
        None => path.to_string(),
    }
}

/// Make a path absolute against a base. Used for `envFile` against `cwd` and
/// for a template's relative paths against its stack directory.
fn rebase(path: &str, cwd: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path)
    }
}

fn split_host_port(target: &str) -> Option<(String, u16)> {
    split_host_port_default(target, 0).filter(|(_, port)| *port != 0)
}

fn split_host_port_default(target: &str, default: u16) -> Option<(String, u16)> {
    match target.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None => Some((target.to_string(), default)),
    }
}

fn describe_ready(ready: &ReadyWhen) -> String {
    match ready {
        ReadyWhen::Spawn => "spawn".into(),
        ReadyWhen::Manual => "manual".into(),
        ReadyWhen::Delay(d) => format!("delay:{}ms", d.ms()),
        ReadyWhen::Path(p) => format!("path:{p}"),
        ReadyWhen::Log(l) => format!("log:{l}"),
        ReadyWhen::Tcp(t) => format!("tcp:{t}"),
        // The scheme already names the probe. Prefixing it produced the
        // user-facing `http:http://…` in status and journal output.
        ReadyWhen::Http(u) => u.clone(),
    }
}

/// The signals worth naming. Anything else is taken as a number.
fn signal_number(name: &str) -> i32 {
    match name.trim().trim_start_matches("SIG") {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "KILL" => 9,
        "USR1" => 10,
        "USR2" => 12,
        "TERM" => 15,
        other => other.parse().unwrap_or(15),
    }
}

mod cli;
mod panel;

/// What a start resolved from `envFile` + `env`, and which files it read.
#[derive(Default)]
struct ResolvedEnv {
    vars: Vec<(String, String, Origin)>,
    sources: Vec<String>,
}

/// One uniform `u64` from the host, for backoff jitter.
fn random() -> u64 {
    let mut bytes = [0u8; 8];
    let _ = yas_guest::fill_random(&mut bytes);
    u64::from_le_bytes(bytes)
}

/// What one instance resolved to.
struct Expansion {
    members: Vec<String>,
    units: Vec<Unit>,
    ports: Option<(i64, u32)>,
}

/// Resolve a unit's relative paths against the directory its file came from.
///
/// This is what lets a repository-resident stack say `"cwd": "../.."` and mean
/// its own checkout rather than the server's working directory. It applies to
/// included units too: where a file lives is what "relative" means, and a rule
/// that held only for templates would be a rule with an exception.
fn rebase_unit_paths(file: &mut UnitFile, base: &str) {
    let relative = |path: &str| !path.starts_with('/') && !path.starts_with('~');
    if let Some(cwd) = &file.cwd
        && relative(cwd)
    {
        file.cwd = Some(format!("{base}/{cwd}"));
    }
    for entry in &mut file.env_file {
        if relative(&entry.path) {
            entry.path = format!("{base}/{}", entry.path);
        }
    }
}

/// A Wayland surface, and the run it belongs to.
///
/// `unit` is `None` for a surface nothing here owns — every other client's
/// windows arrive on the same broadcast, and attributing them would be a lie.
#[derive(Clone, Debug, Default)]
struct Surface {
    unit: Option<String>,
    /// The run's sequence, from the socket's instance id, so a window is tied
    /// to the run that opened it rather than merely to the unit.
    seq: Option<u64>,
    app_handle: u64,
    application_id: String,
    title: String,
    width: u16,
    height: u16,
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn default_and_named_servers_get_separate_muster_directories() {
        let vars = BTreeMap::from([("HOME", "/home/me"), ("YAS_SERVER_NAME", "work")]);
        assert_eq!(
            resolve_dir_from(|key| vars.get(key).map(|value| (*value).to_owned())).unwrap(),
            "/home/me/.config/yas/instances/work/muster"
        );
        assert_eq!(
            resolve_dir_from(|key| (key == "HOME").then(|| "/home/me".to_owned())).unwrap(),
            "/home/me/.config/yas/instances/default/muster"
        );
    }

    #[test]
    fn explicit_muster_directory_still_wins() {
        let vars = BTreeMap::from([
            ("YAS_MUSTER_DIR", "/srv/muster"),
            ("YAS_SERVER_NAME", "work"),
        ]);
        assert_eq!(
            resolve_dir_from(|key| vars.get(key).map(|value| (*value).to_owned())).unwrap(),
            "/srv/muster"
        );
    }

    #[test]
    fn local_socket_path_uses_only_a_canonical_server_template() {
        let sockets = LocalSockets::from_environment(&|key| {
            (key == "YAS_SOCKET_TEMPLATE").then(|| "/run/user/1000/yas/yas-{name}.sock".to_owned())
        });
        assert_eq!(
            sockets.for_name("epic").as_deref(),
            Some("/run/user/1000/yas/yas-epic.sock")
        );
        assert_eq!(LocalSockets::default().for_name("epic"), None);
    }

    #[test]
    fn local_socket_template_rejects_path_text_and_unsafe_names() {
        for malformed in [
            "/tmp/yas-{name}.sock; touch /tmp/pwned",
            "/tmp/../yas/yas-{name}.sock",
            "/tmp/yas/yas-{name}-{name}.sock",
            "relative/yas-{name}.sock",
            "/tmp/yas/yas-${name}.sock",
        ] {
            let sockets = LocalSockets::from_environment(&|key| {
                (key == "YAS_SOCKET_TEMPLATE").then(|| malformed.to_owned())
            });
            assert_eq!(sockets.for_name("epic"), None, "accepted {malformed:?}");
        }

        let sockets = LocalSockets::from_environment(&|key| {
            (key == "YAS_SOCKET_TEMPLATE").then(|| "/tmp/yas-1000/yas-{name}.sock".to_owned())
        });
        for name in ["", "../peer", "work.", "CON", "com1.dev"] {
            assert_eq!(sockets.for_name(name), None, "accepted {name:?}");
        }
        assert_eq!(sockets.for_name(&"x".repeat(65)), None);
    }

    #[test]
    fn local_socket_template_enforces_portable_unix_path_limit() {
        let parent = format!("/{}", "a".repeat(90));
        let template = format!("{parent}/yas-{{name}}.sock");
        let sockets = LocalSockets::from_environment(&|key| {
            (key == "YAS_SOCKET_TEMPLATE").then(|| template.clone())
        });
        assert_eq!(sockets.for_name("short"), None);
    }

    #[test]
    fn local_socket_template_accepts_canonical_windows_pipe() {
        let sockets = LocalSockets::from_environment(&|key| {
            (key == "YAS_SOCKET_TEMPLATE").then(|| r"\\.\pipe\yas-alice-{name}".to_owned())
        });
        assert_eq!(
            sockets.for_name("epic").as_deref(),
            Some(r"\\.\pipe\yas-alice-epic")
        );
    }

    fn file(json: &str) -> UnitFile {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn execution_readiness_and_dependencies_are_spec() {
        let base = file(r#"{"command":["api"]}"#);
        for changed in [
            file(r#"{"command":["api","--debug"]}"#),
            file(r#"{"command":["api"],"requires":["db"]}"#),
            file(r#"{"command":["api"],"wants":["mail"]}"#),
            file(r#"{"command":["api"],"after":["migrate"]}"#),
            file(r#"{"command":["api"],"readyWhen":{"tcp":"127.0.0.1:80"}}"#),
        ] {
            assert!(!same_spec(&base, &changed));
        }
    }

    #[test]
    fn policy_changes_apply_without_replacing_the_process() {
        let base = file(r#"{"command":["api"]}"#);
        let changed =
            file(r#"{"command":["api"],"description":"new","restartOnFailure":false,"keep":4}"#);
        assert!(same_spec(&base, &changed));
    }

    #[test]
    fn terminal_snapshot_observes_owned_rejects_held_and_adopts_unowned_runs() {
        assert_eq!(
            adoption_policy(Phase::Running, Some(41)),
            AdoptionPolicy::Observe(41)
        );
        assert_eq!(adoption_policy(Phase::Held, None), AdoptionPolicy::Reject);
        assert_eq!(adoption_policy(Phase::Stopped, None), AdoptionPolicy::Adopt);
    }
}
