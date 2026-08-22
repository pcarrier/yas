//! Native Terminal view streams used by attach and recording.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::{Duration, Instant};

use yas_wire::{Decode, Encode, Extensions, family, terminal};

use super::{
    NativeClient, close_view, find_terminal, open_view, recording, terminal_exit_code,
    watch_terminal_exit, wire_error,
};

const DETACH: u8 = 0x1d;

#[derive(Default)]
struct FrameAssembler {
    view_id: u32,
    sequence: u32,
    chunk_count: u16,
    logical_len: u32,
    chunks: Vec<Option<Vec<u8>>>,
    next_chunk: u16,
    received: usize,
}

impl FrameAssembler {
    fn push(
        &mut self,
        chunk: terminal::FrameChunk,
        expected_view: u32,
        maximum: u32,
    ) -> Result<Option<terminal::TerminalFrame>, String> {
        if chunk.view_id != expected_view || chunk.logical_frame_len > maximum {
            return Err(
                "YAS Terminal sent a frame chunk outside the negotiated view limits".into(),
            );
        }
        if self.chunks.is_empty() {
            self.view_id = chunk.view_id;
            self.sequence = chunk.frame_sequence;
            self.chunk_count = chunk.chunk_count;
            self.logical_len = chunk.logical_frame_len;
            self.chunks = vec![None; usize::from(chunk.chunk_count)];
            self.next_chunk = 0;
            self.received = 0;
        }
        if self.view_id != chunk.view_id
            || self.sequence != chunk.frame_sequence
            || self.chunk_count != chunk.chunk_count
            || self.logical_len != chunk.logical_frame_len
        {
            return Err("YAS Terminal interleaved or changed a chunked frame".into());
        }
        if chunk.chunk_index != self.next_chunk {
            return Err("YAS Terminal sent frame chunks out of order".into());
        }
        let slot = self
            .chunks
            .get_mut(usize::from(chunk.chunk_index))
            .ok_or_else(|| "YAS Terminal frame chunk index is out of bounds".to_string())?;
        if slot.is_some() {
            return Err("YAS Terminal repeated a frame chunk".into());
        }
        self.received = self
            .received
            .checked_add(chunk.chunk.len())
            .ok_or_else(|| "YAS Terminal frame length overflow".to_string())?;
        if self.received > self.logical_len as usize {
            return Err("YAS Terminal chunked frame exceeded its declared length".into());
        }
        *slot = Some(chunk.chunk);
        self.next_chunk += 1;
        if self.chunks.iter().any(Option::is_none) {
            return Ok(None);
        }
        if self.received != self.logical_len as usize {
            return Err("YAS Terminal chunked frame ended at the wrong length".into());
        }
        // FRAME_CHUNK repeats the view and sequence outside the logical body.
        // Decode that body without applying the ordinary FRAME's one-chunk
        // wire-size limit: exceeding it is why this frame was fragmented.
        let mut bytes = Vec::with_capacity(self.received);
        for chunk in self.chunks.drain(..) {
            bytes.extend(chunk.expect("all chunks were checked"));
        }
        self.next_chunk = 0;
        self.received = 0;
        terminal::TerminalFrame::decode_logical_body(self.view_id, self.sequence, &bytes)
            .map(Some)
            .map_err(wire_error)
    }
}

