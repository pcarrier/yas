//! Native YAS Surface implementations for the compositor CLI.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufWriter, Write},
    time::Duration,
};

use yas_wire::{Encode, Extensions, family, surface};

use crate::yas_native::{MAX_COLLECTED_TRANSFER_BYTES, NativeClient};

const CAPTURE_CREDIT: u64 = 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = MAX_COLLECTED_TRANSFER_BYTES;
const MAX_SURFACE_FRAME_BYTES: u32 = 64 * 1024 * 1024;
const WHEEL_DETENT_PIXELS: f64 = 120.0;

pub(crate) async fn cmd_list(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let mut records = surface_records(&mut client).await?;
    records.sort_by_key(|record| record.surface_handle);

    println!("ID\tTITLE\tSIZE\tAPP_ID");
    for record in records {
        let (width, height) = physical_dimensions(&record)?;
        println!(
            "{}\t{}\t{}x{}\t{}",
            surface_handle(record.surface_handle)?,
            record.title,
            width,
            height,
            record.application_id
        );
    }
    Ok(())
}

pub(crate) async fn cmd_close(on: Option<&str>, hub: &str, id: u64) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    request_empty(
        &mut client,
        surface::request_kind::CLOSE,
        &surface::Close {
            surface_handle: surface_handle(id)?,
            operation_id: operation_id(),
            extensions: Extensions::default(),
        },
        false,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_capture(
    on: Option<&str>,
    hub: &str,
    id: u64,
    output: Option<String>,
    format_arg: Option<String>,
    quality: u8,
    width: Option<u16>,
    height: Option<u16>,
    scale: u16,
) -> Result<(), String> {
    let id = surface_handle(id)?;
    if quality != 0 {
        return Err("YAS Surface v1 CAPTURE does not expose encoder quality".to_string());
    }
    if scale != 0 {
        return Err("YAS Surface v1 CAPTURE does not expose an output scale".to_string());
    }
    let (format, extension) = capture_format(format_arg.as_deref(), output.as_deref())?;
    let mut client = NativeClient::connect(on, hub).await?;
    let mut record = find_surface(&mut client, id).await?;

    if width.is_some() || height.is_some() {
        let width = width.unwrap_or(640);
        let height = height.unwrap_or(480);
        let _: surface::RevisionResult = client
            .request_typed(
                family::SURFACE,
                surface::request_kind::RESIZE,
                &surface::Resize {
                    surface_handle: id,
                    operation_id: operation_id(),
                    logical_width_32_32: i64::from(width) << 32,
                    logical_height_32_32: i64::from(height) << 32,
                    extensions: Extensions::default(),
                },
                false,
            )
            .await?;
        // RESIZE publishes state synchronously, but the Wayland client still
        // needs a brief opportunity to commit the newly sized buffer.
        tokio::time::sleep(Duration::from_millis(200)).await;
        record = find_surface(&mut client, id).await?;
    }

    let result: surface::CaptureResult = client
        .request_typed(
            family::SURFACE,
            surface::request_kind::CAPTURE,
            &surface::Capture {
                surface_handle: id,
                revision: record.revision,
                initial_receive_credit: CAPTURE_CREDIT,
                formats: vec![format],
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let bytes = client
        .receive_inline_or_transfer(result, MAX_CAPTURE_BYTES)
        .await?;
    let path = output.unwrap_or_else(|| format!("surface-{id}.{extension}"));
    std::fs::write(&path, bytes).map_err(|error| format!("write {path}: {error}"))?;
    println!("{path}");
    Ok(())
}

pub(crate) async fn cmd_click(
    on: Option<&str>,
    hub: &str,
    id: u64,
    x: u16,
    y: u16,
    button: &str,
) -> Result<(), String> {
    let button = pointer_button(button)?;
    with_input_view(on, hub, id, |client, view| {
        Box::pin(async move {
            for phase in [
                yas_wire::schema::surface::POINTER_PHASE_DOWN as u8,
                yas_wire::schema::surface::POINTER_PHASE_UP as u8,
            ] {
                let event = surface::Pointer {
                    view_id: view.result.view_id,
                    feedback: view.feedback(),
                    client_monotonic_ns: client.monotonic_ns(),
                    phase,
                    button,
                    x_32_32: i64::from(x) << 32,
                    y_32_32: i64::from(y) << 32,
                };
                client
                    .send_typed_event(family::SURFACE, surface::event_kind::POINTER, &event, true)
                    .await?;
            }
            Ok(())
        })
    })
    .await
}

pub(crate) async fn cmd_scroll(
    on: Option<&str>,
    hub: &str,
    id: u64,
    amount: f64,
    horizontal: bool,
    smooth: bool,
) -> Result<(), String> {
    if !amount.is_finite() {
        return Err("scroll amount must be a finite number".to_string());
    }
    let distance = fixed_32_32(amount * WHEEL_DETENT_PIXELS, "scroll distance")?;
    let steps = if smooth {
        0
    } else {
        rounded_i32(amount, "scroll detents")?
    };
    with_input_view(on, hub, id, |client, view| {
        Box::pin(async move {
            client
                .send_typed_event(
                    family::SURFACE,
                    surface::event_kind::AXIS,
                    &surface::Axis {
                        view_id: view.result.view_id,
                        feedback: view.feedback(),
                        client_monotonic_ns: client.monotonic_ns(),
                        source: if smooth {
                            yas_wire::schema::surface::AXIS_SOURCE_FINGER as u8
                        } else {
                            yas_wire::schema::surface::AXIS_SOURCE_WHEEL as u8
                        },
                        flags: 0,
                        dx_32_32: if horizontal { distance } else { 0 },
                        dy_32_32: if horizontal { 0 } else { distance },
                        steps_x: if horizontal { steps } else { 0 },
                        steps_y: if horizontal { 0 } else { steps },
                    },
                    true,
                )
                .await
        })
    })
    .await
}

pub(crate) async fn cmd_focus(on: Option<&str>, hub: &str, id: u64) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let _: surface::RevisionResult = client
        .request_typed(
            family::SURFACE,
            surface::request_kind::FOCUS,
            &surface::Focus {
                surface_handle: surface_handle(id)?,
                operation_id: operation_id(),
                focused: true,
                extensions: Extensions::default(),
            },
            false,
        )
        .await?;
    Ok(())
}

pub(crate) async fn cmd_text(
    on: Option<&str>,
    hub: &str,
    id: u64,
    text: &str,
) -> Result<(), String> {
    let text = text.to_owned();
    with_input_view(on, hub, id, |client, view| {
        Box::pin(async move {
            client
                .send_typed_event(
                    family::SURFACE,
                    surface::event_kind::TEXT,
                    &surface::Text {
                        view_id: view.result.view_id,
                        feedback: view.feedback(),
                        client_monotonic_ns: client.monotonic_ns(),
                        text,
                    },
                    true,
                )
                .await
        })
    })
    .await
}

pub(crate) async fn cmd_key(on: Option<&str>, hub: &str, id: u64, key: &str) -> Result<(), String> {
    let events = parse_key_combo(key)?;
    send_key_events(on, hub, id, events).await
}

pub(crate) async fn cmd_type(
    on: Option<&str>,
    hub: &str,
    id: u64,
    text: &str,
) -> Result<(), String> {
    let events = parse_type_string(text)?;
    send_key_events(on, hub, id, events).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_record(
    on: Option<&str>,
    hub: &str,
    id: u64,
    output: Option<String>,
    max_frames: u32,
    max_duration: f64,
    codecs: Vec<String>,
    size: Option<String>,
    encode_size: Option<String>,
    fps: u16,
    timing_path: Option<String>,
) -> Result<(), String> {
    if fps == 0 {
        return Err("surface recording fps must be nonzero".to_string());
    }
    if !max_duration.is_finite() || max_duration < 0.0 {
        return Err("surface recording duration must be a finite nonnegative number".to_string());
    }
    let id = surface_handle(id)?;
    let requested_size = parse_record_size(size.as_deref())?;
    let encoded_size = parse_record_encode_size(encode_size.as_deref())?;
    let codec_versions = parse_record_codecs(&codecs)?;

    let mut client = NativeClient::connect(on, hub).await?;
    let mut record = find_surface(&mut client, id).await?;
    if let Some((width, height, scale_120)) = requested_size {
        let _: surface::RevisionResult = client
            .request_typed(
                family::SURFACE,
                surface::request_kind::RESIZE,
                &surface::Resize {
                    surface_handle: id,
                    operation_id: operation_id(),
                    logical_width_32_32: scaled_logical_dimension(width, scale_120)?,
                    logical_height_32_32: scaled_logical_dimension(height, scale_120)?,
                    extensions: Extensions::default(),
                },
                false,
            )
            .await?;
        record = find_surface(&mut client, id).await?;
    }
    let (default_width, default_height) = physical_dimensions(&record)?;
    let (width, height) = requested_size
        .map(|(width, height, _)| (u32::from(width), u32::from(height)))
        .or_else(|| encoded_size.map(|(width, height)| (u32::from(width), u32::from(height))))
        .unwrap_or((default_width, default_height));
    let result: surface::ViewResult = client
        .request_typed(
            family::SURFACE,
            surface::request_kind::OPEN_VIEW,
            &surface::OpenView {
                surface_handle: id,
                width,
                height,
                max_fps: fps,
                decoder_capacity: 4,
                codec_versions,
                extensions: Extensions::default(),
            },
            false,
        )
        .await?;

    let codec_extension = codec_file_extension(result.codec_version)?;
    let path = output.unwrap_or_else(|| format!("surface-{id}.{codec_extension}"));
    let file = std::fs::File::create(&path).map_err(|error| format!("create {path}: {error}"))?;
    let mut file = BufWriter::new(file);
    let mut timing = match timing_path {
        Some(path) => {
            let mut timing = BufWriter::new(
                std::fs::File::create(&path).map_err(|error| format!("create {path}: {error}"))?,
            );
            writeln!(timing, "pts_ms,arrival_ms,bytes,key")
                .map_err(|error| format!("write {path}: {error}"))?;
            Some(timing)
        }
        None => None,
    };
    let limit = record_limit(max_frames, max_duration);
    eprintln!("recording surface {id} ({codec_extension}, {width}x{height}) → {path} ({limit})");

    let maximum_frame = result.max_encoded_frame.min(MAX_SURFACE_FRAME_BYTES);
    let mut assembler = SurfaceFrameAssembler::new(&result, maximum_frame)?;
    let start = std::time::Instant::now();
    let deadline = (max_duration > 0.0)
        .then(|| tokio::time::Instant::now() + Duration::from_secs_f64(max_duration));
    let deadline_sleep = tokio::time::sleep_until(
        deadline.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400 * 365)),
    );
    tokio::pin!(deadline_sleep);
    let cancellation = tokio::signal::ctrl_c();
    tokio::pin!(cancellation);
    let mut frame_count = 0u32;
    let mut total_bytes = 0u64;
    let mut last_written = None;

    loop {
        let fragment = tokio::select! {
            signal = &mut cancellation => {
                signal.map_err(|error| format!("cannot listen for Ctrl+C: {error}"))?;
                break;
            }
            () = &mut deadline_sleep, if deadline.is_some() => break,
            frame = client.next_typed_event::<surface::SurfaceFrame>(
                family::SURFACE,
                surface::event_kind::FRAME,
            ) => frame?,
        };
        let Some(frame) = assembler.push(fragment)? else {
            continue;
        };
        let keyframe = frame.flags & yas_wire::schema::surface::FRAME_KEYFRAME as u16 != 0;
        let dependency_ok = keyframe
            || last_written.is_some_and(|sequence| {
                frame.base_sequence == sequence && frame.sequence > sequence
            });
        if !dependency_ok {
            request_empty(
                &mut client,
                surface::request_kind::RESET_VIEW,
                &surface::ResetView {
                    view_id: result.view_id,
                },
                false,
            )
            .await?;
            last_written = None;
            continue;
        }

        let elementary = elementary_stream(&frame.payload)?;
        file.write_all(elementary)
            .map_err(|error| format!("write {path}: {error}"))?;
        if let Some(timing) = timing.as_mut() {
            writeln!(
                timing,
                "{:.3},{:.3},{},{}",
                frame.capture_ns as f64 / 1_000_000.0,
                start.elapsed().as_secs_f64() * 1000.0,
                elementary.len(),
                u8::from(keyframe)
            )
            .map_err(|error| format!("write timing: {error}"))?;
        }
        total_bytes = total_bytes.saturating_add(elementary.len() as u64);
        frame_count = frame_count.saturating_add(1);
        last_written = Some(frame.sequence);
        client
            .send_typed_event(
                family::SURFACE,
                surface::event_kind::FRAME_ACK,
                &surface::FrameAck {
                    view_id: result.view_id,
                    feedback: surface::FrameFeedback {
                        presented_sequence: frame.sequence,
                        decoder_queue_depth: 0,
                        available_slots: result.max_inflight_frames,
                    },
                },
                false,
            )
            .await?;
        eprint!(
            "\r  frame {frame_count} key={keyframe} {width}x{height} {:.1}s {total_bytes} bytes  ",
            start.elapsed().as_secs_f64()
        );

        if frame.flags & yas_wire::schema::surface::FRAME_END_OF_STREAM as u16 != 0
            || (max_frames > 0 && frame_count >= max_frames)
        {
            break;
        }
    }
    file.flush()
        .map_err(|error| format!("flush {path}: {error}"))?;
    if let Some(timing) = timing.as_mut() {
        timing
            .flush()
            .map_err(|error| format!("flush timing: {error}"))?;
    }
    request_empty(
        &mut client,
        surface::request_kind::CLOSE_VIEW,
        &surface::CloseView {
            view_id: result.view_id,
        },
        false,
    )
    .await?;
    eprintln!(
        "\n  done: {frame_count} frames, {total_bytes} bytes, {:.1}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

async fn send_key_events(
    on: Option<&str>,
    hub: &str,
    id: u64,
    events: Vec<KeyEvent>,
) -> Result<(), String> {
    with_input_view(on, hub, id, |client, view| {
        Box::pin(async move {
            for key in events {
                client
                    .send_typed_event(
                        family::SURFACE,
                        surface::event_kind::KEY,
                        &surface::Key {
                            view_id: view.result.view_id,
                            feedback: view.feedback(),
                            client_monotonic_ns: client.monotonic_ns(),
                            key_code: key.code,
                            state: key.state,
                            modifiers: key.modifiers,
                        },
                        true,
                    )
                    .await?;
            }
            Ok(())
        })
    })
    .await
}

type InputFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + 'a>>;

async fn with_input_view<F>(on: Option<&str>, hub: &str, id: u64, action: F) -> Result<(), String>
where
    F: for<'a> FnOnce(&'a mut NativeClient, &'a InputView) -> InputFuture<'a>,
{
    let id = surface_handle(id)?;
    let mut client = NativeClient::connect(on, hub).await?;
    let record = find_surface(&mut client, id).await?;
    let view = open_input_view(&mut client, &record).await?;
    let result = action(&mut client, &view).await;
    if result.is_ok() {
        request_empty(
            &mut client,
            surface::request_kind::CLOSE_VIEW,
            &surface::CloseView {
                view_id: view.result.view_id,
            },
            false,
        )
        .await?;
    }
    result
}

struct InputView {
    result: surface::ViewResult,
}

impl InputView {
    fn feedback(&self) -> surface::FrameFeedback {
        surface::FrameFeedback {
            presented_sequence: self.result.first_sequence.saturating_sub(1),
            decoder_queue_depth: 0,
            available_slots: self.result.max_inflight_frames,
        }
    }
}

async fn open_input_view(
    client: &mut NativeClient,
    record: &surface::SurfaceRecord,
) -> Result<InputView, String> {
    let (width, height) = physical_dimensions(record)?;
    let result = client
        .request_typed(
            family::SURFACE,
            surface::request_kind::OPEN_VIEW,
            &surface::OpenView {
                surface_handle: record.surface_handle,
                width,
                height,
                max_fps: 1,
                decoder_capacity: 1,
                codec_versions: vec![
                    yas_wire::schema::surface::CODEC_H264_V1 as u16,
                    yas_wire::schema::surface::CODEC_AV1_V1 as u16,
                ],
                extensions: Extensions::default(),
            },
            false,
        )
        .await?;
    Ok(InputView { result })
}

async fn surface_records(client: &mut NativeClient) -> Result<Vec<surface::SurfaceRecord>, String> {
    client
        .snapshot(family::SURFACE)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Surface family".to_string())?
        .iter()
        .map(|record| {
            surface::surface_from_state_record(record)
                .map_err(|error| format!("invalid YAS Surface state: {error}"))
        })
        .collect()
}

async fn find_surface(
    client: &mut NativeClient,
    handle: u64,
) -> Result<surface::SurfaceRecord, String> {
    surface_records(client)
        .await?
        .into_iter()
        .find(|record| record.surface_handle == handle)
        .ok_or_else(|| format!("surface {handle} not found"))
}

async fn request_empty<Request: Encode>(
    client: &mut NativeClient,
    kind: u16,
    request: &Request,
    sensitive: bool,
) -> Result<(), String> {
    let body = client
        .request(
            family::SURFACE,
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
            "YAS Surface request {kind:#06x} returned an unexpected {}-byte body",
            body.len()
        ))
    }
}

fn surface_handle(value: u64) -> Result<u64, String> {
    if value == 0 {
        Err("Surface handle must be nonzero".to_string())
    } else {
        Ok(value)
    }
}

fn physical_dimensions(record: &surface::SurfaceRecord) -> Result<(u32, u32), String> {
    let dimension = |value: i64, scale: u16| -> Result<u32, String> {
        let value = u64::try_from(value)
            .map_err(|_| "Surface state contains a negative dimension".to_string())?;
        let logical = value.saturating_add((1u64 << 32) - 1) >> 32;
        let physical = logical
            .checked_mul(u64::from(scale))
            .ok_or_else(|| "Surface physical dimension overflow".to_string())?;
        u32::try_from(physical)
            .ok()
            .filter(|value| *value != 0)
            .ok_or_else(|| "Surface physical dimension is out of range".to_string())
    };
    Ok((
        dimension(record.logical_width_32_32, record.buffer_scale)?,
        dimension(record.logical_height_32_32, record.buffer_scale)?,
    ))
}

fn capture_format(
    format_arg: Option<&str>,
    output: Option<&str>,
) -> Result<(u8, &'static str), String> {
    let inferred = output
        .and_then(|path| path.rsplit('.').next())
        .map(str::to_ascii_lowercase);
    match format_arg.map(str::to_ascii_lowercase).as_deref() {
        Some("png") => Ok((yas_wire::schema::surface::CAPTURE_PNG as u8, "png")),
        Some("avif") => Ok((yas_wire::schema::surface::CAPTURE_AVIF as u8, "avif")),
        Some(other) => Err(format!("unknown format: {other} (expected png or avif)")),
        None if inferred.as_deref() == Some("avif") => {
            Ok((yas_wire::schema::surface::CAPTURE_AVIF as u8, "avif"))
        }
        None => Ok((yas_wire::schema::surface::CAPTURE_PNG as u8, "png")),
    }
}

fn pointer_button(value: &str) -> Result<u8, String> {
    match value {
        "left" => Ok(yas_wire::schema::surface::POINTER_BUTTON_PRIMARY as u8),
        "right" => Ok(yas_wire::schema::surface::POINTER_BUTTON_SECONDARY as u8),
        "middle" => Ok(yas_wire::schema::surface::POINTER_BUTTON_MIDDLE as u8),
        "back" => Ok(yas_wire::schema::surface::POINTER_BUTTON_BACK as u8),
        "forward" => Ok(yas_wire::schema::surface::POINTER_BUTTON_FORWARD as u8),
        other => Err(format!(
            "unknown button {other:?}: expected left, right, middle, back, or forward"
        )),
    }
}

fn fixed_32_32(value: f64, name: &str) -> Result<i64, String> {
    let scaled = value * 4_294_967_296.0;
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(format!("{name} is out of range"));
    }
    Ok(scaled.round() as i64)
}

