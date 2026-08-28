//! Direct native Surface presentation over the compositor backend.
//!
//! A native view owns one hidden delivery client because the encoder
//! and congestion controller are per viewer.  The client is configured by
//! semantic calls below and emits typed events through [`Sink`]; it has no
//! socket or protocol dispatcher.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Codec {
    H264,
    Av1,
}

#[derive(Debug)]
pub(crate) struct Frame {
    pub(crate) view_id: u32,
    pub(crate) surface_id: u16,
    pub(crate) codec: Codec,
    pub(crate) timestamp_ms: u32,
    pub(crate) timestamp_sub_us: u16,
    pub(crate) keyframe: bool,
    pub(crate) data: Vec<u8>,
}

/// One encoded compositor frame before it is bound to a native YAS view.
/// Keeping the frame metadata together avoids a positional encoder call
/// surface.
pub(crate) struct EncodedFrame {
    pub(crate) surface_id: u16,
    pub(crate) codec: Codec,
    pub(crate) timestamp_ms: u32,
    pub(crate) timestamp_sub_us: u16,
    pub(crate) keyframe: bool,
    pub(crate) data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteInputKind {
    Pointer,
    Touch,
}

#[derive(Debug)]
pub(crate) struct RemoteInput {
    pub(crate) view_id: u32,
    pub(crate) surface_id: u16,
    pub(crate) seat_handle: u64,
    pub(crate) kind: RemoteInputKind,
    pub(crate) points: SmallVec<[(u16, u16); 5]>,
}

#[derive(Debug)]
pub(crate) enum Event {
    Frame(Frame),
    RemoteInput(RemoteInput),
}

#[derive(Clone)]
pub(crate) struct Sink {
    view_id: u32,
    events: mpsc::Sender<Event>,
}

impl Sink {
    fn send_frame(&self, frame: EncodedFrame) -> Result<usize, ()> {
        let bytes = frame.data.len().saturating_add(64);
        self.events
            .try_send(Event::Frame(Frame {
                view_id: self.view_id,
                surface_id: frame.surface_id,
                codec: frame.codec,
                timestamp_ms: frame.timestamp_ms,
                timestamp_sub_us: frame.timestamp_sub_us,
                keyframe: frame.keyframe,
                data: frame.data,
            }))
            .map_err(|_| ())?;
        Ok(bytes)
    }

