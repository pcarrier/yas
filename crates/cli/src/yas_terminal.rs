//! Native YAS implementations of the Rust CLI's core Terminal commands.

mod recording;
pub(crate) mod stream;

use std::time::Duration;

use yas_wire::{
    Class, Decode, Encode, Extension, Extensions, family,
    state::{Phase, RecordKind, StateAck, StateEvent, Watch, WatchResult},
    terminal,
};

use crate::{terminal_args, yas_native::NativeClient};

const NS_PER_SECOND: u64 = 1_000_000_000;
const QUERY_CREDIT: u64 = 1024 * 1024;
const MAX_QUERY_BYTES: u64 = 64 * 1024 * 1024;
const READ_PAGE_BYTES: u32 = 8 * 1024 * 1024;
const STATE_CREDIT: u64 = 1024 * 1024;

pub(crate) async fn cmd_list(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let mut terminals = client
        .snapshot(family::TERMINAL)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Terminal family".to_string())?
        .into_iter()
        .filter_map(|record| terminal::terminal_from_state_record(&record).ok())
        .collect::<Vec<_>>();
    terminals.sort_by_key(|record| record.terminal_handle);

    println!("ID\tTAG\tTITLE\tCOMMAND\tCWD\tSTATUS");
    for record in terminals {
        let id = terminal_id(record.terminal_handle)?;
        let tag = text_extension(
            &record.extensions,
            yas_wire::schema::terminal::STATE_RESOURCE_TAG_EXTENSION,
        )
        .unwrap_or_default();
        let title = text_extension(
            &record.extensions,
            yas_wire::schema::terminal::STATE_TITLE_EXTENSION,
        )
        .unwrap_or_default();
        let command = text_extension(
            &record.extensions,
            yas_wire::schema::terminal::STATE_COMMAND_DISPLAY_EXTENSION,
        )
        .unwrap_or_default();
        let cwd = bytes_extension(
            &record.extensions,
            yas_wire::schema::terminal::STATE_CWD_EXTENSION,
        )
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .unwrap_or_default();
        let status = terminal_status(&record)?;
        println!("{id}\t{tag}\t{title}\t{command}\t{cwd}\t{status}");
    }
    Ok(())
}

pub(crate) async fn cmd_start(
    on: Option<&str>,
    hub: &str,
    request: terminal_args::StartRequest,
) -> Result<u64, String> {
    let launch = launch_from_request(&request)?;
    let mut create_extensions = Vec::new();
    if let Some(tag) = request.tag.as_deref() {
        create_extensions.push(Extension {
            tag: yas_wire::schema::terminal::CREATE_RESOURCE_TAG_EXTENSION as u16,
            required: false,
            value: tag.as_bytes().to_vec(),
        });
    }
    let create = terminal::Create {
        rows: request.rows,
        cols: request.cols,
        operation_id: operation_id(),
        launch,
        extensions: Extensions(create_extensions),
    };
    let mut client = NativeClient::connect(on, hub).await?;
    let result: terminal::CreateResult = client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::CREATE,
            &create,
            true,
        )
        .await?;
    let id = terminal_id(result.terminal_handle)?;
    println!("{id}");
    Ok(id)
}

pub(crate) async fn cmd_send(
    on: Option<&str>,
    hub: &str,
    id: u64,
    text: String,
) -> Result<(), String> {
    let bytes = terminal_args::parse_escapes(&text);
    if bytes.is_empty() {
        return Ok(());
    }
    let mut client = NativeClient::connect(on, hub).await?;
    client
        .send_typed_event(
            family::TERMINAL,
            terminal::event_kind::WRITE,
            &terminal::Write {
                terminal_handle: terminal_handle(id)?,
                data: bytes,
            },
            true,
        )
        .await
}

pub(crate) async fn cmd_restart(on: Option<&str>, hub: &str, id: u64) -> Result<(), String> {
    let request = terminal::Restart {
        terminal_handle: terminal_handle(id)?,
        operation_id: operation_id(),
        launch_mode: terminal::LaunchMode::Replay,
        cutover_mode: terminal::CutoverMode::StopThenStart,
        launch: None,
        extensions: Extensions::default(),
    };
    let mut client = NativeClient::connect(on, hub).await?;
    let _: terminal::RestartResult = client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::RESTART,
            &request,
            true,
        )
        .await?;
    Ok(())
}

pub(crate) async fn cmd_deadline(
    on: Option<&str>,
    hub: &str,
    id: u64,
    seconds: u64,
) -> Result<(), String> {
    let deadline = if seconds == 0 {
        terminal::Deadline::Clear
    } else {
        terminal::Deadline::Set(seconds.saturating_mul(NS_PER_SECOND))
    };
    request_empty(
        on,
        hub,
        terminal::request_kind::SET_DEADLINE,
        &terminal::SetDeadline {
            terminal_handle: terminal_handle(id)?,
            operation_id: operation_id(),
            deadline,
        },
        false,
    )
    .await
}

pub(crate) async fn cmd_kill(
    on: Option<&str>,
    hub: &str,
    id: u64,
    signal: &str,
) -> Result<(), String> {
    let signal = match signal.strip_prefix("SIG").unwrap_or(signal) {
        "2" | "INT" => terminal::SignalKind::Interrupt,
        "15" | "TERM" => terminal::SignalKind::Terminate,
        "9" | "KILL" => terminal::SignalKind::Kill,
        "1" | "HUP" => terminal::SignalKind::Hangup,
        other => {
            return Err(format!(
                "signal {other:?} has no YAS Terminal v1 semantic value; expected HUP, INT, TERM, or KILL"
            ));
        }
    };
    request_empty(
        on,
        hub,
        terminal::request_kind::SIGNAL,
        &terminal::Signal {
            terminal_handle: terminal_handle(id)?,
            operation_id: operation_id(),
            signal,
            extensions: Extensions::default(),
        },
        false,
    )
    .await
}

pub(crate) async fn cmd_close(on: Option<&str>, hub: &str, id: u64) -> Result<(), String> {
    request_empty(
        on,
        hub,
        terminal::request_kind::CLOSE,
        &terminal::Close {
            terminal_handle: terminal_handle(id)?,
            operation_id: operation_id(),
        },
        false,
    )
    .await
}

pub(crate) async fn cmd_resize(
    on: Option<&str>,
    hub: &str,
    id: u64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let _: terminal::ResizeResult = client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::RESIZE,
            &terminal::Resize {
                terminal_handle: terminal_handle(id)?,
                rows,
                cols,
            },
            false,
        )
        .await?;
    Ok(())
}

