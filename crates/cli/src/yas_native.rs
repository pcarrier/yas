//! Native YAS session client used by the Rust CLI.
//!
//! Endpoint selection is performed by [`crate::transport::connect_native`].
//! This module only speaks the YAS preface, HELLO, and framed family protocol;
//! it never probes the byte stream for an alternate protocol.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use yas_wire::frame::DatagramContext;
use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
    core::{
        CatalogStep, ClientHello, FamilyOffer, FamilyUpdate, GoAway, Ping, PingResult,
        ReceiveLimits, ResultPrefix, ServerHello, SessionUpdate, Status,
    },
    family,
    state::{Phase, StateAck, StateEvent, Unwatch, Watch, WatchResult},
    transfer::{
        ByteData, Close as TransferClose, Credit, Delivery, Descriptor, Direction,
        InlineOrTransfer, MessageData, MessageReceiver, Mode, Reset as TransferReset,
    },
};

use crate::transport;

const HELLO_REQUEST_ID: u32 = 1;
const WATCH_CREDIT: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_PENDING_FRAMES: usize = 1_024;
const MAX_PENDING_BYTES: usize = yas_wire::schema::transport::RECOMMENDED_BUFFERED as usize;
pub(crate) const MAX_COLLECTED_TRANSFER_BYTES: u64 = 256 * 1024 * 1024;

type Reader = Box<dyn AsyncRead + Unpin + Send>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

pub(crate) struct NativeClient {
    reader: Reader,
    writer: Writer,
    datagram: Option<transport::DatagramTransport>,
    inbound: FrameCodec,
    outbound: FrameCodec,
    hello: ServerHello,
    pending: VecDeque<Frame>,
    pending_bytes: usize,
    local_receive: ReceiveLimits,
    next_request_id: u32,
    started: std::time::Instant,
    negotiated_codecs: Vec<u16>,
}

/// Cloneable framed writer paired with [`NativeFrameReader`].
///
/// Long-lived multiplexed commands (notably Net) cannot hold `&mut
/// NativeClient` while waiting for the next inbound frame: other flows must be
/// able to write concurrently. Splitting after HELLO keeps one ordered writer
/// and one reader while preserving the negotiated codec and Core control
/// handling.
#[derive(Clone)]
pub(crate) struct NativeFrameSender {
    inner: Arc<NativeFrameSenderInner>,
}

struct NativeFrameSenderInner {
    writer: tokio::sync::Mutex<Writer>,
    outbound: RwLock<FrameCodec>,
    datagram: Option<transport::DatagramSender>,
}

pub(crate) struct NativeFrameReader {
    reader: Reader,
    inbound: FrameCodec,
    sender: NativeFrameSender,
    hello: ServerHello,
    started: std::time::Instant,
    negotiated_codecs: Vec<u16>,
    max_datagram: u32,
    datagram: Option<transport::DatagramReceiver>,
    _datagram_session: Option<transport::DatagramSession>,
}

impl NativeClient {
    pub(crate) async fn connect(on: Option<&str>, hub: &str) -> Result<Self, String> {
        let target = on.map(str::to_owned);
        let transport = transport::connect_native(&target, hub).await?;
        Self::connect_transport(transport, "yas-cli").await
    }

