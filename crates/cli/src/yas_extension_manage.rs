//! Inline extension manager. Rows cycle through installs, updates, and removals.

use std::collections::BTreeSet;
use std::io::{IsTerminal, stdout};

use clap::Args;
use crossterm::cursor::{MoveTo, Show};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use serde::Deserialize;
use url::Url;

use super::*;

const DEFAULT_REGISTRY: &str = "https://yas.run/ext";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Args, Clone, Debug)]
pub(crate) struct ManageArgs {
    /// Registry directory containing manifest.json and extension modules
    #[arg(long, default_value = DEFAULT_REGISTRY)]
    pub from: String,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    version: String,
    extensions: Vec<ManifestEntry>,
}
#[derive(Deserialize)]
struct ManifestEntry {
    name: String,
    #[serde(default)]
    description: String,
    file: Option<String>,
    blake3: String,
    bytes: Option<u64>,
}
#[derive(Clone, Debug)]
struct Offer {
    name: String,
    description: String,
    url: String,
    hash: [u8; 32],
    bytes: Option<u64>,
}
#[derive(Debug)]
struct Registry {
    url: Url,
    version: String,
    offers: Vec<Offer>,
}

impl Registry {
    async fn fetch(from: &str) -> Result<Self, String> {
        let mut base =
            Url::parse(from).map_err(|error| format!("invalid registry URL: {error}"))?;
        validate_http_url(&base)?;
        base.set_fragment(None);
        base.set_path(&format!("{}/", base.path().trim_end_matches('/')));
        let url = base
            .join("manifest.json")
            .map_err(|error| error.to_string())?;
        let bytes = fetch_http(url.as_str(), MAX_MANIFEST_BYTES).await?;
        Self::parse(base, &bytes).map_err(|error| format!("{url}: {error}"))
    }

    fn parse(base: Url, bytes: &[u8]) -> Result<Self, String> {
        let manifest: Manifest = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid extension manifest: {error}"))?;
        let mut names = BTreeSet::new();
        let mut offers = Vec::new();
        for entry in manifest.extensions {
            validate_name(&entry.name)?;
            if !names.insert(entry.name.clone()) {
                return Err(format!("duplicate extension name: {}", entry.name));
            }
            let hash =
                parse_digest(&entry.blake3).map_err(|error| format!("{}: {error}", entry.name))?;
            let file = entry.file.unwrap_or_else(|| format!("{}.wasm", entry.name));
            let mut url = base
                .join(&file)
                .map_err(|error| format!("{file}: {error}"))?;
            validate_http_url(&url)?;
            url.set_fragment(None);
            url.query_pairs_mut().append_pair("blake3", &hex(&hash));
            let bytes = entry.bytes.filter(|size| *size != 0);
            if bytes.is_some_and(|size| size > wire::MAX_OBJECT_BYTES) {
                return Err(format!("{} exceeds the extension size limit", entry.name));
            }
            offers.push(Offer {
                name: entry.name,
                description: plain_text(&entry.description),
                url: url.into(),
                hash,
                bytes,
            });
        }
        Ok(Self {
            url: base,
            version: plain_text(&manifest.version),
            offers,
        })
    }
}

fn validate_http_url(url: &Url) -> Result<(), String> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if loopback_url(url) => Ok(()),
        "http" => Err(format!(
            "{url}: refusing plain HTTP to a non-loopback host; use https://"
        )),
        _ => Err(format!(
            "{url}: registry URL must use https:// (or loopback HTTP)"
        )),
    }
}
fn plain_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Install,
    Update,
    Uninstall,
}
impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
        }
    }
}

#[derive(Clone)]
struct Row {
    name: String,
    offer: Option<Offer>,
    installed: Option<ExtensionRecord>,
    action: Option<Action>,
}
impl Row {
    fn status(&self) -> &'static str {
        if let Some(action) = self.action {
            return action.label();
        }
        match (&self.offer, &self.installed) {
            (Some(offer), Some(installed)) if offer.hash != installed.content_hash => "update",
            (Some(_), Some(_)) => "up to date",
            (Some(_), None) => "install",
            (None, _) => "not in registry",
        }
    }
    fn offered_action(&self) -> Option<Action> {
        match (&self.offer, &self.installed) {
            (Some(offer), Some(installed)) if offer.hash != installed.content_hash => {
                Some(Action::Update)
            }
            (Some(_), None) => Some(Action::Install),
            _ => None,
        }
    }
    fn cycle_action(&mut self) {
        self.action = match self.action {
            None => self
                .offered_action()
                .or_else(|| self.installed.as_ref().map(|_| Action::Uninstall)),
            Some(Action::Update) => Some(Action::Uninstall),
            Some(_) => None,
        };
    }
}