fn rounded_i32(value: f64, name: &str) -> Result<i32, String> {
    let value = value.round();
    if value < i32::MIN as f64 || value > i32::MAX as f64 {
        return Err(format!("{name} are out of range"));
    }
    Ok(value as i32)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyEvent {
    code: u16,
    state: u8,
    modifiers: u32,
}

fn parse_key_combo(value: &str) -> Result<Vec<KeyEvent>, String> {
    let parts = value.split('+').collect::<Vec<_>>();
    let main = parts
        .last()
        .copied()
        .filter(|part| !part.is_empty())
        .ok_or("empty key")?;
    let mut modifiers = Vec::new();
    let mut seen = BTreeSet::new();
    for part in &parts[..parts.len().saturating_sub(1)] {
        let modifier = modifier_key(part).ok_or_else(|| format!("unknown modifier: {part}"))?;
        if !seen.insert(modifier.1) {
            return Err(format!("modifier {part:?} appears more than once"));
        }
        modifiers.push(modifier);
    }
    let main = key_name(main).ok_or_else(|| format!("unknown key: {main}"))?;
    Ok(key_chord(&modifiers, main))
}

fn key_chord(modifiers: &[(u16, u32)], main: u16) -> Vec<KeyEvent> {
    let pressed = yas_wire::schema::surface::KEY_STATE_PRESSED as u8;
    let released = yas_wire::schema::surface::KEY_STATE_RELEASED as u8;
    let mut mask = 0u32;
    let mut events = Vec::with_capacity(2 + modifiers.len() * 2);
    for &(code, bit) in modifiers {
        mask |= bit;
        events.push(KeyEvent {
            code,
            state: pressed,
            modifiers: mask,
        });
    }
    events.push(KeyEvent {
        code: main,
        state: pressed,
        modifiers: mask,
    });
    events.push(KeyEvent {
        code: main,
        state: released,
        modifiers: mask,
    });
    for &(code, bit) in modifiers.iter().rev() {
        events.push(KeyEvent {
            code,
            state: released,
            modifiers: mask,
        });
        mask &= !bit;
    }
    events
}

fn parse_type_string(text: &str) -> Result<Vec<KeyEvent>, String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut events = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '{' {
            let end = chars[index..]
                .iter()
                .position(|character| *character == '}')
                .ok_or_else(|| "unclosed { in type string".to_string())?;
            let inner = chars[index + 1..index + end].iter().collect::<String>();
            events.extend(parse_key_combo(&inner)?);
            index += end + 1;
            continue;
        }
        let (code, shift) = character_key(chars[index])
            .ok_or_else(|| format!("unsupported character: {}", chars[index]))?;
        let modifiers = if shift {
            vec![modifier_key("shift").expect("known Shift modifier")]
        } else {
            Vec::new()
        };
        events.extend(key_chord(&modifiers, code));
        index += 1;
    }
    Ok(events)
}

