//! Native YAS Net client used by `yas forward` and `yas socks`.
//!
//! One native Core session carries every TCP stream and UDP flow. Reliable
//! endpoints use sensitive bidirectional Transfer descriptors; datagrams use
//! typed Net events over the negotiated reliable or optional datagram path.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, oneshot, watch};
use yas_wire::core::{ResultPrefix, Status};
use yas_wire::net::{
    self, Address, Datagram, DatagramDelivery, DatagramStats, DeliveryPreference, DropPolicy,
    Endpoint, FlowMode, Open, TlsOptions, TlsVerification,
};
use yas_wire::transfer::{ByteData, Close as TransferClose, Credit, Reset};
use yas_wire::{Class, Decode, Encode, Extensions, Frame, FrameHeader, family};

use crate::yas_native::{NativeClient, NativeFrameReader, NativeFrameSender};

pub(crate) const DEFAULT_BIND: &str = "127.0.0.1";

const FLOW_RECEIVE_WINDOW: u64 = 256 * 1024;
const DATAGRAM_QUEUE_BYTES: usize = 1024 * 1024;
const TEAR_DOWN_GRACE: Duration = Duration::from_millis(500);

pub(crate) fn bracket(address: &str) -> String {
    if address.contains(':') {
        format!("[{address}]")
    } else {
        address.to_owned()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TlsConfig {
    pub(crate) alpn: Vec<String>,
    pub(crate) insecure: bool,
}

#[derive(Clone)]
pub(crate) struct Connection {
    inner: Arc<ConnectionInner>,
}

struct ConnectionInner {
    sender: NativeFrameSender,
    pending: Mutex<HashMap<u32, Pending>>,
    byte_flows: Mutex<HashMap<u32, Arc<ByteState>>>,
    datagram_flows: Mutex<HashMap<u64, Arc<DatagramState>>>,
    next_request_id: AtomicU32,
    dead: Mutex<Option<String>>,
    dead_notify: Notify,
    active_relays: AtomicUsize,
    relays_notify: Notify,
    limits: net::Limits,
    native_datagram: bool,
    max_transport_datagram: u32,
}

enum Pending {
    Open {
        tx: oneshot::Sender<Result<Opened, OpenFailure>>,
    },
    Close {
        tx: oneshot::Sender<Result<(), OpenFailure>>,
    },
}

enum Opened {
    Byte(Box<ByteFlow>),
    Datagram(DatagramFlow),
}

#[derive(Clone, Debug)]
pub(crate) struct OpenFailure {
    pub(crate) status: Option<Status>,
    pub(crate) detail: String,
}

impl std::fmt::Display for OpenFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(
                formatter,
                "Net OPEN failed with {status:?}: {}",
                self.detail
            ),
            None => formatter.write_str(&self.detail),
        }
    }
}

impl Connection {
    pub(crate) async fn connect(on: Option<&str>, hub: &str) -> Result<Self, String> {
        let client = NativeClient::connect(on, hub).await?;
        for (class, kind) in [
            (Class::Request, net::request_kind::OPEN),
            (Class::Request, net::request_kind::CLOSE),
            (Class::Event, net::event_kind::DATAGRAM),
            (Class::Event, net::event_kind::DATAGRAM_STATS),
        ] {
            if !client.supports(family::NET, class, kind) {
                return Err("YAS server does not provide the native Net family".into());
            }
        }
        for kind in [
            yas_wire::transfer::kind::BYTE_DATA,
            yas_wire::transfer::kind::CREDIT,
            yas_wire::transfer::kind::CLOSE,
            yas_wire::transfer::kind::RESET,
        ] {
            if !client.supports(family::TRANSFER, Class::Event, kind) {
                return Err("YAS server does not provide native Transfer streams".into());
            }
        }
        let descriptor = client
            .hello()
            .families
            .iter()
            .find(|descriptor| descriptor.family_id == family::NET)
            .ok_or_else(|| "YAS server omitted its native Net descriptor".to_string())?;
        let limits = net::Limits::from_extensions(&descriptor.limits)
            .map_err(|error| format!("invalid Net family limits: {error}"))?;
        let native_datagram = client.supports_datagrams();
        let max_transport_datagram = client.hello().receive.max_datagram;
        let (reader, sender) = client.into_framed();
        let inner = Arc::new(ConnectionInner {
            sender,
            pending: Mutex::new(HashMap::new()),
            byte_flows: Mutex::new(HashMap::new()),
            datagram_flows: Mutex::new(HashMap::new()),
            next_request_id: AtomicU32::new(3),
            dead: Mutex::new(None),
            dead_notify: Notify::new(),
            active_relays: AtomicUsize::new(0),
            relays_notify: Notify::new(),
            limits,
            native_datagram,
            max_transport_datagram,
        });
        tokio::spawn(read_loop(reader, Arc::clone(&inner)));
        Ok(Self { inner })
    }

    pub(crate) async fn wait_closed(&self) {
        loop {
            let notified = self.inner.dead_notify.notified();
            if self.inner.dead.lock().expect("Net dead lock").is_some() {
                break;
            }
            notified.await;
        }
        let _ = tokio::time::timeout(TEAR_DOWN_GRACE, async {
            loop {
                let notified = self.inner.relays_notify.notified();
                if self.inner.active_relays.load(Ordering::Acquire) == 0 {
                    break;
                }
                notified.await;
            }
        })
        .await;
    }