pub(crate) async fn cmd_show(
    on: Option<&str>,
    hub: &str,
    id: u64,
    ansi: bool,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(), String> {
    if rows.is_some() || cols.is_some() {
        cmd_resize(on, hub, id, cols.unwrap_or(80), rows.unwrap_or(24)).await?;
    }
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let request = terminal::Read {
        terminal_handle: record.terminal_handle,
        generation: record.generation,
        cursor_kind: yas_wire::schema::terminal::READ_CURSOR_TAIL as u8,
        representation: if ansi {
            yas_wire::schema::terminal::QUERY_REPRESENTATION_STYLED as u8
        } else {
            yas_wire::schema::terminal::QUERY_REPRESENTATION_PLAIN as u8
        },
        flags: yas_wire::schema::terminal::READ_FLAGS as u16,
        cursor_a: 0,
        cursor_b: u32::from(record.rows),
        max_bytes: READ_PAGE_BYTES,
        initial_receive_credit: QUERY_CREDIT,
        extensions: Extensions::default(),
    };
    let query = terminal_query(
        &mut client,
        terminal::request_kind::READ,
        &request,
        READ_PAGE_BYTES as u64,
        Duration::from_secs(10),
    )
    .await?;
    print_query_rows(&query, ansi)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_history(
    on: Option<&str>,
    hub: &str,
    id: u64,
    from_start: Option<u32>,
    from_end: Option<u32>,
    limit: Option<u32>,
    since: Option<String>,
    max_bytes: Option<u32>,
    json: bool,
    ansi: bool,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(), String> {
    if let Some(cursor) = since {
        return cmd_since(
            on,
            hub,
            id,
            &cursor,
            max_bytes.unwrap_or(terminal_args::OUTPUT_MAX_BYTES),
            json,
        )
        .await;
    }
    if rows.is_some() || cols.is_some() {
        cmd_resize(on, hub, id, cols.unwrap_or(80), rows.unwrap_or(24)).await?;
    }
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let cursor_kind;
    let cursor_a;
    let cursor_b;
    if let Some(offset) = from_end {
        cursor_kind = yas_wire::schema::terminal::READ_CURSOR_TAIL as u8;
        cursor_a = u64::from(offset);
        cursor_b = limit.unwrap_or(0);
    } else {
        cursor_kind = yas_wire::schema::terminal::READ_CURSOR_ABSOLUTE as u8;
        cursor_a = u64::from(from_start.unwrap_or(0));
        cursor_b = limit.unwrap_or(0);
    }
    let representation = if ansi {
        yas_wire::schema::terminal::QUERY_REPRESENTATION_STYLED as u8
    } else {
        yas_wire::schema::terminal::QUERY_REPRESENTATION_PLAIN as u8
    };
    let mut next = Some(terminal::QueryCursor {
        kind: cursor_kind,
        a: cursor_a,
        b: cursor_b,
    });
    let mut first = true;
    while let Some(cursor) = next.take() {
        let request = terminal::Read {
            terminal_handle: record.terminal_handle,
            generation: record.generation,
            cursor_kind: cursor.kind,
            representation,
            flags: yas_wire::schema::terminal::READ_FLAGS as u16,
            cursor_a: cursor.a,
            cursor_b: cursor.b,
            max_bytes: READ_PAGE_BYTES,
            initial_receive_credit: QUERY_CREDIT,
            extensions: Extensions::default(),
        };
        let query = terminal_query(
            &mut client,
            terminal::request_kind::READ,
            &request,
            READ_PAGE_BYTES as u64,
            Duration::from_secs(10),
        )
        .await?;
        print_query_rows_with_separator(&query, ansi, &mut first)?;
        next = match query.next_cursor {
            Some(terminal::QueryNextCursor::Read(cursor)) => Some(cursor),
            Some(_) => return Err("YAS Terminal READ returned the wrong cursor type".into()),
            None => None,
        };
    }
    Ok(())
}

pub(crate) async fn cmd_cwd(on: Option<&str>, hub: &str, id: u64) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let query = terminal_query(
        &mut client,
        terminal::request_kind::CWD,
        &terminal::CwdQuery {
            terminal_handle: record.terminal_handle,
            generation: record.generation,
            initial_receive_credit: QUERY_CREDIT,
            extensions: Extensions::default(),
        },
        MAX_QUERY_BYTES,
        Duration::from_secs(10),
    )
    .await?;
    expect_content(&query, yas_wire::schema::terminal::CONTENT_PATH as u8)?;
    let bytes = query_bytes(query).await?;
    std::io::Write::write_all(&mut std::io::stdout(), &bytes)
        .map_err(|error| format!("cannot write terminal cwd: {error}"))?;
    if !bytes.ends_with(b"\n") {
        println!();
    }
    Ok(())
}

pub(crate) async fn cmd_journal(
    on: Option<&str>,
    hub: &str,
    id: u64,
    from: Option<u64>,
    limit: u16,
    json: bool,
) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    if !json {
        println!("INDEX\tSTATUS\tEXIT\tMS\tSTART_SEQ\tEND_SEQ\tCOMMAND");
    }
    let mut cursor = from.unwrap_or(0);
    let mut tail = from.is_none();
    let mut remaining = limit;
    let mut printed = 0usize;
    while remaining != 0 {
        let query = terminal_query(
            &mut client,
            terminal::request_kind::JOURNAL,
            &terminal::Journal {
                terminal_handle: record.terminal_handle,
                generation: record.generation,
                flags: if tail {
                    yas_wire::schema::terminal::JOURNAL_TAIL as u16
                } else {
                    0
                },
                limit: remaining,
                from_index: cursor,
                initial_receive_credit: QUERY_CREDIT,
                extensions: Extensions::default(),
            },
            MAX_QUERY_BYTES,
            Duration::from_secs(10),
        )
        .await?;
        expect_content(&query, yas_wire::schema::terminal::CONTENT_JOURNAL as u8)?;
        let next = query.next_cursor;
        let journal =
            terminal::JournalResult::decode(&query_bytes(query).await?).map_err(wire_error)?;
        for entry in &journal.records {
            if json {
                println!("{}", journal_record_json(entry));
            } else {
                print_journal_record(entry);
            }
        }
        let count = u16::try_from(journal.records.len())
            .map_err(|_| "YAS Terminal JOURNAL returned too many records".to_string())?;
        if count > remaining {
            return Err("YAS Terminal JOURNAL exceeded the requested record count".into());
        }
        printed += usize::from(count);
        remaining -= count;
        let next = match next {
            Some(terminal::QueryNextCursor::JournalIndex(next)) => next,
            Some(_) => return Err("YAS Terminal JOURNAL returned the wrong cursor type".into()),
            None => break,
        };
        if count == 0 || (!tail && next <= cursor) {
            return Err("YAS Terminal JOURNAL cursor made no progress".into());
        }
        cursor = next;
        tail = false;
    }
    if printed == 0 && !json {
        eprintln!(
            "yas: no commands recorded — does the shell emit OSC 133? See docs/shell-integration.md"
        );
    }
    Ok(0)
}

pub(crate) async fn cmd_output(
    on: Option<&str>,
    hub: &str,
    id: u64,
    index: Option<u64>,
    wait: Option<u64>,
    max_bytes: u32,
    json: bool,
) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let terminal_record = find_terminal(&mut client, id).await?;
    let waited = match wait {
        Some(timeout) => {
            let wait_kind = if index.is_some() {
                yas_wire::schema::terminal::WAIT_COMMAND as u8
            } else {
                yas_wire::schema::terminal::WAIT_LATEST_COMMAND as u8
            };
            let query = terminal_query(
                &mut client,
                terminal::request_kind::WAIT,
                &terminal::Wait {
                    terminal_handle: terminal_record.terminal_handle,
                    generation: terminal_record.generation,
                    wait_kind,
                    flags: yas_wire::schema::terminal::WAIT_FLAGS as u8,
                    cursor_a: index.unwrap_or(0),
                    cursor_b: 0,
                    max_bytes,
                    timeout_ns: timeout.saturating_mul(NS_PER_SECOND).max(1),
                    needle: Vec::new(),
                    initial_receive_credit: QUERY_CREDIT,
                    extensions: Extensions::default(),
                },
                MAX_QUERY_BYTES,
                Duration::from_secs(timeout.saturating_add(2)),
            )
            .await?;
            expect_content(&query, yas_wire::schema::terminal::CONTENT_JOURNAL as u8)?;
            let result =
                terminal::JournalResult::decode(&query_bytes(query).await?).map_err(wire_error)?;
            Some(
                result
                    .records
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("pty {id}: no such command"))?,
            )
        }
        None => None,
    };
    let resolved_index = waited.as_ref().map(|record| record.index).or(index);
    let query = terminal_query(
        &mut client,
        terminal::request_kind::OUTPUT,
        &terminal::Output {
            terminal_handle: terminal_record.terminal_handle,
            generation: terminal_record.generation,
            cursor_kind: if resolved_index.is_some() {
                yas_wire::schema::terminal::OUTPUT_CURSOR_COMMAND as u8
            } else {
                yas_wire::schema::terminal::OUTPUT_CURSOR_LATEST_COMMAND as u8
            },
            flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
            cursor_a: resolved_index.unwrap_or(0),
            cursor_b: 0,
            max_bytes,
            initial_receive_credit: QUERY_CREDIT,
            extensions: Extensions::default(),
        },
        u64::from(max_bytes).min(MAX_QUERY_BYTES),
        Duration::from_secs(10),
    )
    .await?;
    expect_content(&query, yas_wire::schema::terminal::CONTENT_OUTPUT as u8)?;
    let output = terminal::OutputResult::decode(&query_bytes(query).await?).map_err(wire_error)?;
    let truncated = output.flags & yas_wire::schema::terminal::OUTPUT_TRUNCATED as u16 != 0;
    let evicted = output.flags & yas_wire::schema::terminal::OUTPUT_EVICTED as u16 != 0;
    if json {
        let mut value = waited
            .as_ref()
            .map(journal_record_json)
            .unwrap_or_else(|| serde_json::json!({ "index": resolved_index }));
        let object = value.as_object_mut().expect("journal JSON is an object");
        object.insert(
            "text".into(),
            String::from_utf8_lossy(&output.text).into_owned().into(),
        );
        object.insert("truncated".into(), truncated.into());
        object.insert("output_evicted".into(), evicted.into());
        object.insert(
            "next_cursor".into(),
            format_cursor(output.next_seq, output.next_col).into(),
        );
        println!("{value}");
    } else {
        std::io::Write::write_all(&mut std::io::stdout(), &output.text)
            .map_err(|error| format!("cannot write terminal output: {error}"))?;
        if !output.text.is_empty() && !output.text.ends_with(b"\n") {
            println!();
        }
        if evicted {
            eprintln!("yas: output start had scrolled out of the backlog");
        }
        if truncated {
            eprintln!(
                "yas: truncated at {max_bytes} bytes; continue from cursor {}",
                format_cursor(output.next_seq, output.next_col)
            );
        }
    }
    Ok(waited.as_ref().map_or(0, journal_exit_code))
}

