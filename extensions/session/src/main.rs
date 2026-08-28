//! `@session` — native YAS application supervision.
//!
//! One typed Event loop services command and panel Channels, Process and
//! Surface state, stream credit, and retry deadlines. No retired packet
//! opcode, compatibility transport, or endpoint-local numeric alias is part
//! of the shipped guest.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;
use yas_ext_session::desktop_entry::{self, DesktopEntry};
use yas_ext_session::icon;
use yas_ext_session::supervisor::{App, Phase, next_deadline_ns};
use yas_guest::Client;
use yas_guest::channel::{
    Channel, Error as ChannelError, Event as ChannelEvent, Listener, ListenerEvent,
};
use yas_guest::command::{CommandProvider, Error as CommandError, ProviderEvent};
use yas_guest::env::Error as EnvError;
use yas_guest::fs::{Error as FsError, Root as FsRoot, Watch as FsWatch};
use yas_guest::kv::{Error as KvError, Namespace as KvNamespace, StateChange as KvStateChange};
use yas_guest::process::{
    Error as ProcessError, Event as ProcessEvent, Process, ProcessWatch, StateChange,
};
use yas_guest::surface::{
    Error as SurfaceError, StateChange as SurfaceStateChange, Watch as SurfaceWatch,
};
use yas_guest::yas::wire;

#[derive(Debug)]
enum Error {
    Client(yas_guest::Error),
    Channel(ChannelError),
    Command(CommandError),
    Process(ProcessError),
    Kv(KvError),
    Env(EnvError),
    Fs(FsError),
    Surface(SurfaceError),
    Invalid(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "native guest error: {error}"),
            Self::Channel(error) => write!(formatter, "native Channel error: {error}"),
            Self::Command(error) => write!(formatter, "native Command error: {error}"),
            Self::Process(error) => write!(formatter, "native Process error: {error}"),
            Self::Kv(error) => write!(formatter, "native KV error: {error}"),
            Self::Env(error) => write!(formatter, "native Env error: {error}"),
            Self::Fs(error) => write!(formatter, "native FS error: {error}"),
            Self::Surface(error) => write!(formatter, "native Surface error: {error}"),
            Self::Invalid(detail) => write!(formatter, "invalid native session state: {detail}"),
        }
    }
}

impl From<yas_guest::Error> for Error {
    fn from(error: yas_guest::Error) -> Self {
        Self::Client(error)
    }
}

impl From<ChannelError> for Error {
    fn from(error: ChannelError) -> Self {
        Self::Channel(error)
    }
}

