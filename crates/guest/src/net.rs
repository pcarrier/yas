//! Native Net byte-flow helpers for extension guests.

use alloc::{string::String, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame,
    core::Status,
    family, net as wire,
    transfer::{ByteData, Close as TransferClose, Credit, Descriptor, Reset},
};

use crate::{
    receive::Lease as ReceiveLease,
    yas::{Client, Error as ClientError, MonotonicInstant},
};

pub const DEFAULT_STREAM_WINDOW: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    FeatureMissing,
    InvalidEndpoint,
    CreditExhausted { required: u64, available: u64 },
    DeliveryPending,
    StaleDelivery,
    CounterOverflow,
    Protocol(&'static str),
    Closed,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Net value: {error}"),
            Self::FeatureMissing => formatter.write_str("native Net operation is unavailable"),
            Self::InvalidEndpoint => formatter.write_str("native Net endpoint is not a BYTE flow"),
            Self::CreditExhausted {
                required,
                available,
            } => write!(
                formatter,
                "native Net flow needs {required} bytes of credit; {available} available"
            ),
            Self::DeliveryPending => formatter.write_str("native Net delivery is pending"),
            Self::StaleDelivery => formatter.write_str("stale native Net delivery"),
            Self::CounterOverflow => formatter.write_str("native Net counter overflow"),
            Self::Protocol(detail) => write!(formatter, "native Net protocol error: {detail}"),
            Self::Closed => formatter.write_str("native Net flow is closed"),
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

#[derive(Debug, Eq, PartialEq)]
#[must_use = "Net bytes must be consumed or deliberately discarded"]
pub struct Delivery {
    flow_handle: u64,
    transfer_id: u32,
    through: u64,
    data: Vec<u8>,
}

impl Delivery {
    pub fn flow_handle(&self) -> u64 {
        self.flow_handle
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    Read(Delivery),
    WriteCredit {
        cumulative_limit: u64,
        available: u64,
    },
    ReadClosed {
        status: u16,
        detail: String,
    },
    Reset {
        status: u16,
        detail: String,
    },
}

/// One native Net BYTE flow backed directly by its negotiated duplex Transfer.
pub struct ByteFlow {
    endpoint: wire::Endpoint,
    descriptor: Descriptor,
    sent: u64,
    write_credit: u64,
    received: u64,
    consumed: u64,
    read_credit: u64,
    receive_window: u64,
    receive_lease: ReceiveLease,
    pending: Option<u64>,
    write_closed: bool,
    read_closed: bool,
    closed: bool,
}

impl fmt::Debug for ByteFlow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ByteFlow")
            .field("flow_handle", &self.endpoint.flow_handle)
            .field("peer_address", &self.endpoint.peer_address)
            .finish_non_exhaustive()
    }
}