pub(crate) async fn cmd_wait(
    on: Option<&str>,
    hub: &str,
    id: u64,
    timeout_secs: u64,
    pattern: Option<String>,
) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    if record.lifecycle == terminal::Lifecycle::Exited {
        return print_terminal_exit(&record);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    if let Some(pattern) = pattern {
        let regex =
            regex::Regex::new(&pattern).map_err(|error| format!("invalid pattern: {error}"))?;
        return wait_for_pattern(&mut client, &record, id, &regex, deadline).await;
    }
    match tokio::time::timeout_at(deadline, watch_terminal_exit(&mut client, id)).await {
        Ok(result) => print_terminal_exit(&result?),
        Err(_) => {
            eprintln!("yas: timed out waiting for pty {id}");
            Ok(124)
        }
    }
}

pub(crate) async fn cmd_mouse(
    on: Option<&str>,
    hub: &str,
    id: u64,
    event: &str,
    col: u16,
    row: u16,
    button: &str,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    if record.lifecycle == terminal::Lifecycle::Exited {
        return Err(format!("pty {id} has exited"));
    }
    let view = open_view(&mut client, &record, record.rows, record.cols, 30).await?;
    let feedback = terminal::ViewFeedback {
        view_id: view.view_id,
        presented_sequence: view.first_sequence.wrapping_sub(1),
        decoder_queue_depth: 0,
        available_frame_slots: view.max_inflight_frames,
    };
    let result = send_mouse_actions(&mut client, feedback, event, col, row, button).await;
    let close = close_view(&mut client, view.view_id).await;
    result.and(close)
}

pub(crate) async fn cmd_terminal_click(
    on: Option<&str>,
    hub: &str,
    id: u64,
    col: u16,
    row: u16,
    button: &str,
) -> Result<(), String> {
    cmd_mouse(on, hub, id, "click", col, row, button).await
}

pub(crate) async fn cmd_attach(on: Option<&str>, hub: &str, id: u64) -> Result<i32, String> {
    stream::attach(on, hub, id).await
}

pub(crate) async fn cmd_record(
    on: Option<&str>,
    hub: &str,
    id: u64,
    output: Option<String>,
    frames: u32,
    duration: f64,
) -> Result<(), String> {
    stream::record(on, hub, id, output, frames, duration).await
}