impl From<CommandError> for Error {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl From<ProcessError> for Error {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

impl From<KvError> for Error {
    fn from(error: KvError) -> Self {
        Self::Kv(error)
    }
}

impl From<EnvError> for Error {
    fn from(error: EnvError) -> Self {
        Self::Env(error)
    }
}

impl From<FsError> for Error {
    fn from(error: FsError) -> Self {
        Self::Fs(error)
    }
}

impl From<SurfaceError> for Error {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

const DESCRIPTOR: &str = r#"{
  "protocol":"yas.cli.v1",
  "summary":"Autostart and supervise GUI applications in this session",
  "commands":[
    {"path":["list"],"summary":"Installed applications and whether they are enabled",
     "usage":"yas @session list"},
    {"path":["enable"],"summary":"Start an application now and on every session start",
     "usage":"yas @session enable <app>"},
    {"path":["disable"],"summary":"Stop an application and stop starting it",
     "usage":"yas @session disable <app>"},
    {"path":["start"],"summary":"Start an application now, without remembering it",
     "usage":"yas @session start <app>"},
    {"path":["stop"],"summary":"Stop an application now, keeping it enabled",
     "usage":"yas @session stop <app>"},
    {"path":["forget"],"summary":"Stop an application and drop it from the list",
     "usage":"yas @session forget <app>"},
    {"path":["status"],"summary":"What one application is doing, and its windows",
     "usage":"yas @session status <app>"}
  ]
}"#;

/// kv key prefix. The store is flat and shared across every session on a
/// desktop server, so the prefix is what keeps two sessions from overwriting
/// each other's intent.
const KV_PREFIX: &str = "ext/session/app/";

/// Channel the browser panel reads.
///
/// Outbound is JSON, one object per message, so the mirror needs no parser of
/// its own. Inbound is a single line of plain text (`enable <id>`) because a
/// Wasm guest has no JSON parser and the command vocabulary is three verbs —
/// hand-rolling a parser for that would be more code than it saves.
const CHANNEL_NAME: &str = "yas.session.v1";

fn main() {}

yas_guest::entry!(run);

/// How long the installed catalog is trusted before it is read again.
///
/// Native watches are the primary refresh path. This is a fallback for roots
/// that appear after startup or a platform watcher that is lost.
const CATALOG_TTL: Duration = Duration::from_secs(60);

/// Most icons a panel may ask for in one request.
///
/// Generous on purpose, and matched by the panel's own batch size: a request
/// costs one child whatever it asks for, so a bigger batch is strictly cheaper
/// per icon, and a scrolling list needs the throughput. It is still a bound —
/// the whole batch is resolved inside the receive loop, so an unbounded request
/// would be a way to stall the supervisor from the browser.
const MAX_ICON_REQUEST: usize = 48;

/// Candidate directories probed in the first icon lookup round.
///
/// Native FS READ evaluates every STAT question in a request; `FirstStat`
/// chooses the first successful answer only after that. Sending all theme
/// directories at once therefore did thousands of syscalls for a screenful,
/// even when every icon was in the first theme. Probe a small ranked prefix,
/// then widen geometrically only for names that were not there.
const ICON_LOOKUP_INITIAL_DIRS: usize = 8;
const ICON_LOOKUP_MAX_DIRS_PER_ROUND: usize = 32;

/// Resolved icon paths kept in the guest before the cache is dropped wholesale.
///
/// Measured in bytes rather than entries because the entries are not
/// comparable: a themed SVG is 3 KB and a 128px PNG can be [`icon::MAX_ICON_BYTES`],
/// so any count that is safe for the second is uselessly small for the first.
/// Base64 art is by far the largest thing this extension holds, and a session
/// whose operator scrolls a thousand-entry catalog would otherwise accumulate
/// all of it.
///
/// Clearing rather than evicting the oldest entry keeps the bookkeeping to a
/// comparison: a miss costs one shell round trip, and the panel has its own
/// cache, so nothing already on screen pays for it.
///
/// Large enough to hold one whole request — [`MAX_ICON_REQUEST`] files of
/// [`icon::MAX_ICON_BYTES`] come to about 8 MiB once base64 has grown them by a
/// third. Below that a single scroll of big artwork is guaranteed to clear the
/// cache it is still filling, which costs a re-read of everything it just did.
const MAX_CACHED_ICON_BYTES: usize = 12 * 1024 * 1024;

/// GUI diagnostics are consumed and discarded as they arrive. A small
/// replenished window prevents every open application from pinning the guest
/// SDK's 4 MiB general-purpose Process default.
const APP_PROCESS_STREAM_WINDOW: u64 = 64 * 1024;
const APP_DIAGNOSTIC_BYTES: usize = 4096;

/// Icon messages a connection may have waiting on credit.
///
/// Icons are queued rather than dropped, because unlike state nothing provokes
/// a repeat: a dropped one leaves a placeholder until the panel asks again. Two
/// full requests' worth, so an ordinary scroll never reaches it, and a panel
/// that has stopped acking altogether still cannot grow the guest without
/// limit. What is dropped past here the panel re-asks for, once it stops
/// counting the id as outstanding.
const MAX_QUEUED_ICONS: usize = 128;

/// One browser connected to [`CHANNEL_NAME`].
struct Conn {
    channel: Channel,
    closed: bool,
    /// The last state this connection was actually *sent*.
    ///
    /// Per-connection, and recorded only after a successful send. A shared
    /// "last published" string updated before the send meant that a panel
    /// which was briefly out of credit missed the message and then had every
    /// repeat of it suppressed as a duplicate — it stayed stale until some
    /// unrelated change came along.
    last_sent: String,
    /// Catalog revision this browser has received. Unlike managed application
    /// state, the catalog is omitted from ordinary publishes because it can be
    /// large; directory changes advance this and force one complete refresh.
    catalog_revision: u64,
    /// Icon messages waiting for credit, oldest first.
    ///
    /// State can be dropped when a panel is out of credit because the next
    /// publish carries it again. An icon reply cannot: it answers a request
    /// that will not be repeated, so dropping one leaves a row with a
    /// placeholder for the rest of the session.
    queued: Vec<String>,
}

/// One live XDG applications directory.
struct CatalogWatch {
    path: String,
    root: FsRoot,
    watch: FsWatch,
}

/// One application's persisted intent.
struct Intent {
    enabled: bool,
    /// Native handles are boot-scoped. The full Core boot ID is persisted
    /// beside the opaque handle so a later server can never reinterpret it.
    boot_id: Option<[u8; 16]>,
    process_handle: Option<u64>,
}

struct State {
    /// Desired state, keyed by desktop-entry id.
    apps: BTreeMap<String, App>,
    /// Installed applications, refreshed live and backed by a TTL.
    installed: BTreeMap<String, DesktopEntry>,
    /// XDG applications directories in precedence order.
    catalog_roots: Vec<String>,
    /// Live native filesystem subscriptions for the roots that exist.
    catalog_watches: Vec<CatalogWatch>,
    /// Advances whenever the installed catalog changes, so every connected
    /// browser receives one new full catalog rather than remaining stale.
    catalog_revision: u64,
    /// When the catalog was last read, for [`CATALOG_TTL`].
    installed_at_ns: Option<i64>,
    /// Themed and flat icon directories, from the same environment read that
    /// found the catalog. Empty until that read happens.
    icon_theme_roots: Vec<String>,
    icon_flat_roots: Vec<String>,
    /// Every directory an icon could be in, best-first. Expanding the roots'
    /// globs is the one part of a lookup that does not depend on what is being
    /// looked up, so it is done once and reused; a catalog refresh clears it,
    /// because that is when a newly installed theme would show up.
    icon_dirs: Vec<String>,
    /// Resolved artwork, keyed by the `Icon=` value rather than by application
    /// id — a desktop and its `-nightly` twin share a key, and so do the dozens
    /// of entries that all say `application-x-executable`.
    ///
    /// `None` records "looked, found nothing", which is worth caching for the
    /// same reason the artwork is: it stops a panel that keeps redrawing an
    /// icon-less row from spawning a shell every time.
    icons: BTreeMap<String, Option<String>>,
    /// What [`State::icons`] holds, for [`MAX_CACHED_ICON_BYTES`].
    icon_bytes: usize,
    /// Stamped identity per surface, so `status` reports windows rather than
    /// guessing from a self-asserted app_id.
    surface_apps: BTreeMap<u64, String>,
    /// The server process this state describes. A different one invalidates
    /// every persisted native Process handle.
    boot_id: [u8; 16],
    /// Panel listener. Publishing it is optional; CLI supervision remains
    /// useful if another attempt already owns the name.
    data_listener: Option<Listener>,
    /// Browsers reading the panel.
    conns: Vec<Conn>,
    /// Attached stream resources for supervised native Process handles.
    processes: BTreeMap<u64, Process>,
    /// One-shot launcher children, separate from the process (if any) whose
    /// lifetime represents an enabled application's supervision state.
    transient_process_apps: BTreeMap<u64, (String, Option<String>)>,
    /// Surface application endpoints retained until their spawned process
    /// exits. `SPAWN` copies the endpoint environment, but the child may not
    /// have connected to its Wayland socket when that request returns.
    process_app_endpoints: BTreeMap<u64, u64>,
    /// Open native namespace for persisted application intent.
    kv: Option<KvNamespace>,
}

impl State {
    /// Remember one lookup's answer, dropping the whole cache first if it has
    /// grown past [`MAX_CACHED_ICON_BYTES`].
    fn cache_icon(&mut self, key: String, data_url: Option<String>) {
        if self.icons.contains_key(&key) {
            return;
        }
        if self.icon_bytes >= MAX_CACHED_ICON_BYTES {
            self.icons.clear();
            self.icon_bytes = 0;
        }
        self.icon_bytes += key.len() + data_url.as_ref().map_or(0, String::len);
        self.icons.insert(key, data_url);
    }
}

fn run(mut client: Client) -> Result<(), Error> {
    let result = (|| -> Result<(), Error> {
        let mut state = State {
            apps: BTreeMap::new(),
            installed: BTreeMap::new(),
            catalog_roots: Vec::new(),
            catalog_watches: Vec::new(),
            catalog_revision: 0,
            installed_at_ns: None,
            icon_theme_roots: Vec::new(),
            icon_flat_roots: Vec::new(),
            icon_dirs: Vec::new(),
            icons: BTreeMap::new(),
            icon_bytes: 0,
            surface_apps: BTreeMap::new(),
            boot_id: client.hello().boot_id,
            data_listener: None,
            conns: Vec::new(),
            processes: BTreeMap::new(),
            transient_process_apps: BTreeMap::new(),
            process_app_endpoints: BTreeMap::new(),
            kv: None,
        };

        // Intent outlives the server, so restore it before anything else. A failure
        // here is not fatal: serving `list` with no catalog is better than not
        // coming up at all.
        if let Err(error) = restore(&mut client, &mut state) {
            let _ = error;
        }

        let listener_name = format!(
            "yas.cli.{:016x}.{}",
            client.context().extension_handle,
            client.context().attempt
        );
        let listener = client.listen_channel(&listener_name, b"")?;
        let mut provider = CommandProvider::register(&mut client, listener, DESCRIPTOR)?;
        let mut process_watch = client.watch_processes(None)?;
        let mut surface_watch = client.watch_surfaces(None)?;

        // Publishing the browser panel Channel is not fatal if another attempt
        // already owns the name; command supervision remains useful.
        state.data_listener = client.listen_channel(CHANNEL_NAME, b"").ok();
        // Anything already enabled starts now.
        reconcile(&mut client, &mut state);
        publish(&mut client, &mut state);

        loop {
            let now = client.monotonic_now();
            let pending = next_deadline_ns(state.apps.values());
            let deadline = match pending {
                Some(ns) => yas_guest::MonotonicInstant::from_raw_nanos(ns.max(now.raw_nanos())),
                // Nothing pending: wake periodically anyway, so a missed
                // notification cannot wedge the supervisor for the whole session.
                None => now + Duration::from_secs(30),
            };
            match client.next_event_until(deadline)? {
                None => {
                    reconcile(&mut client, &mut state);
                    publish(&mut client, &mut state);
                }
                Some(frame) => {
                    // A CLI invocation is the provider's; every other typed Event
                    // is offered to the exact listener/resource that owns it.
                    match provider.offer_frame(&mut client, &frame)? {
                        Some(ProviderEvent::Invocation(invocation)) => {
                            serve(&mut client, &mut state, *invocation)?;
                            reconcile(&mut client, &mut state);
                            publish(&mut client, &mut state);
                        }
                        Some(ProviderEvent::Closed(_)) => return Ok(()),
                        None => {
                            if route_frame(
                                &mut client,
                                &mut state,
                                &mut process_watch,
                                &mut surface_watch,
                                &frame,
                            )? {
                                reconcile(&mut client, &mut state);
                                publish(&mut client, &mut state);
                            }
                        }
                    }
                }
            }
        }
    })();
    if let Err(error) = &result {
        let _ = client.attempt_log(&error.to_string());
    }
    result
}

/// JSON-escape a string into a buffer. Only the characters JSON requires, plus
/// the C0 range — an application name is arbitrary text from a `.desktop` file.
fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    // Copied in runs rather than character by character. This is the hot loop of
    // the whole extension: an icon is a 30 KB data URL of base64, none of which
    // needs escaping, and pushing that a `char` at a time cost seconds per
    // screenful in an interpreter.
    let mut rest = value;
    while let Some(at) = rest.find(|c: char| c == '"' || c == '\\' || (c as u32) < 0x20) {
        out.push_str(&rest[..at]);
        let mut chars = rest[at..].chars();
        let escaped = chars.next().unwrap_or(' ');
        match escaped {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push_str(&format!("\\u{:04x}", other as u32)),
        }
        rest = chars.as_str();
    }
    out.push_str(rest);
    out.push('"');
}

/// The panel's whole view: every managed application, plus the installed
/// catalog so the panel can offer something to enable.
///
/// Sent complete on every change rather than as a delta. The managed set is
/// what an operator typed, so it is small, and a panel that can only ever be
/// correct is worth more here than one that avoids resending a few hundred
/// bytes. The catalog is the larger half and changes only when packages do, so
/// it is sent once per connection unless asked for again.
fn state_json(state: &State, with_catalog: bool) -> String {
    let mut out = String::from("{\"type\":\"state\",\"apps\":[");
    for (index, (id, app)) in state.apps.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let windows = state
            .surface_apps
            .values()
            .filter(|owner| owner.as_str() == id)
            .count();
        let name = state
            .installed
            .get(id)
            .map(|entry| entry.name.as_str())
            .unwrap_or(id.as_str());
        out.push_str("{\"id\":");
        push_json_string(&mut out, id);
        out.push_str(",\"name\":");
        push_json_string(&mut out, name);
        out.push_str(&format!(
            ",\"enabled\":{},\"phase\":\"{}\",\"failures\":{},\"windows\":{}",
            app.enabled,
            match app.phase {
                Phase::Running => "running",
                Phase::Backoff => "backoff",
                Phase::Idle => "starting",
                Phase::Stopped => "stopped",
            },
            app.failures,
            windows
        ));
        if let Some(exit) = app.last_exit {
            out.push_str(&format!(",\"lastExit\":{exit}"));
        }
        if let Some(display) = &app.wayland_display {
            out.push_str(",\"socket\":");
            push_json_string(&mut out, display);
        }
        out.push('}');
    }
    out.push(']');
    if with_catalog {
        out.push_str(",\"catalog\":[");
        for (index, (id, entry)) in state.installed.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            push_json_string(&mut out, id);
            out.push_str(",\"name\":");
            push_json_string(&mut out, &entry.name);
            out.push('}');
        }
        out.push(']');
    }
    out.push('}');
    out
}

/// One application's artwork, or the fact that it has none.
///
/// A missing `icon` field is the answer "there is nothing to draw", and the
/// panel records it so it stops asking. That is why this is a message per id
/// rather than a map of the ones that were found: a silent omission would be
/// indistinguishable from a reply still in flight.
fn icon_json(id: &str, path: Option<&str>) -> String {
    let mut out = String::from("{\"type\":\"icon\",\"id\":");
    push_json_string(&mut out, id);
    if let Some(path) = path {
        out.push_str(",\"path\":");
        push_json_string(&mut out, path);
    }
    out.push('}');
    out
}

/// Pair each requested id with what this batch resolved for its `Icon=` key.
///
/// An id whose key is absent from `resolved` is **omitted** rather than answered
/// "no artwork". The two are not interchangeable: the panel records "no artwork"
/// as final and never asks again, while an id it hears nothing about goes back
/// on the queue once it stops counting it as outstanding. So absence here means
/// "ask me later", and only a key that was genuinely looked for and not found
/// answers `None`.
fn batch_answers(
    keys: Vec<(String, Option<String>)>,
    resolved: &BTreeMap<String, Option<String>>,
) -> Vec<(String, Option<String>)> {
    keys.into_iter()
        .filter_map(|(id, key)| match key {
            // No usable `Icon=` at all: there is nothing to look for, and
            // nothing a later request could find either.
            None => Some((id, None)),
            Some(key) => resolved.get(&key).map(|found| (id, found.clone())),
        })
        .collect()
}

