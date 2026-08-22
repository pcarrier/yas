//! Protocol-neutral terminal frame state shared by the emulator and native
//! YAS server.
//!
//! This crate deliberately contains no transport opcodes, packet framing, or
//! compatibility codecs. It is the semantic terminal model from which native
//! `TerminalFrame` values are produced.

use std::collections::BTreeMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Bytes in the private renderer representation of one terminal cell.
pub const CELL_SIZE: usize = 12;
/// Maximum number of cells retained in one terminal frame.
pub const MAX_CELL_COUNT: usize = 500_000;
/// Per-row flag indicating that content continues on the next row.
pub const ROW_FLAG_WRAPPED: u8 = 1 << 0;
/// Cell flag indicating an OSC 8 hyperlink.
pub const CELL_FLAG1_LINK: u8 = 1 << 6;
/// Sentinel content length for text stored in the overflow table.
pub const CONTENT_OVERFLOW: u8 = 7;
/// Largest hyperlink identifier; zero is reserved for no link.
pub const MAX_LINK_ID: u16 = u16::MAX - 1;
/// Longest OSC 8 URI retained in a frame.
pub const MAX_LINK_URI: usize = 4096;
/// Longest absolute working-directory path retained from a terminal report.
pub const TERM_CWD_MAX: usize = 4096;
/// Grace between a deadline-triggered soft stop and forced termination.
pub const DEADLINE_STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Command is still running.
pub const COMMAND_RUNNING: u8 = 1 << 0;
/// [`CommandRecord::exit_code`] is meaningful.
pub const COMMAND_HAS_EXIT: u8 = 1 << 1;
/// The shell did not delimit a recoverable command line.
pub const COMMAND_NO_TEXT: u8 = 1 << 2;
/// The command was closed by a later prompt instead of a completion marker.
pub const COMMAND_INCOMPLETE: u8 = 1 << 3;
/// The first part of the command output has left scrollback.
pub const COMMAND_OUTPUT_EVICTED: u8 = 1 << 4;
/// The terminal process exited while the command was running.
pub const COMMAND_TERMINAL_EXITED: u8 = 1 << 5;
/// No platform exit status was available for a terminated process.
pub const EXIT_STATUS_UNKNOWN: i32 = i32::MIN;
/// The process ended without a server-enforced deadline or lease action.
pub const EXIT_REASON_NORMAL: u8 = 0;
/// The server terminated the process after its configured deadline.
pub const EXIT_REASON_DEADLINE: u8 = 1;
/// The server terminated the process after its owning lease expired.
pub const EXIT_REASON_LEASE: u8 = 2;
/// The server evicted the process while reclaiming resources.
pub const EXIT_REASON_GC: u8 = 3;
/// A unit supervisor explicitly stopped the process.
pub const EXIT_REASON_UNIT_STOP: u8 = 4;

/// Protocol-neutral command-journal entry derived from semantic prompt marks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandRecord {
    pub index: u64,
    pub flags: u8,
    pub exit_code: i32,
    pub start_seq: u64,
    pub end_seq: u64,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub command: String,
}

impl CommandRecord {
    pub const fn running(&self) -> bool {
        self.flags & COMMAND_RUNNING != 0
    }

