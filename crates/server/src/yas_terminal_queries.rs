//! Native YAS Terminal query semantics over the terminal state.
//!
//! This module deliberately stops at owned, typed query content. Correlation,
//! inline-versus-Transfer delivery, aggregate credit, and request cancellation
//! remain in `yas.rs`.

use std::time::Duration;

use regex::bytes::RegexBuilder;
use yas_terminal_model::{CELL_SIZE, FrameState};
use yas_wire::{
    Encode, schema,
    terminal::{
        self as wire, JournalRecord, JournalResult, OutputResult, SearchMatch, SearchResults,
    },
};

use super::{AppState, JOURNAL_REPLY_MAX, Pty, Session, journal, resolve_term_cwd, take_snapshot};

const QUERY_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone)]
pub(crate) struct Runtime {
    state: AppState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryData {
    pub(crate) content_kind: u8,
    pub(crate) encoding: u8,
    pub(crate) flags: u16,
    pub(crate) bytes: Vec<u8>,
    pub(crate) next_cursor: Option<wire::QueryNextCursor>,
    pub(crate) total_lines: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaitOutcome {
    pub(crate) data: QueryData,
    /// True only when the requested condition, rather than the relative
    /// timeout or terminal exit, produced the answer. The session adapter
    /// attaches its current Terminal catalogue revision in that case.
    pub(crate) satisfied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Invalid(&'static str),
    NotFound,
    Stale,
    TooLarge,
    Internal,
}

#[derive(Clone, Debug)]
struct CapturedRow {
    row: i64,
    start_col: u32,
    cells: Vec<wire::Cell>,
    contents: Vec<String>,
    overflow: Vec<wire::StyledOverflow>,
    hyperlinks: Vec<wire::StyledHyperlink>,
    wrapped: bool,
}

impl Runtime {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }

    pub(crate) async fn read(&self, request: &wire::Read) -> Result<QueryData, Error> {
        let mut session = self.state.session.lock().await;
        let pty = terminal_mut(&mut session, request.terminal_handle, request.generation)?;
        if request.flags != schema::terminal::READ_FLAGS as u16 {
            return Err(Error::Invalid("Terminal READ flags"));
        }
        let rows = capture_rows(pty)?;
        let total = rows.len();
        let (start, end) = match request.cursor_kind {
            kind if kind == schema::terminal::READ_CURSOR_ABSOLUTE as u8 => {
                let start = usize::try_from(request.cursor_a)
                    .unwrap_or(usize::MAX)
                    .min(total);
                let end = if request.cursor_b == 0 {
                    total
                } else {
                    start.saturating_add(request.cursor_b as usize).min(total)
                };
                (start, end)
            }
            kind if kind == schema::terminal::READ_CURSOR_TAIL as u8 => {
                let end = total.saturating_sub(
                    usize::try_from(request.cursor_a)
                        .unwrap_or(usize::MAX)
                        .min(total),
                );
                let start = if request.cursor_b == 0 {
                    0
                } else {
                    end.saturating_sub(request.cursor_b as usize)
                };
                (start, end)
            }
            _ => return Err(Error::Invalid("Terminal READ cursor kind")),
        };
        rows_query(
            &rows[start..end],
            request.representation,
            request.max_bytes,
            total as u64,
            request.cursor_kind,
            request.cursor_b,
        )
    }

    pub(crate) async fn search(&self, request: &wire::Search) -> Result<QueryData, Error> {
        if request.flags & !(schema::terminal::SEARCH_FLAGS as u16) != 0
            || request.max_results == 0
            || request.max_results > wire::Limits::HARD.max_query_records
            || request.query.is_empty()
            || request.query.len() > super::MAX_SEARCH_QUERY
        {
            return Err(Error::Invalid("Terminal SEARCH bounds"));
        }
        let mut session = self.state.session.lock().await;
        let pty = terminal_mut(&mut session, request.terminal_handle, request.generation)?;
        let rows = capture_rows(pty)?;
        let raw_pattern = request.flags & schema::terminal::SEARCH_REGEX as u16 != 0;
        let pattern = if raw_pattern {
            request.query.clone()
        } else {
            regex::escape(
                std::str::from_utf8(&request.query)
                    .map_err(|_| Error::Invalid("Terminal literal SEARCH query is not UTF-8"))?,
            )
            .into_bytes()
        };
        let pattern = std::str::from_utf8(&pattern)
            .map_err(|_| Error::Invalid("Terminal regex SEARCH query is not UTF-8"))?;
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(request.flags & schema::terminal::SEARCH_CASE_SENSITIVE as u16 == 0)
            .build()
            .map_err(|_| Error::Invalid("Terminal SEARCH regular expression"))?;

        let backward = request.flags & schema::terminal::SEARCH_BACKWARD as u16 != 0;
        if request.start_cursor.kind != schema::terminal::SEARCH_CURSOR_POSITION as u8 {
            return Err(Error::Invalid("Terminal SEARCH cursor kind"));
        }
        if rows.is_empty() {
            return Ok(QueryData {
                content_kind: schema::terminal::CONTENT_SEARCH_RESULTS as u8,
                encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
                flags: 0,
                bytes: SearchResults(Vec::new())
                    .encode()
                    .map_err(|_| Error::Internal)?,
                next_cursor: None,
                total_lines: Some(0),
            });
        }
        let start = usize::try_from(request.start_cursor.a).unwrap_or(usize::MAX);
        let indices: Box<dyn Iterator<Item = usize>> = if backward {
            let last = start.min(rows.len().saturating_sub(1));
            Box::new((0..=last).rev())
        } else {
            Box::new(start.min(rows.len())..rows.len())
        };
        let mut matches = Vec::new();
        let mut continuation = None;
        let maximum = request.max_results as usize;
        for index in indices {
            let row = &rows[index];
            let (line, byte_columns) = searchable_row(row);
            let mut row_matches: Vec<_> = regex.find_iter(&line).collect();
            if backward {
                row_matches.reverse();
            }
            for found in row_matches {
                let start_col = byte_to_column(&byte_columns, found.start(), false);
                let end_col = byte_to_column(&byte_columns, found.end(), true);
                if index == start
                    && ((!backward && start_col < request.start_cursor.b)
                        || (backward && end_col > request.start_cursor.b))
                {
                    continue;
                }
                matches.push(SearchMatch {
                    start_row: row.row as u64,
                    start_col,
                    end_row: row.row as u64,
                    end_col,
                    preview: String::from_utf8_lossy(&line).into_owned(),
                });
                if matches.len() == maximum {
                    continuation = search_next_cursor(&rows, index, backward, start_col, end_col);
                    break;
                }
            }
            if matches.len() == maximum {
                break;
            }
        }
        let bytes = SearchResults(matches)
            .encode()
            .map_err(|_| Error::Internal)?;
        Ok(QueryData {
            content_kind: schema::terminal::CONTENT_SEARCH_RESULTS as u8,
            encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            flags: 0,
            bytes,
            next_cursor: continuation,
            total_lines: Some(rows.len() as u64),
        })
    }

    pub(crate) async fn cwd(&self, request: &wire::CwdQuery) -> Result<QueryData, Error> {
        let session = self.state.session.lock().await;
        let pty = terminal(&session, request.terminal_handle, request.generation)?;
        let cwd = resolve_term_cwd(pty.osc7_cwd.as_deref(), || super::pty::pty_cwd(&pty.handle))
            .ok_or(Error::NotFound)?;
        Ok(QueryData {
            content_kind: schema::terminal::CONTENT_PATH as u8,
            encoding: schema::terminal::QUERY_ENCODING_BYTES as u8,
            flags: 0,
            bytes: cwd.into_bytes(),
            next_cursor: None,
            total_lines: None,
        })
    }

    pub(crate) async fn journal(&self, request: &wire::Journal) -> Result<QueryData, Error> {
        if request.flags & !(schema::terminal::JOURNAL_REQUEST_FLAGS as u16) != 0
            || u32::from(request.limit) > wire::Limits::HARD.max_query_records
        {
            return Err(Error::Invalid("Terminal JOURNAL flags"));
        }
        let session = self.state.session.lock().await;
        let pty = terminal(&session, request.terminal_handle, request.generation)?;
        let (cursor_seq, _) = pty.driver.cursor_seq();
        let oldest_seq = pty.driver.oldest_seq();
        let indices: Vec<u64> = pty.journal.iter().map(|record| record.index).collect();
        let limit = request.limit as usize;
        let selected: Vec<u64> = if request.flags & schema::terminal::JOURNAL_TAIL as u16 != 0 {
            let end = indices
                .len()
                .saturating_sub(usize::try_from(request.from_index).unwrap_or(usize::MAX));
            let start = end.saturating_sub(limit);
            indices[start..end].to_vec()
        } else {
            indices
                .into_iter()
                .filter(|index| *index >= request.from_index)
                .take(limit)
                .collect()
        };
        let generation = pty_generation(pty);
        let mut records = Vec::with_capacity(selected.len());
        let mut approximate_bytes = 0usize;
        let mut truncated = false;
        let mut resume_index = None;
        for index in selected {
            let Some(record) = pty.journal.snapshot(index, cursor_seq, oldest_seq) else {
                continue;
            };
            let record_bytes = record.command.len().saturating_add(56);
            if approximate_bytes.saturating_add(record_bytes) > JOURNAL_REPLY_MAX.saturating_sub(20)
            {
                truncated = true;
                resume_index = Some(index);
                break;
            }
            approximate_bytes += record_bytes;
            records.push(journal_record(generation, record));
        }
        let next_cursor = resume_index
            .or_else(|| {
                records
                    .last()
                    .and_then(|record| record.index.checked_add(1))
            })
            .filter(|next| *next < pty.journal.next_index())
            .map(wire::QueryNextCursor::JournalIndex);
        let bytes = JournalResult {
            oldest_index: pty.journal.oldest_index(),
            next_index: pty.journal.next_index(),
            records,
        }
        .encode()
        .map_err(|_| Error::Internal)?;
        Ok(QueryData {
            content_kind: schema::terminal::CONTENT_JOURNAL as u8,
            encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            flags: if truncated {
                schema::terminal::QUERY_TRUNCATED as u16
            } else {
                0
            },
            bytes,
            next_cursor,
            total_lines: None,
        })
    }

    pub(crate) async fn output(&self, request: &wire::Output) -> Result<QueryData, Error> {
        if request.flags != schema::terminal::OUTPUT_REQUEST_FLAGS as u8 {
            return Err(Error::Invalid("Terminal OUTPUT flags"));
        }
        let session = self.state.session.lock().await;
        let pty = terminal(&session, request.terminal_handle, request.generation)?;
        output_query(
            pty,
            request.cursor_kind,
            request.cursor_a,
            request.cursor_b,
            request.max_bytes,
        )
    }

    pub(crate) async fn wait(&self, request: &wire::Wait) -> Result<WaitOutcome, Error> {
        if request.flags != schema::terminal::WAIT_FLAGS as u8 {
            return Err(Error::Invalid("Terminal WAIT flags"));
        }
        let timeout = Duration::from_nanos(request.timeout_ns).min(Duration::from_millis(
            u64::from(journal::WAIT_TIMEOUT_MAX_MS),
        ));
        let deadline = tokio::time::Instant::now()
            .checked_add(timeout)
            .ok_or(Error::Invalid("Terminal WAIT timeout"))?;
        let mut command_index = None;
        loop {
            let now = tokio::time::Instant::now();
            let timed_out = now >= deadline;
            let poll = {
                let session = self.state.session.lock().await;
                let pty = terminal(&session, request.terminal_handle, request.generation)?;
                match request.wait_kind {
                    kind if kind == schema::terminal::WAIT_OUTPUT as u8 => {
                        let (data, matched) = output_wait_query(
                            pty,
                            request.cursor_a,
                            request.cursor_b,
                            request.max_bytes,
                            &request.needle,
                        )?;
                        if matched || timed_out || pty.exited {
                            Some(WaitOutcome {
                                data,
                                satisfied: matched,
                            })
                        } else {
                            None
                        }
                    }
                    kind if kind == schema::terminal::WAIT_COMMAND as u8
                        || kind == schema::terminal::WAIT_LATEST_COMMAND as u8 =>
                    {
                        let index = *command_index.get_or_insert_with(|| {
                            if kind == schema::terminal::WAIT_LATEST_COMMAND as u8 {
                                pty.journal
                                    .running_index()
                                    .unwrap_or_else(|| pty.journal.next_index())
                            } else {
                                request.cursor_a
                            }
                        });
                        poll_command_wait(pty, index, timed_out)?
                    }
                    _ => return Err(Error::Invalid("Terminal WAIT kind")),
                }
            };
            if let Some(outcome) = poll {
                return Ok(outcome);
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + QUERY_POLL_INTERVAL).min(deadline),
            )
            .await;
        }
    }

    pub(crate) async fn copy_range(&self, request: &wire::CopyRange) -> Result<QueryData, Error> {
        let mut session = self.state.session.lock().await;
        let pty = terminal_mut(&mut session, request.terminal_handle, request.generation)?;
        let rows = capture_rows(pty)?;
        let total = rows.len();
        if rows.is_empty() {
            let mut data = rows_query(
                &[],
                request.representation,
                request.max_bytes,
                0,
                schema::terminal::READ_CURSOR_ABSOLUTE as u8,
                0,
            )?;
            data.flags |= schema::terminal::QUERY_TRUNCATED as u16;
            return Ok(data);
        }
        let (start_row, start_row_clamped) = resolve_row(request.start_row, total)?;
        let (end_row, end_row_clamped) = resolve_row(request.end_row, total)?;
        if start_row > end_row {
            return Err(Error::Invalid("Terminal COPY_RANGE row order"));
        }
        let mut clamped = start_row_clamped || end_row_clamped;
        let mut selected = Vec::with_capacity(end_row - start_row + 1);
        for (row, row_data) in rows.iter().enumerate().take(end_row + 1).skip(start_row) {
            let requested_start_col = if row == start_row {
                request.start_col
            } else {
                0
            };
            let requested_end_col = if row == end_row {
                request.end_col
            } else {
                row_data.start_col + row_data.cells.len() as u32
            };
            if requested_start_col > requested_end_col {
                return Err(Error::Invalid("Terminal COPY_RANGE column order"));
            }
            let row_end = row_data
                .start_col
                .saturating_add(row_data.cells.len() as u32);
            clamped |= requested_start_col < row_data.start_col
                || requested_start_col > row_end
                || requested_end_col < row_data.start_col
                || requested_end_col > row_end;
            selected.push(slice_row(row_data, requested_start_col, requested_end_col));
        }
        let mut data = rows_query(
            &selected,
            request.representation,
            request.max_bytes,
            total as u64,
            schema::terminal::READ_CURSOR_ABSOLUTE as u8,
            u32::try_from(selected.len()).unwrap_or(u32::MAX),
        )?;
        if clamped {
            data.flags |= schema::terminal::QUERY_TRUNCATED as u16;
        }
        Ok(data)
    }
}

fn terminal(session: &Session, terminal_handle: u64, generation: u32) -> Result<&Pty, Error> {
    let pty_id = session
        .terminal_backend(terminal_handle)
        .ok_or(Error::NotFound)?;
    let pty = session.ptys.get(&pty_id).ok_or(Error::NotFound)?;
    if pty_generation(pty) != generation {
        return Err(Error::Stale);
    }
    Ok(pty)
}

fn terminal_mut(
    session: &mut Session,
    terminal_handle: u64,
    generation: u32,
) -> Result<&mut Pty, Error> {
    let pty_id = session
        .terminal_backend(terminal_handle)
        .ok_or(Error::NotFound)?;
    let pty = session.ptys.get_mut(&pty_id).ok_or(Error::NotFound)?;
    if pty_generation(pty) != generation {
        return Err(Error::Stale);
    }
    Ok(pty)
}

fn pty_generation(pty: &Pty) -> u32 {
    u32::try_from(pty.generation).unwrap_or(u32::MAX).max(1)
}

fn capture_rows(pty: &mut Pty) -> Result<Vec<CapturedRow>, Error> {
    let viewport = take_snapshot(pty);
    let height = usize::from(viewport.rows());
    if height == 0 {
        return Ok(Vec::new());
    }
    let history = viewport.scrollback_lines() as usize;
    let mut rows = Vec::with_capacity(history.saturating_add(height));
    let mut offset = history;
    while offset > 0 {
        let frame = pty.driver.scrollback_frame(offset);
        let take = offset.min(height).min(frame.rows() as usize);
        capture_frame_rows(&frame, history - offset, take, &mut rows)?;
        offset = offset.saturating_sub(height);
    }
    capture_frame_rows(&viewport, history, height, &mut rows)?;
    Ok(rows)
}

fn capture_frame_rows(
    frame: &FrameState,
    absolute_start: usize,
    count: usize,
    target: &mut Vec<CapturedRow>,
) -> Result<(), Error> {
    let cols = usize::from(frame.cols());
    for row_index in 0..count {
        let row = u16::try_from(row_index).map_err(|_| Error::TooLarge)?;
        let first = row_index.checked_mul(cols).ok_or(Error::TooLarge)?;
        let cells = frame
            .cells()
            .get(first * CELL_SIZE..(first + cols) * CELL_SIZE)
            .ok_or(Error::Internal)?
            .as_chunks::<CELL_SIZE>()
            .0
            .to_vec();
        let contents = (0..frame.cols())
            .map(|col| frame.cell_content(row, col).to_owned())
            .collect();
        let overflow = frame
            .overflow()
            .range(first..first + cols)
            .map(|(&index, text)| wire::StyledOverflow {
                cell_offset: (index - first) as u32,
                text: text.clone(),
            })
            .collect();
        let hyperlinks = capture_hyperlinks(frame, row, 0, frame.cols() as u32);
        target.push(CapturedRow {
            row: i64::try_from(absolute_start + row_index).map_err(|_| Error::TooLarge)?,
            start_col: 0,
            cells,
            contents,
            overflow,
            hyperlinks,
            wrapped: frame.is_wrapped(row),
        });
    }
    Ok(())
}

fn capture_hyperlinks(
    frame: &FrameState,
    row: u16,
    start_col: u32,
    end_col: u32,
) -> Vec<wire::StyledHyperlink> {
    let mut links = Vec::new();
    let mut col = start_col.min(frame.cols() as u32);
    let end = end_col.min(frame.cols() as u32);
    while col < end {
        let Some(uri) = frame.cell_link(row, col as u16) else {
            col += 1;
            continue;
        };
        let begin = col;
        col += 1;
        while col < end && frame.cell_link(row, col as u16) == Some(uri) {
            col += 1;
        }
        links.push(wire::StyledHyperlink {
            start_col: begin,
            cell_count: col - begin,
            uri: uri.to_owned(),
        });
    }
    links
}

fn slice_row(row: &CapturedRow, start_col: u32, end_col: u32) -> CapturedRow {
    let row_start = row.start_col;
    let row_end = row_start.saturating_add(row.cells.len() as u32);
    let start = start_col.clamp(row_start, row_end);
    let end = end_col.clamp(start, row_end);
    let begin_index = (start - row_start) as usize;
    let end_index = (end - row_start) as usize;
    let overflow = row
        .overflow
        .iter()
        .filter_map(|item| {
            let absolute = row_start + item.cell_offset;
            (absolute >= start && absolute < end).then(|| wire::StyledOverflow {
                cell_offset: absolute - start,
                text: item.text.clone(),
            })
        })
        .collect();
    let hyperlinks = row
        .hyperlinks
        .iter()
        .filter_map(|link| {
            let link_end = link.start_col.saturating_add(link.cell_count);
            let clipped_start = link.start_col.max(start);
            let clipped_end = link_end.min(end);
            (clipped_start < clipped_end).then(|| wire::StyledHyperlink {
                start_col: clipped_start,
                cell_count: clipped_end - clipped_start,
                uri: link.uri.clone(),
            })
        })
        .collect();
    CapturedRow {
        row: row.row,
        start_col: start,
        cells: row.cells[begin_index..end_index].to_vec(),
        contents: row.contents[begin_index..end_index].to_vec(),
        overflow,
        hyperlinks,
        wrapped: row.wrapped,
    }
}

fn rows_query(
    rows: &[CapturedRow],
    representation: u8,
    requested_max_bytes: u32,
    total_lines: u64,
    cursor_kind: u8,
    cursor_b: u32,
) -> Result<QueryData, Error> {
    let maximum = (requested_max_bytes as usize).min(journal::output_max());
    let complete = encode_rows(rows, representation)?;
    let (accepted, encoded) = if complete.bytes.len() <= maximum {
        (rows.len(), complete)
    } else {
        let empty = encode_rows(&[], representation)?;
        if empty.bytes.len() > maximum {
            return Err(Error::TooLarge);
        }
        let mut accepted = 0usize;
        let mut encoded = empty;
        let mut upper = rows.len();
        while accepted < upper {
            let candidate_count = accepted + (upper - accepted).div_ceil(2);
            let candidate = encode_rows(&rows[..candidate_count], representation)?;
            if candidate.bytes.len() <= maximum {
                accepted = candidate_count;
                encoded = candidate;
            } else {
                upper = candidate_count - 1;
            }
        }
        if accepted == 0 && !rows.is_empty() {
            return Err(Error::TooLarge);
        }
        (accepted, encoded)
    };
    let truncated = accepted < rows.len();
    let next_cursor = truncated.then(|| {
        let next_row = rows[accepted].row.max(0) as u64;
        if cursor_kind == schema::terminal::READ_CURSOR_TAIL as u8 {
            let selected_end = rows
                .last()
                .map_or(next_row, |row| row.row.max(0) as u64 + 1);
            wire::QueryNextCursor::Read(wire::QueryCursor {
                kind: cursor_kind,
                a: total_lines.saturating_sub(selected_end),
                b: u32::try_from(rows.len() - accepted).unwrap_or(u32::MAX),
            })
        } else {
            wire::QueryNextCursor::Read(wire::QueryCursor {
                kind: cursor_kind,
                a: next_row,
                b: if cursor_b == 0 {
                    0
                } else {
                    cursor_b.saturating_sub(accepted as u32)
                },
            })
        }
    });
    Ok(QueryData {
        content_kind: encoded.content_kind,
        encoding: encoded.encoding,
        flags: if truncated {
            schema::terminal::QUERY_TRUNCATED as u16
        } else {
            0
        },
        bytes: encoded.bytes,
        next_cursor,
        total_lines: Some(total_lines),
    })
}

struct EncodedRows {
    content_kind: u8,
    encoding: u8,
    bytes: Vec<u8>,
}

fn encode_rows(rows: &[CapturedRow], representation: u8) -> Result<EncodedRows, Error> {
    let plain = || plain_rows(rows);
    let styled = || {
        wire::StyledLines(rows.iter().map(styled_row).collect())
            .encode()
            .map_err(|_| Error::Internal)
    };
    match representation {
        value if value == schema::terminal::QUERY_REPRESENTATION_PLAIN as u8 => Ok(EncodedRows {
            content_kind: schema::terminal::CONTENT_TEXT as u8,
            encoding: schema::terminal::QUERY_ENCODING_UTF8 as u8,
            bytes: plain().into_bytes(),
        }),
        value if value == schema::terminal::QUERY_REPRESENTATION_STYLED as u8 => Ok(EncodedRows {
            content_kind: schema::terminal::CONTENT_STYLED_LINES as u8,
            encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            bytes: styled()?,
        }),
        value if value == schema::terminal::QUERY_REPRESENTATION_BOTH as u8 => {
            let value = wire::TextAndStyled {
                plain: plain(),
                styled: wire::StyledLines(rows.iter().map(styled_row).collect()),
            };
            Ok(EncodedRows {
                content_kind: schema::terminal::CONTENT_TEXT_AND_STYLED as u8,
                encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
                bytes: value.encode().map_err(|_| Error::Internal)?,
            })
        }
        _ => Err(Error::Invalid("Terminal query representation")),
    }
}

fn styled_row(row: &CapturedRow) -> wire::StyledLine {
    wire::StyledLine {
        row: row.row,
        start_col: row.start_col,
        cells: row.cells.clone(),
        overflow: row.overflow.clone(),
        hyperlinks: row.hyperlinks.clone(),
    }
}

fn plain_rows(rows: &[CapturedRow]) -> String {
    let mut output = String::new();
    let mut previous_wrapped = true;
    for (index, row) in rows.iter().enumerate() {
        if index != 0 && !previous_wrapped {
            output.push('\n');
        }
        let start = output.len();
        for content in &row.contents {
            output.push_str(content);
        }
        if !row.wrapped {
            output.truncate(start + output[start..].trim_end().len());
        }
        previous_wrapped = row.wrapped;
    }
    output
}

fn searchable_row(row: &CapturedRow) -> (Vec<u8>, Vec<(usize, usize, u32)>) {
    let mut bytes = Vec::new();
    let mut columns = Vec::with_capacity(row.contents.len());
    for (index, content) in row.contents.iter().enumerate() {
        let start = bytes.len();
        bytes.extend_from_slice(content.as_bytes());
        columns.push((start, bytes.len(), row.start_col + index as u32));
    }
    (bytes, columns)
}

fn byte_to_column(columns: &[(usize, usize, u32)], byte: usize, end: bool) -> u32 {
    for &(start, finish, column) in columns {
        if (!end && byte >= start && byte < finish) || (end && byte > start && byte <= finish) {
            return column + u32::from(end);
        }
    }
    columns
        .last()
        .map_or(0, |(_, _, column)| column + u32::from(end))
}

fn search_next_cursor(
    rows: &[CapturedRow],
    row_index: usize,
    backward: bool,
    start_col: u32,
    end_col: u32,
) -> Option<wire::QueryNextCursor> {
    let (row, column) = if backward {
        if start_col == end_col && start_col == 0 {
            let previous = row_index.checked_sub(1)?;
            (rows[previous].row, u32::MAX)
        } else {
            (
                rows[row_index].row,
                if start_col == end_col {
                    start_col - 1
                } else {
                    start_col
                },
            )
        }
    } else {
        let row_end = rows[row_index]
            .start_col
            .saturating_add(rows[row_index].cells.len() as u32);
        if start_col == end_col && end_col >= row_end {
            let next = row_index.checked_add(1).filter(|next| *next < rows.len())?;
            (rows[next].row, 0)
        } else {
            (
                rows[row_index].row,
                if start_col == end_col {
                    end_col.saturating_add(1)
                } else {
                    end_col
                },
            )
        }
    };
    Some(wire::QueryNextCursor::Search(wire::QueryCursor {
        kind: schema::terminal::SEARCH_CURSOR_POSITION as u8,
        a: row.max(0) as u64,
        b: column,
    }))
}

fn output_query(
    pty: &Pty,
    cursor_kind: u8,
    cursor_a: u64,
    cursor_b: u32,
    max_bytes: u32,
) -> Result<QueryData, Error> {
    let budget = journal_read_budget(max_bytes);
    let generation = pty_generation(pty);
    let result = match cursor_kind {
        kind if kind == schema::terminal::OUTPUT_CURSOR_COMMAND as u8 => {
            command_output(pty, generation, cursor_a, cursor_b, budget)?
        }
        kind if kind == schema::terminal::OUTPUT_CURSOR_LATEST_COMMAND as u8 => {
            let index = pty.journal.latest().ok_or(Error::NotFound)?.index;
            command_output(pty, generation, index, cursor_b, budget)?
        }
        kind if kind == schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8 => {
            sequence_output(pty, generation, cursor_a, cursor_b, budget, false)?
        }
        kind if kind == schema::terminal::OUTPUT_CURSOR_PROBE as u8 => {
            let (sequence, column) = pty.driver.cursor_seq();
            OutputResult {
                generation,
                flags: output_flags(pty, false, false, false),
                start_seq: sequence,
                start_col: u32::from(column),
                next_seq: sequence,
                next_col: u32::from(column),
                text: Vec::new(),
            }
        }
        _ => return Err(Error::Invalid("Terminal OUTPUT cursor kind")),
    };
    output_data(result)
}

fn command_output(
    pty: &Pty,
    generation: u32,
    index: u64,
    column: u32,
    budget: usize,
) -> Result<OutputResult, Error> {
    let (cursor_seq, _) = pty.driver.cursor_seq();
    let record = pty
        .journal
        .snapshot(index, cursor_seq, pty.driver.oldest_seq())
        .ok_or(Error::NotFound)?;
    let end = (!record.running()).then_some(record.end_seq);
    let column = u16::try_from(column).map_err(|_| Error::Invalid("Terminal OUTPUT column"))?;
    let read = pty.driver.seq_text(record.start_seq, column, end, budget);
    Ok(seq_result(pty, generation, read, false))
}

fn sequence_output(
    pty: &Pty,
    generation: u32,
    sequence: u64,
    column: u32,
    budget: usize,
    matched: bool,
) -> Result<OutputResult, Error> {
    let column = u16::try_from(column).map_err(|_| Error::Invalid("Terminal OUTPUT column"))?;
    let read = pty.driver.seq_text(sequence, column, None, budget);
    Ok(seq_result(pty, generation, read, matched))
}

fn seq_result(
    pty: &Pty,
    generation: u32,
    read: yas_terminal_driver::SeqText,
    matched: bool,
) -> OutputResult {
    OutputResult {
        generation,
        flags: output_flags(pty, read.truncated, read.evicted, matched),
        start_seq: read.start_seq,
        start_col: u32::from(read.start_col),
        next_seq: read.next_seq,
        next_col: u32::from(read.next_col),
        text: read.text.into_bytes(),
    }
}

fn output_flags(pty: &Pty, truncated: bool, evicted: bool, matched: bool) -> u16 {
    let mut flags = 0;
    if truncated {
        flags |= schema::terminal::OUTPUT_TRUNCATED as u16;
    }
    if evicted {
        flags |= schema::terminal::OUTPUT_EVICTED as u16;
    }
    if pty.driver.alt_screen() {
        flags |= schema::terminal::OUTPUT_ALT_SCREEN as u16;
    }
    if matched {
        flags |= schema::terminal::OUTPUT_MATCHED as u16;
    }
    flags
}

fn output_data(result: OutputResult) -> Result<QueryData, Error> {
    let next_cursor = Some(wire::QueryNextCursor::Output(wire::QueryCursor {
        kind: schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8,
        a: result.next_seq,
        b: result.next_col,
    }));
    let flags = if result.flags & schema::terminal::OUTPUT_TRUNCATED as u16 != 0 {
        schema::terminal::QUERY_TRUNCATED as u16
    } else {
        0
    };
    Ok(QueryData {
        content_kind: schema::terminal::CONTENT_OUTPUT as u8,
        encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
        flags,
        bytes: result.encode().map_err(|_| Error::Internal)?,
        next_cursor,
        total_lines: None,
    })
}

fn output_wait_query(
    pty: &Pty,
    sequence: u64,
    column: u32,
    max_bytes: u32,
    needle: &[u8],
) -> Result<(QueryData, bool), Error> {
    let budget = journal_read_budget(max_bytes);
    let column = u16::try_from(column).map_err(|_| Error::Invalid("Terminal WAIT column"))?;
    let probe = pty.driver.seq_text(sequence, column, None, budget);
    let matched = if needle.is_empty() {
        !probe.text.is_empty()
    } else {
        contains(probe.text.as_bytes(), needle)
    };
    let result = if matched {
        let mut low = sequence;
        let mut high = probe.next_seq;
        while high > low + 1 {
            let middle = low + (high - low) / 2;
            let candidate = pty.driver.seq_text(sequence, column, Some(middle), budget);
            let hit = if needle.is_empty() {
                !candidate.text.is_empty()
            } else {
                contains(candidate.text.as_bytes(), needle)
            };
            if hit {
                high = middle;
            } else {
                low = middle;
            }
        }
        let exact = pty.driver.seq_text(sequence, column, Some(high), budget);
        seq_result(pty, pty_generation(pty), exact, true)
    } else {
        seq_result(pty, pty_generation(pty), probe, false)
    };
    Ok((output_data(result)?, matched))
}

fn poll_command_wait(pty: &Pty, index: u64, timed_out: bool) -> Result<Option<WaitOutcome>, Error> {
    let (cursor_seq, _) = pty.driver.cursor_seq();
    let record = pty
        .journal
        .snapshot(index, cursor_seq, pty.driver.oldest_seq());
    let satisfied = record.as_ref().is_some_and(|record| !record.running());
    if !satisfied && !timed_out && !pty.exited {
        return Ok(None);
    }
    if record.is_none() && (timed_out || pty.exited || index < pty.journal.oldest_index()) {
        return Err(Error::NotFound);
    }
    let records = record
        .map(|record| journal_record(pty_generation(pty), record))
        .into_iter()
        .collect();
    let bytes = JournalResult {
        oldest_index: pty.journal.oldest_index(),
        next_index: pty.journal.next_index(),
        records,
    }
    .encode()
    .map_err(|_| Error::Internal)?;
    Ok(Some(WaitOutcome {
        data: QueryData {
            content_kind: schema::terminal::CONTENT_JOURNAL as u8,
            encoding: schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            flags: 0,
            bytes,
            next_cursor: index
                .checked_add(1)
                .map(wire::QueryNextCursor::JournalIndex),
            total_lines: None,
        },
        satisfied,
    }))
}

fn journal_record(generation: u32, record: yas_terminal_model::CommandRecord) -> JournalRecord {
    JournalRecord {
        index: record.index,
        generation,
        flags: u16::from(record.flags),
        exit_code: record.exit_code,
        start_seq: record.start_seq,
        end_seq: record.end_seq,
        started_unix_ms: record.started_ms,
        ended_unix_ms: record.ended_ms,
        command: record.command,
    }
}

fn journal_read_budget(requested: u32) -> usize {
    (requested as usize).min(journal::output_max())
}

fn resolve_row(row: i64, total: usize) -> Result<(usize, bool), Error> {
    if total == 0 {
        return Err(Error::NotFound);
    }
    let total = i128::try_from(total).map_err(|_| Error::TooLarge)?;
    let resolved = if row < 0 {
        total + i128::from(row)
    } else {
        i128::from(row)
    };
    let clamped = resolved.clamp(0, total - 1);
    Ok((
        usize::try_from(clamped).map_err(|_| Error::TooLarge)?,
        clamped != resolved,
    ))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(row: i64, start_col: u32, text: &[&str], wrapped: bool) -> CapturedRow {
        CapturedRow {
            row,
            start_col,
            cells: vec![[0; CELL_SIZE]; text.len()],
            contents: text.iter().map(|value| (*value).to_owned()).collect(),
            overflow: Vec::new(),
            hyperlinks: Vec::new(),
            wrapped,
        }
    }

    #[test]
    fn row_slicing_keeps_absolute_columns_and_relative_overflow() {
        let mut source = row(7, 0, &["a", "b", "c", "d"], false);
        source.overflow.push(wire::StyledOverflow {
            cell_offset: 2,
            text: "wide".to_owned(),
        });
        source.hyperlinks.push(wire::StyledHyperlink {
            start_col: 1,
            cell_count: 3,
            uri: "https://example.test".to_owned(),
        });
        let sliced = slice_row(&source, 2, 4);
        assert_eq!(sliced.start_col, 2);
        assert_eq!(sliced.contents, ["c", "d"]);
        assert_eq!(sliced.overflow[0].cell_offset, 0);
        assert_eq!(sliced.hyperlinks[0].start_col, 2);
        assert_eq!(sliced.hyperlinks[0].cell_count, 2);
    }

    #[test]
    fn plain_rows_respect_soft_wraps_and_trim_hard_lines() {
        let rows = [
            row(0, 0, &["hello", " "], true),
            row(1, 0, &["world", " "], false),
            row(2, 0, &["next", " "], false),
        ];
        assert_eq!(plain_rows(&rows), "hello world\nnext");
    }

    #[test]
    fn negative_copy_rows_resolve_from_one_past_tail() {
        assert_eq!(resolve_row(-1, 5), Ok((4, false)));
        assert_eq!(resolve_row(-5, 5), Ok((0, false)));
        assert_eq!(resolve_row(-6, 5), Ok((0, true)));
        assert_eq!(resolve_row(5, 5), Ok((4, true)));
        assert_eq!(resolve_row(0, 0), Err(Error::NotFound));
    }

    #[test]
    fn byte_columns_cover_multibyte_cells() {
        let source = row(0, 4, &["a", "é", "z"], false);
        let (line, columns) = searchable_row(&source);
        assert_eq!(line, "aéz".as_bytes());
        assert_eq!(byte_to_column(&columns, 1, false), 5);
        assert_eq!(byte_to_column(&columns, 3, true), 6);
    }

    #[test]
    fn zero_width_search_continuations_always_advance() {
        let rows = [row(4, 0, &["a", "b"], false), row(5, 0, &["c"], false)];
        assert_eq!(
            search_next_cursor(&rows, 0, false, 1, 1),
            Some(wire::QueryNextCursor::Search(wire::QueryCursor {
                kind: schema::terminal::SEARCH_CURSOR_POSITION as u8,
                a: 4,
                b: 2,
            }))
        );
        assert_eq!(
            search_next_cursor(&rows, 0, false, 2, 2),
            Some(wire::QueryNextCursor::Search(wire::QueryCursor {
                kind: schema::terminal::SEARCH_CURSOR_POSITION as u8,
                a: 5,
                b: 0,
            }))
        );
        assert_eq!(search_next_cursor(&rows, 0, true, 0, 0), None);
        assert_eq!(
            search_next_cursor(&rows, 1, true, 0, 0),
            Some(wire::QueryNextCursor::Search(wire::QueryCursor {
                kind: schema::terminal::SEARCH_CURSOR_POSITION as u8,
                a: 4,
                b: u32::MAX,
            }))
        );
    }

    #[test]
    fn row_pages_never_exceed_the_requested_byte_limit() {
        let rows = [
            row(10, 0, &["first"], false),
            row(11, 0, &["second"], false),
        ];
        let first_len = encode_rows(
            &rows[..1],
            schema::terminal::QUERY_REPRESENTATION_PLAIN as u8,
        )
        .unwrap()
        .bytes
        .len();
        let page = rows_query(
            &rows,
            schema::terminal::QUERY_REPRESENTATION_PLAIN as u8,
            first_len as u32,
            20,
            schema::terminal::READ_CURSOR_ABSOLUTE as u8,
            2,
        )
        .unwrap();
        assert_eq!(page.bytes, b"first");
        assert_eq!(page.flags, schema::terminal::QUERY_TRUNCATED as u16);
        assert_eq!(
            page.next_cursor,
            Some(wire::QueryNextCursor::Read(wire::QueryCursor {
                kind: schema::terminal::READ_CURSOR_ABSOLUTE as u8,
                a: 11,
                b: 1,
            }))
        );
        assert!(matches!(
            rows_query(
                &rows,
                schema::terminal::QUERY_REPRESENTATION_PLAIN as u8,
                (first_len - 1) as u32,
                20,
                schema::terminal::READ_CURSOR_ABSOLUTE as u8,
                2,
            ),
            Err(Error::TooLarge)
        ));
    }
}
