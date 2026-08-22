use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap};

use super::model::{MusterEvent, MusterUnit};
use super::{App, NodeKey, NodeKind, TreeRow};
use crate::yas_terminal::stream::GridState;

pub(super) const DEFAULT_EVENT_HEIGHT: u16 = 6;
const FOOTER_HEIGHT: u16 = 2;
const DETAIL_HEIGHT: u16 = 8;
const MIN_MAIN_HEIGHT: u16 = DETAIL_HEIGHT + 2;
const MIN_RIGHT_WIDTH: u16 = 30;

pub(super) fn terminal_dimensions(area: Rect, app: &App) -> (u16, u16) {
    let regions = regions(area, app, &app.rows());
    let inner = panel_inner(regions.terminal);
    (inner.width.max(1), inner.height.max(1))
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App) {
    let rows = app.rows();
    let regions = regions(frame.area(), app, &rows);
    draw_tree(frame, app, &rows, regions.tree);
    draw_detail(frame, app, regions.detail);
    draw_terminal(frame, app, regions.terminal);
    draw_events(frame, app, regions.events, regions.tree.width);
    draw_junctions(frame, &regions);
    draw_footer(frame, app, regions.footer);
    if app.help_open {
        draw_help(frame);
    }
}

struct Regions {
    tree: Rect,
    detail: Rect,
    terminal: Rect,
    events: Rect,
    footer: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MouseTarget {
    Tree,
    TreeRow { index: usize, disclosure: bool },
    Terminal,
    TerminalCell { column: u16, row: u16 },
    EventsDivider,
    Other,
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

pub(super) fn mouse_target(area: Rect, app: &App, column: u16, row: u16) -> MouseTarget {
    let rows = app.rows();
    let regions = regions(area, app, &rows);
    if contains(regions.events, column, row) && row == regions.events.y {
        return MouseTarget::EventsDivider;
    }
    if contains(regions.tree, column, row) {
        let inner = tree_inner(regions.tree);
        if !contains(inner, column, row) {
            return MouseTarget::Tree;
        }
        let available = usize::from(inner.height);
        let index = scroll_start(app.selected, rows.len(), available)
            .saturating_add(usize::from(row.saturating_sub(inner.y)));
        let Some(tree_row) = rows.get(index) else {
            return MouseTarget::Tree;
        };
        let marker = inner
            .x
            .saturating_add((tree_row.depth as u16).saturating_mul(2));
        return MouseTarget::TreeRow {
            index,
            disclosure: matches!(tree_row.kind, NodeKind::Group | NodeKind::Unit)
                && column >= marker
                && column < marker.saturating_add(2),
        };
    }

    let inner = panel_inner(regions.terminal);
    if contains(inner, column, row) {
        return MouseTarget::TerminalCell {
            column: column - inner.x,
            row: row - inner.y,
        };
    }
    if contains(regions.terminal, column, row) {
        return MouseTarget::Terminal;
    }
    MouseTarget::Other
}

fn panel_inner(area: Rect) -> Rect {
    Block::default().borders(Borders::TOP).inner(area)
}

fn tree_inner(area: Rect) -> Rect {
    Block::default()
        .borders(Borders::TOP | Borders::RIGHT)
        .inner(area)
}

fn separator_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn header_title(label: &str, detail: &str, active: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        label.to_owned(),
        Style::default()
            .fg(if active { Color::Cyan } else { Color::White })
            .add_modifier(Modifier::BOLD),
    )];
    if !detail.is_empty() {
        spans.push(Span::styled(format!(" · {detail}"), separator_style()));
    }
    Line::from(spans)
}