    pub(crate) fn relay_guard(&self) -> RelayGuard {
        self.inner.active_relays.fetch_add(1, Ordering::AcqRel);
        RelayGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    pub(crate) async fn open_tcp(
        &self,
        host: &str,
        port: u16,
        tls: Option<&TlsConfig>,
        early_data: Vec<u8>,
    ) -> Result<ByteFlow, OpenFailure> {
        let tls_options = tls.map(|tls| TlsOptions {
            verification: if tls.insecure {
                TlsVerification::Insecure
            } else {
                TlsVerification::Strict
            },
            sni: String::new(),
            alpn: tls
                .alpn
                .iter()
                .map(|value| value.as_bytes().to_vec())
                .collect(),
            extensions: Extensions::default(),
        });
        let opened = self
            .open(Open {
                operation_id: nonzero_operation_id(),
                address: Address::Tcp {
                    host: host.to_owned(),
                    port,
                },
                delivery_preference: DeliveryPreference::NotApplicable,
                drop_policy: DropPolicy::NotApplicable,
                initial_receive_credit: FLOW_RECEIVE_WINDOW
                    .min(self.inner.limits.max_buffered_per_flow),
                early_data,
                tls_options,
                extensions: Extensions::default(),
            })
            .await?;
        match opened {
            Opened::Byte(flow) => Ok(*flow),
            Opened::Datagram(_) => Err(OpenFailure {
                status: None,
                detail: "YAS returned a datagram endpoint for TCP".into(),
            }),
        }
    }

    pub(crate) async fn open_udp(
        &self,
        host: &str,
        port: u16,
    ) -> Result<DatagramFlow, OpenFailure> {
        let opened = self
            .open(Open {
                operation_id: nonzero_operation_id(),
                address: Address::Udp {
                    host: host.to_owned(),
                    port,
                },
                delivery_preference: if self.inner.native_datagram {
                    DeliveryPreference::PreferNative
                } else {
                    DeliveryPreference::ReliableTunnel
                },
                drop_policy: DropPolicy::Oldest,
                initial_receive_credit: 0,
                early_data: Vec::new(),
                tls_options: None,
                extensions: Extensions::default(),
            })
            .await?;
        match opened {
            Opened::Datagram(flow) => Ok(flow),
            Opened::Byte(_) => Err(OpenFailure {
                status: None,
                detail: "YAS returned a reliable endpoint for UDP".into(),
            }),
        }
    }

    async fn open(&self, request: Open) -> Result<Opened, OpenFailure> {
        if request.early_data.len() > self.inner.limits.max_early_data_bytes as usize {
            return Err(OpenFailure {
                status: None,
                detail: "Net early data exceeds the negotiated limit".into(),
            });
        }
        let (request_id, rx) = self.inner.register_open().map_err(|detail| OpenFailure {
            status: None,
            detail,
        })?;
        let payload = request.encode().map_err(|error| OpenFailure {
            status: None,
            detail: format!("invalid Net OPEN: {error}"),
        })?;
        let mut header = FrameHeader::request(family::NET, net::request_kind::OPEN, request_id);
        header.sensitive = true;
        if let Err(error) = self.inner.send(Frame { header, payload }).await {
            self.inner.remove_pending(request_id);
            return Err(OpenFailure {
                status: None,
                detail: error,
            });
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(OpenFailure {
                status: None,
                detail: self.inner.dead_detail(),
            }),
        }
    }

    async fn close_flow(&self, flow_handle: u64) -> Result<(), String> {
        let (request_id, rx) = self.inner.register_close()?;
        let request = net::Close {
            flow_handle,
            operation_id: nonzero_operation_id(),
            extensions: Extensions::default(),
        };
        let mut header = FrameHeader::request(family::NET, net::request_kind::CLOSE, request_id);
        header.sensitive = true;
        if let Err(error) = self
            .inner
            .send(Frame {
                header,
                payload: request
                    .encode()
                    .map_err(|error| format!("invalid Net CLOSE: {error}"))?,
            })
            .await
        {
            self.inner.remove_pending(request_id);
            return Err(error);
        }
        match rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(self.inner.dead_detail()),
        }
    }

    pub(crate) fn max_datagram_payload(&self) -> usize {
        self.inner.limits.max_datagram_payload as usize
    }
}

pub(crate) struct RelayGuard {
    inner: Arc<ConnectionInner>,
}

impl Drop for RelayGuard {
    fn drop(&mut self) {
        if self.inner.active_relays.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.inner.relays_notify.notify_waiters();
        }
    }
}

