//! Browser renderer state for the private JS-to-Wasm snapshot boundary.
//!
//! This is deliberately not a transport codec. TypeScript first validates and
//! applies the normative `yas.terminal.grid/1` frame, then sends one complete
//! renderer snapshot here. Keeping this small decoder local prevents the
//! browser package from depending on the retired packet protocol.

use std::collections::BTreeMap;

use lz4_flex::block::decompress_size_prepended;

pub const CELL_SIZE: usize = 12;
const MAX_CELL_COUNT: usize = 500_000;
const MAX_DECOMPRESSED: usize = 64 * 1024 * 1024;
const TITLE_PRESENT: u16 = 1 << 15;
const OPS_PRESENT: u16 = 1 << 14;
const STRINGS_PRESENT: u16 = 1 << 13;
const LINE_FLAGS_PRESENT: u16 = 1 << 12;
const TITLE_LEN_MASK: u16 = LINE_FLAGS_PRESENT - 1;
const OP_FILL_RECT: u8 = 0x02;
const OP_PATCH_CELLS: u8 = 0x03;
const ROW_FLAG_WRAPPED: u8 = 1;
const CONTENT_OVERFLOW: usize = 7;

#[derive(Clone, Debug)]
pub struct TerminalState {
    rows: u16,
    cols: u16,
    cursor_row: u16,
    cursor_col: u16,
    mode: u16,
    title: String,
    cells: Vec<u8>,
    overflow: BTreeMap<usize, String>,
    line_flags: Vec<u8>,
    scrollback_lines: u32,
    cell_links: Vec<u16>,
    link_uris: BTreeMap<u16, String>,
}

impl TerminalState {
    pub fn new(rows: u16, cols: u16) -> Self {
        let cells = usize::from(rows)
            .checked_mul(usize::from(cols))
            .and_then(|count| count.checked_mul(CELL_SIZE))
            .map_or_else(Vec::new, |len| vec![0; len]);
        Self {
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            mode: 0,
            title: String::new(),
            cells,
            overflow: BTreeMap::new(),
            line_flags: vec![0; usize::from(rows)],
            scrollback_lines: 0,
            cell_links: Vec::new(),
            link_uris: BTreeMap::new(),
        }
    }