    pub(crate) async fn connect_transport(
        transport: transport::Transport,
        client_name: &str,
    ) -> Result<Self, String> {
        let (mut reader, mut writer, datagram) = transport.split_with_datagram();

        let families = yas_wire::schema::FAMILIES
            .iter()
            .filter(|metadata| metadata.id != family::CORE)
            .map(|metadata| FamilyOffer {
                family_id: metadata.id,
                versions: vec![metadata.version],
                required: false,
            })
            .collect();
        let max_datagram = datagram
            .as_ref()
            .map_or(0, transport::DatagramTransport::maximum);
        let hello_request = ClientHello {
            min_minor: 1,
            max_minor: 1,
            receive: ReceiveLimits::recommended(max_datagram),
            client_instance: rand::random(),
            client_name: client_name.to_string(),
            client_release: env!("CARGO_PKG_VERSION").to_string(),
            families,
            codecs: Vec::new(),
            // Say what this build is, so a client list can name it: the
            // extension is optional and a peer that ignores it loses nothing.
            extensions: Extensions(vec![
                yas_wire::core::Platform::current()
                    .extension(yas_wire::schema::core::CLIENT_HELLO_PLATFORM_EXTENSION as u16)
                    .map_err(|error| format!("invalid platform extension: {error}"))?,
            ]),
        };
        hello_request
            .validate()
            .map_err(|error| format!("invalid local HELLO: {error}"))?;
        let hello_frame = Frame {
            header: FrameHeader::request(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                HELLO_REQUEST_ID,
            ),
            payload: hello_request
                .encode()
                .map_err(|error| format!("cannot encode HELLO: {error}"))?,
        };
        let pre_hello = FrameCodec::pre_hello();
        let encoded = pre_hello
            .encode_stream(&hello_frame)
            .map_err(|error| format!("cannot frame HELLO: {error}"))?;
        writer
            .write_all(&yas_wire::PREFACE)
            .await
            .map_err(|error| format!("cannot send YAS preface: {error}"))?;
        writer
            .write_all(&encoded)
            .await
            .map_err(|error| format!("cannot send YAS HELLO: {error}"))?;

        let result = read_frame(&mut reader, &pre_hello).await?;
        if result.header
            != FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                HELLO_REQUEST_ID,
            )
        {
            return Err("native YAS listener returned an unexpected HELLO frame".into());
        }
        let prefix = ResultPrefix::decode(&result.payload)
            .map_err(|error| format!("cannot decode HELLO Result: {error}"))?;
        if !prefix.status.is_ok() {
            return Err(format!(
                "YAS HELLO failed with {:?}: {}",
                prefix.status,
                format_result_detail(&prefix.detail)
            ));
        }
        let hello = ServerHello::decode(&prefix.body)
            .map_err(|error| format!("cannot decode ServerHello: {error}"))?;
        hello
            .validate_for_client(&hello_request)
            .map_err(|error| format!("server selected an invalid YAS catalogue: {error}"))?;
        let codecs = hello
            .negotiated_codecs()
            .map_err(|error| format!("invalid negotiated codecs: {error}"))?
            .0;
        let inbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: hello_request.receive.max_frame,
                max_decoded_frame: hello_request.receive.max_decoded,
            },
            codecs.iter().copied(),
        )
        .map_err(|error| format!("invalid inbound YAS codec: {error}"))?;
        let outbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: hello.receive.max_frame,
                max_decoded_frame: hello.receive.max_decoded,
            },
            codecs.iter().copied(),
        )
        .map_err(|error| format!("invalid outbound YAS codec: {error}"))?;

        Ok(Self {
            reader,
            writer,
            datagram,
            inbound,
            outbound,
            hello,
            pending: VecDeque::new(),
            pending_bytes: 0,
            local_receive: hello_request.receive,
            next_request_id: 3,
            started: std::time::Instant::now(),
            negotiated_codecs: codecs,
        })
    }

    pub(crate) fn hello(&self) -> &ServerHello {
        &self.hello
    }

    pub(crate) fn supports_datagrams(&self) -> bool {
        self.datagram.is_some()
            && self.local_receive.max_datagram > 0
            && self.hello.receive.max_datagram > 0
    }

    /// Convert a connected client into independently driven framed halves.
    /// Request correlation and family-specific demultiplexing become the
    /// caller's responsibility; Core PING/GOAWAY/catalogue traffic remains
    /// handled by [`NativeFrameReader`].
    pub(crate) fn into_framed(self) -> (NativeFrameReader, NativeFrameSender) {
        let max_datagram = self.local_receive.max_datagram;
        let (datagram_sender, datagram_receiver, datagram_session) = match self.datagram {
            Some(datagram) => {
                let (sender, receiver, session) = datagram.into_parts();
                (Some(sender), Some(receiver), Some(session))
            }
            None => (None, None, None),
        };
        let sender = NativeFrameSender {
            inner: Arc::new(NativeFrameSenderInner {
                writer: tokio::sync::Mutex::new(self.writer),
                outbound: RwLock::new(self.outbound),
                datagram: datagram_sender,
            }),
        };
        let reader = NativeFrameReader {
            reader: self.reader,
            inbound: self.inbound,
            sender: sender.clone(),
            hello: self.hello,
            started: self.started,
            negotiated_codecs: self.negotiated_codecs,
            max_datagram,
            datagram: datagram_receiver,
            _datagram_session: datagram_session,
        };
        (reader, sender)
    }

    pub(crate) fn supports(&self, family_id: u16, class: Class, kind: u16) -> bool {
        self.hello
            .families
            .iter()
            .find(|descriptor| descriptor.family_id == family_id)
            .and_then(|descriptor| descriptor.operation(class, kind))
            .is_some_and(|operation| match class {
                Class::Event => operation.server_accepts || operation.server_sends,
                Class::Request => operation.server_accepts,
                Class::Result => false,
            })
    }

    pub(crate) async fn snapshot(
        &mut self,
        family_id: u16,
    ) -> Result<Option<Vec<yas_wire::state::Record>>, String> {
        let Some(descriptor) = self
            .hello
            .families
            .iter()
            .find(|descriptor| descriptor.family_id == family_id)
        else {
            return Ok(None);
        };
        let supports_watch = descriptor
            .operation(Class::Request, 0)
            .is_some_and(|operation| operation.server_accepts)
            && descriptor
                .operation(Class::Event, 0)
                .is_some_and(|operation| operation.server_sends);
        if !supports_watch {
            return Ok(None);
        }

        let watch = Watch {
            initial_credit: WATCH_CREDIT,
            resume: None,
            extensions: Extensions::default(),
        };
        // State snapshots can contain paths, command descriptors, extension
        // names, and other private session data.  Families which merely
        // allow sensitive WATCH frames accept this too, while families such
        // as Extension require it.
        let result = self
            .request(family_id, 0, watch.encode().map_err(wire_error)?, true)
            .await?;
        let watch_result = WatchResult::decode(&result).map_err(wire_error)?;
        let mut records = Vec::new();
        loop {
            let frame =
                tokio::time::timeout(REQUEST_TIMEOUT, self.next_matching_event(family_id, 0))
                    .await
                    .map_err(|_| {
                        format!("timed out waiting for family {family_id:#06x} snapshot")
                    })??;
            let event = StateEvent::decode(&frame.payload).map_err(wire_error)?;
            if event.subscription_id != watch_result.subscription_id {
                continue;
            }
            if event.phase == Phase::SnapshotBegin {
                records.clear();
            }
            if matches!(
                event.phase,
                Phase::SnapshotRecords | Phase::SnapshotEnd | Phase::Delta
            ) {
                records.extend(event.records);
            }
            self.send_event(
                family_id,
                1,
                StateAck {
                    subscription_id: event.subscription_id,
                    applied_revision: event.to_revision,
                    cumulative_byte_limit: WATCH_CREDIT,
                }
                .encode()
                .map_err(wire_error)?,
                false,
            )
            .await?;
            if event.phase == Phase::SnapshotEnd {
                break;
            }
            if event.phase == Phase::Reset {
                return Err(format!("family {family_id:#06x} reset its snapshot"));
            }
        }

        let unwatch = Unwatch {
            subscription_id: watch_result.subscription_id,
        };
        let _ = self
            .request(family_id, 1, unwatch.encode().map_err(wire_error)?, false)
            .await?;
        Ok(Some(records))
    }

    pub(crate) async fn request(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<Vec<u8>, String> {
        self.request_with_timeout(family_id, kind, payload, sensitive, REQUEST_TIMEOUT)
            .await
    }

    pub(crate) async fn request_with_timeout(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
        timeout: std::time::Duration,
    ) -> Result<Vec<u8>, String> {
        let prefix = self
            .request_result_with_timeout(family_id, kind, payload, sensitive, timeout)
            .await?;
        if prefix.status == Status::Ok {
            return Ok(prefix.body);
        }
        Err(format!(
            "YAS request {family_id:#06x}/{kind:#06x} failed with {:?}: {}",
            prefix.status,
            format_result_detail(&prefix.detail)
        ))
    }

    /// Send one correlated Request while leaving operation-level statuses to
    /// the caller. Commands with useful negative answers (for example KV
    /// `NOT_FOUND` and compare-and-swap `CONFLICT`) must not recover those
    /// statuses by parsing an error string.
    pub(crate) async fn request_result(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<ResultPrefix, String> {
        self.request_result_with_timeout(family_id, kind, payload, sensitive, REQUEST_TIMEOUT)
            .await
    }

    pub(crate) async fn request_result_with_timeout(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
        timeout: std::time::Duration,
    ) -> Result<ResultPrefix, String> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(2).max(3) | 1;
        let mut header = FrameHeader::request(family_id, kind, request_id);
        header.sensitive = sensitive;
        self.send(Frame { header, payload }).await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.read_next())
                .await
                .map_err(|_| {
                    format!("timed out waiting for {family_id:#06x}/{kind:#06x} Result")
                })??;
            if frame.header.class == Class::Result
                && frame.header.family == family_id
                && frame.header.kind == kind
                && frame.header.request_id == Some(request_id)
            {
                let prefix = ResultPrefix::decode(&frame.payload).map_err(wire_error)?;
                return Ok(prefix);
            }
            if frame.header.class == Class::Result {
                return Err(format!(
                    "YAS returned an uncorrelated Result for {:#06x}/{:#06x}/{:?}",
                    frame.header.family, frame.header.kind, frame.header.request_id
                ));
            }
            self.defer(frame)?;
        }
    }

    pub(crate) async fn request_typed<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
    ) -> Result<Response, String>
    where
        Request: Encode,
        Response: Decode,
    {
        let payload = request.encode().map_err(wire_error)?;
        let body = self.request(family_id, kind, payload, sensitive).await?;
        Response::decode(&body).map_err(wire_error)
    }

    pub(crate) async fn request_typed_with_timeout<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
        timeout: std::time::Duration,
    ) -> Result<Response, String>
    where
        Request: Encode,
        Response: Decode,
    {
        let payload = request.encode().map_err(wire_error)?;
        let body = self
            .request_with_timeout(family_id, kind, payload, sensitive, timeout)
            .await?;
        Response::decode(&body).map_err(wire_error)
    }

    pub(crate) async fn send_event(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<(), String> {
        let mut header = FrameHeader::event(family_id, kind);
        header.sensitive = sensitive;
        self.send(Frame { header, payload }).await
    }

    pub(crate) async fn send_typed_event<Event: Encode>(
        &mut self,
        family_id: u16,
        kind: u16,
        event: &Event,
        sensitive: bool,
    ) -> Result<(), String> {
        self.send_event(
            family_id,
            kind,
            event.encode().map_err(wire_error)?,
            sensitive,
        )
        .await
    }

    async fn send(&mut self, frame: Frame) -> Result<(), String> {
        let bytes = self.outbound.encode_stream(&frame).map_err(wire_error)?;
        self.writer
            .write_all(&bytes)
            .await
            .map_err(|error| format!("cannot write YAS frame: {error}"))
    }

    pub(crate) async fn next_matching_event(
        &mut self,
        family_id: u16,
        kind: u16,
    ) -> Result<Frame, String> {
        if let Some(index) = self.pending.iter().position(|frame| {
            frame.header.class == Class::Event
                && frame.header.family == family_id
                && frame.header.kind == kind
        }) {
            let frame = self
                .pending
                .remove(index)
                .ok_or_else(|| "pending YAS event disappeared".to_string())?;
            self.pending_bytes = self.pending_bytes.saturating_sub(frame.payload.len());
            return Ok(frame);
        }
        loop {
            let frame = self.read_next().await?;
            if frame.header.class == Class::Event
                && frame.header.family == family_id
                && frame.header.kind == kind
            {
                return Ok(frame);
            }
            self.defer(frame)?;
        }
    }

    pub(crate) async fn next_typed_event<Event: Decode>(
        &mut self,
        family_id: u16,
        kind: u16,
    ) -> Result<Event, String> {
        let frame = self.next_matching_event(family_id, kind).await?;
        Event::decode(&frame.payload).map_err(wire_error)
    }

    pub(crate) async fn next_event(&mut self) -> Result<Frame, String> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|frame| frame.header.class == Class::Event)
        {
            let frame = self
                .pending
                .remove(index)
                .ok_or_else(|| "pending YAS event disappeared".to_string())?;
            self.pending_bytes = self.pending_bytes.saturating_sub(frame.payload.len());
            return Ok(frame);
        }
        loop {
            let frame = self.read_next().await?;
            if frame.header.class == Class::Event {
                return Ok(frame);
            }
            if frame.header.class == Class::Result {
                return Err(format!(
                    "YAS returned an unsolicited Result for {:#06x}/{:#06x}/{:?}",
                    frame.header.family, frame.header.kind, frame.header.request_id
                ));
            }
        }
    }

    pub(crate) async fn receive_inline_or_transfer(
        &mut self,
        value: InlineOrTransfer,
        maximum: u64,
    ) -> Result<Vec<u8>, String> {
        if value.byte_len > maximum {
            return Err(format!(
                "YAS delivery is {} bytes; collection limit is {maximum}",
                value.byte_len
            ));
        }
        let bytes = match value.delivery {
            Delivery::Inline(bytes) => bytes,
            Delivery::Transfer(descriptor) => {
                self.receive_byte_transfer(&descriptor, Some(value.byte_len), maximum)
                    .await?
            }
        };
        if bytes.len() as u64 != value.byte_len {
            return Err(format!(
                "YAS delivery length mismatch: declared {}, received {}",
                value.byte_len,
                bytes.len()
            ));
        }
        if blake3::hash(&bytes).as_bytes() != &value.content_hash {
            return Err("YAS delivery content hash mismatch".to_string());
        }
        Ok(bytes)
    }

    pub(crate) async fn receive_byte_transfer(
        &mut self,
        descriptor: &Descriptor,
        expected_length: Option<u64>,
        maximum: u64,
    ) -> Result<Vec<u8>, String> {
        descriptor.validate().map_err(wire_error)?;
        if descriptor.mode != Mode::Byte || descriptor.direction != Direction::SENDER_TO_RECEIVER {
            return Err("YAS delivery did not provide a server-to-client BYTE Transfer".into());
        }
        if maximum == 0 || expected_length.is_some_and(|length| length > maximum) {
            return Err("invalid YAS Transfer collection limit".into());
        }
        if descriptor.sender_send_credit > maximum {
            return Err(format!(
                "YAS Transfer initial credit {} exceeds collection limit {maximum}",
                descriptor.sender_send_credit
            ));
        }
        if descriptor.sender_send_credit < maximum {
            self.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: descriptor.transfer_id,
                    cumulative_limit: maximum,
                },
                false,
            )
            .await?;
        }

        let capacity = expected_length
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0);
        let mut bytes = Vec::with_capacity(capacity);
        loop {
            let frame = self.next_transfer_event(descriptor.transfer_id).await?;
            let sensitive = descriptor
                .requires_sensitive_frame(frame.header.kind)
                .map_err(wire_error)?;
            if frame.header.sensitive != sensitive {
                return Err(format!(
                    "YAS Transfer {:#010x} sensitivity flag mismatch",
                    descriptor.transfer_id
                ));
            }
            match frame.header.kind {
                yas_wire::transfer::kind::BYTE_DATA => {
                    let data = ByteData::decode(&frame.payload).map_err(wire_error)?;
                    if data.offset != bytes.len() as u64
                        || data.data.len() > descriptor.max_chunk_bytes as usize
                    {
                        return Err(format!(
                            "YAS Transfer {:#010x} sent a non-contiguous or oversized chunk",
                            descriptor.transfer_id
                        ));
                    }
                    let next = (bytes.len() as u64)
                        .checked_add(data.data.len() as u64)
                        .ok_or_else(|| "YAS Transfer length overflow".to_string())?;
                    if next > maximum || expected_length.is_some_and(|length| next > length) {
                        return Err(format!(
                            "YAS Transfer {:#010x} exceeded its declared collection limit",
                            descriptor.transfer_id
                        ));
                    }
                    bytes.extend_from_slice(&data.data);
                }
                yas_wire::transfer::kind::CLOSE => {
                    let close = TransferClose::decode(&frame.payload).map_err(wire_error)?;
                    if close.final_data_bytes != bytes.len() as u64 {
                        return Err(format!(
                            "YAS Transfer {:#010x} CLOSE length mismatch",
                            descriptor.transfer_id
                        ));
                    }
                    if close.status != Status::Ok.code() {
                        return Err(format!(
                            "YAS Transfer {:#010x} closed with status {}: {}",
                            descriptor.transfer_id,
                            close.status,
                            String::from_utf8_lossy(&close.detail)
                        ));
                    }
                    if expected_length.is_some_and(|length| length != bytes.len() as u64) {
                        return Err(format!(
                            "YAS Transfer {:#010x} ended before its declared length",
                            descriptor.transfer_id
                        ));
                    }
                    return Ok(bytes);
                }
                yas_wire::transfer::kind::RESET => {
                    let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                    return Err(format!(
                        "YAS Transfer {:#010x} reset with status {}: {}",
                        descriptor.transfer_id,
                        reset.status,
                        String::from_utf8_lossy(&reset.detail)
                    ));
                }
                other => {
                    return Err(format!(
                        "YAS Transfer {:#010x} received unexpected event {other:#06x}",
                        descriptor.transfer_id
                    ));
                }
            }
        }
    }

    pub(crate) async fn send_byte_transfer(
        &mut self,
        descriptor: &Descriptor,
        bytes: &[u8],
    ) -> Result<(), String> {
        descriptor.validate().map_err(wire_error)?;
        if descriptor.mode != Mode::Byte || descriptor.direction != Direction::RECEIVER_TO_SENDER {
            return Err("YAS upload did not provide a client-to-server BYTE Transfer".into());
        }
        let mut offset = 0u64;
        let mut credit = descriptor.receiver_send_credit;
        while offset < bytes.len() as u64 {
            while offset >= credit {
                let frame = self.next_transfer_event(descriptor.transfer_id).await?;
                match frame.header.kind {
                    yas_wire::transfer::kind::CREDIT => {
                        if frame.header.sensitive {
                            return Err("YAS Transfer CREDIT was marked sensitive".into());
                        }
                        let update = Credit::decode(&frame.payload).map_err(wire_error)?;
                        if update.cumulative_limit <= credit {
                            return Err("YAS Transfer credit did not increase".into());
                        }
                        credit = update.cumulative_limit;
                    }
                    yas_wire::transfer::kind::RESET => {
                        let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                        return Err(format!(
                            "YAS Transfer {:#010x} reset with status {}: {}",
                            descriptor.transfer_id,
                            reset.status,
                            String::from_utf8_lossy(&reset.detail)
                        ));
                    }
                    other => {
                        return Err(format!(
                            "YAS Transfer {:#010x} received unexpected upload event {other:#06x}",
                            descriptor.transfer_id
                        ));
                    }
                }
            }
            let available = usize::try_from(credit - offset).unwrap_or(usize::MAX);
            let remaining = &bytes[usize::try_from(offset).map_err(|_| {
                "YAS Transfer upload offset exceeds this platform's address space".to_string()
            })?..];
            let length = remaining
                .len()
                .min(descriptor.max_chunk_bytes as usize)
                .min(available);
            if length == 0 {
                return Err("YAS Transfer made no upload progress".into());
            }
            let data = ByteData {
                transfer_id: descriptor.transfer_id,
                offset,
                data: remaining[..length].to_vec(),
            };
            self.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::BYTE_DATA,
                &data,
                descriptor
                    .requires_sensitive_frame(yas_wire::transfer::kind::BYTE_DATA)
                    .map_err(wire_error)?,
            )
            .await?;
            offset += length as u64;
        }
        let close = TransferClose {
            transfer_id: descriptor.transfer_id,
            final_data_bytes: offset,
            status: Status::Ok.code(),
            detail: Vec::new(),
        };
        self.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CLOSE,
            &close,
            descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::CLOSE)
                .map_err(wire_error)?,
        )
        .await
    }

    /// Collect a bounded server-to-client MESSAGE Transfer. Returned items
    /// are ordered by sequence number even when the peer interleaves their
    /// fragments within its negotiated open-message window.
    pub(crate) async fn receive_message_transfer(
        &mut self,
        descriptor: &Descriptor,
        maximum_bytes: u64,
        maximum_messages: usize,
    ) -> Result<Vec<Vec<u8>>, String> {
        descriptor.validate().map_err(wire_error)?;
        if descriptor.mode != Mode::Message || descriptor.direction != Direction::SENDER_TO_RECEIVER
        {
            return Err("YAS delivery did not provide a server-to-client MESSAGE Transfer".into());
        }
        if maximum_bytes == 0 || maximum_messages == 0 {
            return Err("invalid YAS MESSAGE Transfer collection limit".into());
        }
        if descriptor.sender_send_credit > maximum_bytes {
            return Err(format!(
                "YAS Transfer initial credit {} exceeds collection limit {maximum_bytes}",
                descriptor.sender_send_credit
            ));
        }
        if descriptor.sender_send_credit < maximum_bytes {
            self.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: descriptor.transfer_id,
                    cumulative_limit: maximum_bytes,
                },
                false,
            )
            .await?;
        }

        let mut validator = MessageReceiver::new(descriptor).map_err(wire_error)?;
        let mut open = BTreeMap::<u64, Vec<u8>>::new();
        let mut complete = BTreeMap::<u64, Vec<u8>>::new();
        let mut received = 0u64;
        loop {
            let frame = self.next_transfer_event(descriptor.transfer_id).await?;
            let sensitive = descriptor
                .requires_sensitive_frame(frame.header.kind)
                .map_err(wire_error)?;
            if frame.header.sensitive != sensitive {
                return Err(format!(
                    "YAS Transfer {:#010x} sensitivity flag mismatch",
                    descriptor.transfer_id
                ));
            }
            match frame.header.kind {
                yas_wire::transfer::kind::MESSAGE_DATA => {
                    let fragment = MessageData::decode(&frame.payload).map_err(wire_error)?;
                    let ended = validator.accept(&fragment).map_err(wire_error)?;
                    received = received
                        .checked_add(fragment.data.len() as u64)
                        .ok_or_else(|| "YAS MESSAGE Transfer length overflow".to_string())?;
                    if received > maximum_bytes {
                        return Err(format!(
                            "YAS Transfer {:#010x} exceeded its byte collection limit",
                            descriptor.transfer_id
                        ));
                    }
                    if fragment.start {
                        if open.len() + complete.len() >= maximum_messages
                            || complete.contains_key(&fragment.sequence)
                        {
                            return Err(format!(
                                "YAS Transfer {:#010x} exceeded its message collection limit",
                                descriptor.transfer_id
                            ));
                        }
                        open.insert(fragment.sequence, Vec::new());
                    }
                    let item = open.get_mut(&fragment.sequence).ok_or_else(|| {
                        format!(
                            "YAS Transfer {:#010x} lost an open message",
                            descriptor.transfer_id
                        )
                    })?;
                    item.extend_from_slice(&fragment.data);
                    if ended {
                        let item = open.remove(&fragment.sequence).ok_or_else(|| {
                            "completed YAS Transfer message disappeared".to_string()
                        })?;
                        if complete.insert(fragment.sequence, item).is_some() {
                            return Err("duplicate YAS Transfer message sequence".into());
                        }
                    }
                }
                yas_wire::transfer::kind::CLOSE => {
                    let close = TransferClose::decode(&frame.payload).map_err(wire_error)?;
                    if close.final_data_bytes != received || validator.open_messages() != 0 {
                        return Err(format!(
                            "YAS Transfer {:#010x} CLOSE accounting mismatch",
                            descriptor.transfer_id
                        ));
                    }
                    if close.status != Status::Ok.code() {
                        return Err(format!(
                            "YAS Transfer {:#010x} closed with status {}: {}",
                            descriptor.transfer_id,
                            close.status,
                            String::from_utf8_lossy(&close.detail)
                        ));
                    }
                    return Ok(complete.into_values().collect());
                }
                yas_wire::transfer::kind::RESET => {
                    let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                    return Err(format!(
                        "YAS Transfer {:#010x} reset with status {}: {}",
                        descriptor.transfer_id,
                        reset.status,
                        String::from_utf8_lossy(&reset.detail)
                    ));
                }
                other => {
                    return Err(format!(
                        "YAS Transfer {:#010x} received unexpected message event {other:#06x}",
                        descriptor.transfer_id
                    ));
                }
            }
        }
    }

    async fn next_transfer_event(&mut self, transfer_id: u32) -> Result<Frame, String> {
        if let Some(index) = self.pending.iter().position(|frame| {
            frame.header.class == Class::Event
                && frame.header.family == family::TRANSFER
                && transfer_event_id(frame) == Some(transfer_id)
        }) {
            let frame = self
                .pending
                .remove(index)
                .ok_or_else(|| "pending YAS Transfer event disappeared".to_string())?;
            self.pending_bytes = self.pending_bytes.saturating_sub(frame.payload.len());
            return Ok(frame);
        }
        loop {
            let frame = self.read_next().await?;
            if frame.header.class == Class::Event
                && frame.header.family == family::TRANSFER
                && transfer_event_id(&frame) == Some(transfer_id)
            {
                return Ok(frame);
            }
            self.defer(frame)?;
        }
    }

    fn defer(&mut self, frame: Frame) -> Result<(), String> {
        let next_bytes = self
            .pending_bytes
            .checked_add(frame.payload.len())
            .ok_or_else(|| "pending YAS frame accounting overflow".to_string())?;
        if self.pending.len() >= MAX_PENDING_FRAMES || next_bytes > MAX_PENDING_BYTES {
            return Err("native YAS peer exceeded the bounded pending-frame queue".into());
        }
        self.pending_bytes = next_bytes;
        self.pending.push_back(frame);
        Ok(())
    }

    async fn read_next(&mut self) -> Result<Frame, String> {
        loop {
            let frame = read_frame(&mut self.reader, &self.inbound).await?;
            if frame.header.class == Class::Request
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::request_kind::PING
            {
                self.answer_ping(frame).await?;
                continue;
            }
            if frame.header.class == Class::Request {
                return Err(format!(
                    "YAS server sent an unsupported peer Request {:#06x}/{:#06x}",
                    frame.header.family, frame.header.kind
                ));
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::GOAWAY
            {
                let goaway = GoAway::decode(&frame.payload).map_err(wire_error)?;
                return Err(format!(
                    "YAS server is closing with {:?}: {}",
                    goaway.status,
                    format_result_detail(&goaway.detail)
                ));
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::SESSION_UPDATE
            {
                self.apply_session_update(&frame.payload)?;
                continue;
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::FAMILY_UPDATE
            {
                self.apply_family_update(&frame.payload)?;
                continue;
            }
            return Ok(frame);
        }
    }

    async fn answer_ping(&mut self, frame: Frame) -> Result<(), String> {
        let request_id = frame
            .header
            .request_id
            .ok_or_else(|| "YAS PING Request has no request ID".to_string())?;
        let _ping = Ping::decode(&frame.payload).map_err(wire_error)?;
        let receive_ns = self.monotonic_ns();
        let result = PingResult {
            receiver_receive_ns: receive_ns,
            receiver_send_ns: self.monotonic_ns().max(receive_ns),
        };
        let prefix = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: result.encode().map_err(wire_error)?,
        };
        self.send(Frame {
            header: FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::PING,
                request_id,
            ),
            payload: prefix.encode().map_err(wire_error)?,
        })
        .await
    }

    pub(crate) fn monotonic_ns(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn apply_session_update(&mut self, payload: &[u8]) -> Result<(), String> {
        let update = SessionUpdate::decode(payload).map_err(wire_error)?;
        let step = update
            .validate_after(self.hello.catalog_revision, &self.hello.receive)
            .map_err(wire_error)?;
        if step == CatalogStep::Gap {
            return Err(format!(
                "YAS catalogue jumped from revision {} to {}; reconnect required",
                self.hello.catalog_revision, update.catalog_revision
            ));
        }
        self.outbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: update.receive.max_frame,
                max_decoded_frame: update.receive.max_decoded,
            },
            self.negotiated_codecs.iter().copied(),
        )
        .map_err(wire_error)?;
        self.hello.receive = update.receive;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }

    fn apply_family_update(&mut self, payload: &[u8]) -> Result<(), String> {
        let update = FamilyUpdate::decode(payload).map_err(wire_error)?;
        let descriptor = self
            .hello
            .families
            .iter_mut()
            .find(|descriptor| descriptor.family_id == update.family.family_id)
            .ok_or_else(|| {
                format!(
                    "YAS FAMILY_UPDATE introduced unknown family {:#06x}; reconnect required",
                    update.family.family_id
                )
            })?;
        let step = update
            .validate_after(self.hello.catalog_revision, descriptor)
            .map_err(wire_error)?;
        if step == CatalogStep::Gap {
            return Err(format!(
                "YAS catalogue jumped from revision {} to {}; reconnect required",
                self.hello.catalog_revision, update.catalog_revision
            ));
        }
        *descriptor = update.family;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }
}