fn modifier_key(name: &str) -> Option<(u16, u32)> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some((
            yas_wire::schema::surface::KEY_CONTROL_LEFT as u16,
            yas_wire::schema::surface::MODIFIER_CONTROL as u32,
        )),
        "shift" => Some((
            yas_wire::schema::surface::KEY_SHIFT_LEFT as u16,
            yas_wire::schema::surface::MODIFIER_SHIFT as u32,
        )),
        "alt" => Some((
            yas_wire::schema::surface::KEY_ALT_LEFT as u16,
            yas_wire::schema::surface::MODIFIER_ALT as u32,
        )),
        "super" | "meta" => Some((
            yas_wire::schema::surface::KEY_SUPER_LEFT as u16,
            yas_wire::schema::surface::MODIFIER_SUPER as u32,
        )),
        _ => None,
    }
}

fn key_name(name: &str) -> Option<u16> {
    let name = name.to_ascii_lowercase();
    if name.len() == 1 {
        return character_key(name.chars().next()?).map(|(code, _)| code);
    }
    let value = match name.as_str() {
        "return" | "enter" => yas_wire::schema::surface::KEY_ENTER,
        "escape" | "esc" => yas_wire::schema::surface::KEY_ESCAPE,
        "tab" => yas_wire::schema::surface::KEY_TAB,
        "backspace" | "bs" => yas_wire::schema::surface::KEY_BACKSPACE,
        "space" => yas_wire::schema::surface::KEY_SPACE,
        "up" => yas_wire::schema::surface::KEY_ARROW_UP,
        "down" => yas_wire::schema::surface::KEY_ARROW_DOWN,
        "left" => yas_wire::schema::surface::KEY_ARROW_LEFT,
        "right" => yas_wire::schema::surface::KEY_ARROW_RIGHT,
        "home" => yas_wire::schema::surface::KEY_HOME,
        "end" => yas_wire::schema::surface::KEY_END,
        "pageup" | "page_up" => yas_wire::schema::surface::KEY_PAGE_UP,
        "pagedown" | "page_down" => yas_wire::schema::surface::KEY_PAGE_DOWN,
        "insert" => yas_wire::schema::surface::KEY_INSERT,
        "delete" | "del" => yas_wire::schema::surface::KEY_DELETE,
        "f1" => yas_wire::schema::surface::KEY_F1,
        "f2" => yas_wire::schema::surface::KEY_F2,
        "f3" => yas_wire::schema::surface::KEY_F3,
        "f4" => yas_wire::schema::surface::KEY_F4,
        "f5" => yas_wire::schema::surface::KEY_F5,
        "f6" => yas_wire::schema::surface::KEY_F6,
        "f7" => yas_wire::schema::surface::KEY_F7,
        "f8" => yas_wire::schema::surface::KEY_F8,
        "f9" => yas_wire::schema::surface::KEY_F9,
        "f10" => yas_wire::schema::surface::KEY_F10,
        "f11" => yas_wire::schema::surface::KEY_F11,
        "f12" => yas_wire::schema::surface::KEY_F12,
        "minus" => yas_wire::schema::surface::KEY_MINUS,
        "equal" => yas_wire::schema::surface::KEY_EQUAL,
        "ctrl" | "control" => yas_wire::schema::surface::KEY_CONTROL_LEFT,
        "shift" => yas_wire::schema::surface::KEY_SHIFT_LEFT,
        "alt" => yas_wire::schema::surface::KEY_ALT_LEFT,
        "super" | "meta" => yas_wire::schema::surface::KEY_SUPER_LEFT,
        _ => return None,
    };
    Some(value as u16)
}

