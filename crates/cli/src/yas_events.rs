//! Native YAS Events command surface.

use std::path::Path;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use yas_wire::{Decode, Encode, Extensions, events, family};

use crate::cli::{EventsCommand, EventsRecordCommand};
use crate::yas_native::NativeClient;

const DUMP_RECEIVE_LIMIT: u64 = yas_wire::schema::events::MAX_RING_BYTES + 4096;

pub(crate) const EVENT_NAMES: &[&str] = &[
    "server.start",
    "server.stop",
    "task.start",
    "task.stop",
    "client.connect",
    "client.disconnect",
    "client.reject",
    "config.change",
    "stream.start",
    "stream.stop",
    "protocol.error",
    "pty.create",
    "pty.exit",
    "pty.remove",
    "pty.deadline",
    "server.capacity",
    "frame.read",
    "frame.write",
    "message.read",
    "message.write",
    "tick.start",
    "tick.stop",
    "tick.nudge",
    "session.lock",
    "pty.read",
    "pty.write",
    "pty.parse",
    "pty.snapshot",
    "pty.resize",
    "pty.input",
    "compositor.event",
    "compositor.command",
    "surface.encode",
    "surface.frame",
    "audio.frame",
    "fs.request",
    "git.request",
    "lsp.request",
    "kv.request",
    "net.request",
    "process.request",
    "extension.request",
    "channel.request",
    "client.control",
    "outbox.queue",
    "supervisor.event",
    "connection.accept",
    "server.error",
];

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: EventsCommand,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    match command {
        EventsCommand::Config => print_config(&get_config(&mut client).await?),
        EventsCommand::Set {
            size,
            events: event_spec,
            if_revision,
        } => {
            let current = get_config(&mut client).await?;
            let request = events::SetConfig {
                operation_id: operation_id(),
                expected_revision: if_revision.unwrap_or(0),
                capacity: size.unwrap_or(current.capacity),
                activations: event_spec
                    .as_deref()
                    .map(parse_activation_spec)
                    .transpose()?
                    .unwrap_or(current.activations),
                extensions: Extensions::default(),
            };
            let config: events::Config = client
                .request_typed(
                    family::EVENTS,
                    events::request_kind::SET_CONFIG,
                    &request,
                    true,
                )
                .await?;
            print_config(&config);
        }
        EventsCommand::Dump { output, binary } => {
            dump(&mut client, output.as_deref().unwrap_or("-"), binary).await?
        }
        EventsCommand::Tail {
            output,
            append,
            from_now,
            binary,
        } => {
            tail(
                &mut client,
                output.as_deref().unwrap_or("-"),
                append,
                from_now,
                binary,
            )
            .await?
        }
        EventsCommand::Record { command } => recording(&mut client, command).await?,
    }
    Ok(())
}

async fn get_config(client: &mut NativeClient) -> Result<events::Config, String> {
    client
        .request_typed(
            family::EVENTS,
            events::request_kind::GET_CONFIG,
            &events::GetConfig {
                extensions: Extensions::default(),
            },
            true,
        )
        .await
}

fn print_config(config: &events::Config) {
    println!("protocol\tyas.events.v1");
    println!("revision\t{}", config.revision);
    println!("size\t{}", config.capacity);
    println!("used\t{}", config.used);
    println!("records\t{}", config.record_count);
    println!("dropped\t{}", config.dropped);
    println!("next_sequence\t{}", config.next_sequence);
    println!(
        "events\t{}",
        EVENT_NAMES
            .iter()
            .enumerate()
            .filter_map(|(id, name)| { config.activations.enabled(id as u16).then_some(*name) })
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "bitset\t{}",
        config
            .activations
            .0
            .iter()
            .map(|word| format!("{word:016x}"))
            .collect::<Vec<_>>()
            .join(":")
    );
}

