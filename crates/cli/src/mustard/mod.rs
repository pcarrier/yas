mod channel;
mod model;
mod ui;

use std::collections::HashSet;
use std::io::{IsTerminal, stdout};
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use yas_wire::terminal::ScrollMode;

use self::model::{MusterState, MusterUnit};
use crate::yas_terminal::stream::{
    GridState, InteractiveView, ViewCommand, start_interactive_view_task,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) enum NodeKey {
    Instance(String),
    Standalone,
    Unit(String),
    Terminal { unit: String, pty: u64 },
    Surface { unit: String, id: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeKind {
    Group,
    Unit,
    Terminal,
    Surface,
}

#[derive(Clone, Debug)]
pub(super) struct TreeRow {
    pub key: NodeKey,
    pub parent: Option<NodeKey>,
    pub kind: NodeKind,
    pub depth: usize,
    pub label: String,
    pub phase: Option<String>,
}

enum ChannelUpdate {
    Reset,
    Message(Vec<u8>),
    Status(String),
}

enum SupervisorCommand {
    Line(String),
    Shutdown,
}

struct App {
    pub state: MusterState,
    pub selected: usize,
    pub expanded: HashSet<NodeKey>,
    initialized_groups: HashSet<NodeKey>,
    pub connection_status: String,
    pub notice: String,
    pub event_height: u16,
    resizing_events: bool,
    pub help_open: bool,
    pub terminal_focus: bool,
    pub terminal_handle: Option<u64>,
    pub terminal_grid: Option<GridState>,
    pub terminal_error: Option<String>,
    pub terminal_exit: Option<i32>,
    terminal_size: (u16, u16),
    view: Option<InteractiveView>,
    commands: mpsc::Sender<SupervisorCommand>,
    on: Option<String>,
    hub: String,
}

impl App {
    fn new(on: Option<String>, hub: String, commands: mpsc::Sender<SupervisorCommand>) -> Self {
        Self {
            state: MusterState::default(),
            selected: 0,
            expanded: HashSet::new(),
            initialized_groups: HashSet::new(),
            connection_status: "connecting".into(),
            notice: String::new(),
            event_height: ui::DEFAULT_EVENT_HEIGHT,
            resizing_events: false,
            help_open: false,
            terminal_focus: false,
            terminal_handle: None,
            terminal_grid: None,
            terminal_error: None,
            terminal_exit: None,
            terminal_size: (0, 0),
            view: None,
            commands,
            on,
            hub,
        }
    }

    pub fn rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        for instance in self.state.instances.values() {
            let key = NodeKey::Instance(instance.name.clone());
            rows.push(TreeRow {
                key: key.clone(),
                parent: None,
                kind: NodeKind::Group,
                depth: 0,
                label: format!("{}  [{}]", instance.name, instance.stack),
                phase: None,
            });
            if self.expanded.contains(&key) {
                for name in &instance.members {
                    if let Some(unit) = self.state.units.get(name) {
                        self.push_unit_rows(&mut rows, unit, key.clone(), 1);
                    }
                }
            }
        }

        let standalone = self
            .state
            .units
            .values()
            .filter(|unit| {
                unit.instance
                    .as_ref()
                    .is_none_or(|name| !self.state.instances.contains_key(name))
            })
            .collect::<Vec<_>>();
        if !standalone.is_empty() {
            let key = NodeKey::Standalone;
            rows.push(TreeRow {
                key: key.clone(),
                parent: None,
                kind: NodeKind::Group,
                depth: 0,
                label: "standalone".into(),
                phase: None,
            });
            if self.expanded.contains(&key) {
                for unit in standalone {
                    self.push_unit_rows(&mut rows, unit, key.clone(), 1);
                }
            }
        }
        rows
    }

    fn push_unit_rows(
        &self,
        rows: &mut Vec<TreeRow>,
        unit: &MusterUnit,
        parent: NodeKey,
        depth: usize,
    ) {
        let key = NodeKey::Unit(unit.name.clone());
        let label = unit
            .instance
            .as_deref()
            .and_then(|instance| unit.name.strip_prefix(&format!("{instance}/")))
            .unwrap_or(&unit.name)
            .to_string();
        rows.push(TreeRow {
            key: key.clone(),
            parent: Some(parent),
            kind: NodeKind::Unit,
            depth,
            label,
            phase: Some(unit.phase.clone()),
        });
        if !self.expanded.contains(&key) {
            return;
        }
        if let Some(pty) = unit.pty {
            rows.push(TreeRow {
                key: NodeKey::Terminal {
                    unit: unit.name.clone(),
                    pty,
                },
                parent: Some(key.clone()),
                kind: NodeKind::Terminal,
                depth: depth + 1,
                label: format!("terminal {pty}  live"),
                phase: None,
            });
        }
        for run in &unit.runs {
            let exit = run
                .exit_code
                .map_or_else(|| "?".into(), |code| code.to_string());
            rows.push(TreeRow {
                key: NodeKey::Terminal {
                    unit: unit.name.clone(),
                    pty: run.pty,
                },
                parent: Some(key.clone()),
                kind: NodeKind::Terminal,
                depth: depth + 1,
                label: format!("terminal {}  run {} exit {exit}", run.pty, run.seq),
                phase: None,
            });
        }
        for surface in &unit.surfaces {
            let title = if surface.title.is_empty() {
                "untitled"
            } else {
                &surface.title
            };
            rows.push(TreeRow {
                key: NodeKey::Surface {
                    unit: unit.name.clone(),
                    id: surface.id,
                },
                parent: Some(key.clone()),
                kind: NodeKind::Surface,
                depth: depth + 1,
                label: format!("window {}  {title}", surface.id),
                phase: None,
            });
        }
    }

    fn selected_key(&self) -> Option<NodeKey> {
        self.rows().get(self.selected).map(|row| row.key.clone())
    }

    pub fn selected_row(&self) -> Option<TreeRow> {
        self.rows().get(self.selected).cloned()
    }

    fn retain_selection(&mut self, key: Option<NodeKey>) {
        let rows = self.rows();
        self.selected = key
            .and_then(|key| rows.iter().position(|row| row.key == key))
            .unwrap_or_else(|| self.selected.min(rows.len().saturating_sub(1)));
    }

    fn initialize_groups(&mut self) {
        let groups = self
            .state
            .instances
            .keys()
            .cloned()
            .map(NodeKey::Instance)
            .chain((!self.state.units.is_empty()).then_some(NodeKey::Standalone));
        for key in groups {
            if self.initialized_groups.insert(key.clone()) {
                self.expanded.insert(key);
            }
        }
    }

    fn move_selection(&mut self, amount: isize) {
        let len = self.rows().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(len.saturating_sub(1));
    }

    fn expand_selected(&mut self) {
        if let Some(row) = self.selected_row()
            && matches!(row.kind, NodeKind::Group | NodeKind::Unit)
        {
            self.expanded.insert(row.key);
        }
    }

    fn collapse_or_parent(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if self.expanded.remove(&row.key) {
            return;
        }
        if let Some(parent) = row.parent
            && let Some(index) = self.rows().iter().position(|row| row.key == parent)
        {
            self.selected = index;
        }
    }

    fn toggle_selected(&mut self) -> bool {
        let Some(row) = self.selected_row() else {
            return false;
        };
        match row.kind {
            NodeKind::Group | NodeKind::Unit => {
                if !self.expanded.remove(&row.key) {
                    self.expanded.insert(row.key);
                }
                false
            }
            NodeKind::Terminal => true,
            NodeKind::Surface => false,
        }
    }

    fn selected_target(&self) -> Option<String> {
        match self.selected_row()?.key {
            NodeKey::Instance(name) | NodeKey::Unit(name) => Some(name),
            NodeKey::Terminal { unit, .. } | NodeKey::Surface { unit, .. } => Some(unit),
            NodeKey::Standalone => None,
        }
    }

    fn desired_terminal(&self) -> Option<u64> {
        match self.selected_row()?.key {
            NodeKey::Terminal { pty, .. } => Some(pty),
            NodeKey::Unit(name) | NodeKey::Surface { unit: name, .. } => {
                self.state.units.get(&name)?.preview_terminal()
            }
            NodeKey::Instance(_) | NodeKey::Standalone => None,
        }
    }

    fn command(&mut self, verb: &str) {
        let line = if matches!(verb, "rewatch" | "resync") {
            verb.to_string()
        } else if let Some(target) = self.selected_target() {
            format!("{verb} {target}")
        } else {
            self.notice = "select a stack or unit first".into();
            return;
        };
        match self
            .commands
            .try_send(SupervisorCommand::Line(line.clone()))
        {
            Ok(()) => self.notice = format!("sent: {line}"),
            Err(_) => self.notice = "Muster command queue is unavailable".into(),
        }
    }

    async fn sync_view(&mut self, area: Rect) {
        let size = ui::terminal_dimensions(area, self);
        let desired = self.desired_terminal();
        if desired == self.terminal_handle {
            if desired.is_some() && size != self.terminal_size {
                self.terminal_size = size;
                if let Some(view) = &self.view {
                    let _ = view
                        .commands
                        .send(ViewCommand::Resize {
                            rows: size.1,
                            cols: size.0,
                        })
                        .await;
                }
            }
            return;
        }
        self.close_view().await;
        self.terminal_handle = desired;
        self.terminal_size = size;
        self.terminal_grid = None;
        self.terminal_error = None;
        self.terminal_exit = None;
        self.terminal_focus = false;
        let Some(handle) = desired else {
            return;
        };
        match start_interactive_view_task(self.on.as_deref(), &self.hub, handle, size.1, size.0)
            .await
        {
            Ok(view) => self.view = Some(view),
            Err(error) => self.terminal_error = Some(error),
        }
    }

    async fn close_view(&mut self) {
        let Some(mut view) = self.view.take() else {
            return;
        };
        let _ = view.commands.send(ViewCommand::Close).await;
        if tokio::time::timeout(Duration::from_millis(100), &mut view.task)
            .await
            .is_err()
        {
            view.task.abort();
        }
    }

    async fn set_terminal_focus(&mut self, focused: bool) {
        if focused && self.terminal_handle.is_none() {
            self.notice = "selected item has no terminal".into();
            return;
        }
        self.terminal_focus = focused;
        if let Some(view) = &self.view {
            let _ = view.commands.send(ViewCommand::Focus(focused)).await;
        }
    }

    async fn send_terminal(&mut self, bytes: Vec<u8>) {
        if let Some(view) = &self.view
            && view.commands.send(ViewCommand::Input(bytes)).await.is_err()
        {
            self.terminal_error = Some("terminal input channel closed".into());
            self.terminal_focus = false;
        }
    }

    async fn scroll_terminal(&mut self, mode: ScrollMode, amount: i64) {
        if let Some(view) = &self.view
            && view
                .commands
                .send(ViewCommand::Scroll { mode, amount })
                .await
                .is_err()
        {
            self.terminal_error = Some("terminal scroll channel closed".into());
        }
    }

    async fn wheel_terminal(&mut self, mouse: MouseEvent, column: u16, row: u16) {
        let up = mouse.kind == MouseEventKind::ScrollUp;
        let application_mouse = self.terminal_focus
            && self.terminal_exit.is_none()
            && !mouse.modifiers.contains(KeyModifiers::SHIFT)
            && self
                .terminal_grid
                .as_ref()
                .is_some_and(|grid| grid.reports_mouse() && grid.scroll_offset == 0);
        if application_mouse {
            self.send_terminal_mouse(
                if up { "wheel-up" } else { "wheel-down" },
                column,
                row,
                "left",
            )
            .await;
        } else {
            self.scroll_terminal(ScrollMode::Relative, if up { 3 } else { -3 })
                .await;
        }
    }

    async fn send_terminal_mouse(
        &mut self,
        event: &'static str,
        column: u16,
        row: u16,
        button: &'static str,
    ) {
        if let Some(view) = &self.view
            && view
                .commands
                .send(ViewCommand::Mouse {
                    event,
                    column,
                    row,
                    button,
                })
                .await
                .is_err()
        {
            self.terminal_error = Some("terminal input channel closed".into());
            self.terminal_focus = false;
        }
    }
}

pub(crate) async fn run(on: Option<&str>, hub: &str) -> Result<(), String> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("mustard needs an interactive terminal".into());
    }

    let (updates_tx, mut updates_rx) = mpsc::channel(32);
    let (commands_tx, commands_rx) = mpsc::channel(32);
    let channel_task = tokio::spawn(channel_task(
        on.map(str::to_string),
        hub.to_string(),
        updates_tx,
        commands_rx,
    ));
    let mut app = App::new(on.map(str::to_string), hub.to_string(), commands_tx);

    enable_raw_mode().map_err(|error| format!("cannot enable terminal raw mode: {error}"))?;
    let _guard = ScreenGuard;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(|error| format!("cannot enter alternate screen: {error}"))?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|error| format!("cannot initialize terminal UI: {error}"))?;
    terminal
        .clear()
        .map_err(|error| format!("cannot clear terminal UI: {error}"))?;

    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut running = true;
    while running {
        let area = terminal
            .size()
            .map_err(|error| format!("cannot read terminal size: {error}"))?;
        app.sync_view(area.into()).await;
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .map_err(|error| format!("cannot draw terminal UI: {error}"))?;

        enum Next {
            Input(Option<Result<Event, std::io::Error>>),
            Channel(Option<ChannelUpdate>),
            View(Option<Result<crate::yas_terminal::stream::InteractiveViewUpdate, String>>),
            Tick,
        }
        let next = tokio::select! {
            event = input.next() => Next::Input(event),
            update = updates_rx.recv() => Next::Channel(update),
            update = receive_view(&mut app.view) => Next::View(update),
            _ = tick.tick() => Next::Tick,
        };
        match next {
            Next::Input(Some(Ok(Event::Key(key)))) if key.kind != KeyEventKind::Release => {
                running = handle_key(&mut app, key).await;
            }
            Next::Input(Some(Ok(Event::Mouse(mouse)))) => {
                handle_mouse(&mut app, area.into(), mouse).await;
            }
            Next::Input(Some(Ok(Event::Paste(text)))) if app.terminal_focus => {
                app.send_terminal(text.into_bytes()).await;
            }
            Next::Input(Some(Ok(_))) | Next::Tick => {}
            Next::Input(Some(Err(error))) => {
                return Err(format!("terminal input failed: {error}"));
            }
            Next::Input(None) => break,
            Next::Channel(Some(ChannelUpdate::Reset)) => {
                let key = app.selected_key();
                app.state = MusterState::default();
                app.retain_selection(key);
            }
            Next::Channel(Some(ChannelUpdate::Status(status))) => {
                app.connection_status = status;
            }
            Next::Channel(Some(ChannelUpdate::Message(message))) => {
                let key = app.selected_key();
                match app.state.apply(&message) {
                    Ok(()) => {
                        app.initialize_groups();
                        app.retain_selection(key);
                    }
                    Err(error) => app.notice = error,
                }
            }
            Next::Channel(None) => app.connection_status = "connection task stopped".into(),
            Next::View(Some(Ok(update))) => {
                app.terminal_grid = Some(update.grid);
                app.terminal_exit = update.final_exit;
            }
            Next::View(Some(Err(error))) => {
                app.terminal_error = Some(error);
                app.terminal_focus = false;
            }
            Next::View(None) => {
                if let Some(view) = app.view.take() {
                    let _ = view.task.await;
                }
                app.terminal_focus = false;
            }
        }
    }

    app.close_view().await;
    let _ = app.commands.send(SupervisorCommand::Shutdown).await;
    if tokio::time::timeout(Duration::from_millis(200), channel_task)
        .await
        .is_err()
    {
        // Dropping the native client closes only this client session. It does
        // not touch the YAS server or supervised units.
    }
    Ok(())
}