impl ConnectionInner {
    async fn send(&self, frame: Frame) -> Result<(), String> {
        match self.sender.send(frame).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.fail(error.clone());
                Err(error)
            }
        }
    }

    fn next_id(&self) -> Result<u32, String> {
        for _ in 0..u32::MAX / 2 {
            let id = self.next_request_id.fetch_add(2, Ordering::Relaxed) | 1;
            if id != 0
                && !self
                    .pending
                    .lock()
                    .expect("Net pending lock")
                    .contains_key(&id)
            {
                return Ok(id);
            }
        }
        Err("native Net request ID space is exhausted".into())
    }

    fn register_open(
        &self,
    ) -> Result<(u32, oneshot::Receiver<Result<Opened, OpenFailure>>), String> {
        self.ensure_alive()?;
        let mut pending = self.pending.lock().expect("Net pending lock");
        if pending
            .values()
            .filter(|pending| matches!(pending, Pending::Open { .. }))
            .count()
            >= self.limits.max_pending_opens as usize
        {
            return Err("too many pending native Net opens".into());
        }
        drop(pending);
        let id = self.next_id()?;
        let (tx, rx) = oneshot::channel();
        pending = self.pending.lock().expect("Net pending lock");
        if pending
            .values()
            .filter(|pending| matches!(pending, Pending::Open { .. }))
            .count()
            >= self.limits.max_pending_opens as usize
        {
            return Err("too many pending native Net opens".into());
        }
        pending.insert(id, Pending::Open { tx });
        Ok((id, rx))
    }

    fn register_close(&self) -> Result<(u32, oneshot::Receiver<Result<(), OpenFailure>>), String> {
        self.ensure_alive()?;
        let id = self.next_id()?;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("Net pending lock")
            .insert(id, Pending::Close { tx });
        Ok((id, rx))
    }

    fn remove_pending(&self, request_id: u32) {
        self.pending
            .lock()
            .expect("Net pending lock")
            .remove(&request_id);
    }

    fn ensure_alive(&self) -> Result<(), String> {
        match &*self.dead.lock().expect("Net dead lock") {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn dead_detail(&self) -> String {
        self.dead
            .lock()
            .expect("Net dead lock")
            .clone()
            .unwrap_or_else(|| "native Net connection closed".into())
    }

    fn fail(&self, detail: String) {
        let mut dead = self.dead.lock().expect("Net dead lock");
        if dead.is_some() {
            return;
        }
        *dead = Some(detail.clone());
        drop(dead);

        for (_, pending) in self.pending.lock().expect("Net pending lock").drain() {
            let failure = OpenFailure {
                status: None,
                detail: detail.clone(),
            };
            match pending {
                Pending::Open { tx } => {
                    let _ = tx.send(Err(failure));
                }
                Pending::Close { tx } => {
                    let _ = tx.send(Err(failure));
                }
            }
        }
        for state in self.byte_flows.lock().expect("Net byte-flow lock").values() {
            state.fail(detail.clone());
        }
        for state in self
            .datagram_flows
            .lock()
            .expect("Net datagram-flow lock")
            .values()
        {
            state.finish(None, Some(detail.clone()));
        }
        self.dead_notify.notify_waiters();
    }

    fn has_flow_capacity(&self) -> bool {
        let byte_count = self.byte_flows.lock().expect("Net byte-flow lock").len();
        let datagram_count = self
            .datagram_flows
            .lock()
            .expect("Net datagram-flow lock")
            .len();
        byte_count.saturating_add(datagram_count) < self.limits.max_flows_per_session as usize
    }
}

#[derive(Clone)]
pub(crate) struct ByteFlow {
    inner: Arc<ConnectionInner>,
    state: Arc<ByteState>,
    endpoint: Endpoint,
}

struct ByteState {
    descriptor: yas_wire::transfer::Descriptor,
    receive_window: u64,
    receive: Mutex<ByteReceive>,
    receive_notify: Notify,
    upload_credit: watch::Sender<u64>,
    upload_sent: AtomicU64,
    upload_window: u64,
    retired: AtomicBool,
}

struct ByteReceive {
    chunks: VecDeque<Vec<u8>>,
    queued_bytes: u64,
    received: u64,
    written: u64,
    receive_limit: u64,
    end: Option<Result<(), String>>,
}

impl ByteFlow {
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub(crate) fn descriptor(&self) -> &yas_wire::transfer::Descriptor {
        &self.state.descriptor
    }

    async fn send_data(&self, offset: u64, data: Vec<u8>) -> Result<(), String> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| "Net upload offset overflow".to_string())?;
        if offset != self.state.upload_sent.load(Ordering::Acquire)
            || end > *self.state.upload_credit.borrow()
        {
            return Err("non-contiguous or over-credit Net upload chunk".into());
        }
        // Publish the bytes before awaiting the transport write. The server can
        // drain them and return new credit as soon as the write is queued.
        self.state.upload_sent.store(end, Ordering::Release);
        let payload = ByteData {
            transfer_id: self.state.descriptor.transfer_id,
            offset,
            data,
        }
        .encode()
        .map_err(|error| format!("invalid Net Transfer data: {error}"))?;
        let mut header = FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::BYTE_DATA);
        header.sensitive = true;
        self.inner.send(Frame { header, payload }).await
    }

    async fn send_input_close(&self, final_data_bytes: u64) -> Result<(), String> {
        let payload = TransferClose {
            transfer_id: self.state.descriptor.transfer_id,
            final_data_bytes,
            status: Status::Ok.code(),
            detail: Vec::new(),
        }
        .encode()
        .map_err(|error| format!("invalid Net Transfer close: {error}"))?;
        let mut header = FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE);
        header.sensitive = true;
        self.inner.send(Frame { header, payload }).await
    }

    async fn next_chunk(&self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let notified = self.state.receive_notify.notified();
            {
                let mut receive = self.state.receive.lock().expect("Net receive lock");
                if let Some(chunk) = receive.chunks.pop_front() {
                    receive.queued_bytes = receive.queued_bytes.saturating_sub(chunk.len() as u64);
                    return Ok(Some(chunk));
                }
                if let Some(end) = receive.end.clone() {
                    return end.map(|()| None);
                }
            }
            notified.await;
        }
    }

    async fn acknowledge_written(&self, bytes: u64) -> Result<(), String> {
        let cumulative_limit = {
            let mut receive = self.state.receive.lock().expect("Net receive lock");
            receive.written = receive
                .written
                .checked_add(bytes)
                .ok_or_else(|| "Net receive accounting overflow".to_string())?;
            let limit = receive
                .written
                .checked_add(self.state.receive_window)
                .ok_or_else(|| "Net receive credit overflow".to_string())?;
            if limit <= receive.receive_limit {
                return Ok(());
            }
            receive.receive_limit = limit;
            limit
        };
        let payload = Credit {
            transfer_id: self.state.descriptor.transfer_id,
            cumulative_limit,
        }
        .encode()
        .map_err(|error| format!("invalid Net Transfer credit: {error}"))?;
        self.inner
            .send(Frame {
                header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT),
                payload,
            })
            .await
    }

    fn upload_credit(&self) -> watch::Receiver<u64> {
        self.state.upload_credit.subscribe()
    }

    fn retire(&self) {
        if !self.state.retired.swap(true, Ordering::AcqRel) {
            self.inner
                .byte_flows
                .lock()
                .expect("Net byte-flow lock")
                .remove(&self.state.descriptor.transfer_id);
        }
    }
}