pub(crate) async fn cmd_grep(
    on: Option<&str>,
    hub: &str,
    opts: crate::grep::Opts,
) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let records = client
        .snapshot(family::TERMINAL)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Terminal family".to_string())?
        .into_iter()
        .filter_map(|record| terminal::terminal_from_state_record(&record).ok())
        .collect::<Vec<_>>();
    let documents = records
        .iter()
        .map(|record| {
            Ok(crate::grep::Document {
                id: terminal_id(record.terminal_handle)?,
                tag: text_extension(
                    &record.extensions,
                    yas_wire::schema::terminal::STATE_RESOURCE_TAG_EXTENSION,
                )
                .unwrap_or_default(),
                title: text_extension(
                    &record.extensions,
                    yas_wire::schema::terminal::STATE_TITLE_EXTENSION,
                )
                .unwrap_or_default(),
                running: record.lifecycle == terminal::Lifecycle::Running,
                text: String::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut documents = crate::grep::select_documents(&opts, documents)?;
    for document in &mut documents {
        let handle = terminal_handle(document.id)?;
        let record = records
            .iter()
            .find(|record| record.terminal_handle == handle)
            .ok_or_else(|| format!("pty {} disappeared", document.id))?;
        let query = terminal_query(
            &mut client,
            terminal::request_kind::COPY_RANGE,
            &terminal::CopyRange {
                terminal_handle: record.terminal_handle,
                generation: record.generation,
                representation: yas_wire::schema::terminal::QUERY_REPRESENTATION_PLAIN as u8,
                start_row: 0,
                start_col: 0,
                end_row: -1,
                end_col: u32::from(record.cols),
                max_bytes: MAX_QUERY_BYTES as u32,
                initial_receive_credit: QUERY_CREDIT,
                extensions: Extensions::default(),
            },
            MAX_QUERY_BYTES,
            Duration::from_secs(30),
        )
        .await?;
        expect_content(&query, yas_wire::schema::terminal::CONTENT_TEXT as u8)?;
        if query.next_cursor.is_some() {
            return Err(format!(
                "pty {} exceeds the {}-byte native grep collection limit",
                document.id, MAX_QUERY_BYTES
            ));
        }
        document.text = String::from_utf8(query.bytes)
            .map_err(|_| format!("pty {} returned non-UTF-8 text", document.id))?;
    }
    crate::grep::run_documents(opts, documents)
}

struct CollectedQuery {
    content_kind: u8,
    next_cursor: Option<terminal::QueryNextCursor>,
    bytes: Vec<u8>,
}

async fn terminal_query<Request: Encode>(
    client: &mut NativeClient,
    kind: u16,
    request: &Request,
    maximum: u64,
    timeout: Duration,
) -> Result<CollectedQuery, String> {
    if maximum == 0 || maximum > MAX_QUERY_BYTES {
        return Err(format!(
            "invalid Terminal query collection limit {maximum}; maximum is {MAX_QUERY_BYTES}"
        ));
    }
    let body = client
        .request_with_timeout(
            family::TERMINAL,
            kind,
            request.encode().map_err(wire_error)?,
            true,
            timeout,
        )
        .await?;
    let query = terminal::QueryBody::decode(&body).map_err(wire_error)?;
    query
        .validate_receive_credit(QUERY_CREDIT)
        .map_err(wire_error)?;
    let bytes = match &query.delivery {
        terminal::QueryDelivery::Inline(bytes) => bytes.clone(),
        terminal::QueryDelivery::Transfer(descriptor) => {
            client
                .receive_byte_transfer(descriptor, None, maximum)
                .await?
        }
    };
    if bytes.len() as u64 > maximum {
        return Err(format!(
            "YAS Terminal query returned {} bytes; collection limit is {maximum}",
            bytes.len()
        ));
    }
    Ok(CollectedQuery {
        content_kind: query.content_kind,
        next_cursor: query.next_cursor,
        bytes,
    })
}

async fn query_bytes(query: CollectedQuery) -> Result<Vec<u8>, String> {
    Ok(query.bytes)
}

fn expect_content(query: &CollectedQuery, expected: u8) -> Result<(), String> {
    if query.content_kind == expected {
        Ok(())
    } else {
        Err(format!(
            "YAS Terminal query returned content kind {} instead of {expected}",
            query.content_kind
        ))
    }
}

pub(crate) async fn find_terminal(
    client: &mut NativeClient,
    id: u64,
) -> Result<terminal::TerminalRecord, String> {
    let handle = terminal_handle(id)?;
    client
        .snapshot(family::TERMINAL)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Terminal family".to_string())?
        .into_iter()
        .filter_map(|record| terminal::terminal_from_state_record(&record).ok())
        .find(|record| record.terminal_handle == handle)
        .ok_or_else(|| format!("pty {id} not found"))
}

async fn watch_terminal_exit(
    client: &mut NativeClient,
    id: u64,
) -> Result<terminal::TerminalRecord, String> {
    let result: WatchResult = client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::WATCH,
            &Watch {
                initial_credit: STATE_CREDIT,
                resume: None,
                extensions: Extensions::default(),
            },
            false,
        )
        .await?;
    let handle = terminal_handle(id)?;
    let mut target_seen = false;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = client
            .next_matching_event(family::TERMINAL, terminal::event_kind::STATE)
            .await?;
        let event = StateEvent::decode_with(&frame.payload, 0, &[]).map_err(wire_error)?;
        if event.subscription_id != result.subscription_id {
            continue;
        }
        if event.phase == Phase::SnapshotBegin {
            target_seen = false;
        }
        let mut exited = None;
        let mut removed = false;
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => {
                    let record =
                        terminal::terminal_from_state_record(record).map_err(wire_error)?;
                    if record.terminal_handle == handle {
                        target_seen = true;
                        if record.lifecycle == terminal::Lifecycle::Exited {
                            exited = Some(record);
                        }
                    }
                }
                RecordKind::Patch => {
                    let patch = terminal::patch_from_state_record(record).map_err(wire_error)?;
                    target_seen |= patch.terminal_handle == handle;
                }
                RecordKind::Remove => {
                    let removal =
                        terminal::removal_from_state_record(record).map_err(wire_error)?;
                    removed |= removal.terminal_handle == handle;
                }
                RecordKind::Family(_) => {}
            }
        }
        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        client
            .send_typed_event(
                family::TERMINAL,
                terminal::event_kind::STATE_ACK,
                &StateAck {
                    subscription_id: result.subscription_id,
                    applied_revision: event.to_revision,
                    cumulative_byte_limit: cumulative_credit,
                },
                false,
            )
            .await?;
        if let Some(record) = exited {
            return Ok(record);
        }
        if removed || (event.phase == Phase::SnapshotEnd && !target_seen) {
            return Err(format!("pty {id} not found"));
        }
        if event.phase == Phase::Reset {
            target_seen = false;
        }
    }
}