fn mouse_button(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Middle => "middle",
        MouseButton::Right => "right",
    }
}

async fn handle_mouse(app: &mut App, area: Rect, mouse: MouseEvent) {
    if app.help_open {
        return;
    }
    // Capture the whole gesture, even when the pointer crosses the terminal.
    if app.resizing_events {
        match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                app.event_height = ui::resize_events(area, mouse.row);
                if matches!(mouse.kind, MouseEventKind::Up(_)) {
                    app.resizing_events = false;
                }
                return;
            }
            MouseEventKind::Down(_) => app.resizing_events = false,
            _ => return,
        }
    }
    let target = ui::mouse_target(area, app, mouse.column, mouse.row);
    match (mouse.kind, target) {
        (MouseEventKind::Down(MouseButton::Left), ui::MouseTarget::EventsDivider) => {
            app.resizing_events = true;
        }
        (MouseEventKind::Down(MouseButton::Left), ui::MouseTarget::Terminal) => {
            app.set_terminal_focus(true).await;
        }
        (
            MouseEventKind::Down(MouseButton::Left),
            ui::MouseTarget::TreeRow { index, disclosure },
        ) => {
            app.set_terminal_focus(false).await;
            app.selected = index;
            if disclosure {
                app.toggle_selected();
            }
        }
        (MouseEventKind::Down(MouseButton::Left), ui::MouseTarget::Tree) => {
            app.set_terminal_focus(false).await;
        }
        (MouseEventKind::ScrollUp, ui::MouseTarget::Tree | ui::MouseTarget::TreeRow { .. }) => {
            app.set_terminal_focus(false).await;
            app.move_selection(-3);
        }
        (MouseEventKind::ScrollDown, ui::MouseTarget::Tree | ui::MouseTarget::TreeRow { .. }) => {
            app.set_terminal_focus(false).await;
            app.move_selection(3);
        }
        (MouseEventKind::Down(button), ui::MouseTarget::TerminalCell { column, row }) => {
            app.set_terminal_focus(true).await;
            app.send_terminal_mouse("down", column, row, mouse_button(button))
                .await;
        }
        (MouseEventKind::Up(button), ui::MouseTarget::TerminalCell { column, row }) => {
            app.send_terminal_mouse("up", column, row, mouse_button(button))
                .await;
        }
        (MouseEventKind::Drag(button), ui::MouseTarget::TerminalCell { column, row }) => {
            app.send_terminal_mouse("move", column, row, mouse_button(button))
                .await;
        }
        (MouseEventKind::Moved, ui::MouseTarget::TerminalCell { column, row }) => {
            app.send_terminal_mouse("hover", column, row, "left").await;
        }
        (
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown,
            ui::MouseTarget::TerminalCell { column, row },
        ) => {
            app.wheel_terminal(mouse, column, row).await;
        }
        (MouseEventKind::ScrollUp | MouseEventKind::ScrollDown, ui::MouseTarget::Terminal) => {
            app.scroll_terminal(
                ScrollMode::Relative,
                if mouse.kind == MouseEventKind::ScrollUp {
                    3
                } else {
                    -3
                },
            )
            .await;
        }
        (MouseEventKind::Down(MouseButton::Left), ui::MouseTarget::Other) => {
            app.set_terminal_focus(false).await;
        }
        _ => {}
    }
}

async fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if app.help_open {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
            app.help_open = false;
        }
        return true;
    }
    let shifted = key.modifiers == KeyModifiers::SHIFT;
    if shifted || (!app.terminal_focus && key.modifiers.is_empty()) {
        let page = i64::from(app.terminal_size.1.saturating_sub(1).max(1));
        let scroll = match key.code {
            KeyCode::PageUp => Some((ScrollMode::Relative, page)),
            KeyCode::PageDown => Some((ScrollMode::Relative, -page)),
            KeyCode::Home if shifted => Some((ScrollMode::Absolute, i64::MAX)),
            KeyCode::End if shifted => Some((ScrollMode::Absolute, 0)),
            _ => None,
        };
        if let Some((mode, amount)) = scroll {
            app.scroll_terminal(mode, amount).await;
            return true;
        }
    }
    if app.terminal_focus {
        let bytes = terminal_key(key);
        if bytes.as_deref() == Some(&[0x1d]) {
            app.set_terminal_focus(false).await;
        } else if let Some(bytes) = bytes {
            app.send_terminal(bytes).await;
        }
        return true;
    }

    match key.code {
        KeyCode::Char('?') => app.help_open = true,
        KeyCode::Char('q') => return false,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = app.rows().len().saturating_sub(1),
        KeyCode::Left | KeyCode::Char('h') => app.collapse_or_parent(),
        KeyCode::Right | KeyCode::Char('l') => app.expand_selected(),
        KeyCode::Enter | KeyCode::Char(' ') => {
            if app.toggle_selected() {
                app.set_terminal_focus(true).await;
            }
        }
        KeyCode::Char('i') => app.set_terminal_focus(true).await,
        KeyCode::Char('s') => app.command("start"),
        KeyCode::Char('x') => app.command("stop"),
        KeyCode::Char('r') => app.command("restart"),
        KeyCode::Char('R') => app.command("reload"),
        KeyCode::Char('w') => app.command("rewatch"),
        KeyCode::Char('g') => app.command("resync"),
        _ => {}
    }
    true
}