struct Picker {
    rows: Vec<Row>,
    selected: usize,
    offset: usize,
    list_area: Rect,
    apply_area: Rect,
    cancel_area: Rect,
}
#[derive(PartialEq, Debug)]
enum Decision {
    Continue,
    Apply,
    Cancel,
}

impl Picker {
    fn new(registry: &Registry, installed: &[ExtensionRecord]) -> Self {
        let mut rows = registry
            .offers
            .iter()
            .map(|offer| Row {
                name: offer.name.clone(),
                offer: Some(offer.clone()),
                installed: installed
                    .iter()
                    .find(|record| {
                        record.flags & schema::DEFINITION_PERSISTENT as u16 != 0
                            && record.name == offer.name
                    })
                    .cloned(),
                action: None,
            })
            .collect::<Vec<_>>();
        for record in installed {
            if record.flags & schema::DEFINITION_PERSISTENT as u16 != 0
                && !registry
                    .offers
                    .iter()
                    .any(|offer| offer.name == record.name)
            {
                rows.push(Row {
                    name: plain_text(&record.name),
                    offer: None,
                    installed: Some(record.clone()),
                    action: None,
                });
            }
        }
        rows.sort_by(|a, b| {
            let rank = |row: &Row| match row.status() {
                "update" => 0,
                "install" => 1,
                "up to date" => 2,
                _ => 3,
            };
            rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name))
        });
        Self {
            rows,
            selected: 0,
            offset: 0,
            list_area: Rect::default(),
            apply_area: Rect::default(),
            cancel_area: Rect::default(),
        }
    }
    fn toggle(&mut self) {
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.cycle_action();
        }
    }
    fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
    }
    fn handle(&mut self, event: Event) -> Decision {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Decision::Cancel;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Decision::Cancel,
                    KeyCode::Enter => return Decision::Apply,
                    KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
                    KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
                    KeyCode::PageUp => {
                        self.move_by(-isize::try_from(self.list_area.height).unwrap_or(1))
                    }
                    KeyCode::PageDown => self.move_by(self.list_area.height as isize),
                    KeyCode::Home => self.selected = 0,
                    KeyCode::End => self.selected = self.rows.len().saturating_sub(1),
                    KeyCode::Char(' ') => self.toggle(),
                    KeyCode::Char('a') => {
                        let check = self.rows.iter().any(|row| {
                            row.offered_action().is_some() && row.action != row.offered_action()
                        });
                        for row in &mut self.rows {
                            row.action = if check { row.offered_action() } else { None };
                        }
                    }
                    KeyCode::Char('u') => {
                        for row in &mut self.rows {
                            row.action = row
                                .offered_action()
                                .filter(|action| *action == Action::Update);
                        }
                    }
                    KeyCode::Char('d') | KeyCode::Delete => {
                        if let Some(row) = self.rows.get_mut(self.selected)
                            && row.installed.is_some()
                        {
                            row.action = if row.action == Some(Action::Uninstall) {
                                None
                            } else {
                                Some(Action::Uninstall)
                            };
                        }
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse) => {
                let position = (mouse.column, mouse.row).into();
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if self.apply_area.contains(position) {
                            return Decision::Apply;
                        }
                        if self.cancel_area.contains(position) {
                            return Decision::Cancel;
                        }
                        if self.list_area.contains(position) {
                            let index = self.offset + usize::from(mouse.row - self.list_area.y);
                            if index < self.rows.len() {
                                self.selected = index;
                                self.toggle();
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => self.move_by(-1),
                    MouseEventKind::ScrollDown => self.move_by(1),
                    _ => {}
                }
            }
            _ => {}
        }
        Decision::Continue
    }
    fn draw(&mut self, frame: &mut Frame, registry: &Registry) {
        let area = frame.area();
        self.list_area = Rect::default();
        self.apply_area = Rect::default();
        self.cancel_area = Rect::default();
        let wide = area.width >= 84;
        let chrome = if wide { 3 } else { 4 };
        let desired_height = (self.rows.len().clamp(1, 10) as u16 + chrome).min(area.height);
        let panel = Rect::new(area.x, area.y, area.width, desired_height);
        if panel.height < chrome + 1 {
            frame.render_widget(Paragraph::new("Enlarge terminal. Esc cancels."), area);
            return;
        }
        let line = |y, height| Rect::new(panel.x, panel.y + y, panel.width, height);
        let title = if registry.version.is_empty() {
            format!("Extensions · {}", registry.url)
        } else {
            format!("Extensions {} · {}", registry.version, registry.url)
        };
        frame.render_widget(
            Paragraph::new(title).style(Style::default().add_modifier(Modifier::BOLD)),
            line(0, 1),
        );
        self.list_area = line(1, panel.height - chrome);
        let visible = usize::from(self.list_area.height);
        self.offset = self.offset.min(self.selected);
        if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
        if self.rows.is_empty() {
            frame.render_widget(
                Paragraph::new("No extensions in this registry or on this server."),
                self.list_area,
            );
        }
        for (index, row) in self.rows.iter().enumerate().skip(self.offset).take(visible) {
            let marker = if row.action.is_some() { "[x]" } else { "[ ]" };
            let style = if index == self.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let name = row.name.chars().take(18).collect::<String>();
            let phase = row
                .installed
                .as_ref()
                .map(|record| phase_name(record.phase))
                .unwrap_or("");
            let text = Line::from(vec![
                Span::raw(format!("{marker} {name:<18} ")),
                Span::styled(
                    format!("{:<12}", row.status()),
                    // Reverse video turns foreground colors into background
                    // patches, so selected rows use one uniform style.
                    if index == self.selected {
                        Style::default()
                    } else {
                        Style::default().fg(if row.action == Some(Action::Uninstall) {
                            Color::Red
                        } else if row.action.is_some() || row.offered_action().is_some() {
                            Color::Cyan
                        } else {
                            Color::DarkGray
                        })
                    },
                ),
                Span::raw(format!(" {phase:<10}")),
                Span::raw(if wide {
                    row.offer
                        .as_ref()
                        .map(|offer| offer.description.as_str())
                        .unwrap_or("")
                } else {
                    ""
                }),
            ]);
            frame.render_widget(
                Paragraph::new(text).style(style),
                line(1 + (index - self.offset) as u16, 1),
            );
        }
        if !wide {
            let description = self
                .rows
                .get(self.selected)
                .and_then(|row| row.offer.as_ref())
                .map(|offer| offer.description.as_str())
                .unwrap_or("");
            frame.render_widget(
                Paragraph::new(description).wrap(Wrap { trim: true }),
                line(panel.height - 3, 1),
            );
        }
        frame.render_widget(
            Paragraph::new(
                "↑/↓ move · Space/click action · d uninstall · Enter apply · Esc cancel",
            ),
            line(panel.height - 2, 1),
        );
        let count = self.rows.iter().filter(|row| row.action.is_some()).count();
        let apply = format!("[ Apply {count} ]");
        self.apply_area = Rect::new(
            panel.x,
            panel.bottom() - 1,
            (apply.len() as u16).min(panel.width),
            1,
        );
        let cancel_x = self.apply_area.right().saturating_add(2).min(panel.right());
        self.cancel_area = Rect::new(
            cancel_x,
            panel.bottom() - 1,
            10.min(panel.right() - cancel_x),
            1,
        );
        frame.render_widget(
            Paragraph::new(apply).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            self.apply_area,
        );
        frame.render_widget(Paragraph::new("[ Cancel ]"), self.cancel_area);
        let shortcuts_x = self
            .cancel_area
            .right()
            .saturating_add(2)
            .min(panel.right());
        frame.render_widget(
            Paragraph::new("a installs/updates · u updates"),
            Rect::new(
                shortcuts_x,
                panel.bottom() - 1,
                panel.right() - shortcuts_x,
                1,
            ),
        );
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    alternate: bool,
}
impl Drop for Screen {
    fn drop(&mut self) {
        let top = self.terminal.get_frame().area().y;
        if self.alternate {
            let _ = execute!(stdout(), DisableMouseCapture, Show, LeaveAlternateScreen);
        } else {
            // The shell redraws its prompt on this row. Explicitly erase the
            // rest of the inline viewport so old list and help rows cannot
            // remain visible below that prompt.
            let _ = execute!(
                stdout(),
                DisableMouseCapture,
                MoveTo(0, top),
                Clear(ClearType::FromCursorDown),
                Show
            );
        }
        let _ = disable_raw_mode();
    }
}