fn print_query_rows(query: &CollectedQuery, ansi: bool) -> Result<(), String> {
    let mut first = true;
    print_query_rows_with_separator(query, ansi, &mut first)
}

fn print_query_rows_with_separator(
    query: &CollectedQuery,
    ansi: bool,
    first: &mut bool,
) -> Result<(), String> {
    let text = if ansi {
        expect_content(
            query,
            yas_wire::schema::terminal::CONTENT_STYLED_LINES as u8,
        )?;
        let lines = terminal::StyledLines::decode(&query.bytes).map_err(wire_error)?;
        render_styled_lines(&lines)
    } else {
        expect_content(query, yas_wire::schema::terminal::CONTENT_TEXT as u8)?;
        String::from_utf8(query.bytes.clone())
            .map_err(|_| "YAS Terminal returned non-UTF-8 text".to_string())?
    };
    if !*first && !text.is_empty() {
        println!();
    }
    print!("{text}");
    *first = false;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CellColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CellStyle {
    fg: CellColor,
    bg: CellColor,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

fn cell_style(cell: &terminal::Cell) -> CellStyle {
    let fg = match cell[0] & 3 {
        1 => CellColor::Indexed(cell[2]),
        2 => CellColor::Rgb(cell[2], cell[3], cell[4]),
        _ => CellColor::Default,
    };
    let bg = match (cell[0] >> 2) & 3 {
        1 => CellColor::Indexed(cell[5]),
        2 => CellColor::Rgb(cell[5], cell[6], cell[7]),
        _ => CellColor::Default,
    };
    CellStyle {
        fg,
        bg,
        bold: cell[0] & (1 << 4) != 0,
        dim: cell[0] & (1 << 5) != 0,
        italic: cell[0] & (1 << 6) != 0,
        underline: cell[0] & (1 << 7) != 0,
        inverse: cell[1] & 1 != 0,
    }
}

fn styled_cell_content<'a>(
    cell: &'a terminal::Cell,
    offset: u32,
    overflow: &'a [terminal::StyledOverflow],
) -> &'a str {
    if cell[1] & 4 != 0 {
        return "";
    }
    let len = usize::from((cell[1] >> 3) & 7);
    if len == 7 {
        return overflow
            .iter()
            .find(|entry| entry.cell_offset == offset)
            .map_or("", |entry| entry.text.as_str());
    }
    if len == 0 {
        " "
    } else if len > 4 {
        // Codec 1 reserves 5 and 6. Treat them as a replacement blank if a
        // future optional peer manages to send one instead of indexing past
        // the fixed four content bytes.
        " "
    } else {
        std::str::from_utf8(&cell[8..8 + len]).unwrap_or(" ")
    }
}

fn render_styled_lines(lines: &terminal::StyledLines) -> String {
    let mut output = String::new();
    for (line_index, line) in lines.0.iter().enumerate() {
        if line_index != 0 {
            output.push('\n');
        }
        let last = line
            .cells
            .iter()
            .enumerate()
            .rfind(|(index, cell)| {
                !styled_cell_content(cell, *index as u32, &line.overflow)
                    .trim()
                    .is_empty()
            })
            .map_or(0, |(index, _)| index + 1);
        let mut style = CellStyle::default();
        let mut link: Option<&str> = None;
        for (index, cell) in line.cells.iter().take(last).enumerate() {
            let next_style = cell_style(cell);
            if next_style != style {
                push_sgr(&mut output, next_style);
                style = next_style;
            }
            let column = line.start_col + index as u32;
            let next_link = line
                .hyperlinks
                .iter()
                .find(|entry| {
                    column >= entry.start_col
                        && column < entry.start_col.saturating_add(entry.cell_count)
                })
                .map(|entry| entry.uri.as_str());
            if next_link != link {
                push_osc8(&mut output, next_link);
                link = next_link;
            }
            output.push_str(styled_cell_content(cell, index as u32, &line.overflow));
        }
        if link.is_some() {
            push_osc8(&mut output, None);
        }
        if style != CellStyle::default() {
            output.push_str("\x1b[0m");
        }
    }
    output
}

fn push_sgr(output: &mut String, style: CellStyle) {
    output.push_str("\x1b[0");
    if style.bold {
        output.push_str(";1");
    }
    if style.dim {
        output.push_str(";2");
    }
    if style.italic {
        output.push_str(";3");
    }
    if style.underline {
        output.push_str(";4");
    }
    if style.inverse {
        output.push_str(";7");
    }
    push_color_sgr(output, style.fg, true);
    push_color_sgr(output, style.bg, false);
    output.push('m');
}

fn push_color_sgr(output: &mut String, color: CellColor, foreground: bool) {
    match color {
        CellColor::Default => output.push_str(if foreground { ";39" } else { ";49" }),
        CellColor::Indexed(index) => {
            output.push_str(if foreground { ";38;5;" } else { ";48;5;" });
            output.push_str(&index.to_string());
        }
        CellColor::Rgb(red, green, blue) => {
            output.push_str(if foreground { ";38;2;" } else { ";48;2;" });
            output.push_str(&format!("{red};{green};{blue}"));
        }
    }
}

fn push_osc8(output: &mut String, uri: Option<&str>) {
    output.push_str("\x1b]8;;");
    if let Some(uri) = uri {
        output.push_str(uri);
    }
    output.push_str("\x1b\\");
}

fn journal_running(record: &terminal::JournalRecord) -> bool {
    record.flags & yas_wire::schema::terminal::JOURNAL_RUNNING as u16 != 0
}

fn journal_status(record: &terminal::JournalRecord) -> &'static str {
    if journal_running(record) {
        "running"
    } else if record.flags & yas_wire::schema::terminal::JOURNAL_INCOMPLETE as u16 != 0 {
        "incomplete"
    } else if record.flags & yas_wire::schema::terminal::JOURNAL_HAS_EXIT as u16 != 0 {
        "exited"
    } else {
        "done"
    }
}

fn journal_duration(record: &terminal::JournalRecord) -> Option<u64> {
    (record.started_unix_ms != 0
        && record.ended_unix_ms != 0
        && record.ended_unix_ms >= record.started_unix_ms)
        .then(|| record.ended_unix_ms - record.started_unix_ms)
}