impl ByteState {
    fn new(descriptor: yas_wire::transfer::Descriptor, receive_window: u64) -> Self {
        let upload_window = descriptor.receiver_send_credit;
        let (upload_credit, _) = watch::channel(descriptor.receiver_send_credit);
        Self {
            receive: Mutex::new(ByteReceive {
                chunks: VecDeque::new(),
                queued_bytes: 0,
                received: 0,
                written: 0,
                receive_limit: descriptor.sender_send_credit,
                end: None,
            }),
            descriptor,
            receive_window,
            receive_notify: Notify::new(),
            upload_credit,
            upload_sent: AtomicU64::new(0),
            upload_window,
            retired: AtomicBool::new(false),
        }
    }

    fn push_data(&self, data: ByteData, sensitive: bool) -> Result<(), String> {
        if !sensitive || data.data.len() > self.descriptor.max_chunk_bytes as usize {
            return Err("invalid sensitive Net Transfer chunk".into());
        }
        let mut receive = self.receive.lock().expect("Net receive lock");
        let end = data
            .offset
            .checked_add(data.data.len() as u64)
            .ok_or_else(|| "Net Transfer offset overflow".to_string())?;
        if data.offset != receive.received || end > receive.receive_limit || receive.end.is_some() {
            return Err("non-contiguous or over-credit Net Transfer chunk".into());
        }
        receive.received = end;
        receive.queued_bytes = receive
            .queued_bytes
            .checked_add(data.data.len() as u64)
            .ok_or_else(|| "Net receive queue overflow".to_string())?;
        if receive.queued_bytes > self.receive_window {
            return Err("Net peer exceeded the bounded receive queue".into());
        }
        if let Some(last) = receive.chunks.back_mut()
            && last.len() + data.data.len() <= self.receive_window as usize
        {
            last.extend_from_slice(&data.data);
        } else {
            receive.chunks.push_back(data.data);
        }
        drop(receive);
        self.receive_notify.notify_one();
        Ok(())
    }

    fn grant_upload(&self, credit: Credit, sensitive: bool) -> Result<(), String> {
        if sensitive {
            return Err("Net Transfer CREDIT was marked sensitive".into());
        }
        let current = *self.upload_credit.borrow();
        if credit.cumulative_limit < current {
            return Err("Net Transfer credit moved backwards".into());
        }
        if credit.cumulative_limit == current {
            return Ok(());
        }
        let maximum = self
            .upload_sent
            .load(Ordering::Acquire)
            .checked_add(self.upload_window)
            .ok_or_else(|| "Net upload credit overflow".to_string())?;
        if credit.cumulative_limit > maximum {
            return Err("Net peer granted more upload credit than its bounded window".into());
        }
        self.upload_credit.send_replace(credit.cumulative_limit);
        Ok(())
    }

    fn close_output(&self, close: TransferClose, sensitive: bool) -> Result<(), String> {
        if !sensitive || close.status != Status::Ok.code() {
            return Err(format!(
                "Net Transfer closed with status {}: {}",
                close.status,
                String::from_utf8_lossy(&close.detail)
            ));
        }
        let mut receive = self.receive.lock().expect("Net receive lock");
        if close.final_data_bytes != receive.received || receive.end.is_some() {
            return Err("Net Transfer CLOSE accounting mismatch".into());
        }
        receive.end = Some(Ok(()));
        drop(receive);
        self.receive_notify.notify_waiters();
        Ok(())
    }

    fn reset(&self, reset: Reset, sensitive: bool) -> Result<(), String> {
        if !sensitive {
            return Err("Net Transfer RESET was not marked sensitive".into());
        }
        self.fail(format!(
            "Net Transfer reset with {:?}: {}",
            Status::from_code(reset.status),
            String::from_utf8_lossy(&reset.detail)
        ));
        Ok(())
    }

    fn fail(&self, detail: String) {
        let mut receive = self.receive.lock().expect("Net receive lock");
        if receive.end.is_none() {
            receive.end = Some(Err(detail));
        }
        drop(receive);
        self.receive_notify.notify_waiters();
    }
}

#[derive(Clone)]
pub(crate) struct DatagramFlow {
    inner: Arc<ConnectionInner>,
    state: Arc<DatagramState>,
}

struct DatagramState {
    handle: u64,
    delivery: DatagramDelivery,
    max_payload: usize,
    max_queue: usize,
    send_sequence: AtomicU64,
    receive: Mutex<DatagramReceive>,
    receive_notify: Notify,
    retired: AtomicBool,
}

struct DatagramReceive {
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    last_sequence: Option<u64>,
    stats: Option<DatagramStats>,
    error: Option<String>,
}