/// Answer a panel's icon request, reading whatever is not already cached.
///
/// One child for the whole batch, and it does the searching and the reading
/// together — see [`icon::fetch_script`]. Deliberately *not* preceded by a
/// catalog refresh: the ids came out of the catalog the panel already holds, so
/// the only thing rereading it could add is a random half-second stall in the
/// middle of a scroll. An id this does not recognise is answered "no artwork",
/// which is what it will be until the panel resyncs anyway.
///
/// What this batch resolved is held here and answered from here, never read back
/// out of [`State::icons`]: that cache is dropped wholesale when it grows past
/// [`MAX_CACHED_ICON_BYTES`], and at [`icon::MAX_ICON_BYTES`] per file it can
/// hit that limit *part way through one batch*. Answering from it afterwards
/// reported every icon cached before the drop as "no artwork" — which the panel
/// believes forever, so a scrolled list grew a permanent gap exactly one
/// cache-worth long, and the row after it was fine.
fn resolve_icons(
    client: &mut Client,
    state: &mut State,
    ids: &[&str],
) -> Vec<(String, Option<String>)> {
    // Ids the catalog knows nothing about, and entries with no `Icon=` at all,
    // are answered "nothing to draw" without touching the icon path.
    let keys: Vec<(String, Option<String>)> = ids
        .iter()
        .map(|id| {
            let key = state
                .installed
                .get(*id)
                .and_then(|entry| entry.icon.clone())
                .filter(|icon| icon.starts_with('/') || icon::is_lookup_name(icon));
            ((*id).to_string(), key)
        })
        .collect();

    // A name is looked up once however many applications name it.
    let mut resolved: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut lookups: Vec<String> = Vec::new();
    let mut absolute: Vec<String> = Vec::new();
    for key in keys.iter().filter_map(|(_, key)| key.as_ref()) {
        if let Some(cached) = state.icons.get(key) {
            resolved.insert(key.clone(), cached.clone());
            continue;
        }
        let bucket = if key.starts_with('/') {
            &mut absolute
        } else {
            &mut lookups
        };
        if !bucket.contains(key) {
            bucket.push(key.clone());
        }
    }

    // Names are probed in ranked directory stages. FS READ evaluates every
    // question it receives, even though FirstStat returns only the first hit
    // from each group. Asking all 80-odd directories about every visible row
    // therefore did thousands of redundant stats before showing one icon.
    // Most names land in the first stage; only misses widen the search.
    if !lookups.is_empty() {
        refresh_icon_dirs(client, state);
        let dirs = state.icon_dirs.clone();
        if dirs.is_empty() {
            for name in &lookups {
                state.cache_icon(name.clone(), None);
                resolved.insert(name.clone(), None);
            }
        } else {
            let mut pending: Vec<&String> = lookups.iter().collect();
            let mut directory_at = 0;
            let mut directory_count = ICON_LOOKUP_INITIAL_DIRS;
            let mut complete = true;
            'directories: while !pending.is_empty() && directory_at < dirs.len() {
                let directory_end = (directory_at + directory_count).min(dirs.len());
                let stage = &dirs[directory_at..directory_end];
                let per_message =
                    (wire::fs::MAX_QUERY_RECORDS / (stage.len() * 2).max(1)).max(1);
                for names in pending.chunks(per_message) {
                    let candidates: Vec<Vec<String>> = names
                        .iter()
                        .map(|name| icon::candidates(stage, name))
                        .collect();
                    let groups: Vec<Vec<&str>> = candidates
                        .iter()
                        .map(|paths| paths.iter().map(String::as_str).collect())
                        .collect();
                    let borrowed: Vec<&[&str]> = groups.iter().map(Vec::as_slice).collect();
                    // Which file, not what is in it: the panel reads the bytes
                    // itself, so these are STAT questions and carry no artwork.
                    let Some(records) = fs_read(
                        client,
                        ReadMode::FirstStat,
                        icon::MAX_ICON_BYTES,
                        &borrowed,
                    ) else {
                        complete = false;
                        break 'directories;
                    };
                    for (name, (found, path, _)) in names.iter().zip(records) {
                        if found && icon::is_drawable_path(&path) {
                            state.cache_icon((*name).clone(), Some(path.clone()));
                            resolved.insert((*name).clone(), Some(path));
                        }
                    }
                }
                pending.retain(|name| !resolved.contains_key(*name));
                directory_at = directory_end;
                directory_count = (directory_count * 2).min(ICON_LOOKUP_MAX_DIRS_PER_ROUND);
            }
            // Only an exhaustive search proves that a name has no artwork. A
            // transport failure leaves it unanswered so the panel retries it.
            if complete {
                for name in pending {
                    state.cache_icon(name.clone(), None);
                    resolved.insert(name.clone(), None);
                }
            }
        }
    }

    // Absolute `Icon=` values need no search, only the read.
    if !absolute.is_empty() {
        for batch in absolute.chunks(wire::fs::MAX_QUERY_RECORDS) {
            let paths: Vec<&str> = batch.iter().map(String::as_str).collect();
            let Some(records) = fs_read(
                client,
                ReadMode::Stat,
                icon::MAX_ICON_BYTES,
                &[paths.as_slice()],
            ) else {
                break;
            };
            for (exists, path, _) in records {
                let found = (exists && icon::is_drawable_path(&path)).then(|| path.clone());
                state.cache_icon(path.clone(), found.clone());
                resolved.insert(path, found);
            }
        }
    }

    batch_answers(keys, &resolved)
}

/// Expand the icon path's globs into a ranked directory list, once.
///
/// The list is the same whatever is being looked up, and expanding it is the
/// expensive half of a lookup — a dozen roots, each with two globs — so a batch
/// that had to redo it would pay for the whole icon path to answer one name.
fn refresh_icon_dirs(client: &mut Client, state: &mut State) {
    if !state.icon_dirs.is_empty() {
        return;
    }
    let theme_roots = state.icon_theme_roots.clone();
    let mut dirs: Vec<String> = Vec::new();
    for root in icon_index_roots(client, &theme_roots) {
        // Directories only: an icon theme is a few dozen directories holding
        // tens of thousands of files, and this wants somewhere to look.
        let Some(indexed) = fs_index(client, &root, IndexKind::Directories) else {
            continue;
        };
        for rel in indexed {
            if icon::is_icon_dir(&rel) {
                dirs.push(format!("{root}/{rel}"));
            }
        }
    }
    // Pixmaps is flat — the name sits directly in it — so it is a candidate
    // without a listing. One that does not exist costs a stat per lookup and
    // answers nothing, which is the same as not being there.
    dirs.extend(state.icon_flat_roots.iter().cloned());
    let borrowed: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    state.icon_dirs = icon::rank_directories(&borrowed)
        .into_iter()
        .filter(|dir| seen.insert(dir.clone()))
        .collect();
}

/// Roots safe for recursive icon-directory indexing.
///
/// FS INDEX deliberately does not cross directory symlinks. Nix profiles use
/// exactly those at `icons/hicolor`, so indexing only the XDG root sees the
/// link but none of the packaged application icons below it. `hicolor` is the
/// freedesktop fallback every theme must inherit; resolve that link explicitly
/// and index its target, as required by the FS contract. Resolving the XDG root
/// itself also covers profiles whose final `icons` component is a link.
fn icon_index_roots(client: &mut Client, roots: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(roots.len() * 4);
    let mut seen = BTreeSet::new();
    for root in roots {
        let fallback = format!("{root}/hicolor");
        for candidate in [root.as_str(), fallback.as_str()] {
            if seen.insert(candidate.to_string()) {
                out.push(candidate.to_string());
            }
            if let Some(resolved) = fs_link_target(client, candidate)
                && seen.insert(resolved.clone())
            {
                out.push(resolved);
            }
        }
    }
    out
}

/// Send one JSON message to one connection, respecting its credit.
///
/// Reports whether the bytes actually went out, because the caller's idea of
/// what this panel has seen must track what was sent and not what was tried.
fn send_json(client: &mut Client, conn: &mut Conn, payload: &str) -> bool {
    if conn.closed {
        return false;
    }
    let bytes = payload.as_bytes();
    match conn.channel.send(client, bytes) {
        Ok(()) => true,
        Err(ChannelError::CreditExhausted { .. }) => false,
        Err(_) => {
            conn.closed = true;
            false
        }
    }
}

/// Send the current state to every attached panel that has not already seen it.
///
/// The "not already seen" is load-bearing. This runs after every Event, and a
/// panel's own ACK is an Event: publishing unconditionally means the ACK for
/// one state message provokes the next, so two idle peers trade messages as
/// fast as the round trip allows and the panel rebuilds its rows the whole
/// time.
///
/// The comparison is per connection and is only updated once the send
/// succeeds, so a panel that was out of credit is caught up by the next
/// publish instead of having the message it missed suppressed as a duplicate.
/// Send what a connection has queued, oldest first, while its credit lasts.
///
/// Stops at the first refusal rather than skipping past it: the panel matches
/// icons to rows by id, but a viewer watching them appear should see them in
/// the order they were asked for.
fn flush_queued(client: &mut Client, conn: &mut Conn) {
    // Taken rather than cloned, and drained once at the end: a queued icon is
    // 30 KB, so copying each one to send it — and memmoving the rest of the
    // queue after every send — was pure overhead on the path that matters.
    let mut sent = 0;
    while sent < conn.queued.len() {
        let payload = core::mem::take(&mut conn.queued[sent]);
        if !send_json(client, conn, &payload) {
            conn.queued[sent] = payload;
            break;
        }
        sent += 1;
    }
    if sent > 0 {
        conn.queued.drain(..sent);
    }
}

fn publish(client: &mut Client, state: &mut State) {
    if state.conns.is_empty() {
        return;
    }
    // Credit freed by an ACK is why this runs on every routed Event, so it is
    // also the moment anything held back gets another try.
    let mut conns = core::mem::take(&mut state.conns);
    for conn in &mut conns {
        flush_queued(client, conn);
    }
    conns.retain(|conn| !conn.closed);
    state.conns = conns;

    let current = state_json(state, false);
    // Nothing to say if every panel already holds both this exact managed
    // state and the current installed catalog.
    if state
        .conns
        .iter()
        .all(|conn| conn.last_sent == current && conn.catalog_revision == state.catalog_revision)
    {
        return;
    }
    let catalog = state_json(state, true);
    let mut conns = core::mem::take(&mut state.conns);
    for conn in &mut conns {
        let needs_catalog = conn.catalog_revision != state.catalog_revision;
        if conn.last_sent == current && !needs_catalog {
            continue;
        }
        if send_json(
            client,
            conn,
            if needs_catalog { &catalog } else { &current },
        ) {
            conn.last_sent.clear();
            conn.last_sent.push_str(&current);
            if needs_catalog {
                conn.catalog_revision = state.catalog_revision;
            }
        }
    }
    conns.retain(|conn| !conn.closed);
    state.conns = conns;
}