fn journal_exit(record: &terminal::JournalRecord) -> Option<i32> {
    (record.flags & yas_wire::schema::terminal::JOURNAL_HAS_EXIT as u16 != 0)
        .then_some(record.exit_code)
}

fn journal_record_json(record: &terminal::JournalRecord) -> serde_json::Value {
    serde_json::json!({
        "index": record.index,
        "command": record.command,
        "status": journal_status(record),
        "exit": journal_exit(record),
        "running": journal_running(record),
        "start_seq": record.start_seq,
        "end_seq": record.end_seq,
        "cursor": format_cursor(record.start_seq, 0),
        "started_ms": record.started_unix_ms,
        "ended_ms": record.ended_unix_ms,
        "duration_ms": journal_duration(record),
        "command_known": record.flags & yas_wire::schema::terminal::JOURNAL_NO_COMMAND as u16 == 0,
        "incomplete": record.flags & yas_wire::schema::terminal::JOURNAL_INCOMPLETE as u16 != 0,
        "evicted": record.flags & yas_wire::schema::terminal::JOURNAL_EVICTED as u16 != 0,
        "pty_exited": record.flags & yas_wire::schema::terminal::JOURNAL_PTY_EXITED as u16 != 0,
    })
}

fn print_journal_record(record: &terminal::JournalRecord) {
    let exit = journal_exit(record).map(|value| value.to_string());
    let duration = journal_duration(record).map(|value| value.to_string());
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        record.index,
        journal_status(record),
        exit.as_deref().unwrap_or("-"),
        duration.as_deref().unwrap_or("-"),
        record.start_seq,
        record.end_seq,
        record.command.replace(['\t', '\n'], " ")
    );
}

fn journal_exit_code(record: &terminal::JournalRecord) -> i32 {
    match journal_exit(record) {
        Some(code) if code >= 0 => code.min(255),
        Some(code) => 128 + (-code).min(127),
        None if journal_running(record) => 124,
        None => 0,
    }
}

fn format_cursor(sequence: u64, column: u32) -> String {
    format!("{sequence}:{column}")
}

fn parse_cursor(value: &str) -> Result<Option<(u64, u32)>, String> {
    if matches!(value, "now" | "end") {
        return Ok(None);
    }
    if matches!(value, "start" | "oldest") {
        return Ok(Some((0, 0)));
    }
    let (sequence, column) = value
        .split_once(':')
        .map_or((value, "0"), |(sequence, column)| (sequence, column));
    let sequence = sequence
        .parse::<u64>()
        .map_err(|_| format!("not a cursor: {value} (want SEQ, SEQ:COL, now, or start)"))?;
    let column = column
        .parse::<u32>()
        .map_err(|_| format!("not a column: {column}"))?;
    Ok(Some((sequence, column)))
}

async fn cmd_since(
    on: Option<&str>,
    hub: &str,
    id: u64,
    cursor: &str,
    max_bytes: u32,
    json: bool,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_terminal(&mut client, id).await?;
    let parsed = parse_cursor(cursor)?;
    let (cursor_kind, sequence, column, request_max) = match parsed {
        None => (
            yas_wire::schema::terminal::OUTPUT_CURSOR_PROBE as u8,
            0,
            0,
            1,
        ),
        Some((sequence, column)) => (
            yas_wire::schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8,
            sequence,
            column,
            max_bytes,
        ),
    };
    let query = terminal_query(
        &mut client,
        terminal::request_kind::OUTPUT,
        &terminal::Output {
            terminal_handle: record.terminal_handle,
            generation: record.generation,
            cursor_kind,
            flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
            cursor_a: sequence,
            cursor_b: column,
            max_bytes: request_max.max(1),
            initial_receive_credit: QUERY_CREDIT,
            extensions: Extensions::default(),
        },
        u64::from(request_max.max(1)).min(MAX_QUERY_BYTES),
        Duration::from_secs(10),
    )
    .await?;
    expect_content(&query, yas_wire::schema::terminal::CONTENT_OUTPUT as u8)?;
    let output = terminal::OutputResult::decode(&query_bytes(query).await?).map_err(wire_error)?;
    let next = format_cursor(output.next_seq, output.next_col);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "pty": id,
                "text": String::from_utf8_lossy(&output.text),
                "cursor": format_cursor(output.start_seq, output.start_col),
                "next_cursor": next,
                "truncated": output.flags & yas_wire::schema::terminal::OUTPUT_TRUNCATED as u16 != 0,
                "evicted": output.flags & yas_wire::schema::terminal::OUTPUT_EVICTED as u16 != 0,
                "alt_screen": output.flags & yas_wire::schema::terminal::OUTPUT_ALT_SCREEN as u16 != 0,
            })
        );
    } else {
        std::io::Write::write_all(&mut std::io::stdout(), &output.text)
            .map_err(|error| format!("cannot write terminal history: {error}"))?;
        if !output.text.is_empty() && !output.text.ends_with(b"\n") {
            println!();
        }
        eprintln!("cursor: {next}");
    }
    Ok(())
}

async fn probe_output_cursor(
    client: &mut NativeClient,
    record: &terminal::TerminalRecord,
) -> Result<(u64, u32), String> {
    let query = terminal_query(
        client,
        terminal::request_kind::OUTPUT,
        &terminal::Output {
            terminal_handle: record.terminal_handle,
            generation: record.generation,
            cursor_kind: yas_wire::schema::terminal::OUTPUT_CURSOR_PROBE as u8,
            flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
            cursor_a: 0,
            cursor_b: 0,
            max_bytes: 1,
            initial_receive_credit: QUERY_CREDIT,
            extensions: Extensions::default(),
        },
        1,
        Duration::from_secs(10),
    )
    .await?;
    expect_content(&query, yas_wire::schema::terminal::CONTENT_OUTPUT as u8)?;
    let output = terminal::OutputResult::decode(&query.bytes).map_err(wire_error)?;
    Ok((output.next_seq, output.next_col))
}