fn character_key(character: char) -> Option<(u16, bool)> {
    let (code, shift) = match character {
        'a'..='z' => (
            yas_wire::schema::surface::KEY_A as u16 + (character as u16 - 'a' as u16),
            false,
        ),
        'A'..='Z' => (
            yas_wire::schema::surface::KEY_A as u16 + (character as u16 - 'A' as u16),
            true,
        ),
        '1'..='9' => (
            yas_wire::schema::surface::KEY_1 as u16 + (character as u16 - '1' as u16),
            false,
        ),
        '0' => (yas_wire::schema::surface::KEY_0 as u16, false),
        ' ' => (yas_wire::schema::surface::KEY_SPACE as u16, false),
        '-' => (yas_wire::schema::surface::KEY_MINUS as u16, false),
        '=' => (yas_wire::schema::surface::KEY_EQUAL as u16, false),
        '[' => (yas_wire::schema::surface::KEY_BRACKET_LEFT as u16, false),
        ']' => (yas_wire::schema::surface::KEY_BRACKET_RIGHT as u16, false),
        '\\' => (yas_wire::schema::surface::KEY_BACKSLASH as u16, false),
        ';' => (yas_wire::schema::surface::KEY_SEMICOLON as u16, false),
        '\'' => (yas_wire::schema::surface::KEY_QUOTE as u16, false),
        '`' => (yas_wire::schema::surface::KEY_BACKQUOTE as u16, false),
        ',' => (yas_wire::schema::surface::KEY_COMMA as u16, false),
        '.' => (yas_wire::schema::surface::KEY_PERIOD as u16, false),
        '/' => (yas_wire::schema::surface::KEY_SLASH as u16, false),
        '\t' => (yas_wire::schema::surface::KEY_TAB as u16, false),
        '\n' => (yas_wire::schema::surface::KEY_ENTER as u16, false),
        '!' => (yas_wire::schema::surface::KEY_1 as u16, true),
        '@' => (yas_wire::schema::surface::KEY_2 as u16, true),
        '#' => (yas_wire::schema::surface::KEY_3 as u16, true),
        '$' => (yas_wire::schema::surface::KEY_4 as u16, true),
        '%' => (yas_wire::schema::surface::KEY_5 as u16, true),
        '^' => (yas_wire::schema::surface::KEY_6 as u16, true),
        '&' => (yas_wire::schema::surface::KEY_7 as u16, true),
        '*' => (yas_wire::schema::surface::KEY_8 as u16, true),
        '(' => (yas_wire::schema::surface::KEY_9 as u16, true),
        ')' => (yas_wire::schema::surface::KEY_0 as u16, true),
        '_' => (yas_wire::schema::surface::KEY_MINUS as u16, true),
        '+' => (yas_wire::schema::surface::KEY_EQUAL as u16, true),
        '{' => (yas_wire::schema::surface::KEY_BRACKET_LEFT as u16, true),
        '}' => (yas_wire::schema::surface::KEY_BRACKET_RIGHT as u16, true),
        '|' => (yas_wire::schema::surface::KEY_BACKSLASH as u16, true),
        ':' => (yas_wire::schema::surface::KEY_SEMICOLON as u16, true),
        '"' => (yas_wire::schema::surface::KEY_QUOTE as u16, true),
        '~' => (yas_wire::schema::surface::KEY_BACKQUOTE as u16, true),
        '<' => (yas_wire::schema::surface::KEY_COMMA as u16, true),
        '>' => (yas_wire::schema::surface::KEY_PERIOD as u16, true),
        '?' => (yas_wire::schema::surface::KEY_SLASH as u16, true),
        _ => return None,
    };
    Some((code, shift))
}