/// Register one accepted native panel Channel and send its complete greeting.
fn accept_panel(client: &mut Client, state: &mut State, channel: Channel) {
    if state
        .conns
        .iter()
        .any(|conn| conn.channel.handle() == channel.handle())
    {
        return;
    }
    let mut conn = Conn {
        channel,
        closed: false,
        last_sent: String::new(),
        catalog_revision: 0,
        queued: Vec::new(),
    };
    refresh_installed_if_stale(client, state);
    let greeting = state_json(state, true);
    if send_json(client, &mut conn, &greeting) {
        conn.last_sent = state_json(state, false);
        conn.catalog_revision = state.catalog_revision;
    }
    state.conns.push(conn);

    // A browser holds this catalog channel open for the page, well before the
    // switcher is normally shown. Expand the stable icon search path now so
    // the first visible rows do not pay the expensive theme-directory walk.
    // The greeting goes first: catalog availability is not held behind this
    // optimization, and any unusually early icon request simply waits in the
    // channel while the one-time native INDEX finishes.
    refresh_icon_dirs(client, state);
}

/// Apply one complete panel command after the Channel helper has consumed and
/// acknowledged its bounded MESSAGE Transfer delivery.
fn on_panel_data(client: &mut Client, state: &mut State, index: usize, payload: &[u8]) {
    let text = String::from_utf8_lossy(payload).trim().to_string();
    let (verb, id) = match text.split_once(' ') {
        Some((verb, id)) => (verb, id.trim()),
        None => (text.as_str(), ""),
    };
    match verb {
        "enable" if !id.is_empty() => {
            refresh_installed_if_stale(client, state);
            if let Some(entry) = state.installed.get(id).cloned() {
                let app = state
                    .apps
                    .entry(id.to_string())
                    .or_insert_with(|| App::new(id.to_string(), entry.argv.clone()));
                app.argv = entry.argv;
                app.enabled = true;
                if app.phase == Phase::Stopped {
                    app.phase = Phase::Idle;
                }
                persist(client, state, id);
            }
        }
        "disable" if !id.is_empty() => stop_app(client, state, id),
        "start" if !id.is_empty() => {
            refresh_installed_if_stale(client, state);
            if state.installed.contains_key(id) {
                let _ = launch_once(client, state, id);
            }
        }
        "stop" if !id.is_empty() => halt_app(client, state, id),
        "icons" if !id.is_empty() => {
            let requested: Vec<&str> = id
                .split('\n')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .take(MAX_ICON_REQUEST)
                .collect();
            let resolved = resolve_icons(client, state, &requested);
            let Some(conn) = state.conns.get_mut(index) else {
                return;
            };
            for (id, data_url) in resolved {
                if conn.queued.len() >= MAX_QUEUED_ICONS {
                    break;
                }
                conn.queued.push(icon_json(&id, data_url.as_deref()));
            }
        }
        "forget" if !id.is_empty() => forget_app(client, state, id),
        "resync" => {
            let _ = refresh_installed(client, state);
            let payload = state_json(state, true);
            let current = state_json(state, false);
            let mut conns = core::mem::take(&mut state.conns);
            if let Some(conn) = conns.get_mut(index)
                && send_json(client, conn, &payload)
            {
                conn.last_sent = current;
                conn.catalog_revision = state.catalog_revision;
            }
            conns.retain(|conn| !conn.closed);
            state.conns = conns;
            return;
        }
        _ => return,
    }
    publish(client, state);
}

fn on_panel_event(
    client: &mut Client,
    state: &mut State,
    index: usize,
    event: ChannelEvent,
) -> Result<bool, Error> {
    let handle = state.conns[index].channel.handle();
    let mut event = Some(event);
    let mut payloads = Vec::new();
    let mut closed = false;
    while let Some(current) = event.take() {
        match current {
            ChannelEvent::Acknowledged { .. } => {}
            ChannelEvent::Closed(_) => {
                closed = true;
                break;
            }
            ChannelEvent::Data(delivery) => {
                payloads.push(state.conns[index].channel.consume(client, delivery)?);
                event = state.conns[index].channel.poll_event()?;
            }
        }
    }
    for payload in payloads {
        let Some(current) = state
            .conns
            .iter()
            .position(|conn| conn.channel.handle() == handle)
        else {
            return Ok(true);
        };
        on_panel_data(client, state, current, &payload);
    }
    if closed
        && let Some(current) = state
            .conns
            .iter()
            .position(|conn| conn.channel.handle() == handle)
    {
        state.conns.remove(current);
    }
    Ok(true)
}

/// Stop an application now, leaving its intent alone.
///
/// The supervisor restarts what it believes should be running, so "stopped"
/// has to be a phase it respects rather than a signal it undoes.
fn halt_app(client: &mut Client, state: &mut State, id: &str) {
    let Some(app) = state.apps.get_mut(id) else {
        return;
    };
    app.phase = Phase::Stopped;
    app.next_attempt_ns = None;
    app.wayland_display = None;
    app.started_at_ns = None;
    let Some(process_handle) = app.process_handle.take() else {
        return;
    };
    terminate(client, state, process_handle);
}

/// Ask the server to end one child.
fn terminate(client: &mut Client, state: &mut State, process_handle: u64) {
    if let Some(mut process) = state.processes.remove(&process_handle) {
        let _ = process.control(client, wire::process::ControlAction::Terminate, 0);
    }
    if let Some(app_handle) = state.process_app_endpoints.remove(&process_handle) {
        let _ = client.release_app_endpoint(app_handle);
    }
}

/// Stop one application and record that it should stay stopped.
fn stop_app(client: &mut Client, state: &mut State, id: &str) {
    let Some(app) = state.apps.get_mut(id) else {
        return;
    };
    app.enabled = false;
    // The exit will arrive and clear the rest too, but a status read in
    // between must not name a socket nothing is listening on.
    halt_app(client, state, id);
    persist(client, state, id);
}

/// Stop one application and drop it from the managed set entirely.
///
/// Disabling keeps the row: an application that just failed is worth being
/// able to look at, and its failure count is the only record of that. But a
/// row that will never be wanted again is noise the operator cannot clear, so
/// this deletes the intent rather than writing "off" over it. What is left is
/// an installed application like any other, which the catalog already offers.
fn forget_app(client: &mut Client, state: &mut State, id: &str) {
    if !state.apps.contains_key(id) {
        return;
    }
    halt_app(client, state, id);
    state.apps.remove(id);
    if let Some(namespace) = state.kv.as_mut() {
        let _ = namespace.delete(client, id.as_bytes(), wire::kv::Precondition::Any, true);
    }
}

/// Route one typed native Event to the exact resource that owns it.
fn route_frame(
    client: &mut Client,
    state: &mut State,
    process_watch: &mut ProcessWatch,
    surface_watch: &mut SurfaceWatch,
    frame: &wire::Frame,
) -> Result<bool, Error> {
    let accepted = match state.data_listener.as_mut() {
        Some(listener) => listener.offer_frame(client, frame)?,
        None => None,
    };
    if let Some(event) = accepted {
        match event {
            ListenerEvent::Accepted(channel) => accept_panel(client, state, *channel),
            ListenerEvent::Closed(_) => state.data_listener = None,
        }
        return Ok(true);
    }

    if let Some(index) = state
        .conns
        .iter()
        .position(|conn| conn.channel.owns_frame(frame))
    {
        if let Some(event) = state.conns[index].channel.offer_frame(frame)? {
            return on_panel_event(client, state, index, event);
        }
        return Ok(false);
    }

    if let Some(update) = process_watch.offer_frame(client, frame)? {
        let now = client.monotonic_now().raw_nanos();
        let mut changed = false;
        for change in update.changes {
            let (process_handle, exit_code) = match change {
                StateChange::Upsert(record)
                    if record.lifecycle == wire::schema::process::LIFECYCLE_EXITED as u8 =>
                {
                    (
                        record.process_handle,
                        record.exit.map_or(-1, |exit| exit.code),
                    )
                }
                StateChange::Remove(process_handle) => (process_handle, -1),
                StateChange::Upsert(_) => continue,
            };
            if state
                .transient_process_apps
                .remove(&process_handle)
                .is_some()
            {
                state.processes.remove(&process_handle);
                if let Some(app_handle) = state.process_app_endpoints.remove(&process_handle) {
                    let _ = client.release_app_endpoint(app_handle);
                }
                // This may be a single-instance handoff. The short launcher
                // exiting says nothing about windows owned by the existing
                // application process.
                continue;
            }
            let Some(app) = state
                .apps
                .values_mut()
                .find(|app| app.process_handle == Some(process_handle))
            else {
                continue;
            };
            let mut random = [0u8; 8];
            let _ = client.random(&mut random);
            app.note_exit(exit_code, now, u64::from_le_bytes(random));
            let id = app.id.clone();
            state.processes.remove(&process_handle);
            if let Some(app_handle) = state.process_app_endpoints.remove(&process_handle) {
                let _ = client.release_app_endpoint(app_handle);
            }
            state.surface_apps.retain(|_, owner| *owner != id);
            persist(client, state, &id);
            changed = true;
        }
        return Ok(changed);
    }

    if let Some(update) = surface_watch.offer_frame(client, frame)? {
        let mut changed = false;
        for change in update.changes {
            match change {
                SurfaceStateChange::Upsert(record) => {
                    if record.application_id.is_empty() {
                        changed |= state.surface_apps.remove(&record.surface_handle).is_some();
                    } else {
                        let application_id = record.application_id;
                        let previous = state
                            .surface_apps
                            .insert(record.surface_handle, application_id.clone());
                        changed |= previous.as_deref() != Some(application_id.as_str());
                    }
                }
                SurfaceStateChange::Remove(record) => {
                    changed |= state.surface_apps.remove(&record.surface_handle).is_some();
                }
                SurfaceStateChange::Patch(_) => {}
            }
        }
        return Ok(changed);
    }

    for index in 0..state.catalog_watches.len() {
        let update = {
            let watched = &mut state.catalog_watches[index];
            watched.watch.offer_frame(client, frame)?
        };
        let Some(update) = update else {
            continue;
        };
        // Deltas are settled by the native FS family, so one package-manager
        // transaction costs one catalog reload/publish. Retire this edge
        // subscription before reading: remaining records from the same settled
        // batch are already represented by the coherent reload. The reload
        // rearms the root for the next transaction.
        if catalog_watch_requires_refresh(update.phase) {
            let mut watched = state.catalog_watches.swap_remove(index);
            let _ = watched.watch.close(client);
            let _ = watched.root.close(client);
            refresh_installed(client, state)?;
            return Ok(true);
        }
        return Ok(false);
    }

    let Some(process_handle) = state
        .processes
        .iter()
        .find_map(|(handle, process)| process.owns_frame(frame).then_some(*handle))
    else {
        return Ok(false);
    };
    let process = state
        .processes
        .get_mut(&process_handle)
        .ok_or(Error::Invalid("native Process disappeared"))?;
    if let Some(event) = process.offer_frame(frame)?
        && let ProcessEvent::Output(delivery) = event
    {
        let application_id = state
            .transient_process_apps
            .get(&process_handle)
            .map(|(id, _)| id.as_str())
            .or_else(|| {
                state
                    .apps
                    .values()
                    .find(|app| app.process_handle == Some(process_handle))
                    .map(|app| app.id.as_str())
            })
            .map(str::to_owned);
        if let Some(application_id) = application_id
            && let Some(app) = state.apps.get_mut(&application_id)
        {
            app.note_output(delivery.data(), APP_DIAGNOSTIC_BYTES);
        }
        // The supervisor does not consume child output, but returning credit
        // is mandatory: the server bounds every Process stream.
        process.discard(client, delivery)?;
    }
    Ok(false)
}