fn terminal_key(key: KeyEvent) -> Option<Vec<u8>> {
    let mut bytes = match key.code {
        KeyCode::Char(character) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let value = match character {
                ' ' | '@' => 0,
                'a'..='z' => character as u8 - b'a' + 1,
                'A'..='Z' => character as u8 - b'A' + 1,
                '[' => 27,
                '\\' => 28,
                ']' => 29,
                '^' => 30,
                '_' => 31,
                // Crossterm normalizes input bytes 0x1c..=0x1f to Ctrl-4..=Ctrl-7.
                // This includes the byte emitted by Ctrl-] (Ctrl-5 / 0x1d).
                '4'..='7' => character as u8 - b'4' + 28,
                '?' => 127,
                _ => return None,
            };
            vec![value]
        }
        KeyCode::Char(character) => {
            let mut buffer = [0; 4];
            character.encode_utf8(&mut buffer).as_bytes().to_vec()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(number) => function_key(number)?.to_vec(),
        _ => return None,
    };
    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn function_key(number: u8) -> Option<&'static [u8]> {
    Some(match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    })
}

async fn receive_view(
    view: &mut Option<InteractiveView>,
) -> Option<Result<crate::yas_terminal::stream::InteractiveViewUpdate, String>> {
    match view {
        Some(view) => view.updates.recv().await,
        None => std::future::pending().await,
    }
}