fn parse_record_size(value: Option<&str>) -> Result<Option<(u16, u16, u16)>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let (dimensions, scale_120) = match value.split_once('@') {
        Some((dimensions, ratio)) => {
            let ratio = ratio
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("bad --size DPR {ratio:?} (expected e.g. 2 or 1.5)"))?;
            let scaled = (ratio * 120.0).round();
            if !scaled.is_finite() || !(120.0..=f64::from(u16::MAX)).contains(&scaled) {
                return Err(format!("--size DPR {ratio} must be finite and at least 1"));
            }
            (dimensions, scaled as u16)
        }
        None => (value, 120),
    };
    let (width, height) = dimensions
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("bad --size {value:?} (expected WIDTHxHEIGHT)"))?;
    let parse = |value: &str, axis: &str| {
        value
            .trim()
            .parse::<u16>()
            .map_err(|_| format!("bad --size {axis} {value:?}"))
            .and_then(|value| {
                (value != 0)
                    .then_some(value)
                    .ok_or_else(|| format!("--size {axis} must be nonzero"))
            })
    };
    Ok(Some((
        parse(width, "width")?,
        parse(height, "height")?,
        scale_120,
    )))
}

fn parse_record_encode_size(value: Option<&str>) -> Result<Option<(u16, u16)>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.contains('@') {
        return Err("--encode-size expects WIDTHxHEIGHT without a DPR".to_string());
    }
    Ok(parse_record_size(Some(value))?.map(|(width, height, _)| (width, height)))
}