fn catalog_watch_requires_refresh(phase: wire::state::Phase) -> bool {
    phase == wire::state::Phase::Delta
}

/// Start whatever is enabled and due.
fn reconcile(client: &mut Client, state: &mut State) {
    let now = client.monotonic_now().raw_nanos();
    let due: Vec<String> = state
        .apps
        .values()
        .filter(|app| app.attempt_due(now))
        .map(|app| app.id.clone())
        .collect();
    for id in due {
        if let Err(error) = start(client, state, &id) {
            let _ = error;
            // Treat a failed launch as a failed run so it backs off rather
            // than spinning on every wake-up.
            let mut random = [0u8; 8];
            let _ = client.random(&mut random);
            if let Some(app) = state.apps.get_mut(&id) {
                app.note_exit(-1, now, u64::from_le_bytes(random));
            }
        }
    }
}

/// Launch one new instance/window without changing supervision intent.
///
/// This remains available while the same application is already supervised:
/// opening another terminal or browser window is the ordinary meaning of an
/// application picker. When an instance already has surfaces, reuse its
/// Wayland display (or the session display when it was started elsewhere).
/// Chromium refuses its single-instance handoff when a second invocation is
/// pointed at a newly minted display, which made Brave work from a terminal
/// while the picker did nothing.
fn launch_once(client: &mut Client, state: &mut State, id: &str) -> Result<(), Error> {
    let argv = state
        .installed
        .get(id)
        .map(|entry| entry.argv.clone())
        .ok_or(Error::Invalid("application disappeared before launch"))?;
    if argv.is_empty() {
        return Ok(());
    }
    if let Some(app) = state.apps.get_mut(id) {
        app.last_output.clear();
    }

    let has_surfaces = state
        .surface_apps
        .values()
        .any(|application_id| application_id == id);
    let mut existing_display = state
        .apps
        .get(id)
        .and_then(|app| {
            (app.phase == Phase::Running)
                .then(|| app.wayland_display.clone())
                .flatten()
        })
        .or_else(|| {
            state
                .transient_process_apps
                .values()
                .find_map(|(application_id, display)| {
                    (application_id == id).then(|| display.clone()).flatten()
                })
        });
    let mut endpoint_handle = None;
    let environment = if let Some(display) = existing_display.clone() {
        vec![wire::process::EnvEntry {
            key: b"WAYLAND_DISPLAY".to_vec(),
            value: display.into_bytes(),
        }]
    } else if has_surfaces {
        // The application was started outside this supervisor. Session
        // inheritance carries its generic display and runtime directory.
        Vec::new()
    } else {
        let endpoint = client.create_app_endpoint(id.to_string())?;
        endpoint_handle = Some(endpoint.app_handle);
        existing_display = endpoint
            .environment
            .iter()
            .find(|entry| entry.key == b"WAYLAND_DISPLAY")
            .map(|entry| String::from_utf8_lossy(&entry.value).into_owned());
        endpoint
            .environment
            .into_iter()
            .map(|entry| wire::process::EnvEntry {
                key: entry.key,
                value: entry.value,
            })
            .collect()
    };
    let extensions = match endpoint_handle {
        Some(app_handle) => wire::Extensions(vec![wire::Extension {
            tag: wire::schema::process::SPAWN_SURFACE_APP_EXTENSION as u16,
            required: true,
            value: app_handle.to_le_bytes().to_vec(),
        }]),
        None => wire::Extensions::default(),
    };
    let spawned = client.spawn_process_with_window(
        (wire::schema::process::SPAWN_DETACHABLE | wire::schema::process::SPAWN_MERGE_STDERR)
            as u16,
        wire::process::EnvironmentKind::Session,
        wire::process::Cwd::ServerDefault,
        argv.into_iter().map(String::into_bytes).collect(),
        environment,
        extensions,
        APP_PROCESS_STREAM_WINDOW,
    );
    let process = match spawned {
        Ok(process) => process,
        Err(error) => {
            if let Some(app_handle) = endpoint_handle {
                let _ = client.release_app_endpoint(app_handle);
            }
            return Err(Error::Process(error));
        }
    };
    let process_handle = process.handle();
    state.processes.insert(process_handle, process);
    state
        .transient_process_apps
        .insert(process_handle, (id.to_string(), existing_display));
    if let Some(app_handle) = endpoint_handle {
        state
            .process_app_endpoints
            .insert(process_handle, app_handle);
    }
    Ok(())
}

/// Mint a native Surface application endpoint and spawn on its exact
/// environment using the opaque `app_handle` extension.
fn start(client: &mut Client, state: &mut State, id: &str) -> Result<(), Error> {
    let argv = match state.apps.get(id) {
        Some(app) => app.argv.clone(),
        None => return Ok(()),
    };
    if argv.is_empty() {
        return Ok(());
    }
    let endpoint = client.create_app_endpoint(id.to_string())?;
    let display = endpoint
        .environment
        .iter()
        .find(|entry| entry.key == b"WAYLAND_DISPLAY")
        .map(|entry| String::from_utf8_lossy(&entry.value).into_owned());
    let environment = endpoint
        .environment
        .iter()
        .map(|entry| wire::process::EnvEntry {
            key: entry.key.clone(),
            value: entry.value.clone(),
        })
        .collect();
    let extensions = wire::Extensions(vec![wire::Extension {
        tag: wire::schema::process::SPAWN_SURFACE_APP_EXTENSION as u16,
        required: true,
        value: endpoint.app_handle.to_le_bytes().to_vec(),
    }]);
    let spawned = client.spawn_process_with_window(
        (wire::schema::process::SPAWN_DETACHABLE | wire::schema::process::SPAWN_MERGE_STDERR)
            as u16,
        wire::process::EnvironmentKind::Session,
        wire::process::Cwd::ServerDefault,
        argv.into_iter().map(String::into_bytes).collect(),
        environment,
        extensions,
        APP_PROCESS_STREAM_WINDOW,
    );
    let process = match spawned {
        Ok(process) => process,
        Err(error) => {
            // No child can use the socket after a refused spawn.
            let _ = client.release_app_endpoint(endpoint.app_handle);
            return Err(Error::Process(error));
        }
    };
    let process_handle = process.handle();
    state.processes.insert(process_handle, process);
    state
        .process_app_endpoints
        .insert(process_handle, endpoint.app_handle);

    let now = client.monotonic_now().raw_nanos();
    if let Some(app) = state.apps.get_mut(id) {
        app.note_started(process_handle, display, now);
    }
    persist(client, state, id);
    Ok(())
}

/// Reload intent from kv and re-adopt anything still running.
fn restore(client: &mut Client, state: &mut State) -> Result<(), Error> {
    refresh_installed(client, state)?;
    let stored = read_intent(client, state)?;
    for (id, intent) in &stored {
        // An application that has since been uninstalled keeps its row: the
        // intent is the operator's, and losing it silently because a package
        // was upgraded out from under the session is worse than a row whose
        // argv is empty until the package comes back.
        let argv = state
            .installed
            .get(id)
            .map(|entry| entry.argv.clone())
            .unwrap_or_default();
        let mut app = App::new(id.clone(), argv);
        app.enabled = intent.enabled;
        app.phase = if app.enabled {
            Phase::Idle
        } else {
            Phase::Stopped
        };
        state.apps.insert(id.clone(), app);
    }
    adopt(client, state, &stored)?;
    Ok(())
}

/// Read every persisted intent under [`KV_PREFIX`] in one exchange.
///
/// A subscription rather than a fetch per application: the previous shape cost
/// one blocking round trip for each installed `.desktop` file — hundreds on an
/// ordinary desktop — and could only ever find intent for applications that
/// were still installed, because the catalog was what it iterated.
fn read_intent(client: &mut Client, state: &mut State) -> Result<BTreeMap<String, Intent>, Error> {
    let mut namespace = client.open_kv(KV_PREFIX.as_bytes())?;
    let mut watch = namespace.watch(client, 4096, None)?;
    let mut live: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();
    loop {
        let frame = client.next_event()?;
        let Some(update) = watch.offer_frame(client, &frame)? else {
            return Err(Error::Invalid("unexpected Event during KV snapshot"));
        };
        for change in update.changes {
            match change {
                KvStateChange::Upsert(record) => {
                    live.insert(record.relative_key, record.inline_value);
                }
                KvStateChange::Remove(record) => {
                    live.remove(&record.relative_key);
                }
            }
        }
        if update.phase == wire::state::Phase::SnapshotEnd {
            break;
        }
    }
    watch.close(client)?;

    let mut stored = BTreeMap::new();
    for (key, inline) in live {
        let Ok(id) = String::from_utf8(key.clone()) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let value = match inline {
            Some(value) => Some(value),
            None => namespace.get(client, &key)?.map(|value| value.bytes),
        };
        if let Some(intent) = value.as_deref().and_then(parse_intent) {
            stored.insert(id, intent);
        }
    }
    state.kv = Some(namespace);
    Ok(stored)
}