async fn wait_for_pattern(
    client: &mut NativeClient,
    record: &terminal::TerminalRecord,
    id: u64,
    regex: &regex::Regex,
    deadline: tokio::time::Instant,
) -> Result<i32, String> {
    let (mut sequence, mut column) = probe_output_cursor(client, record).await?;
    let mut pending = String::new();
    loop {
        if tokio::time::Instant::now() >= deadline {
            eprintln!("yas: timed out waiting for pty {id}");
            return Ok(124);
        }
        let query = terminal_query(
            client,
            terminal::request_kind::OUTPUT,
            &terminal::Output {
                terminal_handle: record.terminal_handle,
                generation: record.generation,
                cursor_kind: yas_wire::schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8,
                flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
                cursor_a: sequence,
                cursor_b: column,
                max_bytes: terminal_args::OUTPUT_MAX_BYTES,
                initial_receive_credit: QUERY_CREDIT,
                extensions: Extensions::default(),
            },
            u64::from(terminal_args::OUTPUT_MAX_BYTES),
            Duration::from_secs(10),
        )
        .await?;
        expect_content(&query, yas_wire::schema::terminal::CONTENT_OUTPUT as u8)?;
        let output = terminal::OutputResult::decode(&query.bytes).map_err(wire_error)?;
        (sequence, column) = (output.next_seq, output.next_col);
        pending.push_str(&String::from_utf8_lossy(&output.text));
        if let Some(line) = first_matching_line(&mut pending, regex) {
            println!("{line}");
            return Ok(0);
        }
        if output.flags & yas_wire::schema::terminal::OUTPUT_TRUNCATED as u16 != 0 {
            continue;
        }
        let current = find_terminal(client, id).await?;
        if current.lifecycle == terminal::Lifecycle::Exited {
            return print_terminal_exit(&current);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn first_matching_line(pending: &mut String, regex: &regex::Regex) -> Option<String> {
    const MAX_PENDING: usize = 64 * 1024;
    let complete = pending.rfind('\n').map_or(0, |index| index + 1);
    for line in pending[..complete].lines() {
        if regex.is_match(line) {
            return Some(line.to_string());
        }
    }
    let tail = pending[complete..].to_string();
    if !tail.is_empty() && regex.is_match(&tail) {
        return Some(tail);
    }
    *pending = tail;
    if pending.len() > MAX_PENDING {
        let mut start = pending.len() - MAX_PENDING;
        while !pending.is_char_boundary(start) {
            start += 1;
        }
        pending.drain(..start);
    }
    None
}

fn print_terminal_exit(record: &terminal::TerminalRecord) -> Result<i32, String> {
    println!("{}", terminal_status(record)?);
    terminal_exit_code(record)
}

fn terminal_exit_code(record: &terminal::TerminalRecord) -> Result<i32, String> {
    let Some(value) = bytes_extension(
        &record.extensions,
        yas_wire::schema::terminal::STATE_EXIT_EXTENSION,
    ) else {
        return Ok(1);
    };
    let exit = terminal::ExitRecord::decode(value).map_err(wire_error)?;
    Ok(match exit {
        terminal::ExitRecord::Code { code, .. } if code >= 0 => code.min(255),
        terminal::ExitRecord::Code { code, .. } => 128 + (-code).min(127),
        terminal::ExitRecord::Signal { native_signal, .. } => 128 + native_signal.clamp(1, 127),
        terminal::ExitRecord::Other { .. } => 1,
    })
}

pub(crate) async fn open_view(
    client: &mut NativeClient,
    record: &terminal::TerminalRecord,
    rows: u16,
    cols: u16,
    max_fps: u16,
) -> Result<terminal::OpenViewResult, String> {
    client
        .request_typed(
            family::TERMINAL,
            terminal::request_kind::OPEN_VIEW,
            &terminal::OpenView {
                terminal_handle: record.terminal_handle,
                rows,
                cols,
                max_fps,
                codec_versions: vec![1],
                extensions: Extensions::default(),
            },
            false,
        )
        .await
}

pub(crate) async fn close_view(client: &mut NativeClient, view_id: u32) -> Result<(), String> {
    let body = client
        .request(
            family::TERMINAL,
            terminal::request_kind::CLOSE_VIEW,
            terminal::CloseView { view_id }
                .encode()
                .map_err(wire_error)?,
            false,
        )
        .await?;
    if body.is_empty() {
        Ok(())
    } else {
        Err("YAS Terminal CLOSE_VIEW returned an unexpected body".into())
    }
}

pub(crate) async fn send_mouse_actions(
    client: &mut NativeClient,
    feedback: terminal::ViewFeedback,
    event: &str,
    col: u16,
    row: u16,
    button: &str,
) -> Result<(), String> {
    let button = match button {
        "left" => yas_wire::schema::terminal::MOUSE_BUTTON_LEFT as u8,
        "middle" => yas_wire::schema::terminal::MOUSE_BUTTON_MIDDLE as u8,
        "right" => yas_wire::schema::terminal::MOUSE_BUTTON_RIGHT as u8,
        "back" => yas_wire::schema::terminal::MOUSE_BUTTON_BACK as u8,
        "forward" => yas_wire::schema::terminal::MOUSE_BUTTON_FORWARD as u8,
        other => {
            return Err(format!(
                "unknown button '{other}': expected left, right, middle, back, or forward"
            ));
        }
    };
    if matches!(
        event,
        "wheel-up"
            | "wheelup"
            | "scroll-up"
            | "scrollup"
            | "wheel-down"
            | "wheeldown"
            | "scroll-down"
            | "scrolldown"
    ) {
        let up = matches!(event, "wheel-up" | "wheelup" | "scroll-up" | "scrollup");
        let client_monotonic_ns = client.monotonic_ns();
        let source = yas_wire::schema::terminal::WHEEL_SOURCE_WHEEL as u8;
        let dy_32_32 = if up { -(1i64 << 32) } else { 1i64 << 32 };
        // WHEEL carries no cell, so its reports land on the origin — the
        // wrong pane in anything that splits its window. Send the cell to a
        // server that takes it, and the older event to one that does not.
        if client.supports(
            family::TERMINAL,
            Class::Event,
            terminal::event_kind::WHEEL_AT,
        ) {
            return client
                .send_typed_event(
                    family::TERMINAL,
                    terminal::event_kind::WHEEL_AT,
                    &terminal::WheelAt {
                        feedback,
                        client_monotonic_ns,
                        source,
                        dx_32_32: 0,
                        dy_32_32,
                        column: i32::from(col),
                        row: i32::from(row),
                    },
                    true,
                )
                .await;
        }
        return client
            .send_typed_event(
                family::TERMINAL,
                terminal::event_kind::WHEEL,
                &terminal::Wheel {
                    feedback,
                    client_monotonic_ns,
                    source,
                    dx_32_32: 0,
                    dy_32_32,
                },
                true,
            )
            .await;
    }
    let actions: &[(u8, u8)] = match event {
        "down" | "press" => &[(yas_wire::schema::terminal::MOUSE_ACTION_DOWN as u8, button)],
        "up" | "release" => &[(yas_wire::schema::terminal::MOUSE_ACTION_UP as u8, button)],
        "move" | "drag" => &[(yas_wire::schema::terminal::MOUSE_ACTION_MOVE as u8, button)],
        "hover" => &[(
            yas_wire::schema::terminal::MOUSE_ACTION_MOVE as u8,
            yas_wire::schema::terminal::MOUSE_BUTTON_NONE as u8,
        )],
        "click" => &[
            (yas_wire::schema::terminal::MOUSE_ACTION_DOWN as u8, button),
            (yas_wire::schema::terminal::MOUSE_ACTION_UP as u8, button),
        ],
        other => {
            return Err(format!(
                "unknown mouse event '{other}': expected down, up, move, hover, click, wheel-up, or wheel-down"
            ));
        }
    };
    for &(action, button) in actions {
        client
            .send_typed_event(
                family::TERMINAL,
                terminal::event_kind::MOUSE,
                &terminal::Mouse {
                    feedback,
                    client_monotonic_ns: client.monotonic_ns(),
                    action,
                    button,
                    modifiers: 0,
                    column: i32::from(col),
                    row: i32::from(row),
                },
                true,
            )
            .await?;
    }
    Ok(())
}

async fn request_empty<Request: yas_wire::Encode>(
    on: Option<&str>,
    hub: &str,
    kind: u16,
    request: &Request,
    sensitive: bool,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let body = client
        .request(
            family::TERMINAL,
            kind,
            request
                .encode()
                .map_err(|error| format!("YAS wire error: {error}"))?,
            sensitive,
        )
        .await?;
    if body.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "YAS Terminal Result {kind:#06x} contained an unexpected operation body"
        ))
    }
}