async fn next_terminal_frame(
    client: &mut NativeClient,
    view_id: u32,
    maximum: u32,
    assembler: &mut FrameAssembler,
) -> Result<terminal::TerminalFrame, String> {
    loop {
        let frame = client.next_event().await?;
        if frame.header.family != family::TERMINAL {
            continue;
        }
        match frame.header.kind {
            terminal::event_kind::FRAME => {
                if !frame.header.sensitive {
                    return Err("YAS Terminal FRAME was not marked sensitive".into());
                }
                if frame.payload.len() < 8 || frame.payload.len() - 8 > maximum as usize {
                    return Err("YAS Terminal FRAME exceeded the negotiated view limit".into());
                }
                let frame = terminal::TerminalFrame::decode(&frame.payload).map_err(wire_error)?;
                if frame.view_id == view_id {
                    return Ok(frame);
                }
            }
            terminal::event_kind::FRAME_CHUNK => {
                if !frame.header.sensitive {
                    return Err("YAS Terminal FRAME_CHUNK was not marked sensitive".into());
                }
                let chunk = terminal::FrameChunk::decode(&frame.payload).map_err(wire_error)?;
                if chunk.view_id == view_id
                    && let Some(frame) = assembler.push(chunk, view_id, maximum)?
                {
                    return Ok(frame);
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct GridState {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) cursor: (u16, u16),
    pub(crate) title: String,
    pub(crate) modes: u16,
    pub(crate) scrollback_lines: u32,
    pub(crate) scroll_offset: i64,
    pub(crate) cells: Vec<terminal::Cell>,
    pub(crate) overflow: BTreeMap<u32, String>,
    links: BTreeMap<u32, String>,
    cell_links: Vec<u32>,
    last_sequence: Option<u32>,
}

impl GridState {
    fn apply(&mut self, frame: &terminal::TerminalFrame, max_decoded: u32) -> Result<(), String> {
        let keyframe = frame.frame_flags & yas_wire::schema::terminal::FRAME_KEYFRAME as u16 != 0;
        if !keyframe {
            let expected = self
                .last_sequence
                .ok_or_else(|| "YAS Terminal sent a delta before a keyframe".to_string())?;
            if frame.base_sequence.is_some_and(|base| base != expected) {
                return Err("YAS Terminal delta referenced the wrong base frame".into());
            }
        }
        let base = (!keyframe).then_some((self.rows, self.cols));
        let grid = frame
            .decode_grid_codec1(max_decoded, base)
            .map_err(wire_error)?;
        if let Some((rows, cols)) = grid.dimensions {
            self.resize(rows, cols)?;
        }
        if let Some(cursor) = grid.cursor {
            self.cursor = cursor;
        }
        if let Some(title) = grid.title {
            self.title = title;
        }
        if let Some(modes) = grid.modes {
            self.modes = modes;
        }
        if let Some(lines) = grid.scrollback_lines {
            self.scrollback_lines = lines;
        }
        if let Some(offset) = grid.scroll_offset {
            self.scroll_offset = offset;
        }
        for operation in grid.operations {
            self.apply_operation(operation)?;
        }
        if frame.frame_flags & yas_wire::schema::terminal::FRAME_COMPONENTS as u16 != 0 {
            for component in grid.components {
                self.apply_component(component)?;
            }
        }
        self.last_sequence = Some(frame.frame_sequence);
        Ok(())
    }

    pub(crate) fn reports_mouse(&self) -> bool {
        (self.modes >> 4) & 7 != 0
    }

    pub(crate) fn cursor_visible(&self) -> bool {
        self.modes & 1 != 0 && self.scroll_offset == 0
    }

    fn apply_styled_lines(
        &mut self,
        lines: terminal::StyledLines,
        start: i64,
    ) -> Result<(), String> {
        for line in lines.0 {
            let row = line.row - start;
            if row < 0 || row >= i64::from(self.rows) {
                return Err(
                    "YAS Terminal READ returned a row outside the requested viewport".into(),
                );
            }
            let base = row as usize * usize::from(self.cols);
            for (offset, cell) in line.cells.iter().enumerate() {
                let col = u64::from(line.start_col) + offset as u64;
                if col < u64::from(self.cols) {
                    self.replace_cell(base + col as usize, *cell)?;
                }
            }
            for overflow in line.overflow {
                let col = u64::from(line.start_col) + u64::from(overflow.cell_offset);
                if col < u64::from(self.cols) {
                    self.overflow
                        .insert((base + col as usize) as u32, overflow.text);
                }
            }
        }
        Ok(())
    }

    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), String> {
        let count = usize::from(rows)
            .checked_mul(usize::from(cols))
            .ok_or_else(|| "YAS Terminal grid size overflow".to_string())?;
        self.rows = rows;
        self.cols = cols;
        self.cells = vec![[0; 12]; count];
        self.overflow.clear();
        self.links.clear();
        self.cell_links = vec![0; count];
        Ok(())
    }

    fn replace_cell(&mut self, index: usize, cell: terminal::Cell) -> Result<(), String> {
        let slot = self
            .cells
            .get_mut(index)
            .ok_or_else(|| "YAS Terminal grid patch is out of bounds".to_string())?;
        *slot = cell;
        self.overflow.remove(&(index as u32));
        if let Some(link) = self.cell_links.get_mut(index) {
            *link = 0;
        }
        Ok(())
    }

    fn apply_operation(&mut self, operation: terminal::GridOperation) -> Result<(), String> {
        match operation {
            terminal::GridOperation::PatchRun { start_cell, cells } => {
                let start = start_cell as usize;
                for (offset, cell) in cells.into_iter().enumerate() {
                    self.replace_cell(start + offset, cell)?;
                }
            }
            terminal::GridOperation::PatchList { indices, cells } => {
                for (index, cell) in indices.into_iter().zip(cells) {
                    self.replace_cell(index as usize, cell)?;
                }
            }
            terminal::GridOperation::PatchBitmap {
                start_cell,
                span,
                bitmap,
                cells,
            } => {
                let mut cells = cells.into_iter();
                for bit in 0..span as usize {
                    if bitmap[bit / 8] & (1 << (bit % 8)) != 0 {
                        self.replace_cell(
                            start_cell as usize + bit,
                            cells
                                .next()
                                .ok_or_else(|| "YAS Terminal bitmap omitted a cell".to_string())?,
                        )?;
                    }
                }
                if cells.next().is_some() {
                    return Err("YAS Terminal bitmap carried extra cells".into());
                }
            }
            terminal::GridOperation::CopyRect {
                src_row,
                src_col,
                dst_row,
                dst_col,
                rows,
                cols,
            } => self.copy_rect(src_row, src_col, dst_row, dst_col, rows, cols)?,
            terminal::GridOperation::FillRect {
                row,
                col,
                rows,
                cols,
                cell,
            } => {
                for y in row..row + rows {
                    for x in col..col + cols {
                        let index = usize::from(y) * usize::from(self.cols) + usize::from(x);
                        self.replace_cell(index, cell)?;
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_rect(
        &mut self,
        src_row: u16,
        src_col: u16,
        dst_row: u16,
        dst_col: u16,
        rows: u16,
        cols: u16,
    ) -> Result<(), String> {
        let mut copied = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        for y in 0..rows {
            for x in 0..cols {
                let source =
                    usize::from(src_row + y) * usize::from(self.cols) + usize::from(src_col + x);
                copied.push((
                    self.cells[source],
                    self.overflow.get(&(source as u32)).cloned(),
                    self.cell_links[source],
                ));
            }
        }
        for y in 0..rows {
            for x in 0..cols {
                let copied_index = usize::from(y) * usize::from(cols) + usize::from(x);
                let destination =
                    usize::from(dst_row + y) * usize::from(self.cols) + usize::from(dst_col + x);
                let (cell, overflow, link) = copied[copied_index].clone();
                self.replace_cell(destination, cell)?;
                if let Some(value) = overflow {
                    self.overflow.insert(destination as u32, value);
                }
                self.cell_links[destination] = link;
            }
        }
        Ok(())
    }

    fn apply_component(&mut self, component: terminal::Component) -> Result<(), String> {
        match component.kind {
            kind if kind == yas_wire::schema::terminal::COMPONENT_OVERFLOW_STRINGS as u8 => {
                let mut input = ComponentDecoder::new(&component.body);
                let count = input.uleb()?;
                for _ in 0..count {
                    let index = input.uleb()?;
                    let value = input.string()?;
                    self.overflow.insert(index, value);
                }
                input.finish()?;
            }
            kind if kind == yas_wire::schema::terminal::COMPONENT_HYPERLINKS as u8 => {
                let mut input = ComponentDecoder::new(&component.body);
                let uri_count = input.uleb()?;
                self.links.clear();
                for _ in 0..uri_count {
                    let id = input.uleb()?;
                    let uri = input.string()?;
                    self.links.insert(id, uri);
                }
                self.cell_links.fill(0);
                let run_count = input.uleb()?;
                for _ in 0..run_count {
                    let start = input.uleb()? as usize;
                    let length = input.uleb()? as usize;
                    let id = input.uleb()?;
                    let end = start
                        .checked_add(length)
                        .ok_or_else(|| "YAS Terminal hyperlink run overflow".to_string())?;
                    self.cell_links
                        .get_mut(start..end)
                        .ok_or_else(|| "YAS Terminal hyperlink run is out of bounds".to_string())?
                        .fill(id);
                }
                input.finish()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn ansi_text(&self) -> String {
        let mut lines = Vec::with_capacity(usize::from(self.rows));
        for row in 0..self.rows {
            let start = usize::from(row) * usize::from(self.cols);
            let end = start + usize::from(self.cols);
            let overflow = self
                .overflow
                .range(start as u32..end as u32)
                .map(|(&index, text)| terminal::StyledOverflow {
                    cell_offset: index - start as u32,
                    text: text.clone(),
                })
                .collect();
            let mut hyperlinks = Vec::new();
            let mut column = 0usize;
            while column < usize::from(self.cols) {
                let id = self.cell_links[start + column];
                if id == 0 {
                    column += 1;
                    continue;
                }
                let first = column;
                while column < usize::from(self.cols) && self.cell_links[start + column] == id {
                    column += 1;
                }
                if let Some(uri) = self.links.get(&id) {
                    hyperlinks.push(terminal::StyledHyperlink {
                        start_col: first as u32,
                        cell_count: (column - first) as u32,
                        uri: uri.clone(),
                    });
                }
            }
            lines.push(terminal::StyledLine {
                row: i64::from(row),
                start_col: 0,
                cells: self.cells[start..end].to_vec(),
                overflow,
                hyperlinks,
            });
        }
        super::render_styled_lines(&terminal::StyledLines(lines))
    }
}

struct ComponentDecoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ComponentDecoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn uleb(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or_else(|| "truncated YAS Terminal component".to_string())?;
            self.offset += 1;
            value |= u32::from(byte & 0x7f)
                .checked_shl(shift)
                .ok_or_else(|| "YAS Terminal component integer overflow".to_string())?;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("YAS Terminal component integer overflow".into())
    }

    fn string(&mut self) -> Result<String, String> {
        let len = self.uleb()? as usize;
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "YAS Terminal component length overflow".to_string())?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "truncated YAS Terminal component string".to_string())?;
        self.offset = end;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "YAS Terminal component string is not UTF-8".to_string())
    }

    fn finish(self) -> Result<(), String> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err("YAS Terminal component has trailing bytes".into())
        }
    }
}

async fn send_frame_ack(
    client: &mut NativeClient,
    view: &terminal::OpenViewResult,
    sequence: u32,
) -> Result<(), String> {
    client
        .send_typed_event(
            family::TERMINAL,
            terminal::event_kind::FRAME_ACK,
            &terminal::ViewFeedback {
                view_id: view.view_id,
                presented_sequence: sequence,
                decoder_queue_depth: 0,
                available_frame_slots: view.max_inflight_frames,
            },
            false,
        )
        .await
}

pub(crate) struct ViewUpdate {
    pub(crate) text: String,
    pub(crate) cursor: (u16, u16),
    pub(crate) final_exit: Option<i32>,
}

pub(crate) async fn start_view_task(
    on: Option<&str>,
    hub: &str,
    id: u64,
    rows: u16,
    cols: u16,
) -> Result<
    (
        tokio::sync::mpsc::Receiver<Result<ViewUpdate, String>>,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let view = open_view(&mut client, &record, rows, cols, 60).await?;
    // One complete rendered grid is enough to hand off at a time. If stdout
    // stalls, stop reading/acking native frames so Terminal credit applies
    // backpressure instead of retaining an unbounded series of full grids.
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(async move {
        let result = async {
            let mut grid = GridState::default();
            let mut assembler = FrameAssembler::default();
            loop {
                let frame = next_terminal_frame(
                    &mut client,
                    view.view_id,
                    view.max_encoded_frame,
                    &mut assembler,
                )
                .await?;
                grid.apply(&frame, view.max_decoded_frame)?;
                let final_state =
                    frame.frame_flags & yas_wire::schema::terminal::FRAME_FINAL_STATE as u16 != 0;
                let final_exit = if final_state {
                    let record = find_terminal(&mut client, id).await?;
                    Some(terminal_exit_code(&record)?)
                } else {
                    None
                };
                if sender
                    .send(Ok(ViewUpdate {
                        text: grid.ansi_text(),
                        cursor: grid.cursor,
                        final_exit,
                    }))
                    .await
                    .is_err()
                {
                    return Ok::<(), String>(());
                }
                send_frame_ack(&mut client, &view, frame.frame_sequence).await?;
                if final_state {
                    return Ok(());
                }
            }
        }
        .await;
        if let Err(error) = result {
            let _ = sender.send(Err(error)).await;
        }
        let _ = close_view(&mut client, view.view_id).await;
    });
    Ok((receiver, task))
}

pub(crate) enum ViewCommand {
    Input(Vec<u8>),
    Scroll {
        mode: terminal::ScrollMode,
        amount: i64,
    },
    Mouse {
        event: &'static str,
        column: u16,
        row: u16,
        button: &'static str,
    },
    Resize {
        rows: u16,
        cols: u16,
    },
    Focus(bool),
    Close,
}

pub(crate) struct InteractiveView {
    pub(crate) updates: tokio::sync::mpsc::Receiver<Result<InteractiveViewUpdate, String>>,
    pub(crate) commands: tokio::sync::mpsc::Sender<ViewCommand>,
    pub(crate) task: tokio::task::JoinHandle<()>,
}

pub(crate) struct InteractiveViewUpdate {
    pub(crate) grid: GridState,
    pub(crate) final_exit: Option<i32>,
}

async fn scroll_view(
    client: &mut NativeClient,
    view_id: u32,
    mode: terminal::ScrollMode,
    amount: i64,
) -> Result<i64, String> {
    let result: terminal::ScrollResult = client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::SCROLL,
            &terminal::Scroll {
                view_id,
                mode,
                amount,
            },
            false,
        )
        .await?;
    Ok(result.applied_offset)
}

async fn set_view_focus(
    client: &mut NativeClient,
    view_id: u32,
    focused: bool,
) -> Result<(), String> {
    let body = client
        .request(
            family::TERMINAL,
            terminal::request_kind::SET_FOCUS,
            terminal::SetFocus { view_id, focused }
                .encode()
                .map_err(wire_error)?,
            false,
        )
        .await?;
    if body.is_empty() {
        Ok(())
    } else {
        Err("YAS Terminal SET_FOCUS returned an unexpected body".into())
    }
}

// FINAL_STATE ends frame delivery until restart. Read the retained generation
// with bounded, styled READ pages instead of requesting further frames.
async fn retained_grid(
    client: &mut NativeClient,
    record: &terminal::TerminalRecord,
    final_grid: &GridState,
    dimensions: (u16, u16),
    offset: i64,
) -> Result<GridState, String> {
    let (rows, cols) = dimensions;
    let total = u64::from(final_grid.scrollback_lines) + u64::from(final_grid.rows);
    let maximum = total.saturating_sub(u64::from(rows));
    let offset = offset.max(0).min(maximum as i64);
    let start = total.saturating_sub(u64::from(rows) + offset as u64);
    let mut rendered = final_grid.clone();
    rendered.resize(rows, cols)?;
    rendered.scroll_offset = offset;
    rendered.scrollback_lines = maximum.min(u64::from(u32::MAX)) as u32;
    rendered.modes &= !1;
    let mut next = Some(terminal::QueryCursor {
        kind: yas_wire::schema::terminal::READ_CURSOR_ABSOLUTE as u8,
        a: start,
        b: u32::from(rows),
    });
    while let Some(cursor) = next.take() {
        let query = super::terminal_query(
            client,
            terminal::request_kind::READ,
            &terminal::Read {
                terminal_handle: record.terminal_handle,
                generation: record.generation,
                cursor_kind: cursor.kind,
                representation: yas_wire::schema::terminal::QUERY_REPRESENTATION_STYLED as u8,
                flags: 0,
                cursor_a: cursor.a,
                cursor_b: cursor.b,
                max_bytes: super::READ_PAGE_BYTES,
                initial_receive_credit: super::QUERY_CREDIT,
                extensions: Extensions::default(),
            },
            u64::from(super::READ_PAGE_BYTES),
            Duration::from_secs(10),
        )
        .await?;
        super::expect_content(
            &query,
            yas_wire::schema::terminal::CONTENT_STYLED_LINES as u8,
        )?;
        rendered.apply_styled_lines(
            terminal::StyledLines::decode(&query.bytes).map_err(wire_error)?,
            start as i64,
        )?;
        next = match query.next_cursor {
            Some(terminal::QueryNextCursor::Read(next)) if next.a > cursor.a => Some(next),
            Some(_) => return Err("YAS Terminal READ returned a non-advancing cursor".into()),
            None => None,
        };
    }
    Ok(rendered)
}

/// Open one live view whose frame acknowledgements and input share a native
/// session. This is the embedded-view primitive used by `yas mustard`.
pub(crate) async fn start_interactive_view_task(
    on: Option<&str>,
    hub: &str,
    id: u64,
    rows: u16,
    cols: u16,
) -> Result<InteractiveView, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let mut view = open_view(&mut client, &record, rows, cols, 30).await?;
    let (updates_tx, updates) = tokio::sync::mpsc::channel(1);
    let (commands, mut commands_rx) = tokio::sync::mpsc::channel(32);
    let task = tokio::spawn(async move {
        enum Next {
            Frame(Result<terminal::TerminalFrame, String>),
            Command(Option<ViewCommand>),
        }

        let result = async {
            let mut grid = GridState::default();
            let mut assembler = FrameAssembler::default();
            let mut dimensions = (rows, cols);
            let mut exited = None;
            let mut offset = 0;
            let mut focused = false;
            let mut return_to_live = false;
            loop {
                let next = tokio::select! {
                    frame = next_terminal_frame(
                        &mut client,
                        view.view_id,
                        view.max_encoded_frame,
                        &mut assembler,
                    ) => Next::Frame(frame),
                    command = commands_rx.recv() => Next::Command(command),
                };
                match next {
                    Next::Frame(frame) => {
                        let frame = frame?;
                        grid.apply(&frame, view.max_decoded_frame)?;
                        let final_state = frame.frame_flags
                            & yas_wire::schema::terminal::FRAME_FINAL_STATE as u16
                            != 0;
                        let final_exit = if final_state {
                            let record = find_terminal(&mut client, id).await?;
                            let code = terminal_exit_code(&record)?;
                            exited = Some(record);
                            Some(code)
                        } else {
                            exited = None;
                            None
                        };
                        offset = grid.scroll_offset;
                        let update = InteractiveViewUpdate {
                            grid: grid.clone(),
                            final_exit,
                        };
                        if updates_tx.send(Ok(update)).await.is_err() {
                            return Ok::<(), String>(());
                        }
                        send_frame_ack(&mut client, &view, frame.frame_sequence).await?;
                    }
                    Next::Command(Some(ViewCommand::Input(data))) => {
                        if exited.is_some() {
                            continue;
                        }
                        if return_to_live || grid.scroll_offset > 0 {
                            offset = scroll_view(
                                &mut client,
                                view.view_id,
                                terminal::ScrollMode::Absolute,
                                0,
                            )
                            .await?;
                            return_to_live = false;
                        }
                        let feedback = terminal::ViewFeedback {
                            view_id: view.view_id,
                            presented_sequence: grid
                                .last_sequence
                                .unwrap_or_else(|| view.first_sequence.wrapping_sub(1)),
                            decoder_queue_depth: 0,
                            available_frame_slots: view.max_inflight_frames,
                        };
                        for data in
                            data.chunks(yas_wire::schema::terminal::MAX_INPUT_BYTES as usize)
                        {
                            if !data.is_empty() {
                                client
                                    .send_typed_event(
                                        family::TERMINAL,
                                        terminal::event_kind::INPUT,
                                        &terminal::Input {
                                            feedback,
                                            data: data.to_vec(),
                                        },
                                        true,
                                    )
                                    .await?;
                            }
                        }
                    }
                    Next::Command(Some(ViewCommand::Mouse {
                        event,
                        column,
                        row,
                        button,
                    })) => {
                        if exited.is_some() || offset > 0 {
                            continue;
                        }
                        let feedback = terminal::ViewFeedback {
                            view_id: view.view_id,
                            presented_sequence: grid
                                .last_sequence
                                .unwrap_or_else(|| view.first_sequence.wrapping_sub(1)),
                            decoder_queue_depth: 0,
                            available_frame_slots: view.max_inflight_frames,
                        };
                        super::send_mouse_actions(
                            &mut client,
                            feedback,
                            event,
                            column,
                            row,
                            button,
                        )
                        .await?;
                    }
                    Next::Command(Some(ViewCommand::Scroll { mode, amount })) => {
                        if let Some(record) = &exited {
                            let requested = match mode {
                                terminal::ScrollMode::Absolute => amount,
                                terminal::ScrollMode::Relative => offset.saturating_add(amount),
                            };
                            let rendered =
                                retained_grid(&mut client, record, &grid, dimensions, requested)
                                    .await?;
                            offset = rendered.scroll_offset;
                            if updates_tx
                                .send(Ok(InteractiveViewUpdate {
                                    grid: rendered,
                                    final_exit: Some(terminal_exit_code(record)?),
                                }))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        } else {
                            offset = scroll_view(&mut client, view.view_id, mode, amount).await?;
                            return_to_live = true;
                        }
                    }
                    Next::Command(Some(ViewCommand::Resize { rows, cols })) => {
                        dimensions = (rows, cols);
                        if let Some(record) = &exited {
                            let rendered =
                                retained_grid(&mut client, record, &grid, dimensions, offset)
                                    .await?;
                            offset = rendered.scroll_offset;
                            if updates_tx
                                .send(Ok(InteractiveViewUpdate {
                                    grid: rendered,
                                    final_exit: Some(terminal_exit_code(record)?),
                                }))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                            continue;
                        }
                        // Frame limits are fixed at OPEN_VIEW. Reopen at the
                        // new geometry so growing a pane cannot exceed them.
                        close_view(&mut client, view.view_id).await?;
                        view = open_view(&mut client, &record, rows, cols, 30).await?;
                        grid = GridState::default();
                        assembler = FrameAssembler::default();
                        if offset > 0 {
                            offset = scroll_view(
                                &mut client,
                                view.view_id,
                                terminal::ScrollMode::Absolute,
                                offset,
                            )
                            .await?;
                        }
                        set_view_focus(&mut client, view.view_id, focused).await?;
                    }
                    Next::Command(Some(ViewCommand::Focus(value))) => {
                        focused = value;
                        set_view_focus(&mut client, view.view_id, focused).await?;
                    }
                    Next::Command(Some(ViewCommand::Close) | None) => return Ok(()),
                }
            }
        }
        .await;
        if let Err(error) = result {
            let _ = updates_tx.send(Err(error)).await;
        }
        let _ = close_view(&mut client, view.view_id).await;
    });
    Ok(InteractiveView {
        updates,
        commands,
        task,
    })
}

async fn start_lifecycle_task(
    on: Option<&str>,
    hub: &str,
    id: u64,
) -> Result<
    (
        tokio::sync::oneshot::Receiver<Result<i32, String>>,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let mut client = NativeClient::connect(on, hub).await?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let result = watch_terminal_exit(&mut client, id)
            .await
            .and_then(|record| terminal_exit_code(&record));
        let _ = sender.send(result);
    });
    Ok((receiver, task))
}

#[cfg(unix)]
mod tty {
    use std::io::{Read as _, Write as _};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub(super) static RESIZED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_sigwinch(_: libc::c_int) {
        RESIZED.store(true, Ordering::Relaxed);
    }

    pub(super) struct RawMode(Option<libc::termios>);

    impl RawMode {
        pub(super) fn enter() -> Result<Self, String> {
            // SAFETY: the calls use fd 0 and initialized termios storage.
            unsafe {
                if libc::isatty(0) != 1 {
                    return Err("stdin is not a terminal (attach needs a tty)".into());
                }
                let mut saved: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(0, &mut saved) != 0 {
                    return Err("cannot read terminal attributes".into());
                }
                let mut raw = saved;
                libc::cfmakeraw(&mut raw);
                raw.c_cc[libc::VMIN] = 1;
                raw.c_cc[libc::VTIME] = 0;
                if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 {
                    return Err("cannot put the terminal in raw mode".into());
                }
                let handler: extern "C" fn(libc::c_int) = on_sigwinch;
                libc::signal(libc::SIGWINCH, handler as usize as libc::sighandler_t);
                Ok(Self(Some(saved)))
            }
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            if let Some(saved) = self.0.take() {
                // SAFETY: saved came from tcgetattr on fd 0.
                unsafe {
                    libc::tcsetattr(0, libc::TCSANOW, &saved);
                }
            }
            let mut output = std::io::stdout();
            let _ = output.write_all(b"\x1b[?25h\x1b[?1049l");
            let _ = output.flush();
        }
    }

    pub(super) fn window_size() -> (u16, u16) {
        // SAFETY: ioctl writes to an owned winsize value.
        unsafe {
            let mut size: libc::winsize = std::mem::zeroed();
            if libc::ioctl(0, libc::TIOCGWINSZ, &mut size) == 0
                && size.ws_col != 0
                && size.ws_row != 0
            {
                (size.ws_col, size.ws_row)
            } else {
                (80, 24)
            }
        }
    }

    pub(super) fn input_channel() -> (tokio::sync::mpsc::Receiver<Vec<u8>>, Arc<AtomicBool>) {
        // Bound pasted input while the remote link is congested. This thread
        // may block safely; it exists solely to bridge blocking stdin.
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buffer = [0; 4096];
            while !thread_stop.load(Ordering::Relaxed) {
                match stdin.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(length) if sender.blocking_send(buffer[..length].to_vec()).is_err() => break,
                    Ok(_) => {}
                }
            }
        });
        (receiver, stop)
    }

    pub(super) fn repaint(text: &str, cursor: (u16, u16)) {
        let mut bytes = Vec::with_capacity(text.len() + 64);
        bytes.extend_from_slice(b"\x1b[?2026h\x1b[H\x1b[2J");
        for (index, line) in text.lines().enumerate() {
            if index != 0 {
                bytes.extend_from_slice(b"\r\n");
            }
            bytes.extend_from_slice(line.as_bytes());
        }
        bytes.extend_from_slice(format!("\x1b[{};{}H", cursor.0 + 1, cursor.1 + 1).as_bytes());
        bytes.extend_from_slice(b"\x1b[?2026l");
        let mut output = std::io::stdout();
        let _ = output.write_all(&bytes);
        let _ = output.flush();
    }
}