async fn choose(
    registry: &Registry,
    installed: &[ExtensionRecord],
) -> Result<Option<Vec<Row>>, String> {
    let mut picker = Picker::new(registry, installed);
    let chrome = crossterm::terminal::size()
        .map(|(width, _)| if width >= 84 { 3 } else { 4 })
        .unwrap_or(4);
    let inline = Terminal::with_options(
        CrosstermBackend::new(stdout()),
        TerminalOptions {
            viewport: Viewport::Inline((picker.rows.len().clamp(1, 10) + chrome) as u16),
        },
    );
    let (terminal, alternate) = match inline {
        Ok(terminal) => (terminal, false),
        Err(_) => {
            execute!(stdout(), EnterAlternateScreen)
                .map_err(|error| format!("cannot enter extension picker: {error}"))?;
            (
                Terminal::new(CrosstermBackend::new(stdout()))
                    .map_err(|error| format!("cannot initialize extension picker: {error}"))?,
                true,
            )
        }
    };
    let mut screen = Screen {
        terminal,
        alternate,
    };
    enable_raw_mode().map_err(|error| format!("cannot enable raw mode: {error}"))?;
    execute!(stdout(), EnableMouseCapture)
        .map_err(|error| format!("cannot enable mouse: {error}"))?;
    let mut events = EventStream::new();
    loop {
        screen
            .terminal
            .draw(|frame| picker.draw(frame, registry))
            .map_err(|error| error.to_string())?;
        let Some(event) = events.next().await else {
            return Ok(None);
        };
        match picker.handle(event.map_err(|error| format!("terminal input failed: {error}"))?) {
            Decision::Cancel => return Ok(None),
            Decision::Apply => {
                return Ok(Some(
                    picker
                        .rows
                        .into_iter()
                        .filter(|row| row.action.is_some())
                        .collect(),
                ));
            }
            Decision::Continue => {}
        }
    }
}