async fn channel_task(
    on: Option<String>,
    hub: String,
    updates: mpsc::Sender<ChannelUpdate>,
    mut commands: mpsc::Receiver<SupervisorCommand>,
) {
    loop {
        let _ = updates
            .send(ChannelUpdate::Status("connecting".into()))
            .await;
        match channel::connect(on.as_deref(), &hub).await {
            Ok(mut channel) => {
                let _ = updates.send(ChannelUpdate::Reset).await;
                let _ = updates
                    .send(ChannelUpdate::Status("connected".into()))
                    .await;
                enum Next {
                    Remote(Result<Vec<u8>, String>),
                    Local(Option<SupervisorCommand>),
                }
                loop {
                    let next = tokio::select! {
                        message = channel.recv() => Next::Remote(message),
                        command = commands.recv() => Next::Local(command),
                    };
                    match next {
                        Next::Remote(Ok(message)) => {
                            if updates.send(ChannelUpdate::Message(message)).await.is_err() {
                                channel.close().await;
                                return;
                            }
                        }
                        Next::Remote(Err(error)) => {
                            let _ = updates
                                .send(ChannelUpdate::Status(format!(
                                    "disconnected: {error}; retrying"
                                )))
                                .await;
                            break;
                        }
                        Next::Local(Some(SupervisorCommand::Line(line))) => {
                            if let Err(error) = channel.send(&line).await {
                                let _ = updates
                                    .send(ChannelUpdate::Status(format!(
                                        "disconnected: {error}; retrying"
                                    )))
                                    .await;
                                break;
                            }
                        }
                        Next::Local(Some(SupervisorCommand::Shutdown) | None) => {
                            channel.close().await;
                            return;
                        }
                    }
                }
            }
            Err(error) => {
                let _ = updates
                    .send(ChannelUpdate::Status(format!("{error}; retrying")))
                    .await;
            }
        }
        let delay = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(delay);
        loop {
            tokio::select! {
                () = &mut delay => break,
                command = commands.recv() => match command {
                    Some(SupervisorCommand::Shutdown) | None => return,
                    Some(SupervisorCommand::Line(_)) => {
                        let _ = updates.send(ChannelUpdate::Status(
                            "command ignored while Muster is disconnected".into()
                        )).await;
                    }
                }
            }
        }
    }
}