fn launch_from_request(request: &terminal_args::StartRequest) -> Result<terminal::Launch, String> {
    if request.shell && request.command.is_empty() {
        return Err("--shell needs a command to run".to_string());
    }
    let command = if request.shell {
        terminal::Command::ShellCommand(request.command.join(" "))
    } else if request.command.is_empty() {
        terminal::Command::DefaultShell
    } else {
        terminal::Command::Argv(
            request
                .command
                .iter()
                .map(|argument| argument.as_bytes().to_vec())
                .collect(),
        )
    };
    let cwd = request
        .cwd
        .as_ref()
        .map(|path| terminal::Cwd::Path(path.as_bytes().to_vec()))
        .unwrap_or(terminal::Cwd::ServerDefault);
    let mut environment = request
        .env
        .iter()
        .map(|entry| {
            let (key, value) = terminal_args::parse_env_assignment(entry)?;
            Ok(terminal::EnvironmentEntry {
                key: key.as_bytes().to_vec(),
                value: terminal::EnvironmentValue::Set(value.as_bytes().to_vec()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    environment.sort_by(|left, right| left.key.cmp(&right.key));
    if environment
        .windows(2)
        .any(|pair| pair[0].key == pair[1].key)
    {
        return Err("--env contains the same variable more than once".to_string());
    }
    let mut extensions = Vec::new();
    if let Some(seconds) = request.deadline {
        let duration = seconds.saturating_mul(NS_PER_SECOND);
        if duration == 0 {
            return Err("--deadline must be greater than zero".to_string());
        }
        extensions.push(Extension {
            tag: yas_wire::schema::terminal::LAUNCH_DEADLINE_AFTER_NS_EXTENSION as u16,
            required: false,
            value: duration.to_le_bytes().to_vec(),
        });
    }
    Ok(terminal::Launch {
        command,
        cwd,
        environment_base: terminal::EnvironmentBase::Server,
        environment,
        extensions: Extensions(extensions),
    })
}

fn terminal_handle(id: u64) -> Result<u64, String> {
    if id == 0 {
        Err("Terminal handle must be nonzero".into())
    } else {
        Ok(id)
    }
}

fn terminal_id(handle: u64) -> Result<u64, String> {
    terminal_handle(handle)
}

fn operation_id() -> [u8; 16] {
    let mut value: [u8; 16] = rand::random();
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

fn bytes_extension(extensions: &Extensions, tag: u64) -> Option<&[u8]> {
    extensions
        .0
        .iter()
        .find(|extension| u64::from(extension.tag) == tag)
        .map(|extension| extension.value.as_slice())
}

fn text_extension(extensions: &Extensions, tag: u64) -> Option<String> {
    std::str::from_utf8(bytes_extension(extensions, tag)?)
        .ok()
        .map(str::to_owned)
}

fn terminal_status(record: &terminal::TerminalRecord) -> Result<String, String> {
    if record.lifecycle == terminal::Lifecycle::Running {
        return Ok("running".to_string());
    }
    let Some(value) = bytes_extension(
        &record.extensions,
        yas_wire::schema::terminal::STATE_EXIT_EXTENSION,
    ) else {
        return Ok("exited".to_string());
    };
    let exit = terminal::ExitRecord::decode(value)
        .map_err(|error| format!("invalid Terminal exit state: {error}"))?;
    Ok(match exit {
        terminal::ExitRecord::Code { code, detail } if detail.is_empty() => {
            format!("exited({code})")
        }
        terminal::ExitRecord::Code { code, detail } => format!("exited({code}): {detail}"),
        terminal::ExitRecord::Signal {
            reason,
            native_signal,
            detail,
        } if detail.is_empty() => format!("signal({native_signal}, {reason:?})"),
        terminal::ExitRecord::Signal {
            reason,
            native_signal,
            detail,
        } => format!("signal({native_signal}, {reason:?}): {detail}"),
        terminal::ExitRecord::Other { detail } => format!("exited: {detail}"),
    })
}

fn wire_error(error: yas_wire::Error) -> String {
    format!("YAS wire error: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_handles_are_opaque_nonzero_u64_values() {
        assert_eq!(terminal_handle(1).unwrap(), 1);
        assert_eq!(terminal_handle(u64::MAX).unwrap(), u64::MAX);
        assert_eq!(terminal_id(1).unwrap(), 1);
        assert_eq!(terminal_id(u64::MAX).unwrap(), u64::MAX);
        assert!(terminal_handle(0).is_err());
        assert!(terminal_id(0).is_err());
    }

    #[test]
    fn native_launch_keeps_argv_and_sorts_environment() {
        let launch = launch_from_request(&terminal_args::StartRequest {
            tag: Some("build".into()),
            command: vec!["printf".into(), "%s".into(), "hello world".into()],
            shell: false,
            cwd: Some("/src".into()),
            env: vec!["Z=last".into(), "A=first".into()],
            rows: 24,
            cols: 80,
            deadline: Some(3),
        })
        .unwrap();
        assert!(matches!(launch.command, terminal::Command::Argv(_)));
        assert_eq!(launch.environment[0].key, b"A");
        assert_eq!(launch.environment[1].key, b"Z");
        assert_eq!(launch.extensions.0[0].value, 3_000_000_000u64.to_le_bytes());
    }
}