/// Re-adopt the children this extension's previous attempt left running.
///
/// Native Process `SPAWN_DETACHABLE` keeps a supervised application alive across a
/// restart of the extension, so without this every restart would start a
/// second copy of everything enabled and leave the first orphaned. A recorded
/// handle is trusted only with the exact Core boot ID it was recorded under.
fn adopt(
    client: &mut Client,
    state: &mut State,
    stored: &BTreeMap<String, Intent>,
) -> Result<(), Error> {
    let wanted: BTreeMap<u64, String> = stored
        .iter()
        .filter(|(_, intent)| intent.boot_id == Some(state.boot_id))
        .filter_map(|(id, intent)| intent.process_handle.map(|handle| (handle, id.clone())))
        .collect();
    let now = client.monotonic_now().raw_nanos();
    for (process_handle, id) in wanted {
        let Ok(process) = client.attach_process(process_handle, 0) else {
            continue;
        };
        state.processes.insert(process_handle, process);
        if let Some(app) = state.apps.get_mut(&id) {
            app.note_adopted(process_handle, None, now);
        }
    }
    Ok(())
}

/// Read the catalog again if it has aged past [`CATALOG_TTL`].
///
/// Live filesystem watches are the normal refresh path. The TTL remains a
/// fallback for roots that did not exist when the extension started or whose
/// platform watcher was lost.
fn refresh_installed_if_stale(client: &mut Client, state: &mut State) {
    let now = client.monotonic_now().raw_nanos();
    let fresh = state.installed_at_ns.is_some_and(|read_at| {
        now.saturating_sub(read_at) < CATALOG_TTL.as_nanos() as i64 && !state.installed.is_empty()
    });
    if fresh {
        return;
    }
    let _ = refresh_installed(client, state);
}

fn application_roots(home: &str, dirs: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    core::iter::once(home)
        .chain(dirs.split(':'))
        .filter(|base| !base.is_empty())
        .map(|base| format!("{base}/applications"))
        // Preserve XDG precedence while dropping duplicate roots.
        .filter(|root| seen.insert(root.clone()))
        .collect()
}

/// Keep one native recursive watch on each XDG applications directory that
/// currently exists. Missing roots are harmless and retried by the TTL path.
fn sync_catalog_watches(client: &mut Client, state: &mut State) {
    let wanted: BTreeSet<String> = state.catalog_roots.iter().cloned().collect();
    let mut kept = Vec::with_capacity(state.catalog_watches.len());
    for mut watched in core::mem::take(&mut state.catalog_watches) {
        if wanted.contains(&watched.path) {
            kept.push(watched);
        } else {
            let _ = watched.watch.close(client);
            let _ = watched.root.close(client);
        }
    }
    state.catalog_watches = kept;

    for path in state.catalog_roots.clone() {
        if state
            .catalog_watches
            .iter()
            .any(|watched| watched.path == path)
        {
            continue;
        }
        let mut root = match client.open_fs(
            wire::fs::RootSource::PlatformPath(path.as_bytes().to_vec()),
            0,
        ) {
            Ok(root) => root,
            Err(_) => continue,
        };
        // Desktop entry IDs may derive from nested paths, hence recursive.
        // Hidden entries are not launchable catalog entries and excluding them
        // also shares the same proven watch policy as `yas fs sync`.
        let flags = wire::schema::fs::WATCH_RECURSIVE as u16;
        let watch = match root.watch(client, flags, 100, 0, String::new(), None) {
            Ok(watch) => watch,
            Err(_) => {
                let _ = root.close(client);
                continue;
            }
        };
        state
            .catalog_watches
            .push(CatalogWatch { path, root, watch });
    }
}

/// Read the installed applications: `XDG_DATA_DIRS` from the server's
/// environment, then the `.desktop` files under each.
fn refresh_installed(client: &mut Client, state: &mut State) -> Result<(), Error> {
    let environment = client.get_environment()?;
    let get = |key: &str| {
        environment
            .iter()
            .find(|entry| entry.key == key.as_bytes())
            .and_then(|entry| core::str::from_utf8(&entry.value).ok())
            .map(str::to_string)
    };
    // The spec's defaults, so a session with these unset still finds apps.
    let home = get("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let base = get("HOME").unwrap_or_default();
            format!("{base}/.local/share")
        });
    let dirs = get("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    // The icon path is the data path, so it is settled by the read that already
    // happened rather than by one of its own.
    let (theme_roots, flat_roots) = icon::roots(&home, &get("HOME").unwrap_or_default(), &dirs);
    state.icon_theme_roots = theme_roots;
    state.icon_flat_roots = flat_roots;
    // A package installed mid-session can bring a theme with it, and this read
    // is the moment that becomes visible.
    state.icon_dirs.clear();
    state.icons.clear();
    state.icon_bytes = 0;

    let roots = application_roots(&home, &dirs);
    let mut installed = BTreeMap::new();
    {
        let files = read_desktop_files(client, &roots)
            .ok_or(Error::Invalid("incomplete application catalog read"))?;
        for (path, contents) in files {
            let Some(id) = path
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".desktop"))
            else {
                continue;
            };
            let Some(entry) = desktop_entry::parse(id, &contents) else {
                continue;
            };
            if entry.hidden || entry.terminal {
                continue;
            }
            // Earlier directories win, per the spec's precedence.
            installed.entry(entry.id.clone()).or_insert(entry);
        }
    }
    if installed != state.installed {
        state.catalog_revision = state.catalog_revision.saturating_add(1).max(1);
    }
    state.installed = installed;
    state.catalog_roots = roots;
    // Persisted intent survives uninstall/reinstall. Keep each managed argv in
    // lockstep with the live entry so a newly installed enabled app starts and
    // a removed one is not restarted through stale arguments.
    for (id, app) in &mut state.apps {
        app.argv = state
            .installed
            .get(id)
            .map(|entry| entry.argv.clone())
            .unwrap_or_default();
    }
    state.installed_at_ns = Some(client.monotonic_now().raw_nanos());
    sync_catalog_watches(client, state);
    Ok(())
}

/// Largest `.desktop` file worth reading. The spec's entries are a few hundred
/// bytes; anything at this size is not one, and reading it would only crowd out
/// the rest of the batch.
const MAX_DESKTOP_BYTES: u32 = 64 * 1024;

/// Read every `*.desktop` under a set of directories.
///
/// Two typed operations per root and no child process: native FS INDEX for what
/// is there, then native FS READ for the entries themselves. This used to be a shell loop over
/// a glob with `cat`, because the fs family could only read inside an established
/// sync session — a watched tree, the wrong shape for reading a fixed set of
/// files once at startup. A root that does not exist answers an empty listing,
/// which matters because most of `XDG_DATA_DIRS` is absent on any given machine.
fn read_desktop_files(client: &mut Client, roots: &[String]) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    for root in roots {
        // Root-relative, so a nested `kde4/foo.desktop` keeps its subdirectory.
        let entries: Vec<String> = fs_index(client, root, IndexKind::Files)?
            .into_iter()
            .filter(|rel| rel.ends_with(".desktop"))
            .map(|rel| format!("{root}/{rel}"))
            .collect();
        for batch in entries.chunks(wire::fs::MAX_QUERY_RECORDS) {
            let paths: Vec<&str> = batch.iter().map(String::as_str).collect();
            let records = fs_read(
                client,
                ReadMode::Content,
                MAX_DESKTOP_BYTES,
                &[paths.as_slice()],
            )?;
            for (found, path, body) in records {
                if !found {
                    continue;
                }
                // Lossy on purpose: one entry with a stray byte in it must not
                // discard the file, let alone the catalog.
                out.push((path, String::from_utf8_lossy(&body).into_owned()));
            }
        }
    }
    Some(out)
}

/// Read a batch of paths in one round trip (`docs/design/fs-read.md`).
///
/// Every record comes back in request order with its own status, so a missing or
/// oversized file is an answer about that path rather than a failure of the
/// batch. `None` means the request never got a reply — the connection is going
/// away — which is the only case a caller has to treat as "ask again later".
#[derive(Clone, Copy)]
enum ReadMode {
    FirstStat,
    Stat,
    LinkTarget,
    Content,
}

/// Resolve one absolute symlink through READ_LINK_TARGET.
fn fs_link_target(client: &mut Client, link: &str) -> Option<String> {
    let paths = [link];
    let groups = [paths.as_slice()];
    let (found, _, target) = fs_read(client, ReadMode::LinkTarget, 4096, &groups)?
        .into_iter()
        .next()?;
    found
        .then(|| resolve_symlink_target(link, &target))
        .flatten()
}

/// Turn an absolute or link-relative Unix symlink target into an absolute path.
fn resolve_symlink_target(link: &str, target: &[u8]) -> Option<String> {
    absolute_path(link)?;
    let target = core::str::from_utf8(target).ok()?;
    if target.is_empty() || target.contains('\0') || target.contains('\\') {
        return None;
    }
    let mut components: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        let parent = link.rsplit_once('/').map_or("", |(parent, _)| parent);
        components.extend(parent.split('/').filter(|component| !component.is_empty()));
    }
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    Some(format!("/{}", components.join("/")))
}

fn absolute_path(path: &str) -> Option<wire::fs::Path> {
    if !path.starts_with('/') {
        return None;
    }
    let components = path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(|component| component.as_bytes().to_vec())
        .collect::<Vec<_>>();
    (!components.iter().any(|component| {
        component.as_slice() == b"."
            || component.as_slice() == b".."
            || component.contains(&0)
            || component.contains(&b'\\')
    }))
    .then_some(wire::fs::Path { components })
}