impl DatagramFlow {
    pub(crate) async fn send(&self, payload: &[u8]) -> Result<(), String> {
        if payload.len() > self.state.max_payload {
            return Err(format!(
                "UDP datagram is {} bytes; negotiated maximum is {}",
                payload.len(),
                self.state.max_payload
            ));
        }
        let sequence = self.state.send_sequence.fetch_add(1, Ordering::Relaxed);
        let payload = Datagram {
            flow_handle: self.state.handle,
            sequence,
            payload: payload.to_vec(),
        }
        .encode()
        .map_err(|error| format!("invalid Net datagram: {error}"))?;
        let mut header = FrameHeader::event(family::NET, net::event_kind::DATAGRAM);
        header.sensitive = true;
        let frame = Frame { header, payload };
        if self.state.delivery == DatagramDelivery::Native {
            // Queue/SCTP pressure is ordinary UDP loss. Encoding failures are
            // still local programming or negotiated-limit errors. Once the
            // optional path closes, preserve the datagram boundary in one
            // reliable Event frame instead of black-holing the flow.
            match self.inner.sender.try_send_datagram(
                &frame,
                self.inner.max_transport_datagram,
                yas_wire::frame::DatagramContext::NetNativeFlow,
            )? {
                crate::transport::DatagramSend::Sent | crate::transport::DatagramSend::Dropped => {
                    Ok(())
                }
                crate::transport::DatagramSend::Closed => self.inner.send(frame).await,
            }
        } else {
            self.inner.send(frame).await
        }
    }

    pub(crate) async fn recv(&self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let notified = self.state.receive_notify.notified();
            {
                let mut receive = self.state.receive.lock().expect("Net datagram queue lock");
                if let Some(payload) = receive.queue.pop_front() {
                    receive.queued_bytes = receive.queued_bytes.saturating_sub(payload.len());
                    return Ok(Some(payload));
                }
                if let Some(error) = &receive.error {
                    return Err(error.clone());
                }
                if receive
                    .stats
                    .as_ref()
                    .is_some_and(|stats| stats.final_stats)
                {
                    return Ok(None);
                }
            }
            notified.await;
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        let receive = self.state.receive.lock().expect("Net datagram queue lock");
        receive.error.is_some()
            || receive
                .stats
                .as_ref()
                .is_some_and(|stats| stats.final_stats)
    }

    pub(crate) fn final_stats(&self) -> Option<DatagramStats> {
        self.state
            .receive
            .lock()
            .expect("Net datagram queue lock")
            .stats
            .clone()
            .filter(|stats| stats.final_stats)
    }

    pub(crate) fn retire(&self) {
        if !self.state.retired.swap(true, Ordering::AcqRel) {
            self.inner
                .datagram_flows
                .lock()
                .expect("Net datagram-flow lock")
                .remove(&self.state.handle);
        }
    }

    pub(crate) fn close_in_background(&self) {
        let connection = Connection {
            inner: Arc::clone(&self.inner),
        };
        let handle = self.state.handle;
        tokio::spawn(async move {
            let _ = connection.close_flow(handle).await;
        });
    }
}

impl DatagramState {
    fn push(&self, datagram: Datagram, sensitive: bool) -> Result<(), String> {
        if !sensitive || datagram.payload.len() > self.max_payload {
            return Err("invalid sensitive Net datagram".into());
        }
        let mut receive = self.receive.lock().expect("Net datagram queue lock");
        if self.delivery == DatagramDelivery::ReliableTunnel {
            if receive
                .last_sequence
                .is_some_and(|sequence| datagram.sequence <= sequence)
            {
                return Err("Net datagram sequence did not increase".into());
            }
            receive.last_sequence = Some(datagram.sequence);
        }
        while receive.queue.len() >= self.max_queue
            || receive.queued_bytes + datagram.payload.len() > DATAGRAM_QUEUE_BYTES
        {
            let Some(dropped) = receive.queue.pop_front() else {
                break;
            };
            receive.queued_bytes = receive.queued_bytes.saturating_sub(dropped.len());
        }
        if datagram.payload.len() <= DATAGRAM_QUEUE_BYTES {
            receive.queued_bytes += datagram.payload.len();
            receive.queue.push_back(datagram.payload);
        }
        drop(receive);
        self.receive_notify.notify_one();
        Ok(())
    }

    fn finish(&self, stats: Option<DatagramStats>, error: Option<String>) {
        let mut receive = self.receive.lock().expect("Net datagram queue lock");
        if let Some(stats) = stats {
            let replace = receive
                .stats
                .as_ref()
                .is_none_or(|previous| stats.revision > previous.revision);
            if replace {
                receive.stats = Some(stats);
            }
        }
        if error.is_some() && receive.error.is_none() {
            receive.error = error;
        }
        drop(receive);
        self.receive_notify.notify_waiters();
    }
}

async fn read_loop(mut reader: NativeFrameReader, inner: Arc<ConnectionInner>) {
    loop {
        let (frame, transport_datagram) = match reader.next_with_source().await {
            Ok(frame) => frame,
            Err(error) => {
                inner.fail(error);
                return;
            }
        };
        let result = match frame.header.class {
            Class::Result => handle_result(&inner, frame),
            Class::Event if frame.header.family == family::TRANSFER => {
                handle_transfer_event(&inner, frame)
            }
            Class::Event if frame.header.family == family::NET => {
                handle_net_event(&inner, frame, transport_datagram)
            }
            _ => Err(format!(
                "unexpected frame on native Net connection: {:?}/{:#06x}/{:#06x}",
                frame.header.class, frame.header.family, frame.header.kind
            )),
        };
        if let Err(error) = result {
            inner.fail(error);
            return;
        }
    }
}