fn parse_record_codecs(values: &[String]) -> Result<Vec<u16>, String> {
    let mut codecs = BTreeSet::new();
    if values.is_empty() {
        codecs.insert(yas_wire::schema::surface::CODEC_H264_V1 as u16);
        codecs.insert(yas_wire::schema::surface::CODEC_AV1_V1 as u16);
    }
    for value in values {
        match value.to_ascii_lowercase().as_str() {
            "h264" => {
                codecs.insert(yas_wire::schema::surface::CODEC_H264_V1 as u16);
            }
            "av1" => {
                codecs.insert(yas_wire::schema::surface::CODEC_AV1_V1 as u16);
            }
            "h264-444" | "av1-444" => {
                return Err(format!(
                    "codec {value:?} has no YAS Surface v1 chroma-profile negotiation"
                ));
            }
            other => {
                return Err(format!("unknown codec: {other} (expected h264 or av1)"));
            }
        }
    }
    Ok(codecs.into_iter().collect())
}

fn scaled_logical_dimension(physical: u16, scale_120: u16) -> Result<i64, String> {
    let numerator = (u128::from(physical) << 32)
        .checked_mul(120)
        .ok_or_else(|| "scaled Surface dimension overflow".to_string())?;
    i64::try_from(numerator / u128::from(scale_120))
        .map_err(|_| "scaled Surface dimension is out of range".to_string())
}