struct ScreenGuard;

impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_scrolling_respects_input_mode_and_mouse_reporting() {
        let (commands, _) = mpsc::channel(1);
        let mut app = App::new(None, String::new(), commands);
        let (commands, mut received) = mpsc::channel(8);
        let (_, updates) = mpsc::channel(1);
        app.view = Some(InteractiveView {
            commands,
            updates,
            task: tokio::spawn(async {}),
        });
        app.terminal_size = (80, 20);
        app.terminal_handle = Some(1);
        app.terminal_grid = Some(GridState::default());

        handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll {
                mode: ScrollMode::Relative,
                amount: 19
            })
        ));
        app.terminal_focus = true;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::SHIFT),
        )
        .await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll {
                mode: ScrollMode::Relative,
                amount: -19
            })
        ));
        handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT)).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll {
                mode: ScrollMode::Absolute,
                amount: i64::MAX
            })
        ));
        handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT)).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll {
                mode: ScrollMode::Absolute,
                amount: 0
            })
        ));
        handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)).await;
        assert!(matches!(received.try_recv(), Ok(ViewCommand::Input(data)) if data == b"\x1b[5~"));

        let mut mouse = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll {
                mode: ScrollMode::Relative,
                amount: 3
            })
        ));
        app.terminal_grid.as_mut().unwrap().modes = 1 << 4;
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Mouse {
                event: "wheel-up",
                column: 4,
                row: 5,
                ..
            })
        ));
        mouse.modifiers = KeyModifiers::SHIFT;
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll { amount: 3, .. })
        ));
        mouse.modifiers = KeyModifiers::NONE;
        app.terminal_grid.as_mut().unwrap().scroll_offset = 6;
        mouse.kind = MouseEventKind::ScrollDown;
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll { amount: -3, .. })
        ));
        app.terminal_grid.as_mut().unwrap().scroll_offset = 0;
        app.terminal_exit = Some(0);
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll { amount: -3, .. })
        ));
        app.terminal_exit = None;
        app.terminal_focus = false;
        app.wheel_terminal(mouse, 4, 5).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Scroll { amount: -3, .. })
        ));
    }

    #[test]
    fn terminal_keys_encode_control_and_navigation() {
        assert_eq!(
            terminal_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![3])
        );
        assert_eq!(
            terminal_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)),
            Some(b"\x1bx".to_vec())
        );
        assert_eq!(
            terminal_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL)),
            Some(vec![0x1d])
        );
        assert_eq!(
            terminal_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)),
            Some(vec![0x1d])
        );
    }
}