fn fs_read(
    client: &mut Client,
    mode: ReadMode,
    max_bytes: u32,
    groups: &[&[&str]],
) -> Option<Vec<(bool, String, Vec<u8>)>> {
    let mut flattened = Vec::<(usize, String)>::new();
    let mut questions = Vec::new();
    for (group, paths) in groups.iter().enumerate() {
        for path in *paths {
            let Some(path_value) = absolute_path(path) else {
                continue;
            };
            flattened.push((group, (*path).to_string()));
            questions.push(wire::fs::ReadQuestion {
                kind: match mode {
                    ReadMode::Content => wire::schema::fs::READ_CONTENT as u16,
                    ReadMode::LinkTarget => wire::schema::fs::READ_LINK_TARGET as u16,
                    ReadMode::FirstStat | ReadMode::Stat => wire::schema::fs::READ_STAT as u16,
                },
                flags: 0,
                path: path_value,
            });
        }
    }
    if questions.is_empty() || questions.len() > wire::fs::MAX_QUERY_RECORDS {
        return Some(Vec::new());
    }
    let mut root = client
        .open_fs(wire::fs::RootSource::PlatformPath(b"/".to_vec()), 0)
        .ok()?;
    // CLOSE even when READ fails. Icon probes are allowed to fail and retry;
    // leaking their root handle eventually exhausted this extension's FS-root
    // allowance, after which the next catalog refresh looked like every XDG
    // applications directory had disappeared.
    let page = root.read(client, questions);
    let _ = root.close(client);
    let page = page.ok()?;

    let mut answers = vec![None; flattened.len()];
    for record in page.records {
        let wire::fs::QueryRecord::Read(record) = record else {
            continue;
        };
        let index = usize::from(record.question_index);
        let Some((_, path)) = flattened.get(index) else {
            continue;
        };
        let found = record.status == wire::core::Status::Ok.code()
            && record.content.len() <= max_bytes as usize;
        answers[index] = Some((
            found,
            path.clone(),
            if found { record.content } else { Vec::new() },
        ));
    }

    match mode {
        ReadMode::FirstStat => {
            let mut grouped = Vec::with_capacity(groups.len());
            for (group, paths) in groups.iter().enumerate() {
                let first = flattened
                    .iter()
                    .enumerate()
                    .filter(|(_, (candidate_group, _))| *candidate_group == group)
                    .find_map(|(index, _)| answers[index].clone().filter(|answer| answer.0))
                    .or_else(|| {
                        paths
                            .first()
                            .map(|path| (false, (*path).to_string(), Vec::new()))
                    });
                if let Some(answer) = first {
                    grouped.push(answer);
                }
            }
            Some(grouped)
        }
        ReadMode::Stat | ReadMode::LinkTarget | ReadMode::Content => Some(
            flattened
                .into_iter()
                .enumerate()
                .map(|(index, (_, path))| {
                    answers[index].clone().unwrap_or((false, path, Vec::new()))
                })
                .collect(),
        ),
    }
}

/// Everything under `root`, root-relative: its files, or its directories.
///
/// The walk is the server's native FS INDEX, which is what makes a directory
/// listing a message rather than a glob. A truncated answer is used as far as it
/// goes: for both callers here that means some applications or some themes, not
/// a wrong answer.
#[derive(Clone, Copy)]
enum IndexKind {
    Files,
    Directories,
}

fn fs_index(client: &mut Client, root_path: &str, kind: IndexKind) -> Option<Vec<String>> {
    const MAX_INDEX_PATHS: usize = 65_536;
    let mut root = match client.open_fs(
        wire::fs::RootSource::PlatformPath(root_path.as_bytes().to_vec()),
        0,
    ) {
        Ok(root) => root,
        Err(_) => {
            // Absent XDG roots are normal. Distinguish that from a transient
            // OPEN failure (notably resource exhaustion), or a failed refresh
            // would erase a previously valid application catalog.
            let paths = [root_path];
            let groups = [paths.as_slice()];
            return match fs_read(client, ReadMode::Stat, u32::MAX, &groups) {
                Some(records) if records.first().is_some_and(|record| !record.0) => {
                    Some(Vec::new())
                }
                _ => None,
            };
        }
    };
    let flags = match kind {
        IndexKind::Files => wire::schema::fs::INDEX_INCLUDE_FILES as u16,
        IndexKind::Directories => wire::schema::fs::INDEX_INCLUDE_DIRECTORIES as u16,
    };
    let mut cursor = Vec::new();
    let mut out = Vec::new();
    let complete = loop {
        let page = match root.index(
            client,
            flags,
            wire::fs::MAX_QUERY_RECORDS.min(u16::MAX as usize) as u16,
            cursor.clone(),
        ) {
            Ok(page) => page,
            Err(_) => break false,
        };
        for record in page.records {
            let wire::fs::QueryRecord::Path(record) = record else {
                continue;
            };
            let path = record
                .path
                .components
                .iter()
                .map(|component| String::from_utf8_lossy(component))
                .collect::<Vec<_>>()
                .join("/");
            if !path.is_empty() {
                out.push(path);
            }
            if out.len() >= MAX_INDEX_PATHS {
                break;
            }
        }
        if out.len() >= MAX_INDEX_PATHS || page.next_cursor.is_empty() || page.next_cursor == cursor
        {
            break true;
        }
        cursor = page.next_cursor;
    };
    let _ = root.close(client);
    complete.then_some(out)
}

/// Persist one application's intent.
///
/// The record is `<enabled> [<boot-id-hex> <process-handle>]`: the operator's
/// choice, plus the exact boot-scoped native handle a restarted extension needs
/// to re-adopt the child instead of spawning a second one. A bare `0`/`1`
/// remains valid intent without a live child.
fn persist(client: &mut Client, state: &mut State, id: &str) {
    let Some(app) = state.apps.get(id) else {
        return;
    };
    let mut value = String::from(if app.enabled { "1" } else { "0" });
    if let Some(process_handle) = app.process_handle {
        value.push(' ');
        for byte in state.boot_id {
            value.push_str(&format!("{byte:02x}"));
        }
        value.push_str(&format!(" {process_handle}"));
    }
    if let Some(namespace) = state.kv.as_mut() {
        let _ = namespace.put(
            client,
            id.as_bytes(),
            value.as_bytes(),
            wire::kv::Precondition::Any,
            true,
        );
    }
}