fn handle_result(inner: &Arc<ConnectionInner>, frame: Frame) -> Result<(), String> {
    if frame.header.family != family::NET || !frame.header.sensitive {
        return Err("invalid Result on native Net connection".into());
    }
    let request_id = frame
        .header
        .request_id
        .ok_or_else(|| "Net Result omitted its request ID".to_string())?;
    let pending = inner
        .pending
        .lock()
        .expect("Net pending lock")
        .remove(&request_id)
        .ok_or_else(|| format!("uncorrelated Net Result {request_id}"))?;
    let prefix = ResultPrefix::decode(&frame.payload)
        .map_err(|error| format!("invalid Net Result: {error}"))?;
    let failure = || OpenFailure {
        status: Some(prefix.status),
        detail: format_result_detail(&prefix),
    };
    match pending {
        Pending::Open { tx } => {
            if frame.header.kind != net::request_kind::OPEN {
                return Err("Net OPEN Result kind mismatch".into());
            }
            if prefix.status != Status::Ok {
                let _ = tx.send(Err(failure()));
                return Ok(());
            }
            let endpoint = Endpoint::decode(&prefix.body)
                .map_err(|error| format!("invalid Net endpoint: {error}"))?;
            if !inner.has_flow_capacity() {
                return Err("native Net flow table exceeded its aggregate limit".into());
            }
            let opened = match endpoint.mode {
                FlowMode::Byte => {
                    let descriptor = endpoint
                        .descriptor
                        .clone()
                        .ok_or_else(|| "Net byte endpoint omitted its Transfer".to_string())?;
                    let receive_window =
                        FLOW_RECEIVE_WINDOW.min(inner.limits.max_buffered_per_flow);
                    if descriptor.sender_send_credit > receive_window
                        || descriptor.receiver_send_credit > inner.limits.max_buffered_per_flow
                    {
                        return Err(
                            "Net Transfer descriptor exceeded negotiated credit limits".into()
                        );
                    }
                    let state = Arc::new(ByteState::new(descriptor.clone(), receive_window));
                    inner
                        .byte_flows
                        .lock()
                        .expect("Net byte-flow lock")
                        .insert(descriptor.transfer_id, Arc::clone(&state));
                    Opened::Byte(Box::new(ByteFlow {
                        inner: Arc::clone(inner),
                        state,
                        endpoint,
                    }))
                }
                FlowMode::Datagram => {
                    if endpoint.max_datagram_payload > inner.limits.max_datagram_payload {
                        return Err("Net endpoint exceeded the negotiated datagram limit".into());
                    }
                    if endpoint.selected_delivery == DatagramDelivery::Native
                        && (!inner.native_datagram || inner.max_transport_datagram == 0)
                    {
                        return Err(
                            "Net selected native datagrams without a composite transport".into(),
                        );
                    }
                    let state = Arc::new(DatagramState {
                        handle: endpoint.flow_handle,
                        delivery: endpoint.selected_delivery,
                        max_payload: endpoint.max_datagram_payload as usize,
                        max_queue: inner.limits.max_datagram_queue as usize,
                        send_sequence: AtomicU64::new(0),
                        receive: Mutex::new(DatagramReceive {
                            queue: VecDeque::new(),
                            queued_bytes: 0,
                            last_sequence: None,
                            stats: None,
                            error: None,
                        }),
                        receive_notify: Notify::new(),
                        retired: AtomicBool::new(false),
                    });
                    inner
                        .datagram_flows
                        .lock()
                        .expect("Net datagram-flow lock")
                        .insert(endpoint.flow_handle, Arc::clone(&state));
                    Opened::Datagram(DatagramFlow {
                        inner: Arc::clone(inner),
                        state,
                    })
                }
                FlowMode::Message => {
                    return Err("Net returned a MESSAGE endpoint for TCP/UDP forwarding".into());
                }
            };
            tx.send(Ok(opened))
                .map_err(|_| "Net OPEN caller disappeared".to_string())
        }
        Pending::Close { tx } => {
            if frame.header.kind != net::request_kind::CLOSE {
                return Err("Net CLOSE Result kind mismatch".into());
            }
            let result = if prefix.status == Status::Ok || prefix.status == Status::NotFound {
                Ok(())
            } else {
                Err(failure())
            };
            let _ = tx.send(result);
            Ok(())
        }
    }
}

fn handle_transfer_event(inner: &Arc<ConnectionInner>, frame: Frame) -> Result<(), String> {
    if frame.payload.len() < 4 {
        return Err("truncated native Net Transfer event".into());
    }
    let transfer_id = u32::from_le_bytes(
        frame.payload[..4]
            .try_into()
            .expect("checked Transfer ID length"),
    );
    let state = inner
        .byte_flows
        .lock()
        .expect("Net byte-flow lock")
        .get(&transfer_id)
        .cloned();
    let Some(state) = state else {
        // A final RESET may race an explicit CLOSE Result and local retirement.
        return Ok(());
    };
    match frame.header.kind {
        yas_wire::transfer::kind::BYTE_DATA => state.push_data(
            ByteData::decode(&frame.payload)
                .map_err(|error| format!("invalid Net BYTE_DATA: {error}"))?,
            frame.header.sensitive,
        ),
        yas_wire::transfer::kind::CREDIT => state.grant_upload(
            Credit::decode(&frame.payload)
                .map_err(|error| format!("invalid Net CREDIT: {error}"))?,
            frame.header.sensitive,
        ),
        yas_wire::transfer::kind::CLOSE => state.close_output(
            TransferClose::decode(&frame.payload)
                .map_err(|error| format!("invalid Net Transfer CLOSE: {error}"))?,
            frame.header.sensitive,
        ),
        yas_wire::transfer::kind::RESET => state.reset(
            Reset::decode(&frame.payload)
                .map_err(|error| format!("invalid Net Transfer RESET: {error}"))?,
            frame.header.sensitive,
        ),
        other => Err(format!("unexpected Net Transfer event {other:#06x}")),
    }
}