    pub const fn exit(&self) -> Option<i32> {
        if self.flags & COMMAND_HAS_EXIT != 0 {
            Some(self.exit_code)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub row: u16,
    pub col: u16,
    pub rows: u16,
    pub cols: u16,
}

impl Rect {
    pub const fn new(row: u16, col: u16, rows: u16, cols: u16) -> Self {
        Self {
            row,
            col,
            rows,
            cols,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameState {
    // Public for renderer integration. Other callers should use the checked
    // methods below.
    #[doc(hidden)]
    pub rows: u16,
    #[doc(hidden)]
    pub cols: u16,
    #[doc(hidden)]
    pub cells: Vec<u8>,
    #[doc(hidden)]
    pub cursor_row: u16,
    #[doc(hidden)]
    pub cursor_col: u16,
    #[doc(hidden)]
    pub mode: u16,
    #[doc(hidden)]
    pub title: String,
    #[doc(hidden)]
    pub overflow: BTreeMap<usize, String>,
    #[doc(hidden)]
    pub line_flags: Vec<u8>,
    #[doc(hidden)]
    pub scrollback_lines: u32,
    #[doc(hidden)]
    pub cell_links: Vec<u16>,
    #[doc(hidden)]
    pub link_uris: BTreeMap<u16, String>,
}

impl FrameState {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self::try_new(rows, cols).unwrap_or_default()
    }

    /// Construct a frame only when its complete cell storage fits the shared
    /// hard cap. This is the checked entry point for dimensions derived from a
    /// peer or backend.
    pub fn try_new(rows: u16, cols: u16) -> Option<Self> {
        let total = usize::from(rows).saturating_mul(usize::from(cols));
        if total > MAX_CELL_COUNT {
            return None;
        }
        Some(Self {
            rows,
            cols,
            cells: vec![0; total * CELL_SIZE],
            cursor_row: 0,
            cursor_col: 0,
            mode: 0,
            title: String::new(),
            overflow: BTreeMap::new(),
            line_flags: vec![0; usize::from(rows)],
            scrollback_lines: 0,
            cell_links: Vec::new(),
            link_uris: BTreeMap::new(),
        })
    }

    pub fn from_parts(
        rows: u16,
        cols: u16,
        cursor_row: u16,
        cursor_col: u16,
        mode: u16,
        title: impl Into<String>,
        cells: Vec<u8>,
    ) -> Self {
        let mut state = Self::new(rows, cols);
        if cells.len() == state.cells.len() {
            state.cells = cells;
        }
        state.cursor_row = cursor_row.min(rows.saturating_sub(1));
        state.cursor_col = cursor_col.min(cols.saturating_sub(1));
        state.mode = mode;
        state.title = title.into();
        state
    }

    pub const fn rows(&self) -> u16 {
        self.rows
    }

    pub const fn cols(&self) -> u16 {
        self.cols
    }

    pub const fn cursor_row(&self) -> u16 {
        self.cursor_row
    }

    pub const fn cursor_col(&self) -> u16 {
        self.cursor_col
    }

    pub const fn mode(&self) -> u16 {
        self.mode
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut [u8] {
        &mut self.cells
    }

    pub const fn overflow(&self) -> &BTreeMap<usize, String> {
        &self.overflow
    }

    pub const fn overflow_mut(&mut self) -> &mut BTreeMap<usize, String> {
        &mut self.overflow
    }

    pub fn cell_links(&self) -> &[u16] {
        &self.cell_links
    }

    pub const fn link_uris(&self) -> &BTreeMap<u16, String> {
        &self.link_uris
    }

    pub fn has_links(&self) -> bool {
        !self.link_uris.is_empty()
    }

    pub fn cell_link(&self, row: u16, col: u16) -> Option<&str> {
        if row >= self.rows || col >= self.cols || self.cell_links.is_empty() {
            return None;
        }
        let mut flat = usize::from(row) * usize::from(self.cols) + usize::from(col);
        if self.cells[flat * CELL_SIZE + 1] & 4 != 0 && col > 0 {
            flat -= 1;
        }
        let id = *self.cell_links.get(flat)?;
        (id != 0)
            .then(|| self.link_uris.get(&id).map(String::as_str))
            .flatten()
    }

    fn link_id_at(&self, row: u16, col: u16) -> u16 {
        if row >= self.rows || col >= self.cols || self.cell_links.is_empty() {
            return 0;
        }
        let mut flat = usize::from(row) * usize::from(self.cols) + usize::from(col);
        if self.cells[flat * CELL_SIZE + 1] & 4 != 0 && col > 0 {
            flat -= 1;
        }
        self.cell_links.get(flat).copied().unwrap_or(0)
    }

    pub fn link_segments(&self, row: u16, col: u16) -> Vec<(u16, u16, u16)> {
        let id = self.link_id_at(row, col);
        if id == 0 || self.cols == 0 {
            return Vec::new();
        }
        let last_col = self.cols - 1;
        let (mut start_row, mut start_col) = (row, col);
        loop {
            while start_col > 0 && self.link_id_at(start_row, start_col - 1) == id {
                start_col -= 1;
            }
            if start_col != 0 || start_row == 0 {
                break;
            }
            let previous = start_row - 1;
            if !self.is_wrapped(previous) || self.link_id_at(previous, last_col) != id {
                break;
            }
            start_row = previous;
            start_col = last_col;
        }

        let mut segments = Vec::new();
        let (mut current_row, mut segment_start) = (start_row, start_col);
        loop {
            let mut end_col = segment_start;
            while end_col < last_col && self.link_id_at(current_row, end_col + 1) == id {
                end_col += 1;
            }
            segments.push((current_row, segment_start, end_col));
            if end_col != last_col
                || current_row + 1 >= self.rows
                || !self.is_wrapped(current_row)
                || self.link_id_at(current_row + 1, 0) != id
            {
                break;
            }
            current_row += 1;
            segment_start = 0;
        }
        segments
    }

    pub fn set_links(&mut self, cell_links: Vec<u16>, link_uris: BTreeMap<u16, String>) {
        let total = usize::from(self.rows) * usize::from(self.cols);
        if link_uris.is_empty() || cell_links.len() != total {
            self.clear_links();
            return;
        }
        let (mut cell_links, mut link_uris) = (cell_links, link_uris);
        if link_uris.values().any(|uri| uri.len() > MAX_LINK_URI) {
            link_uris.retain(|_, uri| uri.len() <= MAX_LINK_URI);
            if link_uris.is_empty() {
                self.clear_links();
                return;
            }
            for slot in &mut cell_links {
                if *slot != 0 && !link_uris.contains_key(slot) {
                    *slot = 0;
                }
            }
        }
        self.cell_links = cell_links;
        self.link_uris = link_uris;
    }

    pub fn clear_links(&mut self) {
        self.cell_links.clear();
        self.link_uris.clear();
    }

    pub fn line_flags(&self) -> &[u8] {
        &self.line_flags
    }

    pub const fn line_flags_mut(&mut self) -> &mut Vec<u8> {
        &mut self.line_flags
    }

    pub const fn scrollback_lines(&self) -> u32 {
        self.scrollback_lines
    }

    pub const fn set_scrollback_lines(&mut self, lines: u32) {
        self.scrollback_lines = lines;
    }

    pub fn is_wrapped(&self, row: u16) -> bool {
        self.line_flags.get(usize::from(row)).copied().unwrap_or(0) & ROW_FLAG_WRAPPED != 0
    }

    pub fn set_wrapped(&mut self, row: u16, wrapped: bool) {
        if let Some(flags) = self.line_flags.get_mut(usize::from(row)) {
            if wrapped {
                *flags |= ROW_FLAG_WRAPPED;
            } else {
                *flags &= !ROW_FLAG_WRAPPED;
            }
        }
    }

    pub fn cell_content(&self, row: u16, col: u16) -> &str {
        if row >= self.rows || col >= self.cols {
            return "";
        }
        let flat = usize::from(row) * usize::from(self.cols) + usize::from(col);
        let offset = flat * CELL_SIZE;
        let flags = self.cells[offset + 1];
        if flags & 4 != 0 {
            return "";
        }
        let content_len = usize::from((flags >> 3) & 7);
        if content_len == usize::from(CONTENT_OVERFLOW) {
            return self.overflow.get(&flat).map_or("", String::as_str);
        }
        if content_len == 0 {
            return " ";
        }
        std::str::from_utf8(&self.cells[offset + 8..offset + 8 + content_len]).unwrap_or(" ")
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        let total = usize::from(rows).saturating_mul(usize::from(cols));
        if total > MAX_CELL_COUNT {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.cells = vec![0; total * CELL_SIZE];
        self.overflow.clear();
        self.clear_links();
        self.line_flags = vec![0; usize::from(rows)];
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    pub fn set_cursor(&mut self, row: u16, col: u16) {
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
    }

    pub const fn set_mode(&mut self, mode: u16) {
        self.mode = mode;
    }

    pub fn set_title(&mut self, title: impl Into<String>) -> bool {
        let title = title.into();
        if self.title == title {
            return false;
        }
        self.title = title;
        true
    }

    pub fn clear(&mut self, style: CellStyle) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.set_blank_cell(row, col, style);
            }
        }
    }

    pub fn fill_rect(&mut self, rect: Rect, ch: char, style: CellStyle) {
        let row_end = rect.row.saturating_add(rect.rows).min(self.rows);
        let col_end = rect.col.saturating_add(rect.cols).min(self.cols);
        for row in rect.row..row_end {
            let mut col = rect.col;
            while col < col_end {
                let width = self.set_cell(row, col, ch, style);
                if width == 0 {
                    break;
                }
                col = col.saturating_add(width);
            }
        }
    }

    pub fn write_text(&mut self, row: u16, col: u16, text: &str, style: CellStyle) -> u16 {
        if row >= self.rows || col >= self.cols {
            return col;
        }
        let mut current_col = col;
        for ch in text.chars() {
            if current_col >= self.cols {
                break;
            }
            let width = self.set_cell(row, current_col, ch, style);
            if width != 0 {
                current_col = current_col.saturating_add(width);
            }
        }
        current_col
    }

    pub fn write_wrapped_text(&mut self, rect: Rect, text: &str, style: CellStyle) -> usize {
        if rect.rows == 0 || rect.cols == 0 {
            return 0;
        }
        let lines = wrap_text_lines(text, usize::from(rect.cols));
        let max_rows = rect.rows.min(self.rows.saturating_sub(rect.row));
        for (index, line) in lines.iter().take(usize::from(max_rows)).enumerate() {
            self.write_text(rect.row + index as u16, rect.col, line, style);
        }
        lines.len()
    }

    pub fn write_scrolling_text<S: AsRef<str>>(
        &mut self,
        rect: Rect,
        lines: &[S],
        offset_from_bottom: usize,
        style: CellStyle,
    ) {
        if rect.rows == 0 || rect.cols == 0 {
            return;
        }
        let mut wrapped = Vec::with_capacity(lines.len());
        for line in lines {
            let output = wrap_text_lines(line.as_ref(), usize::from(rect.cols));
            if output.is_empty() {
                wrapped.push(String::new());
            } else {
                wrapped.extend(output);
            }
        }
        let end = wrapped.len().saturating_sub(offset_from_bottom);
        let start = end.saturating_sub(usize::from(rect.rows));
        for row in 0..rect.rows {
            self.fill_rect(
                Rect::new(rect.row + row, rect.col, 1, rect.cols),
                ' ',
                style,
            );
        }
        for (index, line) in wrapped[start..end].iter().enumerate() {
            self.write_text(rect.row + index as u16, rect.col, line, style);
        }
    }

    pub fn get_text(&self, start_row: u16, start_col: u16, end_row: u16, end_col: u16) -> String {
        let mut result = String::new();
        if self.rows == 0 || self.cols == 0 {
            return result;
        }
        let last_row = end_row.min(self.rows - 1);
        for row in start_row..=last_row {
            let first_col = if row == start_row { start_col } else { 0 };
            let last_col = if row == end_row {
                end_col
            } else {
                self.cols - 1
            };
            let mut line = String::new();
            let mut col = first_col;
            while col <= last_col.min(self.cols - 1) {
                line.push_str(self.cell_content(row, col));
                col += 1;
            }
            let wrapped = self.is_wrapped(row);
            if wrapped {
                result.push_str(&line);
            } else {
                result.push_str(line.trim_end());
            }
            if row < last_row && !wrapped {
                result.push('\n');
            }
        }
        result
    }

    pub fn get_all_text(&self) -> String {
        if self.rows == 0 || self.cols == 0 {
            String::new()
        } else {
            self.get_text(0, 0, self.rows - 1, self.cols - 1)
        }
    }

    fn cell_style(&self, row: u16, col: u16) -> CellStyle {
        if row >= self.rows || col >= self.cols {
            return CellStyle::default();
        }
        let offset = self.cell_offset(row, col);
        let flags0 = self.cells[offset];
        let flags1 = self.cells[offset + 1];
        let color = |kind: u8, bytes: &[u8]| match kind {
            1 => Color::Indexed(bytes[0]),
            2 => Color::Rgb(bytes[0], bytes[1], bytes[2]),
            _ => Color::Default,
        };
        CellStyle {
            fg: color(flags0 & 3, &self.cells[offset + 2..offset + 5]),
            bg: color((flags0 >> 2) & 3, &self.cells[offset + 5..offset + 8]),
            bold: flags0 & (1 << 4) != 0,
            dim: flags0 & (1 << 5) != 0,
            italic: flags0 & (1 << 6) != 0,
            underline: flags0 & (1 << 7) != 0,
            inverse: flags1 & 1 != 0,
        }
    }

    pub fn get_ansi_text(&self) -> String {
        if self.rows == 0 || self.cols == 0 {
            return String::new();
        }
        let mut result = String::new();
        let mut current_style = CellStyle::default();
        let mut current_link: Option<&str> = None;
        for row in 0..self.rows {
            let mut line = String::new();
            for col in 0..self.cols {
                let style = self.cell_style(row, col);
                if style != current_style {
                    push_sgr(&mut line, &style);
                    current_style = style;
                }
                let link = self.cell_link(row, col);
                if link != current_link {
                    push_osc8(&mut line, link);
                    current_link = link;
                }
                line.push_str(self.cell_content(row, col));
            }
            if current_link.is_some() {
                push_osc8(&mut line, None);
                current_link = None;
            }
            result.push_str(line.trim_end());
            if current_style != CellStyle::default() {
                result.push_str("\x1b[0m");
                current_style = CellStyle::default();
            }
            if row + 1 < self.rows {
                result.push('\n');
            }
        }
        result
    }

    pub fn get_cell(&self, row: u16, col: u16) -> Vec<u8> {
        if row >= self.rows || col >= self.cols {
            return Vec::new();
        }
        let offset = self.cell_offset(row, col);
        self.cells[offset..offset + CELL_SIZE].to_vec()
    }

    #[doc(hidden)]
    pub fn cell_offset(&self, row: u16, col: u16) -> usize {
        (usize::from(row) * usize::from(self.cols) + usize::from(col)) * CELL_SIZE
    }

    fn set_cell(&mut self, row: u16, col: u16, ch: char, style: CellStyle) -> u16 {
        if row >= self.rows || col >= self.cols {
            return 0;
        }
        let raw_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if raw_width == 0 {
            return 0;
        }
        let width = u16::from(raw_width > 1 && col + 1 < self.cols) + 1;
        let offset = self.cell_offset(row, col);
        encode_cell(
            &mut self.cells[offset..offset + CELL_SIZE],
            Some(ch),
            style,
            width == 2,
            false,
        );
        if width == 2 {
            let continuation = self.cell_offset(row, col + 1);
            encode_cell(
                &mut self.cells[continuation..continuation + CELL_SIZE],
                None,
                style,
                false,
                true,
            );
        }
        width
    }

    fn set_blank_cell(&mut self, row: u16, col: u16, style: CellStyle) {
        if row < self.rows && col < self.cols {
            let offset = self.cell_offset(row, col);
            encode_cell(
                &mut self.cells[offset..offset + CELL_SIZE],
                None,
                style,
                false,
                false,
            );
        }
    }
}

fn push_osc8(output: &mut String, uri: Option<&str>) {
    output.push_str("\x1b]8;;");
    if let Some(uri) = uri {
        output.extend(
            uri.chars()
                .filter(|character| !matches!(character, '\x1b' | '\x07')),
        );
    }
    output.push_str("\x1b\\");
}

fn push_sgr(output: &mut String, style: &CellStyle) {
    use std::fmt::Write;
    output.push_str("\x1b[0");
    for (enabled, code) in [
        (style.bold, 1),
        (style.dim, 2),
        (style.italic, 3),
        (style.underline, 4),
        (style.inverse, 7),
    ] {
        if enabled {
            let _ = write!(output, ";{code}");
        }
    }
    match style.fg {
        Color::Indexed(value) => {
            let _ = write!(output, ";38;5;{value}");
        }
        Color::Rgb(red, green, blue) => {
            let _ = write!(output, ";38;2;{red};{green};{blue}");
        }
        Color::Default => {}
    }
    match style.bg {
        Color::Indexed(value) => {
            let _ = write!(output, ";48;5;{value}");
        }
        Color::Rgb(red, green, blue) => {
            let _ = write!(output, ";48;2;{red};{green};{blue}");
        }
        Color::Default => {}
    }
    output.push('m');
}

fn encode_cell(
    destination: &mut [u8],
    ch: Option<char>,
    style: CellStyle,
    wide: bool,
    continuation: bool,
) {
    destination.fill(0);
    let mut flags0 = 0;
    encode_color(style.fg, &mut flags0, &mut destination[2..5], false);
    encode_color(style.bg, &mut flags0, &mut destination[5..8], true);
    flags0 |= u8::from(style.bold) << 4;
    flags0 |= u8::from(style.dim) << 5;
    flags0 |= u8::from(style.italic) << 6;
    flags0 |= u8::from(style.underline) << 7;
    destination[0] = flags0;

    let mut flags1 = u8::from(style.inverse);
    flags1 |= u8::from(wide) << 1;
    flags1 |= u8::from(continuation) << 2;
    if let Some(ch) = ch {
        let mut buffer = [0; 4];
        let encoded = ch.encode_utf8(&mut buffer).as_bytes();
        destination[8..8 + encoded.len()].copy_from_slice(encoded);
        flags1 |= (encoded.len() as u8) << 3;
    }
    destination[1] = flags1;
}

fn encode_color(color: Color, flags: &mut u8, destination: &mut [u8], background: bool) {
    let shift = usize::from(background) * 2;
    match color {
        Color::Default => {}
        Color::Indexed(index) => {
            *flags |= 1 << shift;
            destination[0] = index;
        }
        Color::Rgb(red, green, blue) => {
            *flags |= 2 << shift;
            destination.copy_from_slice(&[red, green, blue]);
        }
    }
}

fn wrap_text_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut output = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            output.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_width = 0;
        for word in paragraph.split_whitespace() {
            push_wrapped_word(word, width, &mut output, &mut line, &mut line_width);
        }
        if !line.is_empty() {
            output.push(line);
        }
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}

fn push_wrapped_word(
    word: &str,
    width: usize,
    output: &mut Vec<String>,
    line: &mut String,
    line_width: &mut usize,
) {
    let word_width = UnicodeWidthStr::width(word);
    if line.is_empty() && word_width <= width {
        line.push_str(word);
        *line_width = word_width;
        return;
    }
    if !line.is_empty() && *line_width + 1 + word_width <= width {
        line.push(' ');
        line.push_str(word);
        *line_width += word_width + 1;
        return;
    }
    if !line.is_empty() {
        output.push(std::mem::take(line));
        *line_width = 0;
        if word_width <= width {
            line.push_str(word);
            *line_width = word_width;
            return;
        }
    }
    for character in word.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(1).max(1);
        if *line_width + character_width > width && !line.is_empty() {
            output.push(std::mem::take(line));
            *line_width = 0;
        }
        line.push(character);
        *line_width += character_width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_count_and_link_invariants_are_enforced() {
        let mut frame = FrameState::new(2, 4);
        frame.write_text(0, 0, "wide 界", CellStyle::default());
        let mut links = BTreeMap::from([(1, "https://yas.run".to_owned())]);
        frame.set_links(vec![1; 8], links.clone());
        assert_eq!(frame.cell_link(0, 0), Some("https://yas.run"));

        links.insert(2, "x".repeat(MAX_LINK_URI + 1));
        frame.set_links(vec![2; 8], links);
        assert_eq!(frame.cell_link(0, 0), None);
    }

    #[test]
    fn hostile_frame_dimensions_are_rejected_before_allocation() {
        assert!(FrameState::try_new(u16::MAX, u16::MAX).is_none());
        assert_eq!(FrameState::new(u16::MAX, u16::MAX), FrameState::default());
    }
}