#[cfg(unix)]
pub(super) async fn attach(on: Option<&str>, hub: &str, id: u64) -> Result<i32, String> {
    use std::sync::atomic::Ordering;

    let raw = tty::RawMode::enter()?;
    let mut output = std::io::stdout();
    let _ = output.write_all(b"\x1b[?1049h\x1b[?25l");
    let _ = output.flush();

    let (mut cols, mut rows) = tty::window_size();
    let (mut updates, mut view_task) = start_view_task(on, hub, id, rows, cols).await?;
    let (mut lifecycle, lifecycle_task) = start_lifecycle_task(on, hub, id).await?;
    let mut input_client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut input_client, id).await?;
    let input_view = open_view(&mut input_client, &record, rows, cols, 1).await?;
    let (mut input, stop) = tty::input_channel();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut detached = false;
    let exit = loop {
        tokio::select! {
            chunk = input.recv() => {
                let Some(chunk) = chunk else { break 0 };
                let (bytes, detach) = match chunk.iter().position(|byte| *byte == DETACH) {
                    Some(position) => (&chunk[..position], true),
                    None => (chunk.as_slice(), false),
                };
                for bytes in bytes.chunks(yas_wire::schema::terminal::MAX_INPUT_BYTES as usize) {
                    if bytes.is_empty() {
                        continue;
                    }
                    input_client.send_typed_event(
                        family::TERMINAL,
                        terminal::event_kind::INPUT,
                        &terminal::Input {
                            feedback: terminal::ViewFeedback {
                                view_id: input_view.view_id,
                                presented_sequence: input_view.first_sequence.wrapping_sub(1),
                                decoder_queue_depth: 0,
                                available_frame_slots: 0,
                            },
                            data: bytes.to_vec(),
                        },
                        true,
                    ).await?;
                }
                if detach {
                    detached = true;
                    break 0;
                }
            }
            update = updates.recv() => {
                match update {
                    Some(Ok(update)) => {
                        tty::repaint(&update.text, update.cursor);
                        if let Some(exit) = update.final_exit {
                            break exit;
                        }
                    }
                    Some(Err(error)) => return Err(error),
                    None => break 0,
                }
            }
            status = &mut lifecycle => {
                break status
                    .map_err(|_| "YAS Terminal lifecycle watcher stopped".to_string())??;
            }
            _ = tick.tick() => {
                if tty::RESIZED.swap(false, Ordering::Relaxed) {
                    (cols, rows) = tty::window_size();
                    view_task.abort();
                    (updates, view_task) = start_view_task(on, hub, id, rows, cols).await?;
                    let body = input_client.request(
                        family::TERMINAL,
                        terminal::request_kind::CONFIGURE_VIEW,
                        terminal::ConfigureView {
                            view_id: input_view.view_id,
                            configuration: terminal::ViewConfiguration {
                                rows: Some(rows),
                                cols: Some(cols),
                                ..terminal::ViewConfiguration::default()
                            },
                            extensions: Extensions::default(),
                        }.encode().map_err(wire_error)?,
                        false,
                    ).await?;
                    if !body.is_empty() {
                        return Err("YAS Terminal CONFIGURE_VIEW returned an unexpected body".into());
                    }
                }
            }
        }
    };
    stop.store(true, Ordering::Relaxed);
    view_task.abort();
    lifecycle_task.abort();
    let _ = close_view(&mut input_client, input_view.view_id).await;
    drop(raw);
    if detached {
        eprintln!("yas: detached from {id} (still running)");
    }
    Ok(exit)
}

