//! Native YAS Channel and bidirectional MESSAGE Transfer helpers.

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, ops::Deref};

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, channel as wire,
    core::Status,
    family,
    transfer::{
        Close as TransferClose, Credit, Descriptor, Direction, MessageData, MessageReceiver, Mode,
        Reset as TransferReset,
    },
};

use crate::{
    receive::Lease as ReceiveLease,
    yas::{Client, Error as ClientError},
};

/// Receive credit held open for each connected native Channel.
pub const DEFAULT_RECEIVE_WINDOW: u64 = 4 * 1024 * 1024;

/// A reason for ending a native Channel transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseReason {
    Normal,
    Cancelled,
}

/// Terminal Channel state reported by the peer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Closed {
    pub status: u16,
    pub detail: String,
}

/// One complete peer message awaiting application consumption.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "channel data must be consumed or deliberately discarded"]
pub struct Delivery {
    transfer_id: u32,
    sequence: u64,
    wire_bytes: u64,
    payload: Vec<u8>,
}

impl Delivery {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

impl AsRef<[u8]> for Delivery {
    fn as_ref(&self) -> &[u8] {
        self.payload()
    }
}

impl Deref for Delivery {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.payload()
    }
}

/// One connected native Channel event.
#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    Data(Delivery),
    Acknowledged {
        cumulative_limit: u64,
        available: u64,
    },
    Closed(Closed),
}

/// One listener event.
#[derive(Debug)]
pub enum ListenerEvent {
    Accepted(Box<Channel>),
    Closed(Closed),
}

/// A native Channel helper failure.
#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    FeatureMissing,
    InvalidDescriptor,
    InvalidPayload,
    CreditExhausted { required: u64, available: u64 },
    CounterOverflow,
    DeliveryPending,
    StaleDelivery,
    Protocol(&'static str),
    Closed,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Channel value: {error}"),
            Self::FeatureMissing => formatter.write_str("native Channel operation is unavailable"),
            Self::InvalidDescriptor => formatter.write_str("invalid native Channel descriptor"),
            Self::InvalidPayload => formatter.write_str("invalid native Channel message"),
            Self::CreditExhausted {
                required,
                available,
            } => write!(
                formatter,
                "native Channel needs {required} bytes of credit; {available} available"
            ),
            Self::CounterOverflow => formatter.write_str("native Channel counter overflow"),
            Self::DeliveryPending => {
                formatter.write_str("previous native Channel delivery is still pending")
            }
            Self::StaleDelivery => formatter.write_str("stale native Channel delivery"),
            Self::Protocol(detail) => write!(formatter, "native Channel protocol error: {detail}"),
            Self::Closed => formatter.write_str("native Channel is closed"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::error::Error for Error {}

impl From<ClientError> for Error {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}

impl From<yas_wire::Error> for Error {
    fn from(value: yas_wire::Error) -> Self {
        Self::Wire(value)
    }
}

/// Opaque identity of a named native Channel listener.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListenerIdentity {
    pub handle: u64,
    pub generation: u64,
}

/// One listener owned by this guest session.
pub struct Listener {
    identity: ListenerIdentity,
    name: String,
    closed: bool,
}

impl fmt::Debug for Listener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Listener")
            .field("identity", &self.identity)
            .field("name", &self.name)
            .field("closed", &self.closed)
            .finish()
    }
}

impl Listener {
    pub fn identity(&self) -> ListenerIdentity {
        self.identity
    }

    pub fn handle(&self) -> u64 {
        self.identity.handle
    }

    pub fn generation(&self) -> u64 {
        self.identity.generation
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn accept(&mut self, client: &mut Client) -> Result<ListenerEvent, Error> {
        if self.closed {
            return Ok(ListenerEvent::Closed(Closed {
                status: Status::Ok.code(),
                detail: String::new(),
            }));
        }
        let identity = self.identity;
        let frame = client.next_matching_event(|frame| {
            frame.header.family == family::CHANNEL
                && frame.header.kind == wire::event_kind::ACCEPT
                && wire::Accept::decode(&frame.payload).is_ok_and(|accept| {
                    accept.listener_handle == identity.handle
                        && accept.generation == identity.generation
                })
        })?;
        let accept = wire::Accept::decode(&frame.payload)?;
        Ok(ListenerEvent::Accepted(Box::new(Channel::accepted(
            client,
            accept.endpoint,
        )?)))
    }

    /// Offer one decoded Channel Event to this listener without blocking.
    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<ListenerEvent>, Error> {
        if self.closed
            || frame.header.class != Class::Event
            || frame.header.family != family::CHANNEL
            || frame.header.kind != wire::event_kind::ACCEPT
        {
            return Ok(None);
        }
        let accept = wire::Accept::decode(&frame.payload)?;
        if accept.listener_handle != self.identity.handle
            || accept.generation != self.identity.generation
        {
            return Ok(None);
        }
        Ok(Some(ListenerEvent::Accepted(Box::new(Channel::accepted(
            client,
            accept.endpoint,
        )?))))
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::CHANNEL,
            wire::request_kind::CLOSE_LISTENER,
            wire::CloseListener {
                listener_handle: self.identity.handle,
                generation: self.identity.generation,
                extensions: Extensions::default(),
            }
            .encode()?,
            false,
        )?;
        self.closed = true;
        Ok(())
    }
}