impl NativeFrameSender {
    pub(crate) async fn send(&self, frame: Frame) -> Result<(), String> {
        let bytes = {
            let codec = self
                .inner
                .outbound
                .read()
                .map_err(|_| "native YAS outbound codec lock is poisoned".to_string())?;
            codec.encode_stream(&frame).map_err(wire_error)?
        };
        self.inner
            .writer
            .lock()
            .await
            .write_all(&bytes)
            .await
            .map_err(|error| format!("cannot write YAS frame: {error}"))
    }

    /// Attempt one message-preserving transport datagram. Queue or SCTP
    /// congestion returns `Ok(false)` and is ordinary packet loss.
    pub(crate) fn try_send_datagram(
        &self,
        frame: &Frame,
        maximum: u32,
        context: DatagramContext,
    ) -> Result<transport::DatagramSend, String> {
        let sender = self
            .inner
            .datagram
            .as_ref()
            .ok_or_else(|| "native YAS datagram transport is unavailable".to_string())?;
        let bytes = self
            .inner
            .outbound
            .read()
            .map_err(|_| "native YAS outbound codec lock is poisoned".to_string())?
            .encode_datagram(frame, maximum, context)
            .map_err(wire_error)?;
        Ok(sender.try_send(bytes))
    }

    fn replace_codec(&self, codec: FrameCodec) -> Result<(), String> {
        *self
            .inner
            .outbound
            .write()
            .map_err(|_| "native YAS outbound codec lock is poisoned".to_string())? = codec;
        Ok(())
    }
}