// Every header has one rule cell, one space, its title, one space, then a
// remaining rule. Reserve both spaces even when a long title is clipped.
fn draw_header(frame: &mut Frame<'_>, area: Rect, title: Line<'_>) {
    if area.is_empty() {
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Span::styled("─".repeat(usize::from(area.width)), separator_style()),
        header,
    );
    if area.width < 5 {
        return;
    }
    let width = title.width().min(usize::from(area.width - 4)) as u16;
    frame.render_widget(Span::raw(" "), Rect::new(area.x + 1, area.y, 1, 1));
    frame.render_widget(title, Rect::new(area.x + 2, area.y, width, 1));
    frame.render_widget(Span::raw(" "), Rect::new(area.x + 2 + width, area.y, 1, 1));
}

fn draw_junctions(frame: &mut Frame<'_>, regions: &Regions) {
    if regions.tree.is_empty() || regions.detail.width == 0 {
        return;
    }
    let x = regions.tree.right() - 1;
    for (area, symbol) in [
        (regions.tree, "┬"),
        (regions.terminal, "├"),
        (regions.events, "┴"),
    ] {
        if !area.is_empty() {
            frame.render_widget(
                Span::styled(symbol, separator_style()),
                Rect::new(x, area.y, 1, 1),
            );
        }
    }
}

fn clamp_event_height(area: Rect, height: u16) -> u16 {
    let max = area.height.saturating_sub(FOOTER_HEIGHT + MIN_MAIN_HEIGHT);
    height.clamp(1.min(max), max)
}

pub(super) fn resize_events(area: Rect, row: u16) -> u16 {
    clamp_event_height(
        area,
        area.bottom()
            .saturating_sub(FOOTER_HEIGHT)
            .saturating_sub(row),
    )
}

fn regions(area: Rect, app: &App, rows: &[TreeRow]) -> Regions {
    let [main, events, footer] = Layout::vertical([
        Constraint::Min(MIN_MAIN_HEIGHT),
        Constraint::Length(clamp_event_height(area, app.event_height)),
        Constraint::Length(FOOTER_HEIGHT),
    ])
    .areas(area);
    // Measure all expanded rows, including those outside the scroll viewport,
    // so moving the selection does not change the split.
    let content_width = rows
        .iter()
        .map(|row| tree_line(app, row).width())
        .chain(std::iter::once(tree_title(app).width().saturating_add(4)))
        .max()
        .unwrap_or_default()
        .saturating_add(1);
    let max_tree_width = area.width - MIN_RIGHT_WIDTH.min(area.width / 2);
    let tree_width = content_width.min(usize::from(max_tree_width)) as u16;
    let [tree, right] =
        Layout::horizontal([Constraint::Length(tree_width), Constraint::Min(0)]).areas(main);
    let detail_height = (detail_lines(app).len() as u16)
        .saturating_add(1)
        .min(DETAIL_HEIGHT);
    let [detail, terminal] =
        Layout::vertical([Constraint::Length(detail_height), Constraint::Min(2)]).areas(right);
    Regions {
        tree,
        detail,
        terminal,
        events,
        footer,
    }
}

fn draw_tree(frame: &mut Frame<'_>, app: &App, rows: &[TreeRow], area: Rect) {
    let available = usize::from(tree_inner(area).height);
    let start = scroll_start(app.selected, rows.len(), available);
    let items = rows
        .iter()
        .enumerate()
        .skip(start)
        .take(available)
        .map(|(index, row)| tree_item(app, row, index == app.selected))
        .collect::<Vec<_>>();
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(separator_style()),
        area,
    );
    draw_header(
        frame,
        Rect {
            width: area.width.saturating_sub(1),
            ..area
        },
        tree_title(app),
    );
    frame.render_widget(List::new(items), tree_inner(area));
}

fn tree_title(app: &App) -> Line<'static> {
    let detail = if app.state.ready {
        let count = app.state.units.len();
        format!("{count} unit{}", if count == 1 { "" } else { "s" })
    } else {
        "waiting for state".into()
    };
    header_title("Muster", &detail, false)
}