#[cfg(not(unix))]
pub(super) async fn attach(_on: Option<&str>, _hub: &str, _id: u64) -> Result<i32, String> {
    Err("terminal attach is not supported on this platform".into())
}

pub(super) async fn record(
    on: Option<&str>,
    hub: &str,
    id: u64,
    output: Option<String>,
    max_frames: u32,
    max_duration: f64,
) -> Result<(), String> {
    if !max_duration.is_finite() || max_duration < 0.0 {
        return Err("recording duration must be a finite non-negative number".into());
    }
    let mut client = NativeClient::connect(on, hub).await?;
    let terminal_record = find_terminal(&mut client, id).await?;
    let view = open_view(
        &mut client,
        &terminal_record,
        terminal_record.rows,
        terminal_record.cols,
        60,
    )
    .await?;
    let path = output.unwrap_or_else(|| format!("pty-{id}.yasrec"));
    let file = std::fs::File::create(&path).map_err(|error| format!("create {path}: {error}"))?;
    let mut recording = recording::Writer::new(
        file,
        recording::Header {
            grid_codec: view.codec_version,
            terminal_handle: terminal_record.terminal_handle,
            generation: terminal_record.generation,
            rows: terminal_record.rows,
            cols: terminal_record.cols,
            view_id: view.view_id,
            first_sequence: view.first_sequence,
        },
    )?;
    let start = Instant::now();
    let deadline = (max_duration > 0.0).then(|| start + Duration::from_secs_f64(max_duration));
    let limit = match (max_frames > 0, deadline.is_some()) {
        (true, true) => format!("{max_frames} frames / {max_duration}s"),
        (true, false) => format!("{max_frames} frames"),
        (false, true) => format!("{max_duration}s"),
        (false, false) => "until Ctrl+C".to_string(),
    };
    eprintln!("recording pty {id} → {path} ({limit})");
    let mut assembler = FrameAssembler::default();
    let mut count = 0u32;
    loop {
        if max_frames != 0 && count >= max_frames
            || deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            break;
        }
        let next = next_terminal_frame(
            &mut client,
            view.view_id,
            view.max_encoded_frame,
            &mut assembler,
        );
        let frame = if let Some(deadline) = deadline {
            tokio::select! {
                result = next => result?,
                _ = tokio::time::sleep_until(deadline.into()) => break,
                _ = tokio::signal::ctrl_c() => break,
            }
        } else {
            tokio::select! {
                result = next => result?,
                _ = tokio::signal::ctrl_c() => break,
            }
        };
        let timestamp = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
        recording.write_frame(timestamp, &frame)?;
        send_frame_ack(&mut client, &view, frame.frame_sequence).await?;
        count += 1;
        eprint!("\r  frame {count} {:.1}s  ", start.elapsed().as_secs_f64());
        if frame.frame_flags & yas_wire::schema::terminal::FRAME_FINAL_STATE as u16 != 0 {
            break;
        }
    }
    recording.finish()?;
    let _ = close_view(&mut client, view.view_id).await;
    eprintln!(
        "\n  done: {count} frames, {:.1}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyframe_flags() -> u16 {
        yas_wire::schema::terminal::FRAME_KEYFRAME as u16
            | yas_wire::schema::terminal::FRAME_DIMENSIONS as u16
            | yas_wire::schema::terminal::FRAME_CURSOR as u16
            | yas_wire::schema::terminal::FRAME_MODES as u16
            | yas_wire::schema::terminal::FRAME_SCROLLBACK as u16
            | yas_wire::schema::terminal::FRAME_VIEW_OFFSET as u16
            | yas_wire::schema::terminal::FRAME_TITLE as u16
    }

    fn test_frame() -> terminal::TerminalFrame {
        let flags = keyframe_flags();
        terminal::TerminalFrame {
            view_id: 9,
            frame_sequence: 4,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: Some((1, 1)),
                cursor: Some((0, 0)),
                modes: Some(0),
                scrollback_lines: Some(0),
                scroll_offset: Some(0),
                title: Some(String::new()),
                operations: vec![terminal::GridOperation::PatchRun {
                    start_cell: 0,
                    cells: vec![[0; 12]],
                }],
                components: Vec::new(),
            }
            .encode_codec1(flags, 4096, None)
            .unwrap(),
        }
    }

    #[test]
    fn frame_chunks_reassemble_once_and_in_order() {
        let frame = test_frame();
        let bytes = frame.encode().unwrap();
        let logical = &bytes[8..];
        let middle = logical.len() / 2;
        let mut assembler = FrameAssembler::default();
        assert!(
            assembler
                .push(
                    terminal::FrameChunk {
                        view_id: 9,
                        frame_sequence: 4,
                        chunk_index: 0,
                        chunk_count: 2,
                        logical_frame_len: logical.len() as u32,
                        chunk: logical[..middle].to_vec(),
                    },
                    9,
                    4096,
                )
                .unwrap()
                .is_none()
        );
        let decoded = assembler
            .push(
                terminal::FrameChunk {
                    view_id: 9,
                    frame_sequence: 4,
                    chunk_index: 1,
                    chunk_count: 2,
                    logical_frame_len: logical.len() as u32,
                    chunk: logical[middle..].to_vec(),
                },
                9,
                4096,
            )
            .unwrap()
            .unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn frame_chunks_reassemble_a_body_larger_than_one_bulk_chunk() {
        let frame = terminal::TerminalFrame {
            view_id: 9,
            frame_sequence: 4,
            frame_flags: yas_wire::schema::terminal::FRAME_CURSOR as u16,
            base_sequence: None,
            grid_payload: vec![0x5a; yas_wire::frame::HARD_MAX_BULK_CHUNK as usize + 1],
        };
        let logical = frame.encode_logical_body().unwrap();
        let middle = logical.len() / 2;
        let mut assembler = FrameAssembler::default();
        assert!(
            assembler
                .push(
                    terminal::FrameChunk {
                        view_id: frame.view_id,
                        frame_sequence: frame.frame_sequence,
                        chunk_index: 0,
                        chunk_count: 2,
                        logical_frame_len: logical.len() as u32,
                        chunk: logical[..middle].to_vec(),
                    },
                    frame.view_id,
                    logical.len() as u32,
                )
                .unwrap()
                .is_none()
        );
        let decoded = assembler
            .push(
                terminal::FrameChunk {
                    view_id: frame.view_id,
                    frame_sequence: frame.frame_sequence,
                    chunk_index: 1,
                    chunk_count: 2,
                    logical_frame_len: logical.len() as u32,
                    chunk: logical[middle..].to_vec(),
                },
                frame.view_id,
                logical.len() as u32,
            )
            .unwrap()
            .unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn frame_chunks_reject_out_of_order_delivery() {
        let frame = test_frame();
        let bytes = frame.encode().unwrap();
        let logical = &bytes[8..];
        let middle = logical.len() / 2;
        let mut assembler = FrameAssembler::default();
        let error = assembler
            .push(
                terminal::FrameChunk {
                    view_id: 9,
                    frame_sequence: 4,
                    chunk_index: 1,
                    chunk_count: 2,
                    logical_frame_len: logical.len() as u32,
                    chunk: logical[middle..].to_vec(),
                },
                9,
                4096,
            )
            .unwrap_err();
        assert!(error.contains("out of order"));
    }

    #[test]
    fn grid_state_applies_keyframe() {
        let flags = keyframe_flags();
        let mut cell = [0; 12];
        cell[1] = 1 << 3;
        cell[8] = b'A';
        let frame = terminal::TerminalFrame {
            view_id: 1,
            frame_sequence: 1,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: Some((1, 2)),
                cursor: Some((0, 1)),
                modes: Some(1 | (2 << 4)),
                scrollback_lines: Some(100),
                scroll_offset: Some(5),
                title: Some("test".into()),
                operations: vec![terminal::GridOperation::PatchRun {
                    start_cell: 0,
                    cells: vec![cell, [0; 12]],
                }],
                components: Vec::new(),
            }
            .encode_codec1(flags, 4096, None)
            .unwrap(),
        };
        let mut state = GridState::default();
        state.apply(&frame, 4096).unwrap();
        assert_eq!(state.ansi_text(), "A");
        assert_eq!(state.cursor, (0, 1));
        assert_eq!(state.scrollback_lines, 100);
        assert_eq!(state.scroll_offset, 5);
        assert!(state.reports_mouse());
        assert!(!state.cursor_visible());
        let flags = yas_wire::schema::terminal::FRAME_VIEW_OFFSET as u16;
        let delta = terminal::TerminalFrame {
            view_id: 1,
            frame_sequence: 2,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: None,
                cursor: None,
                modes: None,
                scrollback_lines: None,
                scroll_offset: Some(0),
                title: None,
                operations: Vec::new(),
                components: Vec::new(),
            }
            .encode_codec1(flags, 4096, Some((1, 2)))
            .unwrap(),
        };
        state.apply(&delta, 4096).unwrap();
        assert_eq!(state.scrollback_lines, 100);
        assert!(state.cursor_visible());
        assert!(state.reports_mouse());
    }

    #[test]
    fn retained_styled_rows_preserve_cells_and_overflow_when_clipped() {
        let mut state = GridState::default();
        state.resize(2, 3).unwrap();
        let mut cell = [0; 12];
        cell[0] = 2;
        cell[2..5].copy_from_slice(&[1, 2, 3]);
        cell[1] = 7 << 3;
        state
            .apply_styled_lines(
                terminal::StyledLines(vec![terminal::StyledLine {
                    row: 40,
                    start_col: 1,
                    cells: vec![cell; 4],
                    overflow: vec![terminal::StyledOverflow {
                        cell_offset: 0,
                        text: "e\u{301}".into(),
                    }],
                    hyperlinks: Vec::new(),
                }]),
                40,
            )
            .unwrap();
        assert_eq!(state.cells[1], cell);
        assert_eq!(state.cells[2], cell);
        assert_eq!(state.cells[3], [0; 12]);
        assert_eq!(state.overflow.get(&1).map(String::as_str), Some("e\u{301}"));
    }

    #[test]
    fn delta_keeps_overflow_text_for_unpatched_cells() {
        let flags = keyframe_flags() | yas_wire::schema::terminal::FRAME_COMPONENTS as u16;
        let mut overflow_cell = [0; 12];
        overflow_cell[1] = 7 << 3;
        let keyframe = terminal::TerminalFrame {
            view_id: 1,
            frame_sequence: 1,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: Some((1, 2)),
                cursor: Some((0, 0)),
                modes: Some(0),
                scrollback_lines: Some(0),
                scroll_offset: Some(0),
                title: Some(String::new()),
                operations: vec![terminal::GridOperation::PatchRun {
                    start_cell: 0,
                    cells: vec![overflow_cell, [0; 12]],
                }],
                components: vec![terminal::Component {
                    kind: yas_wire::schema::terminal::COMPONENT_OVERFLOW_STRINGS as u8,
                    required: false,
                    // one entry: cell 0, five UTF-8 bytes, "hello"
                    body: vec![1, 0, 5, b'h', b'e', b'l', b'l', b'o'],
                }],
            }
            .encode_codec1(flags, 4096, None)
            .unwrap(),
        };
        let mut next = [0; 12];
        next[1] = 1 << 3;
        next[8] = b'!';
        let delta = terminal::TerminalFrame {
            view_id: 1,
            frame_sequence: 2,
            frame_flags: 0,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: None,
                cursor: None,
                modes: None,
                scrollback_lines: None,
                scroll_offset: None,
                title: None,
                operations: vec![terminal::GridOperation::PatchRun {
                    start_cell: 1,
                    cells: vec![next],
                }],
                components: Vec::new(),
            }
            .encode_codec1(0, 4096, Some((1, 2)))
            .unwrap(),
        };
        let mut state = GridState::default();
        state.apply(&keyframe, 4096).unwrap();
        state.apply(&delta, 4096).unwrap();
        assert_eq!(state.ansi_text(), "hello!");
    }
}