    pub const fn frame(&self) -> &Self {
        self
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

    pub fn overflow(&self) -> &BTreeMap<usize, String> {
        &self.overflow
    }

    pub const fn scrollback_lines(&self) -> u32 {
        self.scrollback_lines
    }

    pub fn is_wrapped(&self, row: u16) -> bool {
        self.line_flags
            .get(usize::from(row))
            .is_some_and(|flags| flags & ROW_FLAG_WRAPPED != 0)
    }

    pub fn cell_content(&self, row: u16, col: u16) -> &str {
        if row >= self.rows || col >= self.cols {
            return "";
        }
        let cell_index = usize::from(row) * usize::from(self.cols) + usize::from(col);
        let offset = cell_index * CELL_SIZE;
        let cell = &self.cells[offset..offset + CELL_SIZE];
        if cell[1] & 4 != 0 {
            return "";
        }
        let len = usize::from((cell[1] >> 3) & 7);
        if len == CONTENT_OVERFLOW {
            return self.overflow.get(&cell_index).map_or("", String::as_str);
        }
        if len == 0 {
            return " ";
        }
        std::str::from_utf8(&cell[8..8 + len]).unwrap_or("")
    }

    pub fn has_links(&self) -> bool {
        !self.link_uris.is_empty()
    }

    pub fn cell_link(&self, row: u16, col: u16) -> Option<&str> {
        let index = self.link_index(row, col)?;
        let id = self.cell_links.get(index).copied().unwrap_or(0);
        (id != 0)
            .then(|| self.link_uris.get(&id).map(String::as_str))
            .flatten()
    }

    pub fn link_segments(&self, row: u16, col: u16) -> Vec<(u16, u16, u16)> {
        let Some(index) = self.link_index(row, col) else {
            return Vec::new();
        };
        let id = self.cell_links.get(index).copied().unwrap_or(0);
        if id == 0 || self.cols == 0 {
            return Vec::new();
        }
        let last_col = self.cols - 1;
        let (mut start_row, mut start_col) = (row, col);
        loop {
            while start_col > 0 && self.link_id(start_row, start_col - 1) == id {
                start_col -= 1;
            }
            if start_col != 0 || start_row == 0 {
                break;
            }
            let previous = start_row - 1;
            if !self.is_wrapped(previous) || self.link_id(previous, last_col) != id {
                break;
            }
            start_row = previous;
            start_col = last_col;
        }
        let mut segments = Vec::new();
        let (mut current_row, mut segment_start) = (start_row, start_col);
        loop {
            let mut end = segment_start;
            while end < last_col && self.link_id(current_row, end + 1) == id {
                end += 1;
            }
            segments.push((current_row, segment_start, end));
            if end != last_col
                || current_row + 1 >= self.rows
                || !self.is_wrapped(current_row)
                || self.link_id(current_row + 1, 0) != id
            {
                break;
            }
            current_row += 1;
            segment_start = 0;
        }
        segments
    }

    pub fn feed_compressed(&mut self, data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        let declared = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        if declared > MAX_DECOMPRESSED {
            return false;
        }
        let Ok(payload) = decompress_size_prepended(data) else {
            return false;
        };
        self.apply_snapshot(&payload)
    }

    pub fn feed_compressed_batch(&mut self, batch: &[u8]) -> bool {
        let mut cursor = Cursor::new(batch);
        let mut changed = false;
        while let Some(len) = cursor.u32().map(|value| value as usize) {
            if len == 0 {
                break;
            }
            let Some(frame) = cursor.take(len) else {
                break;
            };
            changed |= self.feed_compressed(frame);
        }
        changed
    }

    fn apply_snapshot(&mut self, payload: &[u8]) -> bool {
        let Some(next) = Self::decode_snapshot(payload) else {
            return false;
        };
        *self = next;
        true
    }

    fn decode_snapshot(payload: &[u8]) -> Option<Self> {
        let mut cursor = Cursor::new(payload);
        let rows = cursor.u16()?;
        let cols = cursor.u16()?;
        let total = usize::from(rows).checked_mul(usize::from(cols))?;
        if rows == 0 || cols == 0 || total > MAX_CELL_COUNT {
            return None;
        }
        let cursor_row = cursor.u16()?.min(rows - 1);
        let cursor_col = cursor.u16()?.min(cols - 1);
        let mode = cursor.u16()?;
        let title_field = cursor.u16()?;
        if title_field & TITLE_PRESENT == 0
            || title_field & OPS_PRESENT == 0
            || title_field & LINE_FLAGS_PRESENT == 0
        {
            return None;
        }
        let title = std::str::from_utf8(cursor.take(usize::from(title_field & TITLE_LEN_MASK))?)
            .ok()?
            .to_owned();
        if cursor.u16()? != 2 || cursor.u8()? != OP_FILL_RECT {
            return None;
        }
        if cursor.u16()? != 0
            || cursor.u16()? != 0
            || cursor.u16()? != rows
            || cursor.u16()? != cols
        {
            return None;
        }
        if cursor.take(CELL_SIZE)? != [0; CELL_SIZE] || cursor.u8()? != OP_PATCH_CELLS {
            return None;
        }
        let bitmap_len = total.div_ceil(8);
        let bitmap = cursor.take(bitmap_len)?;
        if bitmap.iter().enumerate().any(|(index, byte)| {
            let valid = if index + 1 == bitmap_len && total % 8 != 0 {
                (1u8 << (total % 8)) - 1
            } else {
                u8::MAX
            };
            *byte != valid
        }) {
            return None;
        }
        let mut cells = vec![0; total.checked_mul(CELL_SIZE)?];
        for plane in 0..CELL_SIZE {
            let values = cursor.take(total)?;
            for (cell, value) in values.iter().copied().enumerate() {
                cells[cell * CELL_SIZE + plane] = value;
            }
        }
        let mut overflow = BTreeMap::new();
        if title_field & STRINGS_PRESENT != 0 {
            let count = usize::from(cursor.u16()?);
            if count > total {
                return None;
            }
            for _ in 0..count {
                let index = usize::try_from(cursor.u32()?).ok()?;
                let len = usize::from(cursor.u16()?);
                let value = std::str::from_utf8(cursor.take(len)?).ok()?.to_owned();
                if index >= total || overflow.insert(index, value).is_some() {
                    return None;
                }
            }
        }
        let line_flags = cursor.take(usize::from(rows))?.to_vec();
        let scrollback_lines = cursor.u32()?;
        let uri_count = usize::from(cursor.u16()?);
        let mut link_uris = BTreeMap::new();
        for _ in 0..uri_count {
            let id = cursor.u16()?;
            let len = usize::from(cursor.u16()?);
            let uri = std::str::from_utf8(cursor.take(len)?).ok()?.to_owned();
            if id == 0 || link_uris.insert(id, uri).is_some() {
                return None;
            }
        }
        let run_count = usize::from(cursor.u16()?);
        let mut cell_links = vec![0; total];
        for _ in 0..run_count {
            let start = usize::try_from(cursor.u32()?).ok()?;
            let len = usize::from(cursor.u16()?);
            let id = cursor.u16()?;
            let end = start.checked_add(len)?;
            if start >= total || end > total || id == 0 || !link_uris.contains_key(&id) {
                return None;
            }
            cell_links[start..end].fill(id);
        }
        if !cursor.is_empty() {
            return None;
        }
        Some(Self {
            rows,
            cols,
            cursor_row,
            cursor_col,
            mode,
            title,
            cells,
            overflow,
            line_flags,
            scrollback_lines,
            cell_links,
            link_uris,
        })
    }

    fn link_index(&self, row: u16, col: u16) -> Option<usize> {
        if row >= self.rows || col >= self.cols || self.cell_links.is_empty() {
            return None;
        }
        let mut index = usize::from(row) * usize::from(self.cols) + usize::from(col);
        if self.cells[index * CELL_SIZE + 1] & 4 != 0 && col > 0 {
            index -= 1;
        }
        Some(index)
    }

    fn link_id(&self, row: u16, col: u16) -> u16 {
        self.link_index(row, col)
            .and_then(|index| self.cell_links.get(index).copied())
            .unwrap_or(0)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(len)?;
        let value = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(value)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(*self.take(1)?.first()?)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn applies_complete_renderer_snapshot_without_packet_protocol() {
        let mut raw = Vec::new();
        u16(&mut raw, 1); // rows
        u16(&mut raw, 2); // columns
        u16(&mut raw, 0); // cursor row
        u16(&mut raw, 1); // cursor column
        u16(&mut raw, 3); // modes
        u16(
            &mut raw,
            TITLE_PRESENT | OPS_PRESENT | LINE_FLAGS_PRESENT | 1,
        );
        raw.push(b'x');
        u16(&mut raw, 2); // operations
        raw.push(OP_FILL_RECT);
        for value in [0, 0, 1, 2] {
            u16(&mut raw, value);
        }
        raw.extend_from_slice(&[0; CELL_SIZE]);
        raw.push(OP_PATCH_CELLS);
        raw.push(0b11); // both cells
        let mut cells = [[0u8; CELL_SIZE]; 2];
        cells[0][1] = 1 << 3;
        cells[0][8] = b'a';
        cells[1][1] = (1 << 3) | 64;
        cells[1][8] = b'b';
        for (left, right) in cells[0].iter().zip(cells[1].iter()) {
            raw.push(*left);
            raw.push(*right);
        }
        raw.push(ROW_FLAG_WRAPPED);
        u32(&mut raw, 7);
        u16(&mut raw, 1); // URI table
        u16(&mut raw, 1);
        u16(&mut raw, 8);
        raw.extend_from_slice(b"https://");
        u16(&mut raw, 1); // link run
        u32(&mut raw, 1);
        u16(&mut raw, 1);
        u16(&mut raw, 1);

        let encoded = lz4_flex::block::compress_prepend_size(&raw);
        let mut state = TerminalState::new(24, 80);
        assert!(state.feed_compressed(&encoded));
        assert_eq!((state.rows(), state.cols()), (1, 2));
        assert_eq!((state.cursor_row(), state.cursor_col()), (0, 1));
        assert_eq!(state.title(), "x");
        assert_eq!(state.cell_content(0, 0), "a");
        assert_eq!(state.cell_content(0, 1), "b");
        assert_eq!(state.cell_link(0, 1), Some("https://"));
        assert_eq!(state.link_segments(0, 1), vec![(0, 1, 1)]);
        assert!(state.is_wrapped(0));
        assert_eq!(state.scrollback_lines(), 7);
    }

    #[test]
    fn rejects_oversized_renderer_snapshot_before_allocation() {
        let mut bytes = (MAX_DECOMPRESSED as u32 + 1).to_le_bytes().to_vec();
        bytes.push(0);
        assert!(!TerminalState::new(24, 80).feed_compressed(&bytes));
    }

    #[test]
    fn blank_cells_are_text_spaces_but_wide_continuations_are_empty() {
        let mut state = TerminalState::new(1, 2);
        assert_eq!(state.cell_content(0, 0), " ");

        state.cells[CELL_SIZE + 1] = 1 << 2;
        assert_eq!(state.cell_content(0, 1), "");
    }
}