    fn send_remote_input(
        &self,
        surface_id: u16,
        seat_handle: u64,
        kind: RemoteInputKind,
        points: &[(u16, u16)],
    ) -> Result<(), ()> {
        self.events
            .try_send(Event::RemoteInput(RemoteInput {
                view_id: self.view_id,
                surface_id,
                seat_handle,
                kind,
                points: points.iter().copied().collect(),
            }))
            .map_err(|_| ())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewConfig {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) max_fps: u16,
    pub(crate) decoder_capacity: u8,
    pub(crate) codec: Codec,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PointerPhase {
    Move,
    Down,
    Up,
    Enter,
    Leave,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TouchContact {
    pub(crate) id: i32,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

#[derive(Debug)]
pub(crate) enum Input {
    Key {
        keycode: u32,
        pressed: bool,
        time_ms: u32,
    },
    Text(String),
    Preedit {
        text: String,
        cursor: u16,
    },
    Pointer {
        phase: PointerPhase,
        button: u8,
        /// Position within the presented frame, normalized to 0..=1. The
        /// compositor expands this against the mapping current when it
        /// consumes the command, so a resize cannot mix browser catalogue
        /// geometry with a different rendered frame.
        x: f64,
        y: f64,
        time_ms: u32,
    },
    Axis {
        dx: f64,
        dy: f64,
        v120_x: i16,
        v120_y: i16,
        source: u8,
        stop: bool,
        time_ms: u32,
    },
    Touch {
        phase: TouchPhase,
        time_ms: u32,
        contacts: Vec<TouchContact>,
    },
}

fn codec_support(codec: Codec) -> u8 {
    match codec {
        Codec::H264 => CODEC_SUPPORT_H264,
        Codec::Av1 => CODEC_SUPPORT_AV1,
    }
}

fn apply_touch(
    session: &mut Session,
    client_id: u64,
    surface_id: u16,
    phase: TouchPhase,
    time_ms: u32,
    contacts: Vec<TouchContact>,
) -> Vec<CompositorCommand> {
    let Some(enabled) = session
        .clients
        .get(&client_id)
        .map(|client| client.direct_touch_enabled)
    else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    match phase {
        TouchPhase::Cancel => {
            if enabled && session.surface_touch_owner == Some(client_id) {
                session.surface_touch_owner = None;
                if let Some(client) = session.clients.get_mut(&client_id) {
                    client.surface_touch_ids.clear();
                }
                session.clear_surface_pointer_owner(client_id);
                commands.push(CompositorCommand::Touch {
                    owner_id: client_id,
                    surface_id,
                    phase: yas_compositor::TouchPhase::Cancel,
                    time_ms,
                    contacts: Vec::new(),
                });
            }
        }
        TouchPhase::Down => {
            if !enabled || contacts.is_empty() {
                return commands;
            }
            if let Some(owner) = session.surface_touch_owner {
                if owner != client_id {
                    return commands;
                }
            } else {
                session.surface_touch_owner = Some(client_id);
            }
            let contacts = session
                .clients
                .get_mut(&client_id)
                .map(|client| {
                    contacts
                        .into_iter()
                        .filter(|point| {
                            client
                                .surface_touch_ids
                                .insert(
                                    point.id,
                                    TouchMark {
                                        surface_id,
                                        at: frame_point(point.x, point.y),
                                    },
                                )
                                .is_none()
                        })
                        .map(|point| yas_compositor::TouchPoint {
                            id: point.id,
                            x: point.x,
                            y: point.y,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !contacts.is_empty() {
                commands.push(CompositorCommand::Touch {
                    owner_id: client_id,
                    surface_id,
                    phase: yas_compositor::TouchPhase::Down,
                    time_ms,
                    contacts,
                });
            }
            session.mirror_owner_touch(client_id);
        }
        TouchPhase::Move => {
            if !enabled || session.surface_touch_owner != Some(client_id) {
                return commands;
            }
            let contacts = session
                .clients
                .get_mut(&client_id)
                .map(|client| {
                    contacts
                        .into_iter()
                        .filter(|point| {
                            let live = client.surface_touch_ids.contains_key(&point.id);
                            if live {
                                client.surface_touch_ids.insert(
                                    point.id,
                                    TouchMark {
                                        surface_id,
                                        at: frame_point(point.x, point.y),
                                    },
                                );
                            }
                            live
                        })
                        .map(|point| yas_compositor::TouchPoint {
                            id: point.id,
                            x: point.x,
                            y: point.y,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !contacts.is_empty() {
                commands.push(CompositorCommand::Touch {
                    owner_id: client_id,
                    surface_id,
                    phase: yas_compositor::TouchPhase::Motion,
                    time_ms,
                    contacts,
                });
            }
            session.mirror_owner_touch(client_id);
        }
        TouchPhase::Up => {
            if !enabled || session.surface_touch_owner != Some(client_id) {
                return commands;
            }
            let (contacts, empty) = session
                .clients
                .get_mut(&client_id)
                .map(|client| {
                    let contacts = contacts
                        .into_iter()
                        .filter(|point| client.surface_touch_ids.remove(&point.id).is_some())
                        .map(|point| yas_compositor::TouchPoint {
                            id: point.id,
                            x: point.x,
                            y: point.y,
                        })
                        .collect::<Vec<_>>();
                    (contacts, client.surface_touch_ids.is_empty())
                })
                .unwrap_or_default();
            if !contacts.is_empty() {
                commands.push(CompositorCommand::Touch {
                    owner_id: client_id,
                    surface_id,
                    phase: yas_compositor::TouchPhase::Up,
                    time_ms,
                    contacts,
                });
            }
            if empty {
                session.surface_touch_owner = None;
            }
            session.mirror_owner_touch(client_id);
        }
    }
    commands
}

fn hidden_client(view_id: u32, events: mpsc::Sender<Event>, config: ViewConfig) -> ClientState {
    ClientState {
        write_blocked_us: Arc::new(AtomicU64::new(0)),
        write_blocked_us_seen: 0,
        outbound_bytes: Arc::new(AtomicU64::new(0)),
        outbound_bytes_seen: 0,
        outbound_sampled_at: Instant::now(),
        outbound_bytes_per_sec: 0,
        inbound_bytes: Arc::new(AtomicU64::new(0)),
        inbound_bytes_seen: 0,
        inbound_sampled_at: Instant::now(),
        inbound_bytes_per_sec: 0,
        connected_at: Instant::now(),
        origin: ConnectionOrigin::Network,
        catalog_visible: false,
        native_identity: None,
        native_surface: Some(Sink { view_id, events }),
        lead: None,
        subscriptions: FxHashSet::default(),
        surface_subscriptions: FxHashSet::default(),
        view_sizes: FxHashMap::default(),
        scroll_offsets: FxHashMap::default(),
        scroll_caches: FxHashMap::default(),
        last_sent: FxHashMap::default(),
        last_used_rows_sent: FxHashMap::default(),
        preview_next_send_at: FxHashMap::default(),
        rtt_ms: 50.0,
        min_rtt_ms: 0.0,
        display_fps: f32::from(config.max_fps.max(1)),
        delivery_bps: 262_144.0,
        goodput_bps: 262_144.0,
        goodput_jitter_bps: 0.0,
        max_goodput_jitter_bps: 0.0,
        last_goodput_sample_bps: 0.0,
        avg_frame_bytes: 1_024.0,
        avg_paced_frame_bytes: 1_024.0,
        avg_preview_frame_bytes: 1_024.0,
        avg_surface_frame_bytes: 8_192.0,
        #[cfg(test)]
        inflight_bytes: 0,
        #[cfg(test)]
        inflight_frames: VecDeque::new(),
        next_send_at: Instant::now(),
        probe_frames: 0.0,
        frames_sent: 0,
        acks_recv: 0,
        acked_bytes_since_log: 0,
        browser_backlog_frames: 0,
        browser_ack_ahead_frames: 0,
        browser_apply_ms: 0.0,
        last_log: Instant::now(),
        last_window_blocked_log: Instant::now(),
        last_skip_log: Instant::now(),
        skip_same_gen_count: 0,
        skip_in_flight_count: 0,
        skip_pacing_count: 0,
        skip_vulkan_await_count: 0,
        skip_no_subs_count: 0,
        skip_not_subbed_count: 0,
        skip_last_pixels_mismatch_count: 0,
        encode_loop_iters: 0,
        goodput_window_bytes: 0,
        goodput_window_start: Instant::now(),
        surface_goodput_bps: 262_144.0,
        surface_goodput_window_bytes: 0,
        surface_goodput_window_start: Instant::now(),
        surface_subs: FxHashMap::default(),
        surface_inflight_frames: VecDeque::new(),
        surface_inflight_bytes: 0,
        surface_schedule_cursor: None,
        vulkan_video_surfaces: FxHashMap::default(),
        surface_view_sizes: FxHashMap::default(),
        surface_claim_lapses: FxHashMap::default(),
        surface_codec_support: codec_support(config.codec),
        surface_max_decode: (config.width, config.height),
        pressed_surface_keys: HashSet::new(),
        direct_touch_enabled: true,
        surface_touch_ids: HashMap::new(),
    }
}

pub(crate) async fn register(
    state: &AppState,
    view_id: u32,
    surface_id: u16,
    config: ViewConfig,
    events: mpsc::Sender<Event>,
) -> Option<u64> {
    let mut session = state.session.lock().await;
    if session
        .compositor
        .as_ref()
        .is_none_or(|compositor| !compositor.surfaces.contains_key(&surface_id))
    {
        return None;
    }
    let client_id = session.next_client_id.max(1);
    session.next_client_id = client_id.checked_add(1)?;
    let mut client = hidden_client(view_id, events, config);
    client.surface_subscriptions.insert(surface_id);
    client
        .surface_view_sizes
        .insert(surface_id, (config.width, config.height, 120));
    let sub = client.surface_subs.entry(surface_id).or_default();
    sub.codec_override = codec_support(config.codec);
    sub.scaled_target = Some((config.width, config.height));
    sub.allow_adaptive_scale = true;
    sub.max_fps = Some(f32::from(config.max_fps.max(1)));
    sub.max_inflight_frames = Some(usize::from(config.decoder_capacity.max(1)));
    sub.burst_remaining = SURFACE_BURST_FRAMES;
    request_surface_keyframe(sub, Instant::now(), true);
    session.clients.insert(client_id, client);
    session.sync_compositor_refresh_rate();
    session.send_surface_pointer_to(client_id, surface_id);
    if let Some(compositor) = session.compositor.as_mut() {
        compositor.frame_clocks_dirty = true;
        if !compositor
            .last_pixels
            .keys()
            .any(|(known, _, _)| *known == surface_id)
        {
            let _ = compositor
                .handle
                .command_tx
                .send(CompositorCommand::Recomposite { surface_id });
            compositor.handle.wake();
        }
    }
    session.sync_touch_capability();
    drop(session);
    state.delivery_notify.notify_one();
    Some(client_id)
}

pub(crate) async fn configure(
    state: &AppState,
    client_id: u64,
    surface_id: u16,
    config: ViewConfig,
) -> bool {
    let mut session = state.session.lock().await;
    let Some(client) = session.clients.get_mut(&client_id) else {
        return false;
    };
    if client.native_surface.is_none() || !client.surface_subscriptions.contains(&surface_id) {
        return false;
    }
    client.display_fps = f32::from(config.max_fps.max(1));
    client.surface_codec_support = codec_support(config.codec);
    client.surface_max_decode = (config.width, config.height);
    client
        .surface_view_sizes
        .insert(surface_id, (config.width, config.height, 120));
    let sub = client.surface_subs.entry(surface_id).or_default();
    retire_encoder(sub.encoder.take());
    sub.codec_override = codec_support(config.codec);
    sub.scaled_target = Some((config.width, config.height));
    sub.allow_adaptive_scale = true;
    sub.max_fps = Some(f32::from(config.max_fps.max(1)));
    sub.max_inflight_frames = Some(usize::from(config.decoder_capacity.max(1)));
    sub.encoder_invalidated |= sub.encode_in_flight || sub.creation_in_flight;
    sub.nal_none_streak = 0;
    sub.nal_none_latched_at = None;
    sub.create_failures = 0;
    sub.burst_remaining = SURFACE_BURST_FRAMES;
    request_surface_keyframe(sub, Instant::now(), true);
    forget_surface_inflight(client, surface_id);
    if let Some(compositor) = session.compositor.as_mut() {
        compositor.frame_clocks_dirty = true;
    }
    session.sync_compositor_refresh_rate();
    drop(session);
    state.delivery_notify.notify_one();
    true
}

pub(crate) async fn reset(state: &AppState, client_id: u64, surface_id: u16) -> bool {
    let mut session = state.session.lock().await;
    let Some(client) = session.clients.get_mut(&client_id) else {
        return false;
    };
    let Some(sub) = client.surface_subs.get_mut(&surface_id) else {
        return false;
    };
    sub.burst_remaining = SURFACE_BURST_FRAMES;
    request_surface_keyframe(sub, Instant::now(), true);
    forget_surface_inflight(client, surface_id);
    drop(session);
    state.delivery_notify.notify_one();
    true
}

pub(crate) async fn acknowledge(
    state: &AppState,
    client_id: u64,
    surface_id: u16,
    count: u64,
    decoder_queue_depth: u16,
) -> bool {
    let mut session = state.session.lock().await;
    let Some(client) = session.clients.get_mut(&client_id) else {
        return false;
    };
    let depth = u8::try_from(decoder_queue_depth).unwrap_or(u8::MAX);
    if let Some(sub) = client.surface_subs.get_mut(&surface_id) {
        update_surface_decoder_queue(sub, depth, Instant::now());
    }
    for _ in 0..count.min(SURFACE_INFLIGHT_HARD_MAX as u64) {
        client.acks_recv = client.acks_recv.saturating_add(1);
        record_surface_ack(client, surface_id);
    }
    true
}

pub(crate) async fn discard_frame(state: &AppState, client_id: u64, surface_id: u16) {
    let mut session = state.session.lock().await;
    if let Some(client) = session.clients.get_mut(&client_id) {
        discard_surface_frame(client, surface_id);
    }
    drop(session);
    state.delivery_notify.notify_one();
}

pub(crate) async fn remove(state: &AppState, client_id: u64) {
    let mut session = state.session.lock().await;
    let Some(client) = session.clients.remove(&client_id) else {
        return;
    };
    session.clear_surface_pointer_owner(client_id);
    let targets = client
        .surface_subs
        .iter()
        .filter_map(|(&surface_id, sub)| {
            sub.last_registered_target
                .map(|target| (surface_id, target))
        })
        .collect::<Vec<_>>();
    for (surface_id, (width, height)) in targets {
        session.resettle_downscale_target(surface_id, width, height);
    }
    if let Some(compositor) = session.compositor.as_mut() {
        compositor.frame_clocks_dirty = true;
        for surface_id in client.vulkan_video_surfaces.keys().copied() {
            compositor.last_encoded.remove(&(surface_id, client_id));
            let _ = compositor
                .handle
                .command_tx
                .send(CompositorCommand::DestroyVulkanEncoder {
                    surface_id: u32::from(surface_id),
                    client_id: Some(client_id),
                });
        }
        if !client.pressed_surface_keys.is_empty() {
            let _ = compositor
                .handle
                .command_tx
                .send(CompositorCommand::ReleaseKeys {
                    keycodes: client.pressed_surface_keys.iter().copied().collect(),
                });
        }
        compositor.handle.wake();
    }
    session.sync_compositor_refresh_rate();
    session.sync_touch_capability();
    let affected = session.mediated_surface_ids();
    let resized = session.resize_surfaces_to_mediated_sizes(
        affected,
        &state.config.surface_encoders,
        state.config.verbose,
    );
    drop(session);
    if resized {
        state.delivery_notify.notify_one();
    }
}

pub(crate) async fn input(state: &AppState, client_id: u64, surface_id: u16, input: Input) -> bool {
    let mut session = state.session.lock().await;
    if session
        .clients
        .get(&client_id)
        .is_none_or(|client| !client.surface_subscriptions.contains(&surface_id))
    {
        return false;
    }
    let mut commands = Vec::new();
    match input {
        Input::Key {
            keycode,
            pressed,
            time_ms,
        } => {
            if let Some(client) = session.clients.get_mut(&client_id) {
                if pressed {
                    client.pressed_surface_keys.insert(keycode);
                } else {
                    client.pressed_surface_keys.remove(&keycode);
                }
            }
            commands.push(CompositorCommand::KeyInput {
                surface_id,
                keycode,
                pressed,
                time_ms,
            });
        }
        Input::Text(text) => commands.push(CompositorCommand::TextInput { text }),
        Input::Preedit { text, cursor } => {
            commands.push(CompositorCommand::Preedit { text, cursor })
        }
        Input::Pointer {
            phase,
            button,
            x,
            y,
            time_ms,
        } => {
            // REMOTE_INPUT still mirrors compositor-frame pixels. Derive that
            // presentation-only copy from the server's current catalogue; the
            // actual input command stays normalized until the compositor can
            // expand it against its exact live mapping.
            let mirrored = session
                .compositor
                .as_ref()
                .and_then(|compositor| compositor.surfaces.get(&surface_id))
                .map(|surface| {
                    let pixel = |fraction: f64, extent: u16| {
                        (fraction.clamp(0.0, 1.0) * f64::from(extent))
                            .floor()
                            .clamp(0.0, f64::from(extent.saturating_sub(1)))
                            as u16
                    };
                    (pixel(x, surface.width), pixel(y, surface.height))
                })
                .unwrap_or((0, 0));
            match phase {
                PointerPhase::Move
                | PointerPhase::Enter
                | PointerPhase::Down
                | PointerPhase::Up => {
                    session.update_surface_pointer(client_id, surface_id, mirrored.0, mirrored.1);
                }
                PointerPhase::Leave => session.clear_surface_pointer_owner(client_id),
            }
            match phase {
                PointerPhase::Down | PointerPhase::Up => {
                    commands.push(CompositorCommand::NormalizedPointerButtonAt {
                        surface_id,
                        x,
                        y,
                        button: evdev_button(button),
                        pressed: matches!(phase, PointerPhase::Down),
                        time_ms,
                    });
                }
                PointerPhase::Move | PointerPhase::Enter => {
                    commands.push(CompositorCommand::NormalizedPointerMotion {
                        surface_id,
                        x,
                        y,
                        time_ms,
                    });
                }
                PointerPhase::Leave => {}
            }
        }
        Input::Axis {
            dx,
            dy,
            v120_x,
            v120_y,
            source,
            stop,
            time_ms,
        } => commands.push(CompositorCommand::PointerAxis {
            surface_id,
            dx,
            dy,
            v120_x,
            v120_y,
            source: Some(source),
            stop,
            time_ms,
        }),
        Input::Touch {
            phase,
            time_ms,
            contacts,
        } => commands.extend(apply_touch(
            &mut session,
            client_id,
            surface_id,
            phase,
            time_ms,
            contacts,
        )),
    }
    if !commands.is_empty() {
        let Some(compositor) = session.compositor.as_mut() else {
            return false;
        };
        for command in commands {
            let reliable = matches!(
                &command,
                CompositorCommand::PointerButtonAt { .. }
                    | CompositorCommand::NormalizedPointerButtonAt { .. }
            );
            let failed = if reliable {
                // A click is discrete state, not a coalescible motion sample.
                // Wait for one bounded-queue slot instead of silently losing
                // the press or release under a busy 120 Hz compositor.
                compositor.handle.command_tx.send(command).is_err()
            } else {
                compositor.handle.command_tx.try_send(command).is_err()
            };
            if failed {
                return false;
            }
        }
        compositor.handle.wake();
    }
    drop(session);
    state.delivery_notify.notify_one();
    true
}

pub(crate) async fn resize(
    state: &AppState,
    owner: [u8; 16],
    surface_id: u16,
    width: u16,
    height: u16,
    scale_120: u16,
) -> bool {
    let mut session = state.session.lock().await;
    let known = session
        .compositor
        .as_ref()
        .is_some_and(|compositor| compositor.surfaces.contains_key(&surface_id));
    if known {
        session.set_native_surface_claim(
            owner,
            surface_id,
            (width, height, scale_120),
            &state.config.surface_encoders,
            state.config.verbose,
        );
    }
    drop(session);
    if known {
        state.delivery_notify.notify_one();
    }
    known
}

pub(crate) async fn release_claims(state: &AppState, owner: [u8; 16]) {
    let mut session = state.session.lock().await;
    let changed = session.remove_native_surface_claims(
        owner,
        &state.config.surface_encoders,
        state.config.verbose,
    );
    drop(session);
    if changed {
        state.delivery_notify.notify_one();
    }
}

pub(crate) async fn focus(state: &AppState, surface_id: u16) -> bool {
    compositor_command(state, surface_id, |surface_id| {
        CompositorCommand::SurfaceFocus { surface_id }
    })
    .await
}

pub(crate) async fn close(state: &AppState, surface_id: u16) -> bool {
    compositor_command(state, surface_id, |surface_id| {
        CompositorCommand::SurfaceClose { surface_id }
    })
    .await
}

async fn compositor_command(
    state: &AppState,
    surface_id: u16,
    command: impl FnOnce(u16) -> CompositorCommand,
) -> bool {
    let session = state.session.lock().await;
    let Some(compositor) = session.compositor.as_ref() else {
        return false;
    };
    if !compositor.surfaces.contains_key(&surface_id) {
        return false;
    }
    if compositor
        .handle
        .command_tx
        .send(command(surface_id))
        .is_err()
    {
        return false;
    }
    compositor.handle.wake();
    drop(session);
    state.delivery_notify.notify_one();
    true
}

pub(crate) fn enqueue_frame(client: &ClientState, frame: EncodedFrame) -> Result<usize, ()> {
    let Some(sink) = &client.native_surface else {
        return Err(());
    };
    sink.send_frame(frame)
}

pub(crate) fn enqueue_remote_input(
    client: &ClientState,
    surface_id: u16,
    seat_handle: u64,
    kind: RemoteInputKind,
    points: &[(u16, u16)],
) -> Result<(), ()> {
    let Some(sink) = &client.native_surface else {
        return Err(());
    };
    sink.send_remote_input(surface_id, seat_handle, kind, points)
}