/// One connected native Channel backed by a bidirectional MESSAGE Transfer.
pub struct Channel {
    handle: u64,
    peer_handle: u64,
    peer_session: [u8; 16],
    listener_metadata: Vec<u8>,
    connector_metadata: Vec<u8>,
    descriptor: Descriptor,
    send_credit: u64,
    sent: u64,
    receive_window: u64,
    granted: u64,
    received: u64,
    consumed: u64,
    next_send_sequence: u64,
    next_receive_sequence: u64,
    receiver: MessageReceiver,
    open: BTreeMap<u64, Vec<u8>>,
    complete: BTreeMap<u64, Vec<u8>>,
    pending_delivery: Option<(u64, u64)>,
    receive_terminal: Option<Closed>,
    receive_lease: ReceiveLease,
    send_closed: bool,
    receive_closed: bool,
}

impl fmt::Debug for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Channel")
            .field("handle", &self.handle)
            .field("peer_handle", &self.peer_handle)
            .field("peer_session", &self.peer_session)
            .field("transfer_id", &self.descriptor.transfer_id)
            .field("send_closed", &self.send_closed)
            .field("receive_closed", &self.receive_closed)
            .finish()
    }
}

impl Channel {
    fn new(
        client: &mut Client,
        endpoint: wire::ChannelEndpoint,
        mut receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        let descriptor = endpoint.descriptor;
        if descriptor.sender_send_credit != 0 {
            receive_lease.commit();
        }
        let valid = descriptor.validate().is_ok()
            && descriptor.mode == Mode::Message
            && descriptor.direction == Direction::BIDIRECTIONAL
            && descriptor.content_family == family::CHANNEL
            && descriptor.content_kind == wire::CHANNEL_CONTENT_KIND
            && descriptor.content_version == wire::VERSION
            && descriptor
                .sensitive_content()
                .is_ok_and(|sensitive| sensitive)
            && descriptor.max_item_bytes <= receive_lease.bytes()
            && descriptor.sender_send_credit <= descriptor.max_item_bytes;
        if !valid {
            if send_descriptor_reset(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidDescriptor);
        }
        let settled = if receive_lease.committed() {
            receive_lease.settle_to(descriptor.max_item_bytes)
        } else {
            receive_lease.shrink_to(descriptor.max_item_bytes)
        };
        if !settled
            || descriptor.mode != Mode::Message
            || descriptor.direction != Direction::BIDIRECTIONAL
            || descriptor.content_family != family::CHANNEL
            || descriptor.content_kind != wire::CHANNEL_CONTENT_KIND
            || descriptor.content_version != wire::VERSION
            || !descriptor.sensitive_content()?
        {
            if send_descriptor_reset(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidDescriptor);
        }
        let receiver = MessageReceiver::new(&descriptor)?;
        Ok(Self {
            handle: endpoint.channel_handle,
            peer_handle: endpoint.peer_channel_handle,
            peer_session: endpoint.peer_session,
            listener_metadata: endpoint.listener_metadata,
            connector_metadata: endpoint.connector_metadata,
            send_credit: descriptor.receiver_send_credit,
            receive_window: descriptor.max_item_bytes,
            granted: descriptor.sender_send_credit,
            descriptor,
            sent: 0,
            received: 0,
            consumed: 0,
            next_send_sequence: 0,
            next_receive_sequence: 0,
            receiver,
            open: BTreeMap::new(),
            complete: BTreeMap::new(),
            pending_delivery: None,
            receive_terminal: None,
            receive_lease,
            send_closed: false,
            receive_closed: false,
        })
    }

    fn activate(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.granted != 0 {
            self.receive_lease.commit();
        }
        if let Err(error) = self.grant_receive_credit(client, self.consumed) {
            // The host may have accepted the CREDIT despite reporting a send
            // failure. Commit before the compensating RESET so dropping this
            // half-activated Channel cannot recycle ambiguous peer authority.
            self.receive_lease.commit();
            if send_descriptor_reset(client, &self.descriptor, Status::ResourceExhausted).is_ok() {
                self.receive_lease.release();
                self.receive_closed = true;
            }
            return Err(error);
        }
        self.receive_lease.commit();
        Ok(())
    }

    /// ACCEPT starts with zero peer-to-listener credit by protocol. Reserve
    /// from the session aggregate and grant only that amount before exposing
    /// the Channel, otherwise both peers wait forever for the other side.
    fn accepted(client: &mut Client, endpoint: wire::ChannelEndpoint) -> Result<Self, Error> {
        let requested = endpoint.descriptor.max_item_bytes;
        let receive_lease = match client.receive_credit_exact(requested) {
            Ok(lease) => lease,
            Err(error) => {
                send_descriptor_reset(client, &endpoint.descriptor, Status::ResourceExhausted)?;
                return Err(error.into());
            }
        };
        let mut channel = Self::new(client, endpoint, receive_lease)?;
        channel.activate(client)?;
        Ok(channel)
    }