fn tree_line(app: &App, row: &TreeRow) -> Line<'static> {
    let marker = match row.kind {
        NodeKind::Group | NodeKind::Unit if app.expanded.contains(&row.key) => "▾",
        NodeKind::Group | NodeKind::Unit => "▸",
        NodeKind::Terminal => "▣",
        NodeKind::Surface => "◇",
    };
    let phase = row.phase.as_deref().unwrap_or_default();
    let phase_span = if phase.is_empty() {
        Span::raw("")
    } else {
        Span::styled(format!(" {phase}"), phase_style(phase))
    };
    let mut spans = vec![
        Span::raw("  ".repeat(row.depth)),
        Span::raw(format!("{marker} ")),
        Span::raw(row.label.clone()),
        phase_span,
    ];
    if matches!(&row.key, NodeKey::Unit(name) if app.state.units.get(name).is_some_and(|unit| unit.stale))
    {
        spans.push(Span::styled(" stale", Style::default().fg(Color::Yellow)));
    }
    Line::from(spans)
}

fn tree_item(app: &App, row: &TreeRow, selected: bool) -> ListItem<'static> {
    let style = if selected {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    ListItem::new(tree_line(app, row)).style(style)
}

fn detail_lines(app: &App) -> Vec<Line<'_>> {
    match app.selected_row().map(|row| row.key) {
        Some(NodeKey::Instance(name)) => app
            .state
            .instances
            .get(&name)
            .map(|instance| {
                vec![
                    Line::from(vec![label("instance"), Span::raw(&instance.name)]),
                    Line::from(vec![label("stack"), Span::raw(&instance.stack)]),
                    Line::from(vec![
                        label("members"),
                        Span::raw(instance.members.len().to_string()),
                    ]),
                ]
            })
            .unwrap_or_default(),
        Some(NodeKey::Unit(name)) => app
            .state
            .units
            .get(&name)
            .map(unit_lines)
            .unwrap_or_default(),
        Some(NodeKey::Terminal { unit: name, pty }) => app
            .state
            .units
            .get(&name)
            .map(|unit| {
                let mut lines = unit_lines(unit);
                let selected = if unit.pty == Some(pty) {
                    format!("{pty} (live)")
                } else if let Some(run) = unit.runs.iter().find(|run| run.pty == pty) {
                    let exit = run
                        .exit_code
                        .map_or_else(|| "?".into(), |code| code.to_string());
                    format!("{pty} (run {}, exit {exit})", run.seq)
                } else {
                    pty.to_string()
                };
                lines.push(Line::from(vec![label("selected"), Span::raw(selected)]));
                lines
            })
            .unwrap_or_default(),
        Some(NodeKey::Surface { unit: name, id }) => app
            .state
            .units
            .get(&name)
            .map(|unit| {
                let mut lines = unit_lines(unit);
                if let Some(surface) = unit.surfaces.iter().find(|surface| surface.id == id) {
                    lines.push(Line::from(vec![
                        label("window"),
                        Span::raw(format!(
                            "{}  {}×{}  {}",
                            surface.id, surface.width, surface.height, surface.title
                        )),
                    ]));
                }
                lines
            })
            .unwrap_or_default(),
        Some(NodeKey::Standalone) => vec![Line::raw("Top-level units")],
        None => vec![Line::raw("No units")],
    }
}

fn draw_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    draw_header(frame, area, header_title("Details", "", false));
    frame.render_widget(
        Paragraph::new(detail_lines(app)).wrap(Wrap { trim: true }),
        panel_inner(area),
    );
}