pub(super) async fn run(on: Option<&str>, hub: &str, args: ManageArgs) -> Result<i32, String> {
    if !std::io::stdin().is_terminal() || !stdout().is_terminal() {
        return Err("yas ext manage needs an interactive terminal; use ext run --persist, ext update, or ext disable followed by ext remove in scripts".into());
    }
    let registry = Registry::fetch(&args.from).await?;
    let mut client = NativeClient::connect(on, hub).await?;
    require_lifecycle(&client)?;
    let installed = snapshot(&mut client).await?;
    let Some(rows) = choose(&registry, &installed).await? else {
        println!("Cancelled; no extensions changed.");
        return Ok(0);
    };
    if rows.is_empty() {
        println!("No extensions selected.");
        return Ok(0);
    }
    let mut failures = 0;
    for row in rows {
        println!("{}: {}…", row.name, row.status());
        match apply(&mut client, &row).await {
            Ok(Some(record)) => println!("{}: saved · {}", row.name, phase_name(record.phase)),
            Ok(None) => println!("{}: uninstalled", row.name),
            Err(error) => {
                eprintln!("{}: {error}", row.name);
                failures += 1;
            }
        }
    }
    Ok(i32::from(failures != 0))
}

fn deployment(row: &Row) -> Deploy {
    let current = row.installed.as_ref();
    let offer = row
        .offer
        .as_ref()
        .expect("only offered rows can be selected");
    Deploy {
        operation_id: operation_id(),
        expected_extension_handle: current.map_or(0, |record| record.extension_handle),
        expected_generation: current.map_or(0, |record| record.generation),
        expected_definition_revision: current.map_or(0, |record| record.definition_revision),
        flags: current.map_or(
            (schema::DEFINITION_PERSISTENT
                | schema::DEFINITION_ENABLED
                | schema::DEFINITION_DESIRED_RUNNING
                | schema::DEFINITION_DETACHED) as u16,
            |record| record.flags,
        ),
        runtime: Runtime::Auto,
        restart_policy: current.map_or(wire::RestartPolicy::Always, |record| record.restart_policy),
        name: offer.name.clone(),
        content_hash: offer.hash,
        argv: Vec::new(),
        runtime_limits: current.map_or_else(default_runtime_limits, |record| {
            record.runtime_limits.clone()
        }),
        extensions: if current.is_some() {
            Extensions(vec![yas_wire::codec::Extension {
                tag: schema::DEPLOY_PRESERVE_ARGV_TAG as u16,
                required: true,
                value: Vec::new(),
            }])
        } else {
            Extensions::default()
        },
    }
}
async fn apply(client: &mut NativeClient, row: &Row) -> Result<Option<ExtensionRecord>, String> {
    match row.action {
        Some(Action::Uninstall) => {
            uninstall(
                client,
                row.installed.as_ref().ok_or("extension is not installed")?,
            )
            .await?;
            return Ok(None);
        }
        Some(Action::Install | Action::Update) => {}
        None => return Err("no extension action selected".into()),
    }
    let offer = row
        .offer
        .as_ref()
        .ok_or("extension is not in this registry")?;
    let object = ModuleSource::Url {
        url: offer.url.clone(),
        pin: Some(offer.hash),
    }
    .load()
    .await?;
    if offer
        .bytes
        .is_some_and(|bytes| bytes != object.bytes.len() as u64)
    {
        return Err("module length differs from the manifest".into());
    }
    admit_object(client, &object).await?;
    let identity = deploy(client, deployment(row)).await?;
    let record = wait_for_identity(client, &identity).await?;
    if record.phase == Phase::Blocked {
        return Err(format!("extension is blocked: {:?}", record.last_exit));
    }
    Ok(Some(record))
}