impl NativeFrameReader {
    /// Read the next family frame after servicing Core control traffic.
    pub(crate) async fn next(&mut self) -> Result<Frame, String> {
        self.next_with_source().await.map(|(frame, _)| frame)
    }

    /// Read the next family frame and report whether it arrived on the lossy
    /// transport sideband. Malformed datagrams are dropped without affecting
    /// the reliable session.
    pub(crate) async fn next_with_source(&mut self) -> Result<(Frame, bool), String> {
        loop {
            let (frame, datagram) = if let Some(receiver) = self.datagram.as_mut() {
                tokio::select! {
                    result = read_frame(&mut self.reader, &self.inbound) => (result?, false),
                    bytes = receiver.recv() => {
                        let Some(bytes) = bytes else {
                            self.datagram = None;
                            continue;
                        };
                        let Some(frame) = decode_transport_datagram(
                            &self.inbound,
                            &bytes,
                            self.max_datagram,
                        ) else {
                            continue;
                        };
                        (frame, true)
                    }
                }
            } else {
                (read_frame(&mut self.reader, &self.inbound).await?, false)
            };
            if frame.header.class == Class::Request
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::request_kind::PING
            {
                self.answer_ping(frame).await?;
                continue;
            }
            if frame.header.class == Class::Request {
                return Err(format!(
                    "YAS server sent an unsupported peer Request {:#06x}/{:#06x}",
                    frame.header.family, frame.header.kind
                ));
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::GOAWAY
            {
                let goaway = GoAway::decode(&frame.payload).map_err(wire_error)?;
                return Err(format!(
                    "YAS server is closing with {:?}: {}",
                    goaway.status,
                    format_result_detail(&goaway.detail)
                ));
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::SESSION_UPDATE
            {
                self.apply_session_update(&frame.payload)?;
                continue;
            }
            if frame.header.class == Class::Event
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::event_kind::FAMILY_UPDATE
            {
                self.apply_family_update(&frame.payload)?;
                continue;
            }
            return Ok((frame, datagram));
        }
    }

    async fn answer_ping(&mut self, frame: Frame) -> Result<(), String> {
        let request_id = frame
            .header
            .request_id
            .ok_or_else(|| "YAS PING Request has no request ID".to_string())?;
        let _ping = Ping::decode(&frame.payload).map_err(wire_error)?;
        let receive_ns = self.monotonic_ns();
        let result = PingResult {
            receiver_receive_ns: receive_ns,
            receiver_send_ns: self.monotonic_ns().max(receive_ns),
        };
        let prefix = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: result.encode().map_err(wire_error)?,
        };
        self.sender
            .send(Frame {
                header: FrameHeader::result(
                    family::CORE,
                    yas_wire::core::request_kind::PING,
                    request_id,
                ),
                payload: prefix.encode().map_err(wire_error)?,
            })
            .await
    }

    fn monotonic_ns(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn apply_session_update(&mut self, payload: &[u8]) -> Result<(), String> {
        let update = SessionUpdate::decode(payload).map_err(wire_error)?;
        let step = update
            .validate_after(self.hello.catalog_revision, &self.hello.receive)
            .map_err(wire_error)?;
        if step == CatalogStep::Gap {
            return Err(format!(
                "YAS catalogue jumped from revision {} to {}; reconnect required",
                self.hello.catalog_revision, update.catalog_revision
            ));
        }
        let codec = FrameCodec::new(
            FrameLimits {
                max_wire_frame: update.receive.max_frame,
                max_decoded_frame: update.receive.max_decoded,
            },
            self.negotiated_codecs.iter().copied(),
        )
        .map_err(wire_error)?;
        self.sender.replace_codec(codec)?;
        self.hello.receive = update.receive;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }

    fn apply_family_update(&mut self, payload: &[u8]) -> Result<(), String> {
        let update = FamilyUpdate::decode(payload).map_err(wire_error)?;
        let descriptor = self
            .hello
            .families
            .iter_mut()
            .find(|descriptor| descriptor.family_id == update.family.family_id)
            .ok_or_else(|| {
                format!(
                    "YAS FAMILY_UPDATE introduced unknown family {:#06x}; reconnect required",
                    update.family.family_id
                )
            })?;
        let step = update
            .validate_after(self.hello.catalog_revision, descriptor)
            .map_err(wire_error)?;
        if step == CatalogStep::Gap {
            return Err(format!(
                "YAS catalogue jumped from revision {} to {}; reconnect required",
                self.hello.catalog_revision, update.catalog_revision
            ));
        }
        *descriptor = update.family;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }
}

fn decode_transport_datagram(codec: &FrameCodec, bytes: &[u8], maximum: u32) -> Option<Frame> {
    let probe = codec.decode(bytes).ok()?;
    let context = match (probe.header.family, probe.header.class, probe.header.kind) {
        (family::NET, Class::Event, yas_wire::schema::net::event::DATAGRAM) => {
            DatagramContext::NetNativeFlow
        }
        (family::SURFACE, Class::Event, yas_wire::schema::surface::event::FRAME) => {
            DatagramContext::SurfaceFrame
        }
        (family::MEDIA, Class::Event, yas_wire::schema::media::event::FRAME) => {
            DatagramContext::MediaFrame
        }
        _ => return None,
    };
    codec.decode_datagram(bytes, maximum, context).ok()
}

async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
    codec: &FrameCodec,
) -> Result<Frame, String> {
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .await
        .map_err(|error| format!("cannot read YAS frame length: {error}"))?;
    let length = u32::from_le_bytes(length) as usize;
    let total = length
        .checked_add(4)
        .ok_or_else(|| "YAS frame length overflow".to_string())?;
    if total > codec.limits().max_wire_frame as usize + 4 {
        return Err(format!("YAS frame exceeds negotiated limit: {length}"));
    }
    let mut bytes = vec![0; total];
    bytes[..4].copy_from_slice(&(length as u32).to_le_bytes());
    reader
        .read_exact(&mut bytes[4..])
        .await
        .map_err(|error| format!("cannot read YAS frame: {error}"))?;
    let (frame, consumed) = codec.decode_stream(&bytes).map_err(wire_error)?;
    if consumed != bytes.len() {
        return Err("YAS decoder did not consume one complete frame".into());
    }
    Ok(frame)
}

fn transfer_event_id(frame: &Frame) -> Option<u32> {
    (frame.header.class == Class::Event
        && frame.header.family == family::TRANSFER
        && matches!(
            frame.header.kind,
            yas_wire::transfer::kind::BYTE_DATA
                | yas_wire::transfer::kind::MESSAGE_DATA
                | yas_wire::transfer::kind::CREDIT
                | yas_wire::transfer::kind::CLOSE
                | yas_wire::transfer::kind::RESET
        )
        && frame.payload.len() >= 4)
        .then(|| {
            u32::from_le_bytes(
                frame.payload[..4]
                    .try_into()
                    .expect("checked Transfer ID length"),
            )
        })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn wire_error(error: yas_wire::Error) -> String {
    format!("YAS wire error: {error}")
}

fn format_result_detail(detail: &Extensions) -> String {
    if detail.0.is_empty() {
        "no detail".to_string()
    } else {
        detail
            .0
            .iter()
            .map(|extension| {
                format!(
                    "tag {}{}={}",
                    extension.tag,
                    if extension.required { "!" } else { "" },
                    hex(&extension.value)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::core::{FamilyDescriptor, Operation, RuntimeState, SessionUpdate};

    fn test_server_hello() -> ServerHello {
        ServerHello {
            minor: 1,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: ReceiveLimits::recommended(0),
            server_monotonic_ns: 3,
            catalog_revision: 1,
            server_name: "home".into(),
            server_release: "test".into(),
            families: vec![FamilyDescriptor {
                family_id: family::CORE,
                version: yas_wire::core::VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![
                    Operation {
                        server_accepts: true,
                        server_sends: true,
                        class: Class::Request,
                        kind: yas_wire::core::request_kind::PING,
                    },
                    Operation {
                        server_accepts: false,
                        server_sends: true,
                        class: Class::Event,
                        kind: yas_wire::core::event_kind::SESSION_UPDATE,
                    },
                ],
                limits: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }
    }

    #[tokio::test]
    async fn native_session_answers_peer_ping_without_legacy_fallback() {
        let (client_stream, mut server_stream) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let mut preface = [0; yas_wire::PREFACE.len()];
            server_stream.read_exact(&mut preface).await.unwrap();
            assert_eq!(preface, yas_wire::PREFACE);

            let pre_hello = FrameCodec::pre_hello();
            let hello_frame = read_frame(&mut server_stream, &pre_hello).await.unwrap();
            assert_eq!(
                hello_frame.header,
                FrameHeader::request(
                    family::CORE,
                    yas_wire::core::request_kind::HELLO,
                    HELLO_REQUEST_ID,
                )
            );
            let client_hello = ClientHello::decode(&hello_frame.payload).unwrap();
            assert_eq!(client_hello.client_name, "yas-test");

            let hello = test_server_hello();
            let hello_result = ResultPrefix {
                status: Status::Ok,
                detail: Extensions::default(),
                body: hello.encode().unwrap(),
            };
            let result_frame = Frame {
                header: FrameHeader::result(
                    family::CORE,
                    yas_wire::core::request_kind::HELLO,
                    HELLO_REQUEST_ID,
                ),
                payload: hello_result.encode().unwrap(),
            };
            server_stream
                .write_all(&pre_hello.encode_stream(&result_frame).unwrap())
                .await
                .unwrap();

            let codec = FrameCodec::new(
                FrameLimits {
                    max_wire_frame: client_hello.receive.max_frame,
                    max_decoded_frame: client_hello.receive.max_decoded,
                },
                [],
            )
            .unwrap();
            let ping = Frame {
                header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::PING, 2),
                payload: Ping {
                    sender_monotonic_ns: 9,
                }
                .encode()
                .unwrap(),
            };
            server_stream
                .write_all(&codec.encode_stream(&ping).unwrap())
                .await
                .unwrap();
            let update = SessionUpdate {
                catalog_revision: 2,
                receive: hello.receive,
                extensions: Extensions::default(),
            };
            let update_frame = Frame {
                header: FrameHeader::event(
                    family::CORE,
                    yas_wire::core::event_kind::SESSION_UPDATE,
                ),
                payload: update.encode().unwrap(),
            };
            server_stream
                .write_all(&codec.encode_stream(&update_frame).unwrap())
                .await
                .unwrap();

            let ping_result_frame = read_frame(&mut server_stream, &codec).await.unwrap();
            assert_eq!(
                ping_result_frame.header,
                FrameHeader::result(family::CORE, yas_wire::core::request_kind::PING, 2,)
            );
            let prefix = ResultPrefix::decode(&ping_result_frame.payload).unwrap();
            assert_eq!(prefix.status, Status::Ok);
            let ping_result = PingResult::decode(&prefix.body).unwrap();
            assert!(ping_result.receiver_send_ns >= ping_result.receiver_receive_ns);
        });

        let mut client = NativeClient::connect_transport(
            transport::Transport::Duplex(client_stream),
            "yas-test",
        )
        .await
        .unwrap();
        let error = client
            .next_matching_event(family::CORE, yas_wire::core::event_kind::SESSION_UPDATE)
            .await
            .unwrap_err();
        assert!(error.contains("cannot read YAS frame length"), "{error}");
        assert_eq!(client.hello().catalog_revision, 2);
        server.await.unwrap();
    }
}