fn handle_net_event(
    inner: &Arc<ConnectionInner>,
    frame: Frame,
    transport_datagram: bool,
) -> Result<(), String> {
    if !frame.header.sensitive {
        return Err("native Net event omitted the SENSITIVE flag".into());
    }
    match frame.header.kind {
        net::event_kind::DATAGRAM => {
            let datagram = Datagram::decode(&frame.payload)
                .map_err(|error| format!("invalid Net DATAGRAM: {error}"))?;
            if let Some(state) = inner
                .datagram_flows
                .lock()
                .expect("Net datagram-flow lock")
                .get(&datagram.flow_handle)
                .cloned()
            {
                if !accepts_datagram_source(state.delivery, transport_datagram) {
                    // A malformed or stale lossy packet must not kill the
                    // reliable Core session. A server placing a reliable-flow
                    // packet on the wrong transport is dropped likewise.
                    return Ok(());
                }
                // A native flow may arrive on the reliable stream after its
                // optional sideband closes. It remains one complete Event, so
                // message boundaries and the flow's lossy sequencing survive.
                state.push(datagram, true)?;
            }
            Ok(())
        }
        net::event_kind::DATAGRAM_STATS => {
            if transport_datagram {
                return Ok(());
            }
            let stats = DatagramStats::decode(&frame.payload)
                .map_err(|error| format!("invalid Net DATAGRAM_STATS: {error}"))?;
            if let Some(state) = inner
                .datagram_flows
                .lock()
                .expect("Net datagram-flow lock")
                .get(&stats.flow_handle)
                .cloned()
            {
                state.finish(Some(stats), None);
            }
            Ok(())
        }
        other => Err(format!("unexpected native Net event {other:#06x}")),
    }
}

const fn accepts_datagram_source(delivery: DatagramDelivery, transport_datagram: bool) -> bool {
    !transport_datagram || matches!(delivery, DatagramDelivery::Native)
}

