//! Process-global named-channel registry and semantic message routing.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use tokio::sync::{mpsc, watch};

const DEFAULT_MAX_LISTEN_PER_CLIENT: usize = 64;
const DEFAULT_MAX_LISTENERS: usize = 1024;
const DEFAULT_MAX_PER_CLIENT: usize = 64;
const DEFAULT_MAX_CONNECTED: usize = 128;
const DEFAULT_BUFFER_MAX: u64 = 256 * 1024 * 1024;
const FLOW_WINDOW_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
struct Listener {
    listener_handle: u64,
    registry_id: u32,
    generation: u64,
    endpoint: u64,
    name: String,
    metadata: Vec<u8>,
    token: [u8; 16],
    owner_session: [u8; 16],
    owner_extension: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeListener {
    pub listener_handle: u64,
    pub generation: u64,
    pub owner_session: [u8; 16],
    pub owner_extension: bool,
    pub name: String,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeCatalogue {
    pub revision: u64,
    pub listeners: Vec<NativeListener>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeError {
    Unavailable,
    Conflict,
    NotFound,
    Stale,
    ResourceExhausted,
    Backpressured,
}

/// Keeps one side's pair and endpoint admission charged while a routed event
/// is queued or being handled.
#[derive(Clone, Debug)]
pub(crate) struct NativePairSide {
    _reservation: PairReservation,
    _slot: HandleSlotReservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeMessageFragment {
    pub sequence: u64,
    pub fragment_offset: u64,
    pub start: bool,
    pub end: bool,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum NativeEvent {
    IncomingConnect {
        connect_id: u64,
        listener_handle: u64,
        generation: u64,
        connector_session: [u8; 16],
        connector_metadata: Vec<u8>,
        connector_initial_credit: u64,
        listener_metadata: Vec<u8>,
    },
    ConnectPrepared {
        connect_id: u64,
        listener_send_credit: u64,
        max_item_bytes: u64,
    },
    ConnectCommitted {
        connect_id: u64,
        connector_channel_handle: u64,
        initial_credit: u64,
        max_item_bytes: u64,
    },
    ConnectAborted {
        connect_id: u64,
        reason: NativeError,
        _guard: NativePairSide,
    },
    Message {
        channel_handle: u64,
        fragment: NativeMessageFragment,
        _guard: NativePairSide,
    },
    Credit {
        channel_handle: u64,
        cumulative_byte_limit: u64,
        _guard: NativePairSide,
    },
    Close {
        channel_handle: u64,
        final_data_bytes: u64,
        _guard: NativePairSide,
    },
    Reset {
        channel_handle: u64,
        reason: NativeError,
        _guard: NativePairSide,
    },
}

/// Resource-retiring events use a separate unbounded lane. Each pending
/// connect or live link can enqueue only a bounded number of these events, so
/// the lane cannot grow with Channel payload traffic. Keeping it independent
/// from the bounded data lane guarantees that backpressure cannot strand a
/// peer's provisional transfer or credit lease.
#[derive(Debug)]
pub(crate) enum NativeTerminalEvent {
    ConnectAborted {
        connect_id: u64,
        reason: NativeError,
        _guard: NativePairSide,
    },
    Reset {
        channel_handle: u64,
        reason: NativeError,
        _guard: NativePairSide,
    },
}

impl From<NativeTerminalEvent> for NativeEvent {
    fn from(event: NativeTerminalEvent) -> Self {
        match event {
            NativeTerminalEvent::ConnectAborted {
                connect_id,
                reason,
                _guard,
            } => Self::ConnectAborted {
                connect_id,
                reason,
                _guard,
            },
            NativeTerminalEvent::Reset {
                channel_handle,
                reason,
                _guard,
            } => Self::Reset {
                channel_handle,
                reason,
                _guard,
            },
        }
    }
}

#[derive(Clone)]
struct NativeEndpoint {
    session_id: [u8; 16],
    owner_extension: bool,
    events: mpsc::Sender<NativeEvent>,
    terminal_events: mpsc::UnboundedSender<NativeTerminalEvent>,
}

struct NativePendingConnect {
    connector_endpoint: u64,
    listener_endpoint: u64,
    listener_handle: u64,
    listener_metadata: Vec<u8>,
    listener_channel_handle: Option<u64>,
    max_item_bytes: Option<u64>,
    connector_side: NativePairSide,
    listener_side: NativePairSide,
}

#[derive(Clone, Debug)]
struct NativeLinkEnd {
    connect_id: u64,
    peer_endpoint: u64,
    peer_channel_handle: u64,
    max_item_bytes: u64,
    side: NativePairSide,
    published: bool,
    output_closed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeConnectInfo {
    pub peer_session: [u8; 16],
    pub listener_metadata: Vec<u8>,
    pub peer_channel_handle: u64,
    pub max_item_bytes: u64,
}

/// Immutable listener identity used to fence extension command publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListenerSnapshot {
    pub endpoint: u64,
    pub registry_id: u32,
    pub generation: u64,
    pub name: String,
    pub token: [u8; 16],
}

#[derive(Clone, Debug)]
struct PairReservation {
    _inner: Arc<PairReservationInner>,
}

#[derive(Debug)]
struct PairReservationInner {
    active_pairs: Arc<AtomicUsize>,
    reserved_window_bytes: Arc<AtomicU64>,
    window_bytes: u64,
}

impl Drop for PairReservationInner {
    fn drop(&mut self) {
        let pairs = self.active_pairs.fetch_sub(1, Ordering::Relaxed);
        let bytes = self
            .reserved_window_bytes
            .fetch_sub(self.window_bytes, Ordering::Relaxed);
        debug_assert!(pairs > 0);
        debug_assert!(bytes >= self.window_bytes);
    }
}

#[derive(Clone, Debug)]
struct HandleSlotReservation {
    _inner: Arc<HandleSlotReservationInner>,
}

#[derive(Debug)]
struct HandleSlotReservationInner {
    endpoint_slots: Arc<Mutex<HashMap<u64, usize>>>,
    endpoint: u64,
}

impl Drop for HandleSlotReservationInner {
    fn drop(&mut self) {
        let mut slots = self
            .endpoint_slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(count) = slots.get_mut(&self.endpoint) else {
            return;
        };
        debug_assert!(*count > 0);
        *count = count.saturating_sub(1);
        if *count == 0 {
            slots.remove(&self.endpoint);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Limits {
    listeners_per_endpoint: usize,
    listeners: usize,
    handles_per_endpoint: usize,
    connected_pairs: usize,
    buffer_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            listeners_per_endpoint: DEFAULT_MAX_LISTEN_PER_CLIENT,
            listeners: DEFAULT_MAX_LISTENERS,
            handles_per_endpoint: DEFAULT_MAX_PER_CLIENT,
            connected_pairs: DEFAULT_MAX_CONNECTED,
            buffer_bytes: DEFAULT_BUFFER_MAX,
        }
    }
}

impl Limits {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            listeners_per_endpoint: crate::deployment_usize(
                "YAS_CHANNEL_MAX_LISTEN_PER_CLIENT",
                defaults.listeners_per_endpoint,
            ),
            listeners: crate::deployment_usize("YAS_CHANNEL_MAX_LISTENERS", defaults.listeners),
            handles_per_endpoint: crate::deployment_usize(
                "YAS_CHANNEL_MAX_PER_CLIENT",
                defaults.handles_per_endpoint,
            ),
            connected_pairs: crate::deployment_usize(
                "YAS_CHANNEL_MAX_CONNECTED",
                defaults.connected_pairs,
            ),
            buffer_bytes: crate::deployment_u64("YAS_CHANNEL_BUFFER_MAX", defaults.buffer_bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PairAdmissionError {
    Connected,
    Window,
}

/// Named listeners and connected pairs shared by every native YAS endpoint.
pub(crate) struct ChannelFabric {
    enabled: bool,
    shutting_down: bool,
    boot_generation: u64,
    next_listener_generation: u64,
    next_listener_handle: u64,
    next_native_endpoint: u64,
    next_native_connect: u64,
    next_registry_id: HashMap<u64, u32>,
    listener_names: HashMap<String, u64>,
    listeners: HashMap<u64, Listener>,
    native_endpoints: HashMap<u64, NativeEndpoint>,
    native_pending: HashMap<u64, NativePendingConnect>,
    native_links: HashMap<(u64, u64), NativeLinkEnd>,
    native_revision: u64,
    native_revision_tx: watch::Sender<u64>,
    endpoint_slots: Arc<Mutex<HashMap<u64, usize>>>,
    active_pairs: Arc<AtomicUsize>,
    reserved_window_bytes: Arc<AtomicU64>,
    limits: Limits,
}

impl ChannelFabric {
    pub(crate) fn new(boot_generation: u64) -> Self {
        let (native_revision_tx, _) = watch::channel(1);
        Self {
            enabled: crate::channels_enabled(),
            shutting_down: false,
            boot_generation,
            next_listener_generation: 1,
            next_listener_handle: 1,
            next_native_endpoint: 1u64 << 63,
            next_native_connect: 1,
            next_registry_id: HashMap::new(),
            listener_names: HashMap::new(),
            listeners: HashMap::new(),
            native_endpoints: HashMap::new(),
            native_pending: HashMap::new(),
            native_links: HashMap::new(),
            native_revision: 1,
            native_revision_tx,
            endpoint_slots: Arc::new(Mutex::new(HashMap::new())),
            active_pairs: Arc::new(AtomicUsize::new(0)),
            reserved_window_bytes: Arc::new(AtomicU64::new(0)),
            limits: Limits::from_env(),
        }
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.shutting_down = true;
    }

    pub(crate) fn register_native_endpoint(
        &mut self,
        session_id: [u8; 16],
        owner_extension: bool,
        events: mpsc::Sender<NativeEvent>,
        terminal_events: mpsc::UnboundedSender<NativeTerminalEvent>,
    ) -> Result<(u64, watch::Receiver<u64>), NativeError> {
        if session_id.iter().all(|byte| *byte == 0) {
            return Err(NativeError::ResourceExhausted);
        }
        let endpoint = self.next_native_endpoint;
        self.next_native_endpoint = endpoint
            .checked_add(1)
            .ok_or(NativeError::ResourceExhausted)?;
        let old = self.native_endpoints.insert(
            endpoint,
            NativeEndpoint {
                session_id,
                owner_extension,
                events,
                terminal_events,
            },
        );
        debug_assert!(old.is_none(), "native Channel endpoint reused");
        Ok((endpoint, self.native_revision_tx.subscribe()))
    }

    pub(crate) fn native_catalogue(&self) -> NativeCatalogue {
        let mut listeners = self
            .listeners
            .values()
            .map(|listener| NativeListener {
                listener_handle: listener.listener_handle,
                generation: listener.generation,
                owner_session: listener.owner_session,
                owner_extension: listener.owner_extension,
                name: listener.name.clone(),
                metadata: listener.metadata.clone(),
            })
            .collect::<Vec<_>>();
        listeners.sort_by_key(|listener| listener.listener_handle);
        NativeCatalogue {
            revision: self.native_revision,
            listeners,
        }
    }

    pub(crate) fn native_revision_receiver(&self) -> watch::Receiver<u64> {
        self.native_revision_tx.subscribe()
    }

    pub(crate) fn native_listener_snapshot(
        &self,
        endpoint: u64,
        listener_handle: u64,
        generation: u64,
    ) -> Option<ListenerSnapshot> {
        let listener = self.listeners.get(&listener_handle)?;
        (listener.endpoint == endpoint && listener.generation == generation).then(|| {
            ListenerSnapshot {
                endpoint,
                registry_id: listener.registry_id,
                generation,
                name: listener.name.clone(),
                token: listener.token,
            }
        })
    }

    pub(crate) fn native_listener_snapshots(&self, endpoint: u64) -> Vec<ListenerSnapshot> {
        self.listeners
            .values()
            .filter(|listener| listener.endpoint == endpoint)
            .map(|listener| ListenerSnapshot {
                endpoint,
                registry_id: listener.registry_id,
                generation: listener.generation,
                name: listener.name.clone(),
                token: listener.token,
            })
            .collect()
    }

    pub(crate) fn native_listen(
        &mut self,
        endpoint: u64,
        session_id: [u8; 16],
        name: &str,
        metadata: &[u8],
    ) -> Result<NativeListener, NativeError> {
        if !self.enabled || self.shutting_down {
            return Err(NativeError::Unavailable);
        }
        let Some(owner) = self.native_endpoints.get(&endpoint) else {
            return Err(NativeError::NotFound);
        };
        if owner.session_id != session_id {
            return Err(NativeError::NotFound);
        }
        if !valid_name(name) || metadata.len() > yas_wire::channel::MAX_METADATA_BYTES {
            return Err(NativeError::ResourceExhausted);
        }
        if self.listener_names.contains_key(name) {
            return Err(NativeError::Conflict);
        }
        let endpoint_limit = self
            .limits
            .listeners_per_endpoint
            .min(yas_wire::channel::MAX_LISTENERS_PER_SESSION as usize);
        if self.listeners.len() >= self.limits.listeners
            || self.listener_count(endpoint) >= endpoint_limit
        {
            return Err(NativeError::ResourceExhausted);
        }

        let listener_handle = self.next_listener_handle;
        let generation = self.next_listener_generation;
        let registry_id = self.registry_id_candidate(endpoint)?;
        let next_listener_handle = listener_handle
            .checked_add(1)
            .ok_or(NativeError::ResourceExhausted)?;
        let next_generation = generation
            .checked_add(1)
            .ok_or(NativeError::ResourceExhausted)?;
        let next_registry_id = registry_id
            .checked_add(2)
            .ok_or(NativeError::ResourceExhausted)?;

        let mut token = [0; 16];
        token[..8].copy_from_slice(&self.boot_generation.to_le_bytes());
        token[8..].copy_from_slice(&generation.to_le_bytes());
        let listener = Listener {
            listener_handle,
            registry_id,
            generation,
            endpoint,
            name: name.to_owned(),
            metadata: metadata.to_vec(),
            token,
            owner_session: session_id,
            owner_extension: owner.owner_extension,
        };
        let native = NativeListener {
            listener_handle,
            generation,
            owner_session: session_id,
            owner_extension: owner.owner_extension,
            name: name.to_owned(),
            metadata: metadata.to_vec(),
        };
        self.next_listener_handle = next_listener_handle;
        self.next_listener_generation = next_generation;
        self.next_registry_id.insert(endpoint, next_registry_id);
        self.listener_names.insert(name.to_owned(), listener_handle);
        self.listeners.insert(listener_handle, listener);
        self.bump_revision();
        Ok(native)
    }

    pub(crate) fn native_close_listener(
        &mut self,
        endpoint: u64,
        listener_handle: u64,
        generation: u64,
    ) -> Result<(), NativeError> {
        let listener = self
            .listeners
            .get(&listener_handle)
            .ok_or(NativeError::NotFound)?;
        if listener.endpoint != endpoint {
            return Err(NativeError::NotFound);
        }
        if listener.generation != generation {
            return Err(NativeError::Stale);
        }
        let pending = self
            .native_pending
            .iter()
            .filter_map(|(connect_id, pending)| {
                (pending.listener_handle == listener_handle).then_some(*connect_id)
            })
            .collect::<Vec<_>>();
        self.remove_listener(listener_handle);
        for connect_id in pending {
            self.native_cancel_connect(endpoint, connect_id, NativeError::NotFound);
        }
        Ok(())
    }

    pub(crate) fn native_begin_connect(
        &mut self,
        connector_endpoint: u64,
        listener_handle: u64,
        generation: u64,
        connector_initial_credit: u64,
        connector_metadata: Vec<u8>,
    ) -> Result<u64, NativeError> {
        if !self.enabled || self.shutting_down {
            return Err(NativeError::Unavailable);
        }
        if connector_metadata.len() > yas_wire::channel::MAX_METADATA_BYTES {
            return Err(NativeError::ResourceExhausted);
        }
        let listener = self
            .listeners
            .get(&listener_handle)
            .cloned()
            .ok_or(NativeError::NotFound)?;
        if listener.generation != generation {
            return Err(NativeError::Stale);
        }
        let connector = self
            .native_endpoints
            .get(&connector_endpoint)
            .cloned()
            .ok_or(NativeError::NotFound)?;
        let acceptor = self
            .native_endpoints
            .get(&listener.endpoint)
            .cloned()
            .ok_or(NativeError::Unavailable)?;
        let pending_count = self
            .native_pending
            .values()
            .filter(|pending| pending.connector_endpoint == connector_endpoint)
            .count();
        if pending_count >= yas_wire::channel::MAX_PENDING_CONNECTS as usize {
            return Err(NativeError::ResourceExhausted);
        }
        let (reservation, connector_slot, listener_slot) = self
            .reserve_pair(connector_endpoint, listener.endpoint)
            .map_err(|_| NativeError::ResourceExhausted)?;
        let connect_id = self.next_native_connect;
        self.next_native_connect = connect_id
            .checked_add(1)
            .ok_or(NativeError::ResourceExhausted)?;
        let pending = NativePendingConnect {
            connector_endpoint,
            listener_endpoint: listener.endpoint,
            listener_handle,
            listener_metadata: listener.metadata.clone(),
            listener_channel_handle: None,
            max_item_bytes: None,
            connector_side: NativePairSide {
                _reservation: reservation.clone(),
                _slot: connector_slot,
            },
            listener_side: NativePairSide {
                _reservation: reservation,
                _slot: listener_slot,
            },
        };
        acceptor
            .events
            .try_send(NativeEvent::IncomingConnect {
                connect_id,
                listener_handle,
                generation,
                connector_session: connector.session_id,
                connector_metadata,
                connector_initial_credit,
                listener_metadata: listener.metadata,
            })
            .map_err(|_| NativeError::Backpressured)?;
        self.native_pending.insert(connect_id, pending);
        Ok(connect_id)
    }

    pub(crate) fn native_prepare_connect(
        &mut self,
        listener_endpoint: u64,
        connect_id: u64,
        listener_channel_handle: u64,
        listener_send_credit: u64,
        max_item_bytes: u64,
    ) -> Result<(), NativeError> {
        let pending = self
            .native_pending
            .get(&connect_id)
            .ok_or(NativeError::NotFound)?;
        if pending.listener_endpoint != listener_endpoint {
            return Err(NativeError::NotFound);
        }
        if pending.listener_channel_handle.is_some()
            || self
                .native_links
                .contains_key(&(listener_endpoint, listener_channel_handle))
        {
            return Err(NativeError::Conflict);
        }
        if max_item_bytes == 0
            || max_item_bytes > listener_send_credit
            || max_item_bytes > yas_wire::channel::MAX_MESSAGE_BYTES
        {
            return Err(NativeError::ResourceExhausted);
        }
        let connector = self
            .native_endpoints
            .get(&pending.connector_endpoint)
            .ok_or(NativeError::Unavailable)?;
        connector
            .events
            .try_send(NativeEvent::ConnectPrepared {
                connect_id,
                listener_send_credit,
                max_item_bytes,
            })
            .map_err(|_| NativeError::Backpressured)?;
        let pending = self
            .native_pending
            .get_mut(&connect_id)
            .expect("pending connect remained live");
        pending.listener_channel_handle = Some(listener_channel_handle);
        pending.max_item_bytes = Some(max_item_bytes);
        Ok(())
    }

    pub(crate) fn native_complete_connect(
        &mut self,
        connector_endpoint: u64,
        connect_id: u64,
        connector_channel_handle: u64,
        max_item_bytes: u64,
    ) -> Result<NativeConnectInfo, NativeError> {
        let pending = self
            .native_pending
            .get(&connect_id)
            .ok_or(NativeError::NotFound)?;
        if pending.connector_endpoint != connector_endpoint {
            return Err(NativeError::NotFound);
        }
        let listener_channel_handle = pending
            .listener_channel_handle
            .ok_or(NativeError::Conflict)?;
        let prepared_max_item_bytes = pending.max_item_bytes.ok_or(NativeError::Conflict)?;
        if max_item_bytes == 0 || max_item_bytes > prepared_max_item_bytes {
            return Err(NativeError::ResourceExhausted);
        }
        if self
            .native_links
            .contains_key(&(connector_endpoint, connector_channel_handle))
            || self
                .native_links
                .contains_key(&(pending.listener_endpoint, listener_channel_handle))
            || (connector_endpoint == pending.listener_endpoint
                && connector_channel_handle == listener_channel_handle)
        {
            return Err(NativeError::Conflict);
        }
        let listener = self
            .native_endpoints
            .get(&pending.listener_endpoint)
            .cloned()
            .ok_or(NativeError::Unavailable)?;
        let pending = self
            .native_pending
            .remove(&connect_id)
            .expect("validated pending connect remained live");
        self.native_links.insert(
            (connector_endpoint, connector_channel_handle),
            NativeLinkEnd {
                connect_id,
                peer_endpoint: pending.listener_endpoint,
                peer_channel_handle: listener_channel_handle,
                max_item_bytes,
                side: pending.connector_side,
                published: false,
                output_closed: false,
            },
        );
        self.native_links.insert(
            (pending.listener_endpoint, listener_channel_handle),
            NativeLinkEnd {
                connect_id,
                peer_endpoint: connector_endpoint,
                peer_channel_handle: connector_channel_handle,
                max_item_bytes,
                side: pending.listener_side,
                published: false,
                output_closed: false,
            },
        );
        Ok(NativeConnectInfo {
            peer_session: listener.session_id,
            listener_metadata: pending.listener_metadata,
            peer_channel_handle: listener_channel_handle,
            max_item_bytes,
        })
    }

    /// Publish the accepted side only after the connector's correlated result
    /// has been written to its reliable transport.
    pub(crate) fn native_publish_connect(
        &mut self,
        connector_endpoint: u64,
        connect_id: u64,
        connector_channel_handle: u64,
        initial_credit: u64,
    ) -> Result<(), NativeError> {
        let link = self
            .native_links
            .get(&(connector_endpoint, connector_channel_handle))
            .cloned()
            .ok_or(NativeError::NotFound)?;
        if link.connect_id != connect_id || link.published {
            return Err(NativeError::Conflict);
        }
        if initial_credit < link.max_item_bytes {
            return Err(NativeError::ResourceExhausted);
        }
        let acceptor = self
            .native_endpoints
            .get(&link.peer_endpoint)
            .ok_or(NativeError::Unavailable)?;
        if acceptor
            .events
            .try_send(NativeEvent::ConnectCommitted {
                connect_id,
                connector_channel_handle,
                initial_credit,
                max_item_bytes: link.max_item_bytes,
            })
            .is_err()
        {
            let _ = self.native_reset(
                connector_endpoint,
                connector_channel_handle,
                NativeError::Backpressured,
            );
            return Err(NativeError::Backpressured);
        }
        let peer_key = (link.peer_endpoint, link.peer_channel_handle);
        self.native_links
            .get_mut(&(connector_endpoint, connector_channel_handle))
            .expect("native Channel connector link remained live")
            .published = true;
        self.native_links
            .get_mut(&peer_key)
            .expect("native Channel listener link remained live")
            .published = true;
        Ok(())
    }

    pub(crate) fn native_cancel_connect(
        &mut self,
        endpoint: u64,
        connect_id: u64,
        reason: NativeError,
    ) {
        let Some(pending) = self.native_pending.get(&connect_id) else {
            return;
        };
        if endpoint != pending.connector_endpoint && endpoint != pending.listener_endpoint {
            return;
        }
        let pending = self
            .native_pending
            .remove(&connect_id)
            .expect("validated pending connect remained live");
        // Both sides may already hold local provisional state. Send one event
        // per role even for a loopback connect, where the same endpoint must
        // clear both its connector request and accepted transfer.
        for (target_endpoint, guard) in [
            (pending.connector_endpoint, pending.connector_side),
            (pending.listener_endpoint, pending.listener_side),
        ] {
            if let Some(target) = self.native_endpoints.get(&target_endpoint) {
                let _ = target
                    .terminal_events
                    .send(NativeTerminalEvent::ConnectAborted {
                        connect_id,
                        reason,
                        _guard: guard,
                    });
            }
        }
    }

    pub(crate) fn native_send_message(
        &mut self,
        endpoint: u64,
        channel_handle: u64,
        fragment: NativeMessageFragment,
    ) -> Result<(), NativeError> {
        if fragment.data.len() as u64 > yas_wire::channel::MAX_MESSAGE_BYTES {
            return Err(NativeError::ResourceExhausted);
        }
        let link = self
            .native_links
            .get(&(endpoint, channel_handle))
            .cloned()
            .ok_or(NativeError::NotFound)?;
        if !link.published || link.output_closed {
            return Err(NativeError::Conflict);
        }
        let peer_side = self.peer_side(&link)?;
        self.native_endpoints
            .get(&link.peer_endpoint)
            .ok_or(NativeError::Unavailable)?
            .events
            .try_send(NativeEvent::Message {
                channel_handle: link.peer_channel_handle,
                fragment,
                _guard: peer_side,
            })
            .map_err(|_| NativeError::Backpressured)
    }

    pub(crate) fn native_send_credit(
        &mut self,
        endpoint: u64,
        channel_handle: u64,
        cumulative_byte_limit: u64,
    ) -> Result<(), NativeError> {
        let link = self
            .native_links
            .get(&(endpoint, channel_handle))
            .cloned()
            .ok_or(NativeError::NotFound)?;
        if !link.published {
            return Err(NativeError::Conflict);
        }
        let peer_side = self.peer_side(&link)?;
        self.native_endpoints
            .get(&link.peer_endpoint)
            .ok_or(NativeError::Unavailable)?
            .events
            .try_send(NativeEvent::Credit {
                channel_handle: link.peer_channel_handle,
                cumulative_byte_limit,
                _guard: peer_side,
            })
            .map_err(|_| NativeError::Backpressured)
    }

    pub(crate) fn native_send_close(
        &mut self,
        endpoint: u64,
        channel_handle: u64,
        final_data_bytes: u64,
    ) -> Result<(), NativeError> {
        let key = (endpoint, channel_handle);
        let link = self
            .native_links
            .get(&key)
            .cloned()
            .ok_or(NativeError::NotFound)?;
        if !link.published || link.output_closed {
            return Err(NativeError::Conflict);
        }
        let peer_key = (link.peer_endpoint, link.peer_channel_handle);
        let peer_side = self.peer_side(&link)?;
        self.native_endpoints
            .get(&link.peer_endpoint)
            .ok_or(NativeError::Unavailable)?
            .events
            .try_send(NativeEvent::Close {
                channel_handle: link.peer_channel_handle,
                final_data_bytes,
                _guard: peer_side,
            })
            .map_err(|_| NativeError::Backpressured)?;
        self.native_links
            .get_mut(&key)
            .expect("native Channel link remained live")
            .output_closed = true;
        if self
            .native_links
            .get(&peer_key)
            .is_some_and(|peer| peer.output_closed)
        {
            self.native_links.remove(&key);
            self.native_links.remove(&peer_key);
        }
        Ok(())
    }

    pub(crate) fn native_reset(
        &mut self,
        endpoint: u64,
        channel_handle: u64,
        reason: NativeError,
    ) -> Result<(), NativeError> {
        let key = (endpoint, channel_handle);
        let link = self
            .native_links
            .remove(&key)
            .ok_or(NativeError::NotFound)?;
        let peer_key = (link.peer_endpoint, link.peer_channel_handle);
        let peer = self.native_links.remove(&peer_key);
        if let (Some(target), Some(peer)) = (self.native_endpoints.get(&link.peer_endpoint), peer) {
            let _ = target.terminal_events.send(NativeTerminalEvent::Reset {
                channel_handle: link.peer_channel_handle,
                reason,
                _guard: peer.side,
            });
        }
        Ok(())
    }

    pub(crate) fn unregister_native_endpoint(&mut self, endpoint: u64) {
        let pending = self
            .native_pending
            .iter()
            .filter_map(|(connect_id, pending)| {
                (pending.connector_endpoint == endpoint || pending.listener_endpoint == endpoint)
                    .then_some(*connect_id)
            })
            .collect::<Vec<_>>();
        for connect_id in pending {
            self.native_cancel_connect(endpoint, connect_id, NativeError::Unavailable);
        }
        let channel_handles = self
            .native_links
            .keys()
            .filter_map(|(owner, handle)| (*owner == endpoint).then_some(*handle))
            .collect::<Vec<_>>();
        for channel_handle in channel_handles {
            let _ = self.native_reset(endpoint, channel_handle, NativeError::Unavailable);
        }
        self.native_endpoints.remove(&endpoint);
        let listeners = self
            .listeners
            .values()
            .filter_map(|listener| {
                (listener.endpoint == endpoint).then_some(listener.listener_handle)
            })
            .collect::<Vec<_>>();
        for listener_handle in listeners {
            self.remove_listener(listener_handle);
        }
        self.next_registry_id.remove(&endpoint);
    }

    fn peer_side(&self, link: &NativeLinkEnd) -> Result<NativePairSide, NativeError> {
        self.native_links
            .get(&(link.peer_endpoint, link.peer_channel_handle))
            .map(|peer| peer.side.clone())
            .ok_or(NativeError::NotFound)
    }

    fn listener_count(&self, endpoint: u64) -> usize {
        self.listeners
            .values()
            .filter(|listener| listener.endpoint == endpoint)
            .count()
    }

    fn registry_id_candidate(&self, endpoint: u64) -> Result<u32, NativeError> {
        let mut candidate = self.next_registry_id.get(&endpoint).copied().unwrap_or(1);
        loop {
            if !self
                .listeners
                .values()
                .any(|listener| listener.endpoint == endpoint && listener.registry_id == candidate)
            {
                return Ok(candidate);
            }
            candidate = candidate
                .checked_add(2)
                .ok_or(NativeError::ResourceExhausted)?;
        }
    }

    fn remove_listener(&mut self, listener_handle: u64) {
        if let Some(listener) = self.listeners.remove(&listener_handle) {
            self.listener_names.remove(&listener.name);
            self.bump_revision();
        }
    }

    fn bump_revision(&mut self) {
        self.native_revision = self.native_revision.wrapping_add(1).max(1);
        self.native_revision_tx.send_replace(self.native_revision);
    }

    fn reserve_pair(
        &self,
        connector_endpoint: u64,
        accepted_endpoint: u64,
    ) -> Result<
        (
            PairReservation,
            HandleSlotReservation,
            HandleSlotReservation,
        ),
        PairAdmissionError,
    > {
        let (connector_slot, accepted_slot) =
            self.reserve_handle_slots(connector_endpoint, accepted_endpoint)?;
        if self.active_pairs.load(Ordering::Relaxed) >= self.limits.connected_pairs {
            return Err(PairAdmissionError::Connected);
        }
        let pair_window = FLOW_WINDOW_BYTES.saturating_mul(2);
        let current = self.reserved_window_bytes.load(Ordering::Relaxed);
        let next = current
            .checked_add(pair_window)
            .ok_or(PairAdmissionError::Window)?;
        if next > self.limits.buffer_bytes {
            return Err(PairAdmissionError::Window);
        }
        self.active_pairs.fetch_add(1, Ordering::Relaxed);
        self.reserved_window_bytes
            .fetch_add(pair_window, Ordering::Relaxed);
        Ok((
            PairReservation {
                _inner: Arc::new(PairReservationInner {
                    active_pairs: Arc::clone(&self.active_pairs),
                    reserved_window_bytes: Arc::clone(&self.reserved_window_bytes),
                    window_bytes: pair_window,
                }),
            },
            connector_slot,
            accepted_slot,
        ))
    }

    fn reserve_handle_slots(
        &self,
        connector_endpoint: u64,
        accepted_endpoint: u64,
    ) -> Result<(HandleSlotReservation, HandleSlotReservation), PairAdmissionError> {
        let mut slots = self
            .endpoint_slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if connector_endpoint == accepted_endpoint {
            let current = slots.get(&connector_endpoint).copied().unwrap_or(0);
            let next = current
                .checked_add(2)
                .ok_or(PairAdmissionError::Connected)?;
            if next > self.limits.handles_per_endpoint {
                return Err(PairAdmissionError::Connected);
            }
            slots.insert(connector_endpoint, next);
        } else {
            let connector_count = slots.get(&connector_endpoint).copied().unwrap_or(0);
            let accepted_count = slots.get(&accepted_endpoint).copied().unwrap_or(0);
            if connector_count
                .checked_add(1)
                .is_none_or(|count| count > self.limits.handles_per_endpoint)
                || accepted_count
                    .checked_add(1)
                    .is_none_or(|count| count > self.limits.handles_per_endpoint)
            {
                return Err(PairAdmissionError::Connected);
            }
            slots.insert(connector_endpoint, connector_count + 1);
            slots.insert(accepted_endpoint, accepted_count + 1);
        }
        drop(slots);
        let reservation = |endpoint| HandleSlotReservation {
            _inner: Arc::new(HandleSlotReservationInner {
                endpoint_slots: Arc::clone(&self.endpoint_slots),
                endpoint,
            }),
        };
        Ok((
            reservation(connector_endpoint),
            reservation(accepted_endpoint),
        ))
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= yas_wire::channel::MAX_NAME_BYTES
        && !name.as_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEvents {
        events: mpsc::Receiver<NativeEvent>,
        terminal_events: mpsc::UnboundedReceiver<NativeTerminalEvent>,
    }

    impl TestEvents {
        async fn recv(&mut self) -> Option<NativeEvent> {
            tokio::select! {
                event = self.terminal_events.recv() => match event {
                    Some(event) => Some(NativeEvent::from(event)),
                    None => self.events.recv().await,
                },
                event = self.events.recv() => match event {
                    Some(event) => Some(event),
                    None => self.terminal_events.recv().await.map(NativeEvent::from),
                },
            }
        }
    }

    fn fabric() -> ChannelFabric {
        let mut fabric = ChannelFabric::new(0x1234);
        fabric.enabled = true;
        fabric
    }

    fn endpoint(
        fabric: &mut ChannelFabric,
        session_id: [u8; 16],
        owner_extension: bool,
    ) -> (u64, TestEvents) {
        let (tx, rx) = mpsc::channel(16);
        let (terminal_tx, terminal_rx) = mpsc::unbounded_channel();
        let (endpoint, _) = fabric
            .register_native_endpoint(session_id, owner_extension, tx, terminal_tx)
            .unwrap();
        (
            endpoint,
            TestEvents {
                events: rx,
                terminal_events: terminal_rx,
            },
        )
    }

    #[test]
    fn listener_catalogue_and_fences_are_semantic() {
        let mut fabric = fabric();
        let (endpoint, _events) = endpoint(&mut fabric, [1; 16], true);
        let mut revisions = fabric.native_revision_receiver();
        let listener = fabric
            .native_listen(endpoint, [1; 16], "commands.build", b"metadata")
            .unwrap();
        assert_eq!(*revisions.borrow_and_update(), 2);
        assert_eq!(fabric.native_catalogue().listeners, vec![listener.clone()]);
        let snapshot = fabric
            .native_listener_snapshot(endpoint, listener.listener_handle, listener.generation)
            .unwrap();
        assert_eq!(snapshot.registry_id, 1);
        assert_eq!(snapshot.name, "commands.build");
        assert_eq!(&snapshot.token[..8], &0x1234u64.to_le_bytes());
        assert!(
            fabric
                .native_listener_snapshot(endpoint, listener.listener_handle, 99)
                .is_none()
        );
        fabric
            .native_close_listener(endpoint, listener.listener_handle, listener.generation)
            .unwrap();
        assert_eq!(fabric.native_catalogue().revision, 3);
        assert!(fabric.native_catalogue().listeners.is_empty());
    }

    #[test]
    fn invalid_or_conflicting_listeners_do_not_move_revision() {
        let mut fabric = fabric();
        let (first, _events) = endpoint(&mut fabric, [1; 16], false);
        let (second, _events) = endpoint(&mut fabric, [2; 16], false);
        assert_eq!(
            fabric.native_listen(first, [2; 16], "wrong-owner", b""),
            Err(NativeError::NotFound)
        );
        assert_eq!(
            fabric.native_listen(first, [1; 16], "", b""),
            Err(NativeError::ResourceExhausted)
        );
        fabric.native_listen(first, [1; 16], "same", b"").unwrap();
        assert_eq!(
            fabric.native_listen(second, [2; 16], "same", b""),
            Err(NativeError::Conflict)
        );
        assert_eq!(fabric.native_catalogue().revision, 2);
    }

    #[tokio::test]
    async fn connect_routes_message_credit_and_ordered_close() {
        let mut fabric = fabric();
        let (connector, mut connector_events) = endpoint(&mut fabric, [1; 16], false);
        let (acceptor, mut acceptor_events) = endpoint(&mut fabric, [2; 16], true);
        let listener = fabric
            .native_listen(acceptor, [2; 16], "echo", b"listener")
            .unwrap();
        let connect_id = fabric
            .native_begin_connect(
                connector,
                listener.listener_handle,
                listener.generation,
                4096,
                b"connector".to_vec(),
            )
            .unwrap();
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::IncomingConnect {
                connect_id: observed,
                connector_session,
                ..
            }) if observed == connect_id && connector_session == [1; 16]
        ));
        fabric
            .native_prepare_connect(acceptor, connect_id, 20, 2048, 2048)
            .unwrap();
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::ConnectPrepared {
                connect_id: observed,
                listener_send_credit: 2048,
                max_item_bytes: 2048,
            }) if observed == connect_id
        ));
        let info = fabric
            .native_complete_connect(connector, connect_id, 10, 1024)
            .unwrap();
        assert_eq!(info.peer_session, [2; 16]);
        assert_eq!(info.listener_metadata, b"listener");
        assert_eq!(info.peer_channel_handle, 20);
        assert_eq!(info.max_item_bytes, 1024);
        assert_eq!(
            fabric.native_publish_connect(connector, connect_id + 1, 10, 1024),
            Err(NativeError::Conflict)
        );
        assert_eq!(
            fabric.native_send_credit(connector, 10, 4096),
            Err(NativeError::Conflict)
        );
        assert_eq!(
            fabric.native_publish_connect(connector, connect_id, 10, 512),
            Err(NativeError::ResourceExhausted)
        );
        fabric
            .native_publish_connect(connector, connect_id, 10, 1024)
            .unwrap();
        assert_eq!(
            fabric.native_publish_connect(connector, connect_id, 10, 1024),
            Err(NativeError::Conflict)
        );
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::ConnectCommitted {
                connect_id: observed,
                connector_channel_handle: 10,
                initial_credit: 1024,
                max_item_bytes: 1024,
            }) if observed == connect_id
        ));

        let fragment = NativeMessageFragment {
            sequence: 3,
            fragment_offset: 0,
            start: true,
            end: true,
            data: b"hello".to_vec(),
        };
        fabric
            .native_send_message(connector, 10, fragment.clone())
            .unwrap();
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::Message {
                channel_handle: 20,
                fragment: observed,
                ..
            }) if observed == fragment
        ));
        fabric.native_send_credit(acceptor, 20, 8192).unwrap();
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::Credit {
                channel_handle: 10,
                cumulative_byte_limit: 8192,
                ..
            })
        ));
        fabric.native_send_close(connector, 10, 5).unwrap();
        assert_eq!(
            fabric.native_send_message(connector, 10, fragment),
            Err(NativeError::Conflict)
        );
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::Close {
                channel_handle: 20,
                final_data_bytes: 5,
                ..
            })
        ));
        fabric.native_send_close(acceptor, 20, 0).unwrap();
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::Close {
                channel_handle: 10,
                final_data_bytes: 0,
                ..
            })
        ));
        assert!(fabric.native_links.is_empty());
        assert_eq!(fabric.active_pairs.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn publication_backpressure_uses_terminal_lane_and_releases_pair() {
        let mut fabric = fabric();
        let (connector, mut connector_events) = endpoint(&mut fabric, [1; 16], false);
        let (acceptor_tx, mut acceptor_events) = mpsc::channel(1);
        let (acceptor_terminal_tx, mut acceptor_terminal_events) = mpsc::unbounded_channel();
        let (acceptor, _) = fabric
            .register_native_endpoint([2; 16], false, acceptor_tx.clone(), acceptor_terminal_tx)
            .unwrap();
        let listener = fabric
            .native_listen(acceptor, [2; 16], "full", b"")
            .unwrap();
        let connect_id = fabric
            .native_begin_connect(
                connector,
                listener.listener_handle,
                listener.generation,
                4096,
                Vec::new(),
            )
            .unwrap();
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::IncomingConnect {
                connect_id: observed,
                ..
            }) if observed == connect_id
        ));
        fabric
            .native_prepare_connect(acceptor, connect_id, 20, 4096, 4096)
            .unwrap();
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::ConnectPrepared {
                connect_id: observed,
                ..
            }) if observed == connect_id
        ));
        fabric
            .native_complete_connect(connector, connect_id, 10, 4096)
            .unwrap();

        // Deterministically occupy the only ordinary event slot. Publication
        // must fail, remove both fabric links, and deliver RESET through the
        // independent terminal lane rather than dropping the cleanup signal.
        acceptor_tx
            .try_send(NativeEvent::ConnectPrepared {
                connect_id: u64::MAX,
                listener_send_credit: 1,
                max_item_bytes: 1,
            })
            .unwrap();
        assert_eq!(
            fabric.native_publish_connect(connector, connect_id, 10, 4096),
            Err(NativeError::Backpressured),
        );
        assert!(fabric.native_links.is_empty());
        assert!(matches!(
            acceptor_events.try_recv(),
            Ok(NativeEvent::ConnectPrepared {
                connect_id: u64::MAX,
                ..
            })
        ));
        let terminal = acceptor_terminal_events.recv().await.unwrap();
        assert!(matches!(
            &terminal,
            NativeTerminalEvent::Reset {
                channel_handle: 20,
                reason: NativeError::Backpressured,
                ..
            }
        ));
        drop(terminal);
        assert_eq!(fabric.active_pairs.load(Ordering::Relaxed), 0);
        assert!(
            fabric
                .endpoint_slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn endpoint_removal_cancels_pending_and_removes_listeners() {
        let mut fabric = fabric();
        let (connector, mut connector_events) = endpoint(&mut fabric, [1; 16], false);
        let (acceptor, mut acceptor_events) = endpoint(&mut fabric, [2; 16], false);
        let listener = fabric
            .native_listen(acceptor, [2; 16], "gone", b"")
            .unwrap();
        let connect_id = fabric
            .native_begin_connect(
                connector,
                listener.listener_handle,
                listener.generation,
                1,
                Vec::new(),
            )
            .unwrap();
        let _incoming = acceptor_events.recv().await.unwrap();
        fabric.unregister_native_endpoint(acceptor);
        assert!(fabric.native_catalogue().listeners.is_empty());
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::Unavailable,
                ..
            }) if observed == connect_id
        ));
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::Unavailable,
                ..
            }) if observed == connect_id
        ));
        assert_eq!(fabric.active_pairs.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn closing_listener_aborts_both_provisional_sides() {
        let mut fabric = fabric();
        let (connector, mut connector_events) = endpoint(&mut fabric, [1; 16], false);
        let (acceptor, mut acceptor_events) = endpoint(&mut fabric, [2; 16], false);
        let listener = fabric
            .native_listen(acceptor, [2; 16], "closing", b"")
            .unwrap();
        let connect_id = fabric
            .native_begin_connect(
                connector,
                listener.listener_handle,
                listener.generation,
                1,
                Vec::new(),
            )
            .unwrap();
        let _incoming = acceptor_events.recv().await.unwrap();
        fabric
            .native_prepare_connect(acceptor, connect_id, 7, 1, 1)
            .unwrap();
        let _prepared = connector_events.recv().await.unwrap();

        fabric
            .native_close_listener(acceptor, listener.listener_handle, listener.generation)
            .unwrap();
        assert!(matches!(
            connector_events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::NotFound,
                ..
            }) if observed == connect_id
        ));
        assert!(matches!(
            acceptor_events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::NotFound,
                ..
            }) if observed == connect_id
        ));
        assert!(fabric.native_pending.is_empty());
        assert_eq!(fabric.active_pairs.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn same_endpoint_pair_charges_two_handle_slots_atomically() {
        let mut fabric = fabric();
        fabric.limits.handles_per_endpoint = 2;
        let (endpoint, _events) = endpoint(&mut fabric, [1; 16], false);
        let listener = fabric
            .native_listen(endpoint, [1; 16], "loop", b"")
            .unwrap();
        fabric
            .native_begin_connect(
                endpoint,
                listener.listener_handle,
                listener.generation,
                1,
                Vec::new(),
            )
            .unwrap();
        assert_eq!(
            fabric.native_begin_connect(
                endpoint,
                listener.listener_handle,
                listener.generation,
                1,
                Vec::new(),
            ),
            Err(NativeError::ResourceExhausted)
        );
        assert_eq!(
            fabric
                .endpoint_slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&endpoint),
            Some(&2)
        );
    }

    #[tokio::test]
    async fn loopback_connect_requires_distinct_channel_handles() {
        let mut fabric = fabric();
        let (endpoint, mut events) = endpoint(&mut fabric, [1; 16], false);
        let listener = fabric
            .native_listen(endpoint, [1; 16], "loopback", b"")
            .unwrap();
        let connect_id = fabric
            .native_begin_connect(
                endpoint,
                listener.listener_handle,
                listener.generation,
                1,
                Vec::new(),
            )
            .unwrap();
        let _incoming = events.recv().await.unwrap();
        fabric
            .native_prepare_connect(endpoint, connect_id, 9, 1, 1)
            .unwrap();
        let _prepared = events.recv().await.unwrap();
        assert_eq!(
            fabric.native_complete_connect(endpoint, connect_id, 9, 1),
            Err(NativeError::Conflict)
        );
        fabric.native_cancel_connect(endpoint, connect_id, NativeError::Conflict);
        assert!(matches!(
            events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::Conflict,
                ..
            }) if observed == connect_id
        ));
        assert!(matches!(
            events.recv().await,
            Some(NativeEvent::ConnectAborted {
                connect_id: observed,
                reason: NativeError::Conflict,
                ..
            }) if observed == connect_id
        ));
        assert_eq!(fabric.active_pairs.load(Ordering::Relaxed), 0);
        assert!(
            fabric
                .endpoint_slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }
}