impl ByteFlow {
    fn new(
        client: &mut Client,
        endpoint: wire::Endpoint,
        mut receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        receive_lease.commit();
        if endpoint.mode != wire::FlowMode::Byte
            || endpoint.direction != wire::FlowDirection::DUPLEX
        {
            match &endpoint.descriptor {
                Some(descriptor)
                    if reset_endpoint(client, descriptor, Status::ResourceExhausted).is_ok() =>
                {
                    receive_lease.release();
                }
                Some(_) => {}
                None => client.poison(),
            }
            return Err(Error::InvalidEndpoint);
        }
        let descriptor = match endpoint.descriptor.clone() {
            Some(descriptor) => descriptor,
            None => {
                client.poison();
                return Err(Error::InvalidEndpoint);
            }
        };
        let receive_window = receive_lease.bytes();
        let valid = descriptor.validate().is_ok()
            && descriptor.mode == yas_wire::transfer::Mode::Byte
            && descriptor.direction == yas_wire::transfer::Direction::BIDIRECTIONAL
            && descriptor.sender_send_credit <= receive_window;
        if !valid {
            if reset_endpoint(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidEndpoint);
        }
        if descriptor.sender_send_credit < receive_window {
            receive_lease.commit();
            if let Err(error) = client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: descriptor.transfer_id,
                    cumulative_limit: receive_window,
                },
                false,
            ) {
                client.poison();
                return Err(error.into());
            }
        }
        Ok(Self {
            endpoint,
            sent: 0,
            write_credit: descriptor.receiver_send_credit,
            received: 0,
            consumed: 0,
            read_credit: receive_window,
            receive_window,
            receive_lease,
            pending: None,
            write_closed: false,
            read_closed: false,
            closed: false,
            descriptor,
        })
    }

    pub fn handle(&self) -> u64 {
        self.endpoint.flow_handle
    }

    pub fn endpoint(&self) -> &wire::Endpoint {
        &self.endpoint
    }

    pub fn available_write_credit(&self) -> u64 {
        self.write_credit.saturating_sub(self.sent)
    }

    pub fn owns_frame(&self, frame: &Frame) -> bool {
        self.pending.is_none()
            && !self.closed
            && frame.header.class == Class::Event
            && frame.header.family == family::TRANSFER
            && transfer_id(frame) == Some(self.descriptor.transfer_id)
    }

    pub fn write(&mut self, client: &mut Client, bytes: &[u8]) -> Result<(), Error> {
        if self.closed || self.write_closed {
            return Err(Error::Closed);
        }
        let required = bytes.len() as u64;
        let available = self.available_write_credit();
        if required > available {
            return Err(Error::CreditExhausted {
                required,
                available,
            });
        }
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = bytes
                .len()
                .min(offset.saturating_add(self.descriptor.max_chunk_bytes as usize));
            client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::BYTE_DATA,
                &ByteData {
                    transfer_id: self.descriptor.transfer_id,
                    offset: self.sent + offset as u64,
                    data: bytes[offset..end].to_vec(),
                },
                self.descriptor
                    .requires_sensitive_frame(yas_wire::transfer::kind::BYTE_DATA)?,
            )?;
            offset = end;
        }
        self.sent = self
            .sent
            .checked_add(required)
            .ok_or(Error::CounterOverflow)?;
        Ok(())
    }

    pub fn shutdown_write(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.write_closed {
            return Ok(());
        }
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CLOSE,
            &TransferClose {
                transfer_id: self.descriptor.transfer_id,
                final_data_bytes: self.sent,
                status: Status::Ok.code(),
                detail: Vec::new(),
            },
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::CLOSE)?,
        )?;
        self.write_closed = true;
        Ok(())
    }

    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<Event>, Error> {
        if self.closed
            || frame.header.class != Class::Event
            || frame.header.family != family::TRANSFER
            || transfer_id(frame) != Some(self.descriptor.transfer_id)
        {
            return Ok(None);
        }
        if frame.header.sensitive
            != self
                .descriptor
                .requires_sensitive_frame(frame.header.kind)?
        {
            return Err(Error::Protocol("Net Transfer sensitivity mismatch"));
        }
        match frame.header.kind {
            yas_wire::transfer::kind::CREDIT => {
                let credit = Credit::decode(&frame.payload)?;
                if credit.cumulative_limit < self.write_credit
                    || credit.cumulative_limit < self.sent
                {
                    return Err(Error::Protocol("Net write credit moved backwards"));
                }
                self.write_credit = credit.cumulative_limit;
                Ok(Some(Event::WriteCredit {
                    cumulative_limit: self.write_credit,
                    available: self.available_write_credit(),
                }))
            }
            yas_wire::transfer::kind::BYTE_DATA => {
                if self.pending.is_some() {
                    return Err(Error::DeliveryPending);
                }
                let data = ByteData::decode(&frame.payload)?;
                if data.offset != self.received {
                    return Err(Error::Protocol("Net read offset is not contiguous"));
                }
                let through = self
                    .received
                    .checked_add(data.data.len() as u64)
                    .ok_or(Error::CounterOverflow)?;
                if through > self.read_credit {
                    return Err(Error::Protocol("Net peer exceeded read credit"));
                }
                self.received = through;
                self.pending = Some(through);
                Ok(Some(Event::Read(Delivery {
                    flow_handle: self.endpoint.flow_handle,
                    transfer_id: self.descriptor.transfer_id,
                    through,
                    data: data.data,
                })))
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload)?;
                self.read_closed = true;
                if self.pending.is_none() {
                    self.receive_lease.release();
                }
                if close.final_data_bytes != self.received {
                    return Err(Error::Protocol("Net read CLOSE length mismatch"));
                }
                Ok(Some(Event::ReadClosed {
                    status: close.status,
                    detail: String::from_utf8_lossy(&close.detail).into_owned(),
                }))
            }
            yas_wire::transfer::kind::RESET => {
                let reset = Reset::decode(&frame.payload)?;
                self.read_closed = true;
                self.write_closed = true;
                self.closed = true;
                if self.pending.is_none() {
                    self.receive_lease.release();
                }
                Ok(Some(Event::Reset {
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                }))
            }
            _ => Err(Error::Protocol("unexpected Net BYTE Transfer event")),
        }
    }

    /// Wait for this flow's next Transfer event without consuming unrelated
    /// family traffic. `None` means the absolute deadline elapsed.
    pub fn next_event_until(
        &mut self,
        client: &mut Client,
        deadline: MonotonicInstant,
    ) -> Result<Option<Event>, Error> {
        if self.pending.is_some() {
            return Err(Error::DeliveryPending);
        }
        let Some(frame) =
            client.next_matching_frame_until(deadline, |frame| self.owns_frame(frame))?
        else {
            return Ok(None);
        };
        self.offer_frame(&frame)
    }

    pub fn consume(&mut self, client: &mut Client, delivery: Delivery) -> Result<Vec<u8>, Error> {
        self.finish_delivery(client, &delivery)?;
        Ok(delivery.data)
    }

    pub fn discard(&mut self, client: &mut Client, delivery: Delivery) -> Result<(), Error> {
        self.finish_delivery(client, &delivery)
    }

    fn finish_delivery(&mut self, client: &mut Client, delivery: &Delivery) -> Result<(), Error> {
        if delivery.flow_handle != self.endpoint.flow_handle
            || delivery.transfer_id != self.descriptor.transfer_id
            || self.pending != Some(delivery.through)
        {
            return Err(Error::StaleDelivery);
        }
        self.consumed = delivery.through;
        if self.read_closed || self.closed {
            self.pending = None;
            self.receive_lease.release();
            return Ok(());
        }
        let cumulative_limit = self
            .consumed
            .checked_add(self.receive_window)
            .ok_or(Error::CounterOverflow)?;
        if cumulative_limit > self.read_credit {
            client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: self.descriptor.transfer_id,
                    cumulative_limit,
                },
                false,
            )?;
            self.read_credit = cumulative_limit;
        }
        self.pending = None;
        Ok(())
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        let mut operation_id = [0; 16];
        client.random(&mut operation_id)?;
        if operation_id == [0; 16] {
            operation_id[15] = 1;
        }
        client.request(
            family::NET,
            wire::request_kind::CLOSE,
            wire::Close {
                flow_handle: self.endpoint.flow_handle,
                operation_id,
                extensions: Extensions::default(),
            }
            .encode()?,
            true,
        )?;
        self.read_closed = true;
        self.write_closed = true;
        self.closed = true;
        if self.pending.is_none() {
            self.receive_lease.release();
        }
        Ok(())
    }
}