async fn uninstall(client: &mut NativeClient, installed: &ExtensionRecord) -> Result<(), String> {
    let identity = control(client, installed, ControlAction::Disable).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let records = snapshot(client).await?;
        let Ok(record) = find_identity(&records, identity.extension_handle, identity.generation)
        else {
            return Ok(());
        };
        if record.definition_revision > identity.definition_revision {
            return Err(
                "extension changed while uninstalling; reopen ext manage to review it".into(),
            );
        }
        if record.definition_revision == identity.definition_revision
            && matches!(record.phase, Phase::Stopped | Phase::Blocked)
        {
            control(client, record, ControlAction::Remove).await?;
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "extension did not stop; disabled but not removed. Retry ext manage after it stops"
                    .into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_resolves_modules_and_rejects_duplicates() {
        let base = Url::parse("https://example.test/ext/").unwrap();
        let registry = Registry::parse(base.clone(), format!(r#"{{"version":"1","extensions":[{{"name":"doctor","description":"checks\nthings","file":"doctor.js","blake3":"{}","bytes":12}}]}}"#, "ab".repeat(32)).as_bytes()).unwrap();
        assert_eq!(registry.offers[0].hash, [0xab; 32]);
        assert_eq!(registry.offers[0].description, "checks things");
        assert!(registry.offers[0].url.contains("doctor.js?blake3="));
        let duplicate = format!(
            r#"{{"extensions":[{{"name":"x","blake3":"{}"}},{{"name":"x","blake3":"{}"}}]}}"#,
            "00".repeat(32),
            "11".repeat(32)
        );
        assert!(
            Registry::parse(base, duplicate.as_bytes())
                .unwrap_err()
                .contains("duplicate")
        );
    }
    fn installed(name: &str, hash: u8) -> ExtensionRecord {
        ExtensionRecord {
            extension_handle: 1,
            generation: 2,
            definition_revision: 3,
            phase: Phase::Running,
            runtime: Runtime::QuickJs,
            restart_policy: wire::RestartPolicy::Always,
            flags: (schema::DEFINITION_PERSISTENT
                | schema::DEFINITION_ENABLED
                | schema::DEFINITION_DESIRED_RUNNING
                | schema::DEFINITION_DETACHED) as u16,
            attempt: 1,
            last_running_attempt: 1,
            task_id: 1,
            next_start_unix_ms: 0,
            directory_revision: 0,
            content_hash: [hash; 32],
            name: name.into(),
            last_exit: None,
            runtime_limits: default_runtime_limits(),
            extensions: Extensions::default(),
        }
    }

    fn registry() -> Registry {
        Registry {
            url: Url::parse("https://example.test/ext/").unwrap(),
            version: String::new(),
            offers: ["new", "outdated", "current"]
                .into_iter()
                .map(|name| Offer {
                    name: name.into(),
                    description: String::new(),
                    url: format!("https://example.test/ext/{name}.js"),
                    hash: [1; 32],
                    bytes: None,
                })
                .collect(),
        }
    }

    fn picker(registry: &Registry) -> Picker {
        let mut transient = installed("transient", 1);
        transient.flags = 0;
        Picker::new(
            registry,
            &[
                installed("outdated", 2),
                installed("current", 1),
                installed("local", 3),
                transient,
            ],
        )
    }

    fn key(picker: &mut Picker, code: KeyCode) -> Decision {
        picker.handle(Event::Key(crossterm::event::KeyEvent::new(
            code,
            KeyModifiers::NONE,
        )))
    }

    #[test]
    fn picker_cycles_installs_updates_and_uninstalls_without_selecting_by_default() {
        let mut picker = picker(&registry());
        assert_eq!(picker.rows.len(), 4);
        assert!(picker.rows.iter().all(|row| row.action.is_none()));
        for (name, actions) in [
            ("new", vec![Some(Action::Install), None]),
            (
                "outdated",
                vec![Some(Action::Update), Some(Action::Uninstall), None],
            ),
            ("current", vec![Some(Action::Uninstall), None]),
            ("local", vec![Some(Action::Uninstall), None]),
        ] {
            picker.selected = picker.rows.iter().position(|row| row.name == name).unwrap();
            for action in actions {
                assert_eq!(key(&mut picker, KeyCode::Char(' ')), Decision::Continue);
                assert_eq!(picker.rows[picker.selected].action, action, "{name}");
            }
        }
    }

    #[test]
    fn uninstall_shortcut_requires_an_installed_extension_and_toggles() {
        let mut picker = picker(&registry());
        for index in 0..picker.rows.len() {
            picker.selected = index;
            key(&mut picker, KeyCode::Char('d'));
            assert_eq!(
                picker.rows[index].action,
                picker.rows[index]
                    .installed
                    .as_ref()
                    .map(|_| Action::Uninstall)
            );
            key(&mut picker, KeyCode::Delete);
            assert_eq!(picker.rows[index].action, None);
        }
    }

    #[test]
    fn bulk_actions_never_select_uninstalls() {
        let mut picker = picker(&registry());
        key(&mut picker, KeyCode::Char('d'));
        assert_eq!(picker.rows[0].action, Some(Action::Uninstall));
        key(&mut picker, KeyCode::Char('a'));
        assert!(
            picker
                .rows
                .iter()
                .all(|row| row.action == row.offered_action())
        );
        assert_eq!(
            picker
                .rows
                .iter()
                .filter(|row| row.action.is_some())
                .count(),
            2
        );
        key(&mut picker, KeyCode::Char('a'));
        assert!(picker.rows.iter().all(|row| row.action.is_none()));
        key(&mut picker, KeyCode::Char('d'));
        key(&mut picker, KeyCode::Char('u'));
        let selected = picker
            .rows
            .iter()
            .filter(|row| row.action.is_some())
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "outdated");
        assert_eq!(selected[0].action, Some(Action::Update));
    }

    #[test]
    fn mouse_can_select_uninstall_and_cancel_without_applying() {
        let registry = registry();
        let mut picker = picker(&registry);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| picker.draw(frame, &registry))
            .unwrap();
        let row = picker
            .rows
            .iter()
            .position(|row| row.name == "local")
            .unwrap();
        assert_eq!(
            picker.handle(Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: picker.list_area.x,
                row: picker.list_area.y + row as u16,
                modifiers: KeyModifiers::NONE,
            })),
            Decision::Continue
        );
        assert_eq!(picker.rows[row].action, Some(Action::Uninstall));
        terminal
            .draw(|frame| picker.draw(frame, &registry))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("uninstall"));
        assert!(screen.contains("[ Apply 1 ]"));
        assert_eq!(key(&mut picker, KeyCode::Esc), Decision::Cancel);
    }
}