/// Read one persisted intent record back.
fn parse_intent(value: &[u8]) -> Option<Intent> {
    let text = core::str::from_utf8(value).ok()?;
    let mut fields = text.split_whitespace();
    let enabled = match fields.next()? {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    // The two halves are written together and are meaningless apart, so a
    // record carrying only one of them carries neither. A tail that does not
    // parse costs the reference and nothing else: the enabled bit in front of
    // it is the operator's choice, and dropping that would silently
    // un-autostart the application.
    let (boot_id, process_handle) = match (fields.next(), fields.next()) {
        (Some(encoded_boot), Some(encoded_handle)) if encoded_boot.len() == 32 => {
            let mut decoded = [0u8; 16];
            let valid = decoded
                .iter_mut()
                .zip(encoded_boot.as_bytes().as_chunks::<2>().0)
                .all(|(slot, pair)| {
                    core::str::from_utf8(pair)
                        .ok()
                        .and_then(|value| u8::from_str_radix(value, 16).ok())
                        .is_some_and(|value| {
                            *slot = value;
                            true
                        })
                });
            match encoded_handle.parse::<u64>() {
                Ok(handle) if valid && handle != 0 => (Some(decoded), Some(handle)),
                _ => (None, None),
            }
        }
        _ => (None, None),
    };
    Some(Intent {
        enabled,
        boot_id,
        process_handle,
    })
}

fn serve(
    client: &mut Client,
    state: &mut State,
    mut invocation: yas_guest::command::Invocation,
) -> Result<(), Error> {
    let args = invocation.request().args.clone();
    let (command, target) = (
        args.first().map(String::as_str).unwrap_or("list"),
        args.get(1).map(String::as_str),
    );
    let mut out = String::new();
    let mut code = 0;

    match (command, target) {
        ("list", _) => {
            refresh_installed_if_stale(client, state);
            out.push_str("APP\tENABLED\tPHASE\tNAME\n");
            // A managed application that is no longer installed still has a
            // row: it is the only way to see that something enabled has gone
            // missing, and the only way to `forget` it.
            let ids: BTreeSet<&String> = state.installed.keys().chain(state.apps.keys()).collect();
            for id in ids {
                let app = state.apps.get(id);
                let enabled = app.is_some_and(|app| app.enabled);
                let phase = match app.map(|app| app.phase) {
                    Some(Phase::Running) => "running",
                    Some(Phase::Backoff) => "backoff",
                    Some(Phase::Idle) => "starting",
                    _ => "-",
                };
                let name = state
                    .installed
                    .get(id)
                    .map(|entry| entry.name.as_str())
                    .unwrap_or("(not installed)");
                out.push_str(&format!(
                    "{id}\t{}\t{phase}\t{name}\n",
                    if enabled { "yes" } else { "no" },
                ));
            }
        }
        ("enable", Some(id)) => {
            refresh_installed_if_stale(client, state);
            match state.installed.get(id) {
                Some(entry) => {
                    let app = state
                        .apps
                        .entry(id.to_string())
                        .or_insert_with(|| App::new(id.to_string(), entry.argv.clone()));
                    app.argv = entry.argv.clone();
                    app.enabled = true;
                    if app.phase == Phase::Stopped {
                        app.phase = Phase::Idle;
                    }
                    persist(client, state, id);
                    out.push_str(&format!("enabled {id}\n"));
                }
                None => {
                    out.push_str(&format!("no application called {id}\n"));
                    code = 1;
                }
            }
        }
        ("disable", Some(id)) => {
            if state.apps.contains_key(id) {
                stop_app(client, state, id);
                out.push_str(&format!("disabled {id}\n"));
            } else {
                out.push_str(&format!("{id} was not enabled\n"));
                code = 1;
            }
        }
        // start and stop are this session only. Intent is untouched, so
        // `stop` on an enabled application stays stopped until something asks
        // otherwise -- and the next session start still brings it up.
        ("start", Some(id)) => {
            refresh_installed_if_stale(client, state);
            if !state.installed.contains_key(id) {
                out.push_str(&format!("no application called {id}\n"));
                code = 1;
            } else {
                match launch_once(client, state, id) {
                    Ok(()) => out.push_str(&format!("starting {id}\n")),
                    Err(error) => {
                        out.push_str(&format!("could not start {id}: {error}\n"));
                        code = 1;
                    }
                }
            }
        }
        ("stop", Some(id)) => {
            if state.apps.contains_key(id) {
                halt_app(client, state, id);
                out.push_str(&format!("stopped {id}\n"));
            } else {
                out.push_str(&format!("{id} is not running\n"));
                code = 1;
            }
        }
        // Forgetting is not disabling: the row goes away, and with it the
        // failure history that made keeping a disabled one worth it.
        ("forget", Some(id)) => {
            if state.apps.contains_key(id) {
                forget_app(client, state, id);
                out.push_str(&format!("forgot {id}\n"));
            } else {
                out.push_str(&format!("{id} was not managed\n"));
                code = 1;
            }
        }
        ("status", Some(id)) => match state.apps.get(id) {
            Some(app) => {
                let windows = state
                    .surface_apps
                    .values()
                    .filter(|owner| owner.as_str() == id)
                    .count();
                out.push_str(&format!("app\t{id}\n"));
                out.push_str(&format!(
                    "enabled\t{}\n",
                    if app.enabled { "yes" } else { "no" }
                ));
                out.push_str(&format!(
                    "phase\t{}\n",
                    match app.phase {
                        Phase::Running => "running",
                        Phase::Backoff => "backoff",
                        Phase::Idle => "starting",
                        Phase::Stopped => "stopped",
                    }
                ));
                out.push_str(&format!("failures\t{}\n", app.failures));
                if let Some(exit) = app.last_exit {
                    out.push_str(&format!("last-exit\t{exit}\n"));
                }
                if let Some(display) = &app.wayland_display {
                    out.push_str(&format!("socket\t{display}\n"));
                }
                if !app.last_output.is_empty() {
                    out.push_str("output\n");
                    out.push_str(&String::from_utf8_lossy(&app.last_output));
                    if !app.last_output.ends_with(b"\n") {
                        out.push('\n');
                    }
                }
                // Counted from stamped identity, not from a self-asserted
                // app_id — the whole reason this number can be trusted.
                out.push_str(&format!("windows\t{windows}\n"));
            }
            None => {
                out.push_str(&format!("{id} is not managed\n"));
                code = 1;
            }
        },
        (other, None)
            if matches!(
                other,
                "enable" | "disable" | "start" | "stop" | "forget" | "status"
            ) =>
        {
            out.push_str(&format!("{other} needs an application name\n"));
            code = 2;
        }
        (other, _) => {
            out.push_str(&format!("unknown command {other}\n"));
            code = 2;
        }
    }

    invocation.stdout(client, out.as_bytes())?;
    invocation.exit(client, code, "")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records written before the native Process handle existed are still out
    /// there, and reading one as "not enabled" would silently un-autostart
    /// every application on the first upgrade.
    #[test]
    fn a_bare_enabled_bit_still_parses() {
        let intent = parse_intent(b"1").expect("parses");
        assert!(intent.enabled);
        assert_eq!(intent.process_handle, None);
        assert!(!parse_intent(b"0").expect("parses").enabled);
        assert!(parse_intent(b"").is_none());
        assert!(parse_intent(b"yes").is_none());
    }

    #[test]
    fn a_full_record_round_trips() {
        let intent = parse_intent(b"1 000102030405060708090a0b0c0d0e0f 12345").expect("parses");
        assert!(intent.enabled);
        assert_eq!(
            intent.boot_id,
            Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
        );
        assert_eq!(intent.process_handle, Some(12345));
    }

    #[test]
    fn the_full_opaque_u64_handle_is_preserved() {
        let intent = parse_intent(b"1 fedcba98765432100123456789abcdef 18446744073709551615")
            .expect("parses");
        assert_eq!(intent.process_handle, Some(u64::MAX));
        assert_eq!(
            intent.boot_id,
            Some([
                0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ])
        );
    }

    /// The boot ID and handle are only meaningful together: a handle without
    /// the boot it came from could be matched against a different server's
    /// process of the same number.
    #[test]
    fn half_a_handle_is_no_handle() {
        let intent = parse_intent(b"1 000102030405060708090a0b0c0d0e0f").expect("parses");
        assert!(intent.enabled, "the intent itself still counts");
        assert_eq!(intent.process_handle, None);
        assert_eq!(intent.boot_id, None);

        // Unparseable halves are dropped rather than guessed at.
        let intent = parse_intent(b"1 zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz 12345").expect("parses");
        assert_eq!(intent.process_handle, None);
        let intent = parse_intent(b"1 000102030405060708090a0b0c0d0e0f many").expect("parses");
        assert_eq!(intent.process_handle, None);

        // Retired generation/reference records preserve intent but cannot be
        // reinterpreted as a native boot ID or handle.
        let intent = parse_intent(b"1 7 12345").expect("parses");
        assert!(intent.enabled);
        assert_eq!(intent.boot_id, None);
        assert_eq!(intent.process_handle, None);
    }

    /// The bug that put a permanent gap in a scrolled list: the icon cache is
    /// dropped wholesale, and it can happen part way through the batch that is
    /// filling it. Whatever the batch resolved before the drop must still be
    /// answered, because "no artwork" is a final answer to the panel.
    #[test]
    fn a_batch_answers_what_it_resolved_even_if_the_cache_was_dropped() {
        // What the batch resolved, held apart from the cache the drop emptied.
        let resolved = BTreeMap::from([
            ("early".to_string(), Some("/icons/early.png".to_string())),
            ("late".to_string(), Some("/icons/late.png".to_string())),
        ]);
        let keys = vec![
            ("Portal 2.desktop".to_string(), Some("early".to_string())),
            ("Shatter.desktop".to_string(), Some("late".to_string())),
        ];
        assert_eq!(
            batch_answers(keys, &resolved),
            vec![
                (
                    "Portal 2.desktop".to_string(),
                    Some("/icons/early.png".to_string())
                ),
                (
                    "Shatter.desktop".to_string(),
                    Some("/icons/late.png".to_string())
                ),
            ]
        );
    }

    /// "No artwork" and "ask me later" are different answers, and only one of
    /// them is safe to give for a read that never happened: the panel treats a
    /// null as final, so a failed child must leave the id unanswered.
    #[test]
    fn an_unresolved_key_is_omitted_but_a_missing_icon_field_is_answered() {
        let resolved = BTreeMap::from([("found".to_string(), None)]);
        let answers = batch_answers(
            vec![
                ("no-icon-key.desktop".to_string(), None),
                ("looked-for.desktop".to_string(), Some("found".to_string())),
                (
                    "child-failed.desktop".to_string(),
                    Some("never".to_string()),
                ),
            ],
            &resolved,
        );
        assert_eq!(
            answers,
            vec![
                // Nothing to look for: final.
                ("no-icon-key.desktop".to_string(), None),
                // Looked for and not found: also final.
                ("looked-for.desktop".to_string(), None),
            ],
            "the id whose lookup never ran is left for the panel to re-ask"
        );
    }

    /// A disabled application can still have a live child -- `disable` stops
    /// it, but the exit arrives later -- so the two halves are independent.
    #[test]
    fn a_disabled_record_can_still_carry_a_handle() {
        let intent = parse_intent(b"0 000102030405060708090a0b0c0d0e0f 99").expect("parses");
        assert!(!intent.enabled);
        assert_eq!(intent.process_handle, Some(99));
    }

    #[test]
    fn fs_paths_are_absolute_and_cannot_escape_the_root() {
        assert_eq!(
            absolute_path("/usr/share/applications/example.desktop"),
            Some(wire::fs::Path {
                components: vec![
                    b"usr".to_vec(),
                    b"share".to_vec(),
                    b"applications".to_vec(),
                    b"example.desktop".to_vec(),
                ],
            })
        );
        assert!(absolute_path("relative/path").is_none());
        assert!(absolute_path("/usr/../etc/passwd").is_none());
        assert!(absolute_path("/usr/./share").is_none());
    }

    #[test]
    fn symlink_targets_are_resolved_from_the_links_parent() {
        assert_eq!(
            resolve_symlink_target(
                "/etc/profiles/me/share/icons/hicolor",
                b"/nix/store/profile/share/icons/hicolor",
            )
            .as_deref(),
            Some("/nix/store/profile/share/icons/hicolor")
        );
        assert_eq!(
            resolve_symlink_target("/icons/theme", b"../themes/theme").as_deref(),
            Some("/themes/theme")
        );
        assert!(resolve_symlink_target("relative/link", b"/target").is_none());
        assert!(resolve_symlink_target("/link", b"../../escape").is_none());
        assert!(resolve_symlink_target("/link", b"bad\\target").is_none());
    }

    #[test]
    fn xdg_application_roots_keep_precedence_and_drop_duplicates() {
        assert_eq!(
            application_roots(
                "/home/me/.local/share",
                "/opt/share:/usr/share:/opt/share::/usr/local/share",
            ),
            vec![
                "/home/me/.local/share/applications",
                "/opt/share/applications",
                "/usr/share/applications",
                "/usr/local/share/applications",
            ]
        );
    }

    #[test]
    fn catalog_watches_refresh_only_on_settled_delta() {
        assert!(!catalog_watch_requires_refresh(
            wire::state::Phase::SnapshotBegin
        ));
        assert!(!catalog_watch_requires_refresh(
            wire::state::Phase::SnapshotRecords
        ));
        assert!(!catalog_watch_requires_refresh(
            wire::state::Phase::SnapshotEnd
        ));
        assert!(catalog_watch_requires_refresh(wire::state::Phase::Delta));
        assert!(!catalog_watch_requires_refresh(wire::state::Phase::Reset));
    }
}