    fn connected(
        client: &mut Client,
        endpoint: wire::ChannelEndpoint,
        receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        let mut channel = Self::new(client, endpoint, receive_lease)?;
        channel.activate(client)?;
        Ok(channel)
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn peer_handle(&self) -> u64 {
        self.peer_handle
    }

    pub fn peer_session(&self) -> &[u8; 16] {
        &self.peer_session
    }

    pub fn listener_metadata(&self) -> &[u8] {
        &self.listener_metadata
    }

    pub fn connector_metadata(&self) -> &[u8] {
        &self.connector_metadata
    }

    pub fn available_credit(&self) -> u64 {
        self.send_credit.saturating_sub(self.sent)
    }

    pub fn has_pending_delivery(&self) -> bool {
        self.pending_delivery.is_some()
    }

    pub fn transfer_id(&self) -> u32 {
        self.descriptor.transfer_id
    }

    pub fn owns_frame(&self, frame: &Frame) -> bool {
        transfer_id(frame) == Some(self.descriptor.transfer_id)
    }

    pub fn send(&mut self, client: &mut Client, payload: &[u8]) -> Result<(), Error> {
        if self.send_closed {
            return Err(Error::Closed);
        }
        if payload.is_empty() || payload.len() as u64 > self.descriptor.max_item_bytes {
            return Err(Error::InvalidPayload);
        }
        let required = payload.len() as u64;
        if required > self.available_credit() {
            return Err(Error::CreditExhausted {
                required,
                available: self.available_credit(),
            });
        }
        let sequence = self.next_send_sequence;
        self.next_send_sequence = sequence.checked_add(1).ok_or(Error::CounterOverflow)?;
        let mut offset = 0usize;
        while offset < payload.len() {
            let end = payload
                .len()
                .min(offset.saturating_add(self.descriptor.max_chunk_bytes as usize));
            let fragment = MessageData {
                transfer_id: self.descriptor.transfer_id,
                sequence,
                fragment_offset: offset as u64,
                start: offset == 0,
                end: end == payload.len(),
                data: payload[offset..end].to_vec(),
            };
            client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::MESSAGE_DATA,
                &fragment,
                self.descriptor
                    .requires_sensitive_frame(yas_wire::transfer::kind::MESSAGE_DATA)?,
            )?;
            offset = end;
        }
        self.sent = self
            .sent
            .checked_add(required)
            .ok_or(Error::CounterOverflow)?;
        Ok(())
    }

    pub fn receive(&mut self, client: &mut Client) -> Result<Event, Error> {
        if self.pending_delivery.is_some() {
            return Err(Error::DeliveryPending);
        }
        loop {
            if let Some(event) = self.poll_event()? {
                return Ok(event);
            }
            if self.receive_closed {
                return Err(Error::Closed);
            }
            let transfer_id = self.descriptor.transfer_id;
            let frame = client.next_matching_event(|frame| {
                frame.header.family == family::TRANSFER
                    && frame.payload.get(..4).is_some_and(|bytes| {
                        u32::from_le_bytes(bytes.try_into().expect("four-byte Transfer ID"))
                            == transfer_id
                    })
            })?;
            if let Some(event) = self.offer_frame(&frame)? {
                return Ok(event);
            }
        }
    }