fn unit_lines(unit: &MusterUnit) -> Vec<Line<'_>> {
    let terminal = unit.pty.map_or_else(|| "—".into(), |pty| pty.to_string());
    let exit = unit
        .last_exit
        .map_or_else(|| "—".into(), |code| code.to_string());
    let description = unit.description.as_deref().unwrap_or_default();
    let requirements = if unit.requires.is_empty() {
        "—".into()
    } else {
        unit.requires.join(", ")
    };
    vec![
        Line::from(vec![
            label("unit"),
            Span::styled(&unit.name, Modifier::BOLD),
            Span::raw("  "),
            Span::styled(&unit.phase, phase_style(&unit.phase)),
        ]),
        Line::from(vec![label("about"), Span::raw(description)]),
        Line::from(vec![
            label("kind"),
            Span::raw(format!(
                "{}  autostart={}  stale={}",
                unit.unit_type, unit.autostart, unit.stale
            )),
        ]),
        Line::from(vec![label("requires"), Span::raw(requirements)]),
        Line::from(vec![
            label("terminal"),
            Span::raw(terminal),
            Span::raw(format!(
                "  retained={}  windows={}",
                unit.runs.len(),
                unit.surfaces.len()
            )),
        ]),
        Line::from(vec![
            label("failures"),
            Span::raw(unit.restarts.to_string()),
            Span::raw(format!("  last exit={exit}")),
        ]),
    ]
}

fn label(text: &'static str) -> Span<'static> {
    Span::styled(format!("{text:<10}"), Style::default().fg(Color::DarkGray))
}

fn draw_terminal(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let label = app
        .terminal_handle
        .map_or_else(|| "Terminal".into(), |handle| format!("Terminal {handle}"));
    let mut details = Vec::new();
    if let Some(grid) = &app.terminal_grid {
        details.push(if grid.scroll_offset > 0 {
            format!(
                "{}/{} lines back",
                grid.scroll_offset, grid.scrollback_lines
            )
        } else {
            "latest".into()
        });
    }
    if let Some(code) = app.terminal_exit {
        details.push(format!("exit {code}"));
    }
    if app.terminal_focus {
        details.push("INPUT".into());
    }
    if let Some(grid) = &app.terminal_grid
        && !grid.title.is_empty()
    {
        details.push(grid.title.clone());
    }
    draw_header(
        frame,
        area,
        header_title(&label, &details.join(" · "), app.terminal_focus),
    );
    let inner = panel_inner(area);
    if let Some(error) = &app.terminal_error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(Color::Red)),
            inner,
        );
        return;
    }
    let Some(grid) = &app.terminal_grid else {
        let message = if app.terminal_handle.is_some() {
            "opening terminal…"
        } else {
            "select a unit or terminal"
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };
    frame.render_widget(TerminalGrid(grid), inner);
    if app.terminal_focus
        && app.terminal_exit.is_none()
        && grid.cursor_visible()
        && grid.cursor.1 < inner.width
        && grid.cursor.0 < inner.height
        && inner.width > 0
        && inner.height > 0
    {
        frame.set_cursor_position((inner.x + grid.cursor.1, inner.y + grid.cursor.0));
    }
}

struct TerminalGrid<'a>(&'a GridState);

impl Widget for TerminalGrid<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let grid = self.0;
        let rows = grid.rows.min(area.height);
        let cols = grid.cols.min(area.width);
        for row in 0..rows {
            for col in 0..cols {
                let index = usize::from(row) * usize::from(grid.cols) + usize::from(col);
                let Some(cell) = grid.cells.get(index) else {
                    continue;
                };
                let symbol = cell_symbol(cell, grid.overflow.get(&(index as u32)));
                buffer[(area.x + col, area.y + row)]
                    .set_symbol(symbol)
                    .set_style(cell_style(cell));
            }
        }
    }
}

fn cell_symbol<'a>(cell: &'a yas_wire::terminal::Cell, overflow: Option<&'a String>) -> &'a str {
    if cell[1] & 4 != 0 {
        return "";
    }
    match usize::from((cell[1] >> 3) & 7) {
        0 => " ",
        1..=4 => {
            let length = usize::from((cell[1] >> 3) & 7);
            std::str::from_utf8(&cell[8..8 + length]).unwrap_or(" ")
        }
        7 => overflow.map_or(" ", String::as_str),
        _ => " ",
    }
}