fn codec_file_extension(codec: u16) -> Result<&'static str, String> {
    match codec {
        value if value == yas_wire::schema::surface::CODEC_H264_V1 as u16 => Ok("h264"),
        value if value == yas_wire::schema::surface::CODEC_AV1_V1 as u16 => Ok("obu"),
        value if value == yas_wire::schema::surface::CODEC_PNG_V1 as u16 => Ok("png"),
        other => Err(format!("server selected unknown Surface codec {other}")),
    }
}

fn record_limit(frames: u32, duration: f64) -> String {
    match (frames > 0, duration > 0.0) {
        (true, true) => format!("{frames} frames / {duration}s"),
        (true, false) => format!("{frames} frames"),
        (false, true) => format!("{duration}s"),
        (false, false) => "until Ctrl+C".to_string(),
    }
}

struct FrameAssembly {
    template: surface::SurfaceFrame,
    next_fragment: u16,
    payload: Vec<u8>,
}

struct SurfaceFrameAssembler {
    view_id: u32,
    codec_version: u16,
    first_sequence: u64,
    maximum_frame: u32,
    maximum_inflight: usize,
    retained: u64,
    assemblies: BTreeMap<u64, FrameAssembly>,
    completed: BTreeSet<u64>,
}

impl SurfaceFrameAssembler {
    fn new(result: &surface::ViewResult, maximum_frame: u32) -> Result<Self, String> {
        if maximum_frame == 0 {
            return Err("YAS Surface view has a zero encoded-frame limit".to_string());
        }
        Ok(Self {
            view_id: result.view_id,
            codec_version: result.codec_version,
            first_sequence: result.first_sequence,
            maximum_frame,
            maximum_inflight: usize::from(result.max_inflight_frames),
            retained: 0,
            assemblies: BTreeMap::new(),
            completed: BTreeSet::new(),
        })
    }

    fn push(
        &mut self,
        fragment: surface::SurfaceFrame,
    ) -> Result<Option<surface::SurfaceFrame>, String> {
        if fragment.view_id != self.view_id || fragment.codec_version != self.codec_version {
            return Err("YAS Surface FRAME does not match its negotiated view".to_string());
        }
        if fragment.sequence < self.first_sequence {
            return Err("YAS Surface FRAME predates the negotiated sequence".to_string());
        }
        if fragment.complete_len > self.maximum_frame {
            return Err(format!(
                "YAS Surface FRAME is {} bytes; negotiated CLI limit is {}",
                fragment.complete_len, self.maximum_frame
            ));
        }
        if self.completed.contains(&fragment.sequence) {
            return Err("YAS Surface repeated a completed FRAME sequence".to_string());
        }
        if !self.assemblies.contains_key(&fragment.sequence) {
            if fragment.fragment_index != 0 {
                return Err("YAS Surface FRAME began with a later fragment".to_string());
            }
            if self.assemblies.len() >= self.maximum_inflight {
                return Err("YAS Surface exceeded its in-flight frame limit".to_string());
            }
            self.assemblies.insert(
                fragment.sequence,
                FrameAssembly {
                    template: fragment.clone(),
                    next_fragment: 0,
                    payload: Vec::new(),
                },
            );
        }
        let assembly = self
            .assemblies
            .get_mut(&fragment.sequence)
            .ok_or_else(|| "YAS Surface frame assembly disappeared".to_string())?;
        let original = &assembly.template;
        if assembly.next_fragment != fragment.fragment_index
            || original.fragment_count != fragment.fragment_count
            || original.complete_len != fragment.complete_len
            || original.base_sequence != fragment.base_sequence
            || original.capture_ns != fragment.capture_ns
            || original.presentation_ns != fragment.presentation_ns
            || original.flags != fragment.flags
            || original.codec_version != fragment.codec_version
        {
            return Err("YAS Surface sent inconsistent FRAME fragments".to_string());
        }
        let next_length = assembly
            .payload
            .len()
            .checked_add(fragment.payload.len())
            .ok_or_else(|| "YAS Surface FRAME length overflow".to_string())?;
        let remaining_fragments = usize::from(
            fragment
                .fragment_count
                .saturating_sub(fragment.fragment_index.saturating_add(1)),
        );
        if next_length > fragment.complete_len as usize
            || next_length.saturating_add(remaining_fragments) > fragment.complete_len as usize
        {
            return Err("YAS Surface FRAME fragments exceed their declared length".to_string());
        }
        let next_retained = self
            .retained
            .checked_add(fragment.payload.len() as u64)
            .ok_or_else(|| "YAS Surface retained-byte accounting overflow".to_string())?;
        let retained_limit = u64::from(self.maximum_frame)
            .saturating_mul(self.maximum_inflight as u64)
            .min(MAX_COLLECTED_TRANSFER_BYTES);
        if next_retained > retained_limit {
            return Err("YAS Surface frame assemblies exceed the bounded CLI budget".to_string());
        }
        self.retained = next_retained;
        assembly.payload.extend_from_slice(&fragment.payload);
        assembly.next_fragment = assembly.next_fragment.saturating_add(1);
        if assembly.next_fragment != fragment.fragment_count {
            return Ok(None);
        }
        let assembly = self
            .assemblies
            .remove(&fragment.sequence)
            .ok_or_else(|| "completed YAS Surface frame disappeared".to_string())?;
        self.retained = self.retained.saturating_sub(assembly.payload.len() as u64);
        if assembly.payload.len() != fragment.complete_len as usize {
            return Err("YAS Surface FRAME ended at the wrong length".to_string());
        }
        self.completed.insert(fragment.sequence);
        if self.completed.len() > self.maximum_inflight.saturating_mul(4).max(16) {
            let keep_from = *self.completed.iter().next_back().unwrap_or(&0);
            self.completed
                .retain(|sequence| sequence.saturating_add(16) >= keep_from);
        }
        Ok(Some(surface::SurfaceFrame {
            fragment_index: 0,
            fragment_count: 1,
            payload: assembly.payload,
            ..assembly.template
        }))
    }
}