    /// Offer one decoded Transfer Event to this Channel without blocking.
    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<Event>, Error> {
        if !self.owns_frame(frame) {
            return Ok(None);
        }
        let sensitive = self
            .descriptor
            .requires_sensitive_frame(frame.header.kind)?;
        if frame.header.sensitive != sensitive {
            return Err(Error::Protocol("Transfer sensitivity mismatch"));
        }
        match frame.header.kind {
            yas_wire::transfer::kind::CREDIT => {
                let credit = Credit::decode(&frame.payload)?;
                if credit.cumulative_limit < self.send_credit || credit.cumulative_limit < self.sent
                {
                    return Err(Error::Protocol("Transfer credit moved backwards"));
                }
                self.send_credit = credit.cumulative_limit;
                Ok(Some(Event::Acknowledged {
                    cumulative_limit: credit.cumulative_limit,
                    available: self.available_credit(),
                }))
            }
            yas_wire::transfer::kind::MESSAGE_DATA => {
                if self.receive_closed {
                    return Err(Error::Protocol("Channel data followed terminal event"));
                }
                let fragment = MessageData::decode(&frame.payload)?;
                let end = self
                    .received
                    .checked_add(fragment.data.len() as u64)
                    .ok_or(Error::CounterOverflow)?;
                if end > self.granted {
                    return Err(Error::Protocol("peer exceeded Channel receive credit"));
                }
                let complete = self.receiver.accept(&fragment)?;
                if fragment.start && self.open.insert(fragment.sequence, Vec::new()).is_some() {
                    return Err(Error::Protocol("duplicate Channel message sequence"));
                }
                self.open
                    .get_mut(&fragment.sequence)
                    .ok_or(Error::Protocol("Channel message fragment has no start"))?
                    .extend_from_slice(&fragment.data);
                self.received = end;
                if complete {
                    let payload = self
                        .open
                        .remove(&fragment.sequence)
                        .ok_or(Error::Protocol("completed Channel message disappeared"))?;
                    if self.complete.insert(fragment.sequence, payload).is_some() {
                        return Err(Error::Protocol("duplicate completed Channel message"));
                    }
                }
                self.poll_event()
            }
            yas_wire::transfer::kind::CLOSE => {
                if self.receive_closed {
                    return Err(Error::Protocol("duplicate Channel terminal event"));
                }
                let close = TransferClose::decode(&frame.payload)?;
                if close.final_data_bytes != self.received
                    || self.receiver.open_messages() != 0
                    || !self.open.is_empty()
                {
                    return Err(Error::Protocol("Channel CLOSE accounting mismatch"));
                }
                self.receive_closed = true;
                self.receive_terminal = Some(Closed {
                    status: close.status,
                    detail: String::from_utf8_lossy(&close.detail).into_owned(),
                });
                self.poll_event()
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload)?;
                self.open.clear();
                self.complete.clear();
                self.receiver = MessageReceiver::new(&self.descriptor)
                    .expect("validated Channel descriptor remains valid");
                self.send_closed = true;
                self.receive_closed = true;
                self.receive_terminal = Some(Closed {
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                });
                self.poll_event()
            }
            _ => Err(Error::Protocol("unexpected Channel Transfer event")),
        }
    }

    pub fn consume(&mut self, client: &mut Client, delivery: Delivery) -> Result<Vec<u8>, Error> {
        self.finish_delivery(client, &delivery)?;
        Ok(delivery.payload)
    }

    pub fn discard(&mut self, client: &mut Client, delivery: Delivery) -> Result<(), Error> {
        self.finish_delivery(client, &delivery)
    }

    pub fn discard_pending(&mut self, client: &mut Client) -> Result<(), Error> {
        let (sequence, wire_bytes) = self.pending_delivery.ok_or(Error::StaleDelivery)?;
        self.acknowledge(client, sequence, wire_bytes)
    }

    /// Return already-buffered ordered data or a delayed peer terminal event.
    /// A CLOSE is observable only after every complete message preceding it
    /// has crossed the application consumption boundary.
    pub fn poll_event(&mut self) -> Result<Option<Event>, Error> {
        if self.pending_delivery.is_some() {
            return Ok(None);
        }
        if let Some(delivery) = self.pop_delivery()? {
            return Ok(Some(Event::Data(delivery)));
        }
        if self.receive_closed
            && self.open.is_empty()
            && self.complete.is_empty()
            && let Some(closed) = self.receive_terminal.take()
        {
            self.receive_lease.release();
            return Ok(Some(Event::Closed(closed)));
        }
        Ok(None)
    }

    pub fn close(&mut self, client: &mut Client, reason: CloseReason) -> Result<(), Error> {
        if self.send_closed && self.receive_closed {
            return Ok(());
        }
        if self.has_buffered_receive_data() {
            return Err(Error::DeliveryPending);
        }
        if reason == CloseReason::Cancelled {
            return self.reset(client, Status::Cancelled);
        }
        self.close_send(client)?;
        if self.receive_closed {
            self.clear_receive_buffers();
            self.receive_lease.release();
            return Ok(());
        }
        self.send_reset_frame(client, Status::Ok)?;
        self.clear_receive_buffers();
        self.receive_closed = true;
        self.receive_lease.release();
        Ok(())
    }

    /// Half-close only this Channel's send direction. The receive authority
    /// and its aggregate lease stay live until peer CLOSE/RESET or full close.
    pub fn close_send(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.send_closed {
            return Ok(());
        }
        let close = TransferClose {
            transfer_id: self.descriptor.transfer_id,
            final_data_bytes: self.sent,
            status: Status::Ok.code(),
            detail: Vec::new(),
        };
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CLOSE,
            &close,
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::CLOSE)?,
        )?;
        self.send_closed = true;
        Ok(())
    }

    pub fn reset(&mut self, client: &mut Client, status: Status) -> Result<(), Error> {
        if self.send_closed && self.receive_closed {
            return Ok(());
        }
        if self.has_buffered_receive_data() {
            return Err(Error::DeliveryPending);
        }
        self.send_reset_frame(client, status)?;
        self.clear_receive_buffers();
        self.send_closed = true;
        self.receive_closed = true;
        self.receive_lease.release();
        Ok(())
    }

    /// Fail-closed retirement for an internally owned Channel that cannot be
    /// returned to the application. Unlike `reset`, abandonment may discard
    /// buffered data because no Delivery survives the call. A failed RESET is
    /// authority-ambiguous, so poison the session and leave the committed
    /// receive lease pinned for session teardown.
    pub(crate) fn abandon(&mut self, client: &mut Client, status: Status) -> Result<(), Error> {
        if self.send_closed && self.receive_closed {
            self.clear_receive_buffers();
            self.receive_lease.release();
            return Ok(());
        }
        let reset = TransferReset {
            transfer_id: self.descriptor.transfer_id,
            status: status.code(),
            detail: Vec::new(),
        };
        let result = client.send_typed_terminal_cleanup(
            family::TRANSFER,
            yas_wire::transfer::kind::RESET,
            &reset,
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::RESET)
                .unwrap_or(false),
        );
        self.clear_receive_buffers();
        if let Err(error) = result {
            client.poison();
            return Err(error.into());
        }
        self.send_closed = true;
        self.receive_closed = true;
        self.receive_lease.release();
        Ok(())
    }

    /// Terminal close for a higher-level owner that has no outstanding raw
    /// Delivery. Buffered peer messages are intentionally discarded. Normal
    /// retirement preserves CLOSE-before-RESET ordering; cancellation is
    /// RESET-only.
    pub(crate) fn close_owned(
        &mut self,
        client: &mut Client,
        reason: CloseReason,
    ) -> Result<(), Error> {
        self.clear_receive_buffers();
        if reason == CloseReason::Cancelled {
            return self.abandon(client, Status::Cancelled);
        }
        if let Err(error) = self.close_send(client) {
            client.poison();
            let _ = self.abandon(client, Status::Ok);
            return Err(error);
        }
        if self.receive_closed {
            self.receive_lease.release();
            return Ok(());
        }
        self.abandon(client, Status::Ok)
    }

    fn send_reset_frame(&self, client: &mut Client, status: Status) -> Result<(), Error> {
        let reset = TransferReset {
            transfer_id: self.descriptor.transfer_id,
            status: status.code(),
            detail: Vec::new(),
        };
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::RESET,
            &reset,
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::RESET)?,
        )?;
        Ok(())
    }

    fn clear_receive_buffers(&mut self) {
        self.open.clear();
        self.complete.clear();
        self.pending_delivery = None;
        self.receive_terminal = None;
        self.receiver = MessageReceiver::new(&self.descriptor)
            .expect("validated Channel descriptor remains valid");
    }

    fn finish_delivery(&mut self, client: &mut Client, delivery: &Delivery) -> Result<(), Error> {
        if delivery.transfer_id != self.descriptor.transfer_id
            || self.pending_delivery != Some((delivery.sequence, delivery.wire_bytes))
        {
            return Err(Error::StaleDelivery);
        }
        self.acknowledge(client, delivery.sequence, delivery.wire_bytes)
    }

    fn pop_delivery(&mut self) -> Result<Option<Delivery>, Error> {
        let Some(payload) = self.complete.remove(&self.next_receive_sequence) else {
            return Ok(None);
        };
        let sequence = self.next_receive_sequence;
        self.next_receive_sequence = sequence.checked_add(1).ok_or(Error::CounterOverflow)?;
        let wire_bytes = payload.len() as u64;
        self.pending_delivery = Some((sequence, wire_bytes));
        Ok(Some(Delivery {
            transfer_id: self.descriptor.transfer_id,
            sequence,
            wire_bytes,
            payload,
        }))
    }

    fn acknowledge(
        &mut self,
        client: &mut Client,
        sequence: u64,
        wire_bytes: u64,
    ) -> Result<(), Error> {
        if self.pending_delivery != Some((sequence, wire_bytes)) {
            return Err(Error::StaleDelivery);
        }
        let consumed = self
            .consumed
            .checked_add(wire_bytes)
            .ok_or(Error::CounterOverflow)?;
        if !self.receive_closed {
            self.grant_receive_credit(client, consumed)?;
        }
        self.consumed = consumed;
        self.pending_delivery = None;
        Ok(())
    }

    fn has_buffered_receive_data(&self) -> bool {
        self.pending_delivery.is_some() || !self.open.is_empty() || !self.complete.is_empty()
    }

    fn grant_receive_credit(&mut self, client: &mut Client, consumed: u64) -> Result<(), Error> {
        let cumulative_limit = consumed
            .checked_add(self.receive_window)
            .ok_or(Error::CounterOverflow)?;
        if cumulative_limit <= self.granted {
            return Ok(());
        }
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CREDIT,
            &Credit {
                transfer_id: self.descriptor.transfer_id,
                cumulative_limit,
            },
            false,
        )?;
        self.granted = cumulative_limit;
        Ok(())
    }
}