fn cell_style(cell: &yas_wire::terminal::Cell) -> Style {
    let mut modifiers = Modifier::empty();
    for (enabled, modifier) in [
        (cell[0] & (1 << 4) != 0, Modifier::BOLD),
        (cell[0] & (1 << 5) != 0, Modifier::DIM),
        (cell[0] & (1 << 6) != 0, Modifier::ITALIC),
        (cell[0] & (1 << 7) != 0, Modifier::UNDERLINED),
        (cell[1] & 1 != 0, Modifier::REVERSED),
    ] {
        if enabled {
            modifiers.insert(modifier);
        }
    }
    Style::default()
        .fg(decode_color(cell[0] & 3, &cell[2..5]))
        .bg(decode_color((cell[0] >> 2) & 3, &cell[5..8]))
        .add_modifier(modifiers)
}

fn decode_color(kind: u8, bytes: &[u8]) -> Color {
    match kind {
        1 => Color::Indexed(bytes[0]),
        2 => Color::Rgb(bytes[0], bytes[1], bytes[2]),
        _ => Color::Reset,
    }
}

fn draw_events(frame: &mut Frame<'_>, app: &App, area: Rect, split: u16) {
    let available = usize::from(panel_inner(area).height);
    let events = app
        .state
        .events
        .iter()
        .rev()
        .take(available)
        .collect::<Vec<_>>();
    let lines = events.into_iter().rev().map(event_line).collect::<Vec<_>>();
    let split = split.min(area.width);
    draw_header(
        frame,
        Rect {
            width: split.saturating_sub(1),
            ..area
        },
        header_title(
            "Events",
            &app.state.events.len().to_string(),
            app.resizing_events,
        ),
    );
    draw_header(
        frame,
        Rect {
            x: area.x + split,
            width: area.width - split,
            ..area
        },
        Line::styled("drag to resize", separator_style()),
    );
    frame.render_widget(Paragraph::new(lines), panel_inner(area));
}

fn event_line(event: &MusterEvent) -> Line<'_> {
    let detail = event
        .detail
        .as_deref()
        .or(event.cause.as_deref())
        .unwrap_or_default();
    let pty = event
        .pty
        .map_or_else(String::new, |pty| format!(" pty={pty}"));
    let exit = event
        .exit_code
        .map_or_else(String::new, |exit| format!(" exit={exit}"));
    Line::from(vec![
        Span::styled(
            format!("{:>5} ", event.seq),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{:<9}", event.event), phase_style(&event.phase)),
        Span::raw(format!(" {:<24} {detail}{pty}{exit}", event.unit)),
    ])
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let keys = if app.terminal_focus {
        "Shift+PgUp/PgDn scroll · Shift+End latest · Ctrl-] navigate · typing returns to latest"
    } else {
        "↑↓ select · ←→ fold · PgUp/PgDn scroll · i input · ? help · q quit"
    };
    let status = if app.notice.is_empty() {
        format!("{} · {}", app.connection_status, app.state.dir)
    } else {
        format!("{} · {}", app.connection_status, app.notice)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(keys, Style::default().fg(Color::Cyan))),
            Line::from(Span::styled(status, Style::default().fg(Color::DarkGray))),
        ]),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>) {
    let area = frame.area();
    let width = area.width.min(72);
    let height = area.height.min(15);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(
        "↑↓ / jk       Select a stack, unit, terminal, or window\n←→ / hl       Collapse / expand\nHome / End    First / last item\ni / click     Send input to terminal; Ctrl-] returns to navigation\nPgUp / PgDn   Scroll terminal (hold Shift while sending input)\nShift+Home    Oldest output\nShift+End     Latest output; typing also returns to latest\nWheel         Scroll history; Shift overrides application mouse mode\ns / x / r     Start / stop / restart selected stack or unit\nR / w / g     Reload / rewatch / resync\nEvents        Drag its header to resize, down to one header line\nEsc / ?       Close help"
    ).block(Block::default().title(" Mustard keys ").borders(Borders::ALL)).wrap(Wrap { trim: false }), popup);
}