fn elementary_stream(payload: &[u8]) -> Result<&[u8], String> {
    if payload.len() < 4 || payload[1..4] != [0; 3] {
        return Err("YAS Surface codec payload has an invalid metadata header".to_string());
    }
    let count = usize::from(payload[0]);
    let mut offset = 4usize;
    let mut previous_tag = 0u16;
    for _ in 0..count {
        let header = payload
            .get(offset..offset.saturating_add(8))
            .ok_or_else(|| "truncated YAS Surface codec metadata".to_string())?;
        let tag = u16::from_le_bytes([header[0], header[1]]);
        let _flags = u16::from_le_bytes([header[2], header[3]]);
        let length =
            u32::from_le_bytes(header[4..8].try_into().expect("eight-byte header")) as usize;
        if tag == 0 || tag <= previous_tag {
            return Err("YAS Surface codec metadata is not uniquely tag-ordered".to_string());
        }
        previous_tag = tag;
        offset = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(length))
            .ok_or_else(|| "YAS Surface codec metadata length overflow".to_string())?;
        let body = payload
            .get(offset - length..offset)
            .ok_or_else(|| "truncated YAS Surface codec metadata body".to_string())?;
        match tag {
            1 if body.len() != 4 => {
                return Err("invalid YAS Surface color-space metadata".to_string());
            }
            2 => validate_damage_metadata(body)?,
            _ => {}
        }
    }
    let stream = payload
        .get(offset..)
        .ok_or_else(|| "truncated YAS Surface codec payload".to_string())?;
    if stream.is_empty() {
        return Err("YAS Surface codec payload has no elementary stream".to_string());
    }
    Ok(stream)
}

fn validate_damage_metadata(body: &[u8]) -> Result<(), String> {
    if body.len() < 4 || body[2..4] != [0; 2] {
        return Err("invalid YAS Surface damage metadata".to_string());
    }
    let count = usize::from(u16::from_le_bytes([body[0], body[1]]));
    if count > 256 || body.len() != 4usize.saturating_add(count.saturating_mul(16)) {
        return Err("invalid YAS Surface damage rectangle list".to_string());
    }
    Ok(())
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

    fn view_result() -> surface::ViewResult {
        surface::ViewResult {
            view_id: 7,
            codec_version: yas_wire::schema::surface::CODEC_H264_V1 as u16,
            max_inflight_frames: 2,
            max_encoded_frame: 1024,
            max_decoded_frame: 4096,
            first_sequence: 11,
            extensions: Extensions::default(),
        }
    }

    fn fragment(index: u16, payload: &[u8]) -> surface::SurfaceFrame {
        surface::SurfaceFrame {
            view_id: 7,
            sequence: 11,
            base_sequence: 0,
            capture_ns: 12,
            presentation_ns: 13,
            flags: yas_wire::schema::surface::FRAME_KEYFRAME as u16,
            codec_version: yas_wire::schema::surface::CODEC_H264_V1 as u16,
            fragment_index: index,
            fragment_count: 2,
            complete_len: 7,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn surface_handles_are_opaque_nonzero_u64_values() {
        assert_eq!(surface_handle(1).unwrap(), 1);
        assert_eq!(surface_handle(u64::MAX).unwrap(), u64::MAX);
        assert!(surface_handle(0).is_err());
    }

    #[test]
    fn native_surface_fragments_reassemble_in_order() {
        let mut assembler = SurfaceFrameAssembler::new(&view_result(), 1024).unwrap();
        assert!(assembler.push(fragment(0, b"abc")).unwrap().is_none());
        let frame = assembler.push(fragment(1, b"defg")).unwrap().unwrap();
        assert_eq!(frame.payload, b"abcdefg");
        assert_eq!(frame.fragment_count, 1);
    }

    #[test]
    fn native_surface_fragments_reject_out_of_order_delivery() {
        let mut assembler = SurfaceFrameAssembler::new(&view_result(), 1024).unwrap();
        assert!(assembler.push(fragment(1, b"defg")).is_err());
    }

    #[test]
    fn codec_metadata_is_stripped_without_touching_the_access_unit() {
        let mut payload = vec![1, 0, 0, 0];
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&4u32.to_le_bytes());
        payload.extend_from_slice(&[1, 2, 3, 4]);
        payload.extend_from_slice(&[0, 0, 0, 1, 0x65, 0x88]);
        assert_eq!(
            elementary_stream(&payload).unwrap(),
            &[0, 0, 0, 1, 0x65, 0x88]
        );
        assert_eq!(
            elementary_stream(&[0, 0, 0, 0, 0, 0, 1, 0x65]).unwrap(),
            &[0, 0, 1, 0x65]
        );
    }

    #[test]
    fn key_combos_use_native_hid_codes_and_modifier_bits() {
        let events = parse_key_combo("ctrl+a").unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0].code,
            yas_wire::schema::surface::KEY_CONTROL_LEFT as u16
        );
        assert_eq!(events[1].code, yas_wire::schema::surface::KEY_A as u16);
        assert_eq!(
            events[1].modifiers,
            yas_wire::schema::surface::MODIFIER_CONTROL as u32
        );
    }

    #[test]
    fn record_size_parses_dpr_and_rejects_zero() {
        assert_eq!(
            parse_record_size(Some("1200x900@1.5")).unwrap(),
            Some((1200, 900, 180))
        );
        assert!(parse_record_size(Some("0x900")).is_err());
    }
}