fn transfer_id(frame: &Frame) -> Option<u32> {
    (frame.header.class == Class::Event
        && frame.header.family == family::TRANSFER
        && frame.payload.len() >= 4)
        .then(|| {
            u32::from_le_bytes(
                frame.payload[..4]
                    .try_into()
                    .expect("four-byte Transfer ID"),
            )
        })
}

fn send_descriptor_reset(
    client: &mut Client,
    descriptor: &Descriptor,
    status: Status,
) -> Result<(), Error> {
    if let Err(error) = client.send_typed_event(
        family::TRANSFER,
        yas_wire::transfer::kind::RESET,
        &TransferReset {
            transfer_id: descriptor.transfer_id,
            status: status.code(),
            detail: Vec::new(),
        },
        descriptor
            .requires_sensitive_frame(yas_wire::transfer::kind::RESET)
            .unwrap_or(false),
    ) {
        client.poison();
        return Err(error.into());
    }
    Ok(())
}

impl Client {
    /// Publish one named listener owned by this extension attempt.
    pub fn listen_channel(&mut self, name: &str, metadata: &[u8]) -> Result<Listener, Error> {
        if !self.supports(family::CHANNEL, Class::Request, wire::request_kind::LISTEN) {
            return Err(Error::FeatureMissing);
        }
        let mut operation_id = [0; 16];
        self.random(&mut operation_id)?;
        let result: wire::ListenResult = self.request_typed(
            family::CHANNEL,
            wire::request_kind::LISTEN,
            &wire::Listen {
                operation_id,
                name: name.to_string(),
                metadata: metadata.to_vec(),
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(Listener {
            identity: ListenerIdentity {
                handle: result.listener_handle,
                generation: result.generation,
            },
            name: name.to_string(),
            closed: false,
        })
    }

    /// Connect to one exact opaque listener identity.
    pub fn connect_channel(
        &mut self,
        listener: ListenerIdentity,
        metadata: &[u8],
    ) -> Result<Channel, Error> {
        if !self.supports(family::CHANNEL, Class::Request, wire::request_kind::CONNECT) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = self.receive_credit_exact(DEFAULT_RECEIVE_WINDOW)?;
        let initial_receive_credit = receive_lease.bytes();
        let endpoint: wire::ChannelEndpoint = self.request_typed_with_receive_lease(
            family::CHANNEL,
            wire::request_kind::CONNECT,
            &wire::Connect {
                listener_handle: listener.handle,
                generation: listener.generation,
                initial_receive_credit,
                metadata: metadata.to_vec(),
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        Channel::connected(self, endpoint, receive_lease)
    }
}

#[cfg(test)]
pub(crate) fn accepted_for_test(
    client: &mut Client,
    transfer_id: u32,
    receive_window: u64,
) -> Result<Channel, Error> {
    Channel::accepted(
        client,
        wire::ChannelEndpoint {
            channel_handle: u64::from(transfer_id) + 100,
            peer_channel_handle: u64::from(transfer_id) + 200,
            peer_session: [5; 16],
            listener_metadata: Vec::new(),
            connector_metadata: Vec::new(),
            descriptor: Descriptor {
                transfer_id,
                mode: Mode::Message,
                direction: Direction::BIDIRECTIONAL,
                receiver_send_credit: receive_window,
                sender_send_credit: 0,
                max_item_bytes: receive_window,
                max_chunk_bytes: 64 * 1024,
                content_family: family::CHANNEL,
                content_kind: wire::CHANNEL_CONTENT_KIND,
                content_version: wire::VERSION,
                extensions: Extensions(alloc::vec![
                    yas_wire::Extension {
                        tag: yas_wire::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16,
                        required: false,
                        value: wire::MAX_OPEN_MESSAGES.to_le_bytes().to_vec(),
                    },
                    yas_wire::Extension {
                        tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                        required: true,
                        value: Vec::new(),
                    },
                ]),
            },
            extensions: Extensions::default(),
        },
    )
}

#[cfg(test)]
pub(crate) fn listener_for_test(handle: u64, generation: u64) -> Listener {
    Listener {
        identity: ListenerIdentity { handle, generation },
        name: String::from("test-command"),
        closed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::VecDeque, rc::Rc, vec, vec::Vec};
    use std::cell::RefCell;
    use yas_wire::{
        Extension, FrameCodec, FrameHeader, FrameLimits,
        core::{
            FamilyDescriptor, Operation, ReceiveLimits, ResultPrefix, RuntimeState, ServerHello,
        },
        extension::{AttemptContext, Runtime, event_kind},
    };

    use crate::native_host;

    const LISTENER_HANDLE: u64 = 11;
    const LISTENER_GENERATION: u64 = 12;
    const TRANSFER_ID: u32 = 13;

    #[derive(Default)]
    struct HostState {
        incoming: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
        fail_sends: usize,
    }

    struct MockHost(Rc<RefCell<HostState>>);

    impl native_host::Host for MockHost {
        fn send(&mut self, packet: &[u8]) -> i32 {
            let mut state = self.0.borrow_mut();
            if state.fail_sends > 0 {
                state.fail_sends -= 1;
                return -1;
            }
            state.sent.push(packet.to_vec());
            0
        }

        fn recv(&mut self, buffer: &mut [u8]) -> i32 {
            let mut state = self.0.borrow_mut();
            let Some(packet) = state.incoming.front() else {
                return 0;
            };
            let length = packet.len();
            if length <= buffer.len() {
                buffer[..length].copy_from_slice(packet);
                state.incoming.pop_front();
            }
            length as i32
        }

        fn wait(&mut self, _deadline: i64) -> i32 {
            i32::from(!self.0.borrow().incoming.is_empty())
        }

        fn clock(&mut self, _kind: i32) -> i64 {
            0
        }

        fn random(&mut self, destination: &mut [u8]) {
            destination.fill(7);
        }
    }

    fn server_hello() -> ServerHello {
        ServerHello {
            minor: 1,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: ReceiveLimits::recommended(0),
            server_monotonic_ns: 3,
            catalog_revision: 1,
            server_name: String::from("test"),
            server_release: String::from("1"),
            families: vec![
                FamilyDescriptor {
                    family_id: family::CORE,
                    version: yas_wire::core::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::TRANSFER,
                    version: yas_wire::transfer::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::CHANNEL,
                    version: wire::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: wire::Limits::HARD.to_extensions().unwrap(),
                },
                FamilyDescriptor {
                    family_id: family::EXTENSION,
                    version: yas_wire::extension::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: vec![
                        Operation {
                            server_accepts: false,
                            server_sends: true,
                            class: Class::Event,
                            kind: event_kind::ATTEMPT_CONTEXT,
                        },
                        Operation {
                            server_accepts: true,
                            server_sends: false,
                            class: Class::Event,
                            kind: event_kind::ATTEMPT_OUTPUT,
                        },
                    ],
                    limits: yas_wire::extension::Limits::HARD.to_extensions().unwrap(),
                },
            ],
            extensions: Extensions::default(),
        }
    }

    fn attempt_context() -> AttemptContext {
        AttemptContext {
            extension_handle: 21,
            generation: 22,
            definition_revision: 23,
            attempt: 24,
            task_id: 25,
            flags: (yas_wire::schema::extension::DEFINITION_ENABLED
                | yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING)
                as u16,
            runtime: Runtime::Wasmi,
            content_hash: [4; 32],
            name: String::from("channel-test"),
            argv: Vec::new(),
            extensions: Extensions::default(),
        }
    }

    fn bootstrap_client() -> (Client, Rc<RefCell<HostState>>, native_host::Guard) {
        let hello = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: server_hello().encode().unwrap(),
        };
        let pre_hello = FrameCodec::pre_hello();
        let hello_frame = pre_hello
            .encode_stream(&Frame {
                header: FrameHeader::result(family::CORE, yas_wire::core::request_kind::HELLO, 1),
                payload: hello.encode().unwrap(),
            })
            .unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let context_frame = codec
            .encode_stream(&Frame {
                header: FrameHeader {
                    sensitive: true,
                    ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT)
                },
                payload: attempt_context().encode().unwrap(),
            })
            .unwrap();
        let state = Rc::new(RefCell::new(HostState {
            incoming: [hello_frame, context_frame].into(),
            sent: Vec::new(),
            fail_sends: 0,
        }));
        let guard = native_host::install(MockHost(state.clone()));
        let client = Client::bootstrap().unwrap();
        state.borrow_mut().sent.clear();
        (client, state, guard)
    }

    fn accept_with_window(receive_window: u64) -> wire::Accept {
        wire::Accept {
            listener_handle: LISTENER_HANDLE,
            generation: LISTENER_GENERATION,
            endpoint: wire::ChannelEndpoint {
                channel_handle: 31,
                peer_channel_handle: 32,
                peer_session: [5; 16],
                listener_metadata: Vec::new(),
                connector_metadata: Vec::new(),
                descriptor: Descriptor {
                    transfer_id: TRANSFER_ID,
                    mode: Mode::Message,
                    direction: Direction::BIDIRECTIONAL,
                    receiver_send_credit: receive_window,
                    sender_send_credit: 0,
                    max_item_bytes: receive_window,
                    max_chunk_bytes: 64 * 1024,
                    content_family: family::CHANNEL,
                    content_kind: wire::CHANNEL_CONTENT_KIND,
                    content_version: wire::VERSION,
                    extensions: Extensions(vec![
                        Extension {
                            tag: yas_wire::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16,
                            required: false,
                            value: wire::MAX_OPEN_MESSAGES.to_le_bytes().to_vec(),
                        },
                        Extension {
                            tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                            required: true,
                            value: Vec::new(),
                        },
                    ]),
                },
                extensions: Extensions::default(),
            },
        }
    }

    fn accept() -> wire::Accept {
        accept_with_window(DEFAULT_RECEIVE_WINDOW)
    }

    fn transfer_event(kind: u16, payload: Vec<u8>) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: kind != yas_wire::transfer::kind::CREDIT,
                ..FrameHeader::event(family::TRANSFER, kind)
            },
            payload,
        }
    }

    fn message(sequence: u64, payload: &[u8]) -> Frame {
        transfer_event(
            yas_wire::transfer::kind::MESSAGE_DATA,
            MessageData {
                transfer_id: TRANSFER_ID,
                sequence,
                fragment_offset: 0,
                start: true,
                end: true,
                data: payload.to_vec(),
            }
            .encode()
            .unwrap(),
        )
    }

    fn peer_close(final_data_bytes: u64) -> Frame {
        transfer_event(
            yas_wire::transfer::kind::CLOSE,
            TransferClose {
                transfer_id: TRANSFER_ID,
                final_data_bytes,
                status: Status::Ok.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        )
    }

    fn sent_event_kinds(state: &Rc<RefCell<HostState>>) -> Vec<u16> {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state
            .borrow()
            .sent
            .iter()
            .map(|packet| codec.decode_stream(packet).unwrap().0.header.kind)
            .collect()
    }

    fn listener() -> Listener {
        Listener {
            identity: ListenerIdentity {
                handle: LISTENER_HANDLE,
                generation: LISTENER_GENERATION,
            },
            name: String::from("test"),
            closed: false,
        }
    }

    fn assert_initial_credit(state: &Rc<RefCell<HostState>>) {
        let state = state.borrow();
        assert_eq!(state.sent.len(), 1);
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let (frame, consumed) = codec.decode_stream(&state.sent[0]).unwrap();
        assert_eq!(consumed, state.sent[0].len());
        assert_eq!(
            frame.header,
            FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT)
        );
        assert_eq!(
            Credit::decode(&frame.payload).unwrap(),
            Credit {
                transfer_id: TRANSFER_ID,
                cumulative_limit: DEFAULT_RECEIVE_WINDOW,
            }
        );
    }

    #[test]
    fn routed_accept_grants_receive_credit_before_exposing_channel() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut listener = listener();
        let frame = Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::CHANNEL, wire::event_kind::ACCEPT)
            },
            payload: accept().encode().unwrap(),
        };

        let event = listener.offer_frame(&mut client, &frame).unwrap().unwrap();
        let ListenerEvent::Accepted(channel) = event else {
            panic!("expected accepted Channel");
        };
        assert_eq!(channel.receive_window, DEFAULT_RECEIVE_WINDOW);
        assert_eq!(channel.granted, DEFAULT_RECEIVE_WINDOW);
        assert_initial_credit(&state);
    }

    #[test]
    fn blocking_accept_grants_receive_credit_before_exposing_channel() {
        let (mut client, state, _guard) = bootstrap_client();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::event(family::CHANNEL, wire::event_kind::ACCEPT)
                    },
                    payload: accept().encode().unwrap(),
                })
                .unwrap(),
        );

        let event = listener().accept(&mut client).unwrap();
        let ListenerEvent::Accepted(channel) = event else {
            panic!("expected accepted Channel");
        };
        assert_eq!(channel.receive_window, DEFAULT_RECEIVE_WINDOW);
        assert_eq!(channel.granted, DEFAULT_RECEIVE_WINDOW);
        assert_initial_credit(&state);
    }

    #[test]
    fn negotiated_smaller_channel_gets_exact_available_credit() {
        let (mut client, state, _guard) = bootstrap_client();
        let held = client.receive_credit_exact(14 * 1024 * 1024).unwrap();
        let frame = Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::CHANNEL, wire::event_kind::ACCEPT)
            },
            payload: accept_with_window(2 * 1024 * 1024).encode().unwrap(),
        };

        let event = listener()
            .offer_frame(&mut client, &frame)
            .unwrap()
            .unwrap();
        let ListenerEvent::Accepted(channel) = event else {
            panic!("expected accepted Channel");
        };
        assert_eq!(channel.receive_window, 2 * 1024 * 1024);
        assert_eq!(channel.granted, 2 * 1024 * 1024);
        assert_eq!(client.available_receive_credit(), 0);
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let (credit, _) = codec
            .decode_stream(state.borrow().sent.last().unwrap())
            .unwrap();
        assert_eq!(
            Credit::decode(&credit.payload).unwrap().cumulative_limit,
            2 * 1024 * 1024
        );

        drop(channel);
        drop(held);
        // The abandoned accepted Channel committed live peer authority, so
        // only the provisional holding lease is reusable.
        assert_eq!(client.available_receive_credit(), 14 * 1024 * 1024);
    }

    #[test]
    fn accepted_channel_resets_when_no_receive_authority_is_available() {
        let (mut client, state, _guard) = bootstrap_client();
        let held = client.receive_credit_exact(16 * 1024 * 1024).unwrap();
        let frame = Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::CHANNEL, wire::event_kind::ACCEPT)
            },
            payload: accept_with_window(1).encode().unwrap(),
        };

        assert!(matches!(
            listener().offer_frame(&mut client, &frame),
            Err(Error::Client(ClientError::ReceiveBudgetExhausted { .. }))
        ));
        assert_eq!(
            sent_event_kinds(&state),
            vec![yas_wire::transfer::kind::RESET]
        );
        assert_eq!(client.available_receive_credit(), 0);
        drop(held);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn accepted_credit_send_failure_resets_and_releases_reserved_authority() {
        let (mut client, state, _guard) = bootstrap_client();
        state.borrow_mut().fail_sends = 1;

        assert!(matches!(
            Channel::accepted(&mut client, accept().endpoint),
            Err(Error::Client(_))
        ));
        assert_eq!(
            sent_event_kinds(&state),
            vec![yas_wire::transfer::kind::RESET]
        );
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(client.receive_credit_exact(16 * 1024 * 1024).is_ok());
    }

    #[test]
    fn accepted_credit_and_reset_failures_poison_and_pin_authority() {
        let (mut client, state, _guard) = bootstrap_client();
        state.borrow_mut().fail_sends = 2;

        assert!(matches!(
            Channel::accepted(&mut client, accept().endpoint),
            Err(Error::Client(_))
        ));
        assert!(state.borrow().sent.is_empty());
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }

    #[test]
    fn internal_abandon_discards_buffered_delivery_and_releases_after_reset() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut channel = Channel::accepted(&mut client, accept().endpoint).unwrap();
        let Some(Event::Data(delivery)) = channel.offer_frame(&message(0, b"held")).unwrap() else {
            panic!("expected held Channel delivery");
        };
        drop(delivery);
        state.borrow_mut().sent.clear();

        channel.abandon(&mut client, Status::Cancelled).unwrap();
        assert_eq!(
            sent_event_kinds(&state),
            vec![yas_wire::transfer::kind::RESET]
        );
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn internal_abandon_reset_failure_poisons_and_pins_authority() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut channel = Channel::accepted(&mut client, accept().endpoint).unwrap();
        state.borrow_mut().sent.clear();
        state.borrow_mut().fail_sends = 1;

        assert!(matches!(
            channel.abandon(&mut client, Status::Cancelled),
            Err(Error::Client(_))
        ));
        assert!(state.borrow().sent.is_empty());
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }

    #[test]
    fn queued_out_of_order_data_drains_before_peer_close_releases_credit() {
        let (mut client, _state, _guard) = bootstrap_client();
        let mut channel = Channel::accepted(&mut client, accept().endpoint).unwrap();
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);

        assert!(channel.offer_frame(&message(1, b"b")).unwrap().is_none());
        let Some(Event::Data(first)) = channel.offer_frame(&message(0, b"a")).unwrap() else {
            panic!("expected sequence zero delivery");
        };
        assert_eq!(first.sequence(), 0);
        assert_eq!(first.payload(), b"a");
        assert!(channel.offer_frame(&peer_close(2)).unwrap().is_none());
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);

        assert_eq!(channel.consume(&mut client, first).unwrap(), b"a");
        let Some(Event::Data(second)) = channel.poll_event().unwrap() else {
            panic!("expected queued sequence one delivery");
        };
        assert_eq!(second.sequence(), 1);
        assert_eq!(second.payload(), b"b");
        assert_eq!(channel.consume(&mut client, second).unwrap(), b"b");
        let Some(Event::Closed(closed)) = channel.poll_event().unwrap() else {
            panic!("expected delayed peer close");
        };
        assert_eq!(closed.status, Status::Ok.code());
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn full_close_sends_close_then_reset_before_releasing_authority() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut channel = Channel::accepted(&mut client, accept().endpoint).unwrap();

        channel.close(&mut client, CloseReason::Normal).unwrap();

        assert_eq!(
            sent_event_kinds(&state),
            vec![
                yas_wire::transfer::kind::CREDIT,
                yas_wire::transfer::kind::CLOSE,
                yas_wire::transfer::kind::RESET,
            ]
        );
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn abandoned_live_channel_authority_is_not_reused() {
        let (mut client, _state, _guard) = bootstrap_client();
        let channel = Channel::accepted(&mut client, accept().endpoint).unwrap();
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);

        drop(channel);

        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(
            client.receive_credit_exact(13 * 1024 * 1024),
            Err(ClientError::ReceiveBudgetExhausted { .. })
        ));
    }
}