fn scroll_start(selected: usize, length: usize, available: usize) -> usize {
    if available == 0 || length <= available {
        0
    } else {
        selected
            .saturating_sub(available / 2)
            .min(length.saturating_sub(available))
    }
}

fn phase_style(phase: &str) -> Style {
    let color = match phase {
        "running" | "ready" | "exited" => Color::Green,
        "activating" | "waiting" | "backoff" | "start" | "restart" => Color::Yellow,
        "failed" | "invalid" => Color::Red,
        "stopped" | "held" | "unloaded" => Color::DarkGray,
        _ => Color::Cyan,
    };
    Style::default().fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio::sync::mpsc;

    fn app() -> App {
        let (commands, _) = mpsc::channel(1);
        App::new(None, String::new(), commands)
    }

    fn add_unit(app: &mut App, value: serde_json::Value) {
        let unit: MusterUnit = serde_json::from_value(value).unwrap();
        app.state.units.insert(unit.name.clone(), unit);
        app.state.ready = true;
        app.initialize_groups();
    }

    #[test]
    fn tree_fits_rendered_content_and_tracks_expansion() {
        let mut app = app();
        let name = "服务-e\u{301}abcdefghijklmnop";
        add_unit(
            &mut app,
            serde_json::json!({
                "name": name, "phase": "running", "stale": true,
                "surfaces": [{"id": "0000000000000009", "title": "z".repeat(40)}]
            }),
        );
        let area = Rect::new(0, 0, 100, 30);
        // Shared divider + indentation + marker + 22 display cells + phase + stale.
        assert_eq!(regions(area, &app, &app.rows()).tree.width, 41);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert_eq!(terminal.backend().buffer()[(39, 2)].symbol(), "e");
        assert_eq!(terminal.backend().buffer()[(40, 2)].symbol(), "├");

        app.expanded.insert(NodeKey::Unit(name.into()));
        assert_eq!(regions(area, &app, &app.rows()).tree.width, 57);
        assert_eq!(terminal_dimensions(area, &app).0, 43);
        app.expanded.remove(&NodeKey::Unit(name.into()));
        assert_eq!(regions(area, &app, &app.rows()).tree.width, 41);
        app.expanded.remove(&NodeKey::Standalone);
        assert_eq!(regions(area, &app, &app.rows()).tree.width, 20);
    }

    #[test]
    fn layout_reserves_terminal_space_and_clamps_events_on_small_screens() {
        let mut app = app();
        add_unit(&mut app, serde_json::json!({"name": "x".repeat(200)}));
        app.selected = 1;
        app.event_height = u16::MAX;
        let area = Rect::new(5, 4, 80, 30);
        let layout = regions(area, &app, &app.rows());
        assert_eq!(layout.tree.width, 50);
        assert_eq!(layout.terminal.width, 30);
        assert_eq!(layout.events.height, 18);
        assert_eq!(layout.detail.height, 7);
        assert_eq!(layout.terminal.height, 3);
        assert_eq!(resize_events(area, 0), 18);
        assert_eq!(resize_events(area, u16::MAX), 1);

        for size in 0..30 {
            let area = Rect::new(5, 4, size, size);
            let layout = regions(area, &app, &app.rows());
            for region in [
                layout.tree,
                layout.detail,
                layout.terminal,
                layout.events,
                layout.footer,
            ] {
                assert_eq!(region.intersection(area), region);
            }
            assert_eq!(layout.terminal.width, size / 2);
            let (cols, rows) = terminal_dimensions(area, &app);
            assert!(cols >= 1 && rows >= 1);
        }
    }

    #[tokio::test]
    async fn events_drag_captures_mouse_and_resizes_the_terminal_view() {
        use crate::mustard::handle_mouse;
        use crate::yas_terminal::stream::{InteractiveView, ViewCommand};

        let mut app = app();
        add_unit(
            &mut app,
            serde_json::json!({"name": "api", "pty": "0000000000000001"}),
        );
        app.selected = 1;
        app.terminal_handle = Some(1);
        app.terminal_focus = true;
        let area = Rect::new(0, 0, 100, 40);
        let layout = regions(area, &app, &app.rows());
        app.terminal_size = terminal_dimensions(area, &app);
        let (commands, mut received) = mpsc::channel(8);
        let (_, updates) = mpsc::channel(1);
        app.view = Some(InteractiveView {
            commands,
            updates,
            task: tokio::spawn(async {}),
        });
        let column = layout.terminal.x + 2;
        let mouse = |kind, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };

        assert_eq!(
            mouse_target(area, &app, column, layout.events.y),
            MouseTarget::EventsDivider
        );
        handle_mouse(
            &mut app,
            area,
            mouse(MouseEventKind::Down(MouseButton::Left), layout.events.y),
        )
        .await;
        assert!(app.resizing_events);
        // Move and release over terminal cells from the original layout.
        handle_mouse(
            &mut app,
            area,
            mouse(MouseEventKind::Drag(MouseButton::Left), layout.events.y - 4),
        )
        .await;
        assert_eq!(app.event_height, 10);
        assert!(
            received.try_recv().is_err(),
            "divider drag must not send terminal mouse input"
        );
        app.sync_view(area).await;
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Resize { rows: 20, cols: 80 })
        ));
        handle_mouse(
            &mut app,
            area,
            mouse(MouseEventKind::Up(MouseButton::Left), layout.events.y - 5),
        )
        .await;
        assert!(!app.resizing_events);
        assert_eq!(app.event_height, 11);
        assert!(app.terminal_focus);
        assert!(received.try_recv().is_err());
        let layout = regions(area, &app, &app.rows());
        assert_eq!(
            mouse_target(area, &app, column, layout.events.y),
            MouseTarget::EventsDivider
        );
        handle_mouse(
            &mut app,
            area,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                layout.terminal.y + 1,
            ),
        )
        .await;
        assert!(matches!(received.try_recv(), Ok(ViewCommand::Focus(true))));
        assert!(matches!(
            received.try_recv(),
            Ok(ViewCommand::Mouse {
                event: "down",
                column: 2,
                row: 0,
                ..
            })
        ));
    }

    #[test]
    fn tree_scroll_keeps_selection_visible() {
        assert_eq!(scroll_start(0, 20, 5), 0);
        assert_eq!(scroll_start(10, 20, 5), 8);
        assert_eq!(scroll_start(19, 20, 5), 15);
    }

    #[test]
    fn terminal_cell_decodes_text_and_color() {
        let mut cell = [0; 12];
        cell[0] = 2;
        cell[1] = 1 << 3;
        cell[2..5].copy_from_slice(&[1, 2, 3]);
        cell[8] = b'x';
        assert_eq!(cell_symbol(&cell, None), "x");
        assert_eq!(cell_style(&cell).fg, Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn mouse_hit_testing_tracks_scrolled_tree_and_terminal_cells() {
        let mut app = app();
        for index in 0..20 {
            add_unit(
                &mut app,
                serde_json::json!({"name": format!("unit-{index:02}")}),
            );
        }
        app.selected = 19;
        let rows = app.rows();
        let area = Rect::new(0, 0, 100, 24);
        let layout = regions(area, &app, &rows);
        app.selected = 0;
        assert_eq!(regions(area, &app, &rows).tree.width, layout.tree.width);
        app.selected = 19;
        let start = scroll_start(19, rows.len(), usize::from(tree_inner(layout.tree).height));
        assert_eq!(
            mouse_target(area, &app, layout.tree.x + 1, layout.tree.y + 1),
            MouseTarget::TreeRow {
                index: start,
                disclosure: rows[start].depth == 0,
            }
        );
        assert_eq!(
            mouse_target(area, &app, layout.terminal.x + 4, layout.terminal.y + 3),
            MouseTarget::TerminalCell { column: 4, row: 2 }
        );
    }
}