pub(crate) enum OnOpen {
    Report {
        announce_alpn: Option<Arc<AtomicBool>>,
    },
    Answer(fn(Status) -> Vec<u8>),
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn relay_tcp(
    mut local: tokio::net::TcpStream,
    connection: Connection,
    host: String,
    port: u16,
    tls: Option<TlsConfig>,
    on_open: OnOpen,
) -> Result<(), String> {
    let _guard = connection.relay_guard();
    let _ = local.set_nodelay(true);
    let target = format!("{}:{port}", bracket(&host));
    let mut early = vec![0; connection.inner.limits.max_early_data_bytes.min(16_384) as usize];
    let early = if matches!(&on_open, OnOpen::Report { .. }) && !early.is_empty() {
        match local.try_read(&mut early) {
            Ok(count) => {
                early.truncate(count);
                early
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Vec::new(),
            Err(error) => return Err(format!("reading local client: {error}")),
        }
    } else {
        Vec::new()
    };
    let flow = match connection.open_tcp(&host, port, tls.as_ref(), early).await {
        Ok(flow) => flow,
        Err(error) => {
            if let OnOpen::Answer(reply) = on_open {
                let status = error.status.unwrap_or(Status::Internal);
                let _ = local.write_all(&reply(status)).await;
            }
            return Err(format!("{target}: {error}"));
        }
    };
    if let OnOpen::Answer(reply) = &on_open {
        local
            .write_all(&reply(Status::Ok))
            .await
            .map_err(|error| format!("writing local handshake: {error}"))?;
    }
    if let OnOpen::Report {
        announce_alpn: Some(announced),
    } = &on_open
        && !announced.swap(true, Ordering::Relaxed)
    {
        let alpn = String::from_utf8_lossy(&flow.endpoint().negotiated_alpn);
        eprintln!(
            "yas: tls to {target} established ({})",
            if alpn.is_empty() {
                "no alpn".to_owned()
            } else {
                format!("alpn={alpn}")
            }
        );
    }

    let (mut local_read, mut local_write) = local.into_split();
    let upload = flow.clone();
    let mut upload = tokio::spawn(async move {
        let descriptor = upload.descriptor().clone();
        let mut credit = upload.upload_credit();
        let mut offset = 0u64;
        let mut buffer = vec![0; descriptor.max_chunk_bytes as usize];
        loop {
            let room = loop {
                let limit = *credit.borrow_and_update();
                if limit > offset {
                    break usize::try_from(limit - offset)
                        .unwrap_or(usize::MAX)
                        .min(buffer.len());
                }
                credit
                    .changed()
                    .await
                    .map_err(|_| "Net upload credit channel closed".to_string())?;
            };
            match local_read.read(&mut buffer[..room]).await {
                Ok(0) => {
                    upload.send_input_close(offset).await?;
                    return Ok::<(), String>(());
                }
                Ok(count) => {
                    upload.send_data(offset, buffer[..count].to_vec()).await?;
                    offset = offset
                        .checked_add(count as u64)
                        .ok_or_else(|| "Net upload offset overflow".to_string())?;
                }
                Err(error) => return Err(format!("reading local client: {error}")),
            }
        }
    });

    let mut upload_done = false;
    let mut download_done = false;
    let mut abnormal = None;
    while !upload_done || !download_done {
        tokio::select! {
            result = &mut upload, if !upload_done => {
                upload_done = true;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => abnormal = Some(error),
                    Err(error) => abnormal = Some(format!("Net upload task failed: {error}")),
                }
            }
            result = flow.next_chunk(), if !download_done => {
                match result {
                    Ok(Some(bytes)) => {
                        if let Err(error) = local_write.write_all(&bytes).await {
                            abnormal = Some(format!("writing local client: {error}"));
                        } else if let Err(error) = flow.acknowledge_written(bytes.len() as u64).await {
                            abnormal = Some(error);
                        }
                    }
                    Ok(None) => {
                        let _ = local_write.shutdown().await;
                        download_done = true;
                    }
                    Err(error) => abnormal = Some(error),
                }
            }
        }
        if abnormal.is_some() {
            break;
        }
    }

    if !upload_done {
        upload.abort();
    }
    flow.retire();
    if let Some(error) = abnormal {
        let closer = connection.clone();
        let handle = flow.endpoint().flow_handle;
        tokio::spawn(async move {
            let _ = closer.close_flow(handle).await;
        });
        let _ = local_write.as_ref().set_zero_linger();
        local_write.forget();
        Err(error)
    } else {
        Ok(())
    }
}

fn format_result_detail(prefix: &ResultPrefix) -> String {
    if prefix.detail.0.is_empty() {
        return format!("{:?}", prefix.status);
    }
    prefix
        .detail
        .0
        .iter()
        .map(|extension| {
            format!(
                "extension {} ({} bytes)",
                extension.tag,
                extension.value.len()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn nonzero_operation_id() -> [u8; 16] {
    loop {
        let value: [u8; 16] = rand::random();
        if value != [0; 16] {
            return value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_display_brackets_only_ipv6() {
        assert_eq!(bracket("127.0.0.1"), "127.0.0.1");
        assert_eq!(bracket("example.test"), "example.test");
        assert_eq!(bracket("::1"), "[::1]");
    }

    #[test]
    fn datagram_queue_drops_oldest_under_both_bounds() {
        let state = DatagramState {
            handle: 1,
            delivery: DatagramDelivery::ReliableTunnel,
            max_payload: net::MAX_DATAGRAM_PAYLOAD,
            max_queue: 2,
            send_sequence: AtomicU64::new(0),
            receive: Mutex::new(DatagramReceive {
                queue: VecDeque::new(),
                queued_bytes: 0,
                last_sequence: None,
                stats: None,
                error: None,
            }),
            receive_notify: Notify::new(),
            retired: AtomicBool::new(false),
        };
        for sequence in 0..3 {
            state
                .push(
                    Datagram {
                        flow_handle: 1,
                        sequence,
                        payload: vec![sequence as u8],
                    },
                    true,
                )
                .unwrap();
        }
        let receive = state.receive.lock().unwrap();
        assert_eq!(receive.queue.len(), 2);
        assert_eq!(receive.queue.front().unwrap(), &[1]);
    }

    #[test]
    fn native_datagrams_preserve_duplicates_and_reordering() {
        let state = DatagramState {
            handle: 1,
            delivery: DatagramDelivery::Native,
            max_payload: net::MAX_DATAGRAM_PAYLOAD,
            max_queue: 8,
            send_sequence: AtomicU64::new(0),
            receive: Mutex::new(DatagramReceive {
                queue: VecDeque::new(),
                queued_bytes: 0,
                last_sequence: None,
                stats: None,
                error: None,
            }),
            receive_notify: Notify::new(),
            retired: AtomicBool::new(false),
        };
        for sequence in [4, 2, 4, 3] {
            state
                .push(
                    Datagram {
                        flow_handle: 1,
                        sequence,
                        payload: vec![sequence as u8],
                    },
                    true,
                )
                .unwrap();
        }
        let receive = state.receive.lock().unwrap();
        assert_eq!(
            receive.queue.iter().cloned().collect::<Vec<_>>(),
            vec![vec![4], vec![2], vec![4], vec![3]]
        );
    }

    #[test]
    fn native_flow_accepts_reliable_fallback_after_sideband_loss() {
        assert!(accepts_datagram_source(DatagramDelivery::Native, true));
        assert!(accepts_datagram_source(DatagramDelivery::Native, false));
        assert!(accepts_datagram_source(
            DatagramDelivery::ReliableTunnel,
            false
        ));
        assert!(!accepts_datagram_source(
            DatagramDelivery::ReliableTunnel,
            true
        ));
    }

    #[test]
    fn byte_transfer_credit_is_cumulative_and_window_bounded() {
        let descriptor = yas_wire::transfer::Descriptor {
            transfer_id: 2,
            mode: yas_wire::transfer::Mode::Byte,
            direction: yas_wire::transfer::Direction::BIDIRECTIONAL,
            receiver_send_credit: 8,
            sender_send_credit: 8,
            max_item_bytes: 0,
            max_chunk_bytes: 8,
            content_family: family::NET,
            content_kind: 0,
            content_version: net::VERSION,
            extensions: Extensions::default(),
        };
        let state = ByteState::new(descriptor, 8);
        state.upload_sent.store(4, Ordering::Release);
        state
            .grant_upload(
                Credit {
                    transfer_id: 2,
                    cumulative_limit: 12,
                },
                false,
            )
            .unwrap();
        assert_eq!(*state.upload_credit.borrow(), 12);
        assert!(
            state
                .grant_upload(
                    Credit {
                        transfer_id: 2,
                        cumulative_limit: 13,
                    },
                    false,
                )
                .is_err()
        );
        assert!(
            state
                .grant_upload(
                    Credit {
                        transfer_id: 2,
                        cumulative_limit: 12,
                    },
                    true,
                )
                .is_err()
        );
    }
}