async fn dump(client: &mut NativeClient, path: &str, binary: bool) -> Result<(), String> {
    let result: events::DumpResult = client
        .request_typed(
            family::EVENTS,
            events::request_kind::DUMP,
            &events::Dump {
                initial_receive_credit: DUMP_RECEIVE_LIMIT,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    if result.byte_len > DUMP_RECEIVE_LIMIT {
        return Err(format!(
            "Events dump is {} bytes; collection limit is {DUMP_RECEIVE_LIMIT}",
            result.byte_len
        ));
    }
    let bytes = client
        .receive_byte_transfer(
            &result.descriptor,
            Some(result.byte_len),
            DUMP_RECEIVE_LIMIT,
        )
        .await?;
    if blake3::hash(&bytes).as_bytes() != &result.content_hash {
        return Err("Events dump content hash mismatch".into());
    }
    let mut writer = open_output(path, false).await?;
    if binary {
        writer
            .write_all(&bytes)
            .await
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    } else {
        let rendered = crate::events_human::render_dump(&bytes)?;
        writer
            .write_all(rendered.as_bytes())
            .await
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    writer
        .flush()
        .await
        .map_err(|error| format!("cannot flush {path}: {error}"))
}

async fn tail(
    client: &mut NativeClient,
    path: &str,
    append: bool,
    from_now: bool,
    binary: bool,
) -> Result<(), String> {
    let started: events::StreamStarted = client
        .request_typed(
            family::EVENTS,
            events::request_kind::START_STREAM,
            &events::StartStream {
                operation_id: operation_id(),
                history: !from_now,
                start_sequence: 0,
                max_batch_bytes: 0,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let mut writer = open_output(path, append).await?;
    loop {
        tokio::select! {
            frame = client.next_event() => {
                let frame = frame?;
                if frame.header.family != family::EVENTS {
                    continue;
                }
                if !frame.header.sensitive {
                    return Err("Events server Event was not marked sensitive".into());
                }
                match frame.header.kind {
                    events::event_kind::RECORD => {
                        let event = events::RecordEvent::decode(&frame.payload)
                            .map_err(|error| format!("invalid Events RECORD: {error}"))?;
                        if event.stream_handle != started.stream_handle {
                            continue;
                        }
                        if binary {
                            writer.write_all(&event.batch.encode().map_err(|error| {
                                format!("cannot encode Events packed batch: {error}")
                            })?).await.map_err(|error| format!("cannot write {path}: {error}"))?;
                        } else {
                            let rendered = crate::events_human::render_native_batch(&event.batch);
                            writer.write_all(rendered.as_bytes()).await
                                .map_err(|error| format!("cannot write {path}: {error}"))?;
                        }
                    }
                    events::event_kind::GAP => {
                        let event = events::Gap::decode(&frame.payload)
                            .map_err(|error| format!("invalid Events GAP: {error}"))?;
                        if event.stream_handle != started.stream_handle {
                            continue;
                        }
                        if !binary {
                            writer.write_all(crate::events_human::render_gap(event.lost).as_bytes())
                                .await.map_err(|error| format!("cannot write {path}: {error}"))?;
                        }
                        eprintln!(
                            "yas: event stream lost {} records; first available sequence is {}",
                            event.lost, event.first_available_sequence
                        );
                    }
                    events::event_kind::STREAM_STOPPED => {
                        let event = events::StreamStopped::decode(&frame.payload)
                            .map_err(|error| format!("invalid Events STREAM_STOPPED: {error}"))?;
                        if event.stream_handle != started.stream_handle {
                            continue;
                        }
                        if !event.status.is_ok() {
                            return Err(if event.detail.is_empty() {
                                format!("event stream stopped with {:?}", event.status)
                            } else {
                                format!("event stream stopped with {:?}: {}", event.status, event.detail)
                            });
                        }
                        break;
                    }
                    _ => {}
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| format!("cannot listen for Ctrl-C: {error}"))?;
                stop_stream(client, started.stream_handle).await?;
                break;
            }
        }
    }
    writer
        .flush()
        .await
        .map_err(|error| format!("cannot flush {path}: {error}"))
}

async fn stop_stream(client: &mut NativeClient, stream_handle: u64) -> Result<(), String> {
    let request = events::StopStream {
        stream_handle,
        operation_id: operation_id(),
        extensions: Extensions::default(),
    };
    let body = client
        .request(
            family::EVENTS,
            events::request_kind::STOP_STREAM,
            request
                .encode()
                .map_err(|error| format!("cannot encode Events STOP_STREAM: {error}"))?,
            true,
        )
        .await?;
    if !body.is_empty() {
        return Err("Events STOP_STREAM returned an unexpected response body".into());
    }
    Ok(())
}

async fn recording(client: &mut NativeClient, command: EventsRecordCommand) -> Result<(), String> {
    match command {
        EventsRecordCommand::Start {
            path,
            append,
            from_now,
        } => {
            let info: events::RecordingInfo = client
                .request_typed(
                    family::EVENTS,
                    events::request_kind::START_RECORDING,
                    &events::StartRecording {
                        operation_id: operation_id(),
                        history: !from_now,
                        append,
                        path: path.into_bytes(),
                        extensions: Extensions::default(),
                    },
                    true,
                )
                .await?;
            println!("{}", info.recording_handle);
        }
        EventsRecordCommand::List => {
            let list: events::RecordingList = client
                .request_typed(
                    family::EVENTS,
                    events::request_kind::LIST_RECORDINGS,
                    &events::ListRecordings {
                        extensions: Extensions::default(),
                    },
                    true,
                )
                .await?;
            for info in list.recordings {
                print_recording(&info);
            }
        }
        EventsRecordCommand::Stop { id } => {
            let _: events::RecordingInfo = client
                .request_typed(
                    family::EVENTS,
                    events::request_kind::STOP_RECORDING,
                    &events::StopRecording {
                        recording_handle: id,
                        operation_id: operation_id(),
                        extensions: Extensions::default(),
                    },
                    true,
                )
                .await?;
        }
    }
    Ok(())
}

fn print_recording(info: &events::RecordingInfo) {
    let state = match info.state {
        events::RecordingState::Running => "running",
        events::RecordingState::Stopped => "stopped",
        events::RecordingState::Failed => "failed",
    };
    let history = if info.history { "history" } else { "from-now" };
    let mode = if info.append { "append" } else { "truncate" };
    println!(
        "{}\t{state}\t{}\t{}\t{}\t{history}\t{mode}\t{}\t{}",
        info.recording_handle,
        info.records,
        info.bytes,
        info.lost,
        escape_bytes(&info.path),
        escape_text(&info.error),
    );
}

async fn open_output(
    path: &str,
    append: bool,
) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
    if path == "-" {
        return Ok(Box::new(tokio::io::stdout()));
    }
    let mut options = tokio::fs::OpenOptions::new();
    options
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append);
    Ok(Box::new(
        options
            .open(Path::new(path))
            .await
            .map_err(|error| format!("cannot open {path}: {error}"))?,
    ))
}

fn parse_activation_spec(spec: &str) -> Result<events::ActivationSet, String> {
    let first = spec.split(',').map(str::trim).find(|part| !part.is_empty());
    let mut set = if first.is_some_and(|part| part.starts_with(['+', '-'])) {
        events::ActivationSet::low_throughput()
    } else {
        events::ActivationSet::default()
    };
    for raw in spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (enabled, selector) = match raw.as_bytes().first() {
            Some(b'+') => (true, &raw[1..]),
            Some(b'-') => (false, &raw[1..]),
            _ => (true, raw),
        };
        match selector {
            "all" => {
                set = if enabled {
                    events::ActivationSet::all()
                } else {
                    events::ActivationSet::default()
                };
            }
            "none" if enabled => set = events::ActivationSet::default(),
            "none" => {}
            "default" if enabled => set = events::ActivationSet::low_throughput(),
            "default" => {
                for id in 0..16 {
                    set.set(id, false);
                }
            }
            _ => {
                let mut matched = false;
                if let Some(prefix) = selector.strip_suffix(".*") {
                    for (id, name) in EVENT_NAMES.iter().enumerate() {
                        if name
                            .strip_prefix(prefix)
                            .is_some_and(|tail| tail.starts_with('.'))
                        {
                            set.set(id as u16, enabled);
                            matched = true;
                        }
                    }
                } else if let Some(id) = EVENT_NAMES.iter().position(|name| *name == selector) {
                    set.set(id as u16, enabled);
                    matched = true;
                }
                if !matched {
                    return Err(format!("unknown event selector {selector:?}"));
                }
            }
        }
    }
    Ok(set)
}

fn escape_bytes(value: &[u8]) -> String {
    escape_text(&String::from_utf8_lossy(value))
}

fn escape_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            '\n' => output.push_str("\\n"),
            '\\' => output.push_str("\\\\"),
            other => output.push(other),
        }
    }
    output
}

fn operation_id() -> [u8; 16] {
    let mut value: [u8; 16] = rand::random();
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_selectors_are_native_and_exact() {
        let selected = parse_activation_spec("none,+pty.*,-pty.input").unwrap();
        assert!(selected.enabled(11));
        assert!(selected.enabled(28));
        assert!(!selected.enabled(29));
        assert!(!selected.enabled(0));
        assert!(parse_activation_spec("pty").is_err());
    }

    #[test]
    fn record_fields_are_tsv_safe() {
        assert_eq!(escape_bytes(b"a\tb\n"), "a\\tb\\n");
    }
}