fn reset_endpoint(
    client: &mut Client,
    descriptor: &Descriptor,
    status: Status,
) -> Result<(), Error> {
    if let Err(error) = client.send_typed_event(
        family::TRANSFER,
        yas_wire::transfer::kind::RESET,
        &Reset {
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

fn transfer_id(frame: &Frame) -> Option<u32> {
    frame
        .payload
        .get(..4)
        .map(|prefix| u32::from_le_bytes(prefix.try_into().expect("four-byte Transfer ID")))
}

impl Client {
    /// Open a raw native Net endpoint. Reliable callers normally use
    /// [`Client::open_byte_flow`] so Transfer credit is tracked for them.
    pub(crate) fn open_net_endpoint(
        &mut self,
        request: &wire::Open,
        receive_lease: &mut ReceiveLease,
    ) -> Result<wire::Endpoint, Error> {
        if !self.supports(family::NET, Class::Request, wire::request_kind::OPEN) {
            return Err(Error::FeatureMissing);
        }
        self.request_typed_with_receive_lease(
            family::NET,
            wire::request_kind::OPEN,
            request,
            true,
            receive_lease,
        )
        .map_err(Into::into)
    }

    pub(crate) fn open_net_endpoint_until(
        &mut self,
        request: &wire::Open,
        deadline: MonotonicInstant,
        receive_lease: &mut ReceiveLease,
    ) -> Result<wire::Endpoint, Error> {
        if !self.supports(family::NET, Class::Request, wire::request_kind::OPEN) {
            return Err(Error::FeatureMissing);
        }
        self.request_typed_with_receive_lease_until(
            family::NET,
            wire::request_kind::OPEN,
            request,
            true,
            deadline,
            receive_lease,
        )
        .map_err(Into::into)
    }

    /// Open one TCP/Unix/pipe BYTE flow with a bounded peer-to-client window.
    pub fn open_byte_flow(
        &mut self,
        address: wire::Address,
        tls_options: Option<wire::TlsOptions>,
        early_data: Vec<u8>,
    ) -> Result<ByteFlow, Error> {
        let mut operation_id = [0; 16];
        self.random(&mut operation_id)?;
        if operation_id == [0; 16] {
            operation_id[15] = 1;
        }
        let mut receive_lease = self.receive_credit_exact(DEFAULT_STREAM_WINDOW)?;
        let endpoint = self.open_net_endpoint(
            &wire::Open {
                operation_id,
                address,
                delivery_preference: wire::DeliveryPreference::NotApplicable,
                drop_policy: wire::DropPolicy::NotApplicable,
                initial_receive_credit: DEFAULT_STREAM_WINDOW,
                early_data,
                tls_options,
                extensions: Extensions::default(),
            },
            &mut receive_lease,
        )?;
        ByteFlow::new(self, endpoint, receive_lease)
    }

    pub fn open_byte_flow_until(
        &mut self,
        address: wire::Address,
        tls_options: Option<wire::TlsOptions>,
        early_data: Vec<u8>,
        deadline: MonotonicInstant,
    ) -> Result<ByteFlow, Error> {
        self.open_byte_flow_window_until(
            address,
            tls_options,
            early_data,
            deadline,
            DEFAULT_STREAM_WINDOW,
        )
    }

    /// Open one BYTE flow with an explicit peer-to-client window.
    ///
    /// The window is a standing invitation: the peer may send that many bytes
    /// before the flow is read even once, and every one of them lands in the
    /// client's bounded pending queue. A caller which reads a greeting and
    /// hangs up — a readiness probe — wants a window the size of the greeting,
    /// not [`DEFAULT_STREAM_WINDOW`]: a handful of probes against peers that
    /// answer with megabytes is enough to fill the queue, and a client which
    /// then makes any blocking request cannot read past what it invited.
    pub fn open_byte_flow_window_until(
        &mut self,
        address: wire::Address,
        tls_options: Option<wire::TlsOptions>,
        early_data: Vec<u8>,
        deadline: MonotonicInstant,
        receive_window: u64,
    ) -> Result<ByteFlow, Error> {
        let mut operation_id = [0; 16];
        self.random(&mut operation_id)?;
        if operation_id == [0; 16] {
            operation_id[15] = 1;
        }
        let mut receive_lease = self.receive_credit_exact(receive_window)?;
        let endpoint = self.open_net_endpoint_until(
            &wire::Open {
                operation_id,
                address,
                delivery_preference: wire::DeliveryPreference::NotApplicable,
                drop_policy: wire::DropPolicy::NotApplicable,
                initial_receive_credit: receive_window,
                early_data,
                tls_options,
                extensions: Extensions::default(),
            },
            deadline,
            &mut receive_lease,
        )?;
        ByteFlow::new(self, endpoint, receive_lease)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec, vec::Vec};
    use yas_wire::{
        FrameCodec, FrameHeader, FrameLimits,
        core::ResultPrefix,
        transfer::{Direction, Mode},
    };

    use crate::test_support::bootstrap_client;

    fn descriptor(transfer_id: u32) -> Descriptor {
        Descriptor {
            transfer_id,
            mode: Mode::Byte,
            direction: Direction::BIDIRECTIONAL,
            receiver_send_credit: DEFAULT_STREAM_WINDOW,
            sender_send_credit: DEFAULT_STREAM_WINDOW,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::NET,
            content_kind: yas_wire::schema::net::FLOW_CONTENT_KIND as u16,
            content_version: wire::VERSION,
            extensions: Extensions(vec![yas_wire::Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn endpoint(transfer_id: u32) -> wire::Endpoint {
        wire::Endpoint {
            flow_handle: u64::from(transfer_id) + 100,
            mode: wire::FlowMode::Byte,
            direction: wire::FlowDirection::DUPLEX,
            selected_delivery: wire::DatagramDelivery::NotApplicable,
            max_datagram_payload: 0,
            server_instance_limit: 1,
            max_message_bytes: 0,
            local_address: None,
            peer_address: wire::Address::Tcp {
                host: String::from("peer"),
                port: 1,
            },
            negotiated_alpn: Vec::new(),
            descriptor: Some(descriptor(transfer_id)),
            extensions: Extensions::default(),
        }
    }

    fn result_packet(
        codec: &FrameCodec,
        family_id: u16,
        kind: u16,
        request_id: u32,
        status: Status,
        body: Vec<u8>,
        sensitive: bool,
    ) -> Vec<u8> {
        codec
            .encode_stream(&Frame {
                header: FrameHeader {
                    sensitive,
                    ..FrameHeader::result(family_id, kind, request_id)
                },
                payload: ResultPrefix {
                    status,
                    detail: Extensions::default(),
                    body,
                }
                .encode()
                .unwrap(),
            })
            .unwrap()
    }

    fn flow(client: &mut Client, transfer_id: u32) -> ByteFlow {
        let lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        ByteFlow::new(client, endpoint(transfer_id), lease).unwrap()
    }

    fn data(transfer_id: u32, offset: u64, bytes: &[u8]) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::BYTE_DATA)
            },
            payload: ByteData {
                transfer_id,
                offset,
                data: bytes.to_vec(),
            }
            .encode()
            .unwrap(),
        }
    }

    fn close(transfer_id: u32, final_data_bytes: u64) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE)
            },
            payload: TransferClose {
                transfer_id,
                final_data_bytes,
                status: Status::Ok.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        }
    }

    fn reset(transfer_id: u32) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::RESET)
            },
            payload: Reset {
                transfer_id,
                status: Status::Cancelled.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        }
    }

    fn delivery(event: Option<Event>) -> Delivery {
        let Some(Event::Read(delivery)) = event else {
            panic!("expected Net read delivery");
        };
        delivery
    }

    #[test]
    fn held_delivery_retires_credit_only_after_close_or_reset_and_consume() {
        let (mut client, state, _guard) = bootstrap_client();

        let mut closed = flow(&mut client, 61);
        let first = delivery(closed.offer_frame(&data(61, 0, b"a")).unwrap());
        let sent_before_terminal = state.borrow().sent.len();
        assert!(matches!(
            closed.offer_frame(&close(61, 1)).unwrap(),
            Some(Event::ReadClosed { .. })
        ));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert_eq!(closed.consume(&mut client, first).unwrap(), b"a");
        assert_eq!(state.borrow().sent.len(), sent_before_terminal);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        let mut reset_flow = flow(&mut client, 62);
        let second = delivery(reset_flow.offer_frame(&data(62, 0, b"b")).unwrap());
        let sent_before_terminal = state.borrow().sent.len();
        assert!(matches!(
            reset_flow.offer_frame(&reset(62)).unwrap(),
            Some(Event::Reset { .. })
        ));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        reset_flow.discard(&mut client, second).unwrap();
        assert_eq!(state.borrow().sent.len(), sent_before_terminal);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn blocking_receive_does_not_skip_second_data_to_later_close() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut flow = flow(&mut client, 71);
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.extend([
            codec.encode_stream(&data(71, 0, b"a")).unwrap(),
            codec.encode_stream(&data(71, 1, b"b")).unwrap(),
            codec.encode_stream(&close(71, 2)).unwrap(),
        ]);

        let Some(Event::Read(first)) = flow
            .next_event_until(&mut client, MonotonicInstant::MAX)
            .unwrap()
        else {
            panic!("expected first Net read");
        };
        assert!(!flow.owns_frame(&data(71, 1, b"b")));
        assert!(!flow.owns_frame(&close(71, 2)));
        assert!(matches!(
            flow.next_event_until(&mut client, MonotonicInstant::MAX),
            Err(Error::DeliveryPending)
        ));
        assert_eq!(state.borrow().incoming.len(), 2);
        flow.consume(&mut client, first).unwrap();

        let Some(Event::Read(second)) = flow
            .next_event_until(&mut client, MonotonicInstant::MAX)
            .unwrap()
        else {
            panic!("expected second Net read");
        };
        assert!(matches!(
            flow.next_event_until(&mut client, MonotonicInstant::MAX),
            Err(Error::DeliveryPending)
        ));
        assert_eq!(state.borrow().incoming.len(), 1);
        flow.consume(&mut client, second).unwrap();
        assert!(matches!(
            flow.next_event_until(&mut client, MonotonicInstant::MAX)
                .unwrap(),
            Some(Event::ReadClosed { .. })
        ));
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn deadline_settlement_accepts_original_ok_on_either_side_of_cancel_result() {
        for original_first in [false, true] {
            let (mut client, state, _guard) = bootstrap_client();
            let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
            let original = result_packet(
                &codec,
                family::NET,
                wire::request_kind::OPEN,
                3,
                Status::Ok,
                endpoint(81).encode().unwrap(),
                true,
            );
            let cancel = result_packet(
                &codec,
                family::CORE,
                yas_wire::core::request_kind::CANCEL,
                5,
                Status::Conflict,
                Vec::new(),
                false,
            );
            let responses = if original_first {
                vec![original, cancel]
            } else {
                vec![cancel, original]
            };
            state
                .borrow_mut()
                .responses_after_send
                .push_back((2, responses));

            let mut flow = client
                .open_byte_flow_until(
                    wire::Address::Tcp {
                        host: String::from("peer"),
                        port: 1,
                    },
                    None,
                    Vec::new(),
                    MonotonicInstant::from_raw_nanos(1),
                )
                .unwrap();
            assert_eq!(flow.handle(), 181);
            assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
            assert!(matches!(
                flow.offer_frame(&reset(81)).unwrap(),
                Some(Event::Reset { .. })
            ));
            assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

            let sent = &state.borrow().sent;
            let (open, _) = codec.decode_stream(&sent[0]).unwrap();
            let (cancel, _) = codec.decode_stream(&sent[1]).unwrap();
            assert_eq!(open.header.family, family::NET);
            assert_eq!(open.header.kind, wire::request_kind::OPEN);
            assert_eq!(cancel.header.family, family::CORE);
            assert_eq!(cancel.header.kind, yas_wire::core::request_kind::CANCEL);
        }
    }

    #[test]
    fn repeated_deadline_cancellation_settles_original_and_reuses_full_budget() {
        let (mut client, state, _guard) = bootstrap_client();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        for attempt in 0..8usize {
            let original_request_id = 3 + u32::try_from(attempt).unwrap() * 4;
            let cancel_request_id = original_request_id + 2;
            state.borrow_mut().responses_after_send.push_back((
                (attempt + 1) * 2,
                vec![
                    result_packet(
                        &codec,
                        family::CORE,
                        yas_wire::core::request_kind::CANCEL,
                        cancel_request_id,
                        Status::Ok,
                        Vec::new(),
                        false,
                    ),
                    result_packet(
                        &codec,
                        family::NET,
                        wire::request_kind::OPEN,
                        original_request_id,
                        Status::Cancelled,
                        Vec::new(),
                        true,
                    ),
                ],
            ));
        }

        for _ in 0..8 {
            assert!(matches!(
                client.open_byte_flow_until(
                    wire::Address::Tcp {
                        host: String::from("peer"),
                        port: 1,
                    },
                    None,
                    Vec::new(),
                    MonotonicInstant::from_raw_nanos(1),
                ),
                Err(Error::Client(ClientError::RequestFailed {
                    family: family::NET,
                    kind: wire::request_kind::OPEN,
                    status: Status::Cancelled,
                    ..
                }))
            ));
            assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        }
        assert_eq!(state.borrow().sent.len(), 16);
    }
}
