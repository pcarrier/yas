//! Bounded native BYTE and MESSAGE Transfer collection for guest helpers.

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Frame,
    core::Status,
    family,
    transfer::{
        ByteData, Close, Credit, Delivery, Descriptor, Direction, InlineOrTransfer, MessageData,
        MessageReceiver as WireMessageReceiver, Mode, Reset,
    },
};

use crate::receive::Lease as ReceiveLease;
use crate::yas::{Client, Error as ClientError};

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    InvalidDescriptor,
    LimitExceeded { declared: u64, maximum: u64 },
    LengthMismatch,
    HashMismatch,
    NonContiguous,
    Sensitivity,
    Closed { status: u16, detail: String },
    Reset { status: u16, detail: String },
    UnexpectedEvent(u16),
    TooManyMessages { actual: usize, maximum: usize },
}

/// Send one finite client-to-server BYTE Transfer, waiting for cumulative
/// credit as needed and closing with the exact final byte count.
pub fn send_byte_transfer(
    client: &mut Client,
    descriptor: &Descriptor,
    bytes: &[u8],
) -> Result<(), Error> {
    match send_byte_transfer_inner(client, descriptor, bytes) {
        Ok(()) => Ok(()),
        // A peer terminal Event already retired the Transfer.
        Err(error @ (Error::Reset { .. } | Error::Closed { .. })) => Err(error),
        Err(error) => {
            retire_failed_byte_sender(client, descriptor);
            Err(error)
        }
    }
}

fn send_byte_transfer_inner(
    client: &mut Client,
    descriptor: &Descriptor,
    bytes: &[u8],
) -> Result<(), Error> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte || descriptor.direction != Direction::RECEIVER_TO_SENDER {
        return Err(Error::InvalidDescriptor);
    }
    let mut sent = 0u64;
    let mut credit = descriptor.receiver_send_credit;
    while sent < bytes.len() as u64 {
        if sent == credit {
            let frame = client.next_matching_event(|frame| {
                frame_transfer_id(frame) == Some(descriptor.transfer_id)
            })?;
            if frame.header.sensitive != descriptor.requires_sensitive_frame(frame.header.kind)? {
                return Err(Error::Sensitivity);
            }
            match frame.header.kind {
                yas_wire::transfer::kind::CREDIT => {
                    let update = Credit::decode(&frame.payload)?;
                    if update.cumulative_limit < credit || update.cumulative_limit < sent {
                        return Err(Error::NonContiguous);
                    }
                    credit = update.cumulative_limit;
                }
                yas_wire::transfer::kind::RESET => {
                    let reset = Reset::decode(&frame.payload)?;
                    return Err(Error::Reset {
                        status: reset.status,
                        detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                    });
                }
                yas_wire::transfer::kind::CLOSE => {
                    let close = Close::decode(&frame.payload)?;
                    return Err(Error::Closed {
                        status: close.status,
                        detail: String::from_utf8_lossy(&close.detail).into_owned(),
                    });
                }
                kind => return Err(Error::UnexpectedEvent(kind)),
            }
            continue;
        }
        let available = usize::try_from(credit.saturating_sub(sent)).unwrap_or(usize::MAX);
        let offset = usize::try_from(sent).map_err(|_| Error::LengthMismatch)?;
        let end = bytes.len().min(
            offset
                .saturating_add(descriptor.max_chunk_bytes as usize)
                .min(offset.saturating_add(available)),
        );
        if end == offset {
            return Err(Error::NonContiguous);
        }
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::BYTE_DATA,
            &ByteData {
                transfer_id: descriptor.transfer_id,
                offset: sent,
                data: bytes[offset..end].to_vec(),
            },
            descriptor.requires_sensitive_frame(yas_wire::transfer::kind::BYTE_DATA)?,
        )?;
        sent = end as u64;
    }
    client.send_typed_event(
        family::TRANSFER,
        yas_wire::transfer::kind::CLOSE,
        &Close {
            transfer_id: descriptor.transfer_id,
            final_data_bytes: sent,
            status: Status::Ok.code(),
            detail: Vec::new(),
        },
        descriptor.requires_sensitive_frame(yas_wire::transfer::kind::CLOSE)?,
    )?;
    Ok(())
}

fn retire_failed_byte_sender(client: &mut Client, descriptor: &Descriptor) {
    let result = client.send_typed_terminal_cleanup(
        family::TRANSFER,
        yas_wire::transfer::kind::RESET,
        &Reset {
            transfer_id: descriptor.transfer_id,
            status: Status::Cancelled.code(),
            detail: Vec::new(),
        },
        descriptor
            .requires_sensitive_frame(yas_wire::transfer::kind::RESET)
            .unwrap_or(false),
    );
    if result.is_err() {
        client.poison();
        return;
    }
    client.purge_pending_transfer_ids(&[descriptor.transfer_id]);
}

fn frame_transfer_id(frame: &Frame) -> Option<u32> {
    (frame.header.class == Class::Event && frame.header.family == family::TRANSFER)
        .then(|| frame.payload.get(..4))
        .flatten()
        .map(|prefix| u32::from_le_bytes(prefix.try_into().expect("four-byte Transfer ID")))
}

/// Incremental bounded server-to-client BYTE Transfer receiver.
pub struct ByteReceiver {
    descriptor: Descriptor,
    expected_length: Option<u64>,
    maximum: u64,
    bytes: Vec<u8>,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl ByteReceiver {
    pub fn new(
        client: &mut Client,
        descriptor: Descriptor,
        expected_length: Option<u64>,
        maximum: u64,
    ) -> Result<Self, Error> {
        let lease = match client.receive_credit_exact(maximum) {
            Ok(lease) => lease,
            Err(error) => {
                reset_resource_exhausted(client, &descriptor)?;
                return Err(error.into());
            }
        };
        Self::new_with_lease(client, descriptor, expected_length, maximum, lease)
    }

    pub(crate) fn new_with_lease(
        client: &mut Client,
        descriptor: Descriptor,
        expected_length: Option<u64>,
        maximum: u64,
        mut receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        if descriptor.sender_send_credit != 0 {
            receive_lease.commit();
        }
        if let Err(error) = descriptor.validate() {
            if reset_descriptor(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(error.into());
        }
        if descriptor.mode != Mode::Byte || descriptor.direction != Direction::SENDER_TO_RECEIVER {
            if reset_resource_exhausted(client, &descriptor).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidDescriptor);
        }
        if maximum == 0 || expected_length.is_some_and(|length| length > maximum) {
            if reset_resource_exhausted(client, &descriptor).is_ok() {
                receive_lease.release();
            }
            return Err(Error::LimitExceeded {
                declared: expected_length.unwrap_or(maximum),
                maximum,
            });
        }
        let target = maximum;
        if descriptor.sender_send_credit > target {
            if reset_resource_exhausted(client, &descriptor).is_ok() {
                receive_lease.release();
            }
            return Err(Error::LimitExceeded {
                declared: descriptor.sender_send_credit,
                maximum: target,
            });
        }
        let settled = if receive_lease.committed() {
            receive_lease.settle_to(target)
        } else {
            receive_lease.shrink_to(target)
        };
        if receive_lease.bytes() < target || !settled {
            reset_resource_exhausted(client, &descriptor)?;
            return Err(ClientError::ReceiveBudgetExhausted {
                requested: target,
                available: receive_lease.bytes(),
            }
            .into());
        }
        if descriptor.sender_send_credit != 0 {
            // The descriptor has already committed this peer authority. Pin
            // the reservation if a later CREDIT send or wrapper construction
            // fails without a successful RESET.
            receive_lease.commit();
        }
        if descriptor.sender_send_credit < target {
            if let Err(error) = client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: descriptor.transfer_id,
                    cumulative_limit: target,
                },
                false,
            ) {
                client.poison();
                return Err(error.into());
            }
            receive_lease.commit();
        }
        let capacity = expected_length
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0);
        Ok(Self {
            descriptor,
            expected_length,
            maximum,
            bytes: Vec::with_capacity(capacity),
            closed: false,
            receive_lease,
        })
    }

    pub fn transfer_id(&self) -> u32 {
        self.descriptor.transfer_id
    }

    pub fn owns_frame(&self, frame: &Frame) -> bool {
        !self.closed && frame_transfer_id(frame) == Some(self.descriptor.transfer_id)
    }

    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<Vec<u8>>, Error> {
        if !self.owns_frame(frame) {
            return Ok(None);
        }
        if frame.header.sensitive
            != self
                .descriptor
                .requires_sensitive_frame(frame.header.kind)?
        {
            return Err(Error::Sensitivity);
        }
        match frame.header.kind {
            yas_wire::transfer::kind::BYTE_DATA => {
                let data = ByteData::decode(&frame.payload)?;
                if data.offset != self.bytes.len() as u64
                    || data.data.len() > self.descriptor.max_chunk_bytes as usize
                {
                    return Err(Error::NonContiguous);
                }
                let next = (self.bytes.len() as u64)
                    .checked_add(data.data.len() as u64)
                    .ok_or(Error::LengthMismatch)?;
                if next > self.maximum || self.expected_length.is_some_and(|length| next > length) {
                    return Err(Error::LimitExceeded {
                        declared: next,
                        maximum: self
                            .expected_length
                            .unwrap_or(self.maximum)
                            .min(self.maximum),
                    });
                }
                self.bytes.extend_from_slice(&data.data);
                Ok(None)
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = Close::decode(&frame.payload)?;
                let valid_length = close.final_data_bytes == self.bytes.len() as u64
                    && !self
                        .expected_length
                        .is_some_and(|length| length != self.bytes.len() as u64);
                let bytes = core::mem::take(&mut self.bytes);
                self.closed = true;
                self.receive_lease.release();
                if !valid_length {
                    return Err(Error::LengthMismatch);
                }
                if close.status != Status::Ok.code() {
                    return Err(Error::Closed {
                        status: close.status,
                        detail: String::from_utf8_lossy(&close.detail).into_owned(),
                    });
                }
                Ok(Some(bytes))
            }
            yas_wire::transfer::kind::RESET => {
                let reset = Reset::decode(&frame.payload)?;
                self.bytes.clear();
                self.closed = true;
                self.receive_lease.release();
                Err(Error::Reset {
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                })
            }
            kind => Err(Error::UnexpectedEvent(kind)),
        }
    }

    pub fn cancel(&mut self, client: &mut Client) -> Result<bool, Error> {
        if self.closed {
            return Ok(false);
        }
        if let Err(error) = client.send_typed_terminal_cleanup(
            family::TRANSFER,
            yas_wire::transfer::kind::RESET,
            &Reset {
                transfer_id: self.descriptor.transfer_id,
                status: Status::Cancelled.code(),
                detail: Vec::new(),
            },
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::RESET)?,
        ) {
            client.poison();
            return Err(error.into());
        }
        client.purge_pending_transfer_ids(&[self.descriptor.transfer_id]);
        self.bytes.clear();
        self.closed = true;
        self.receive_lease.release();
        Ok(true)
    }

    fn retire_after_error(&mut self, client: &mut Client) {
        if self.closed {
            return;
        }
        let sensitivity = self
            .descriptor
            .requires_sensitive_frame(yas_wire::transfer::kind::RESET);
        let result = sensitivity.map_err(Error::from).and_then(|sensitive| {
            client
                .send_typed_terminal_cleanup(
                    family::TRANSFER,
                    yas_wire::transfer::kind::RESET,
                    &Reset {
                        transfer_id: self.descriptor.transfer_id,
                        status: Status::Cancelled.code(),
                        detail: Vec::new(),
                    },
                    sensitive,
                )
                .map_err(Error::from)
        });
        if result.is_err() {
            client.poison();
            return;
        }
        client.purge_pending_transfer_ids(&[self.descriptor.transfer_id]);
        self.bytes.clear();
        self.closed = true;
        self.receive_lease.release();
    }
}

/// Incremental bounded server-to-client MESSAGE Transfer collector.
pub struct MessageCollector {
    descriptor: Descriptor,
    validator: WireMessageReceiver,
    maximum_bytes: u64,
    maximum_messages: usize,
    open: BTreeMap<u64, Vec<u8>>,
    messages: Vec<Vec<u8>>,
    total_data: u64,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl MessageCollector {
    pub fn new(
        client: &mut Client,
        descriptor: Descriptor,
        maximum_bytes: u64,
        maximum_messages: usize,
    ) -> Result<Self, Error> {
        let lease = match client.receive_credit_exact(maximum_bytes) {
            Ok(lease) => lease,
            Err(error) => {
                reset_resource_exhausted(client, &descriptor)?;
                return Err(error.into());
            }
        };
        Self::new_with_lease(client, descriptor, maximum_bytes, maximum_messages, lease)
    }

    pub(crate) fn new_with_lease(
        client: &mut Client,
        descriptor: Descriptor,
        maximum_bytes: u64,
        maximum_messages: usize,
        mut receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        if descriptor.sender_send_credit != 0 {
            receive_lease.commit();
        }
        if let Err(error) = descriptor.validate() {
            if reset_descriptor(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(error.into());
        }
        if descriptor.mode != Mode::Message
            || descriptor.direction != Direction::SENDER_TO_RECEIVER
            || maximum_bytes == 0
            || maximum_messages == 0
        {
            if reset_resource_exhausted(client, &descriptor).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidDescriptor);
        }
        if descriptor.sender_send_credit > maximum_bytes {
            if reset_resource_exhausted(client, &descriptor).is_ok() {
                receive_lease.release();
            }
            return Err(Error::LimitExceeded {
                declared: descriptor.sender_send_credit,
                maximum: maximum_bytes,
            });
        }
        let settled = if receive_lease.committed() {
            receive_lease.settle_to(maximum_bytes)
        } else {
            receive_lease.shrink_to(maximum_bytes)
        };
        if receive_lease.bytes() < maximum_bytes || !settled {
            reset_resource_exhausted(client, &descriptor)?;
            return Err(ClientError::ReceiveBudgetExhausted {
                requested: maximum_bytes,
                available: receive_lease.bytes(),
            }
            .into());
        }
        if descriptor.sender_send_credit != 0 {
            receive_lease.commit();
        }
        if descriptor.sender_send_credit < maximum_bytes {
            if let Err(error) = client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::CREDIT,
                &Credit {
                    transfer_id: descriptor.transfer_id,
                    cumulative_limit: maximum_bytes,
                },
                false,
            ) {
                client.poison();
                return Err(error.into());
            }
            receive_lease.commit();
        }
        let validator = WireMessageReceiver::new(&descriptor)?;
        Ok(Self {
            descriptor,
            validator,
            maximum_bytes,
            maximum_messages,
            open: BTreeMap::new(),
            messages: Vec::new(),
            total_data: 0,
            closed: false,
            receive_lease,
        })
    }

    pub fn transfer_id(&self) -> u32 {
        self.descriptor.transfer_id
    }

    pub fn owns_frame(&self, frame: &Frame) -> bool {
        !self.closed && frame_transfer_id(frame) == Some(self.descriptor.transfer_id)
    }

    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<Vec<Vec<u8>>>, Error> {
        if !self.owns_frame(frame) {
            return Ok(None);
        }
        if frame.header.sensitive
            != self
                .descriptor
                .requires_sensitive_frame(frame.header.kind)?
        {
            return Err(Error::Sensitivity);
        }
        match frame.header.kind {
            yas_wire::transfer::kind::MESSAGE_DATA => {
                let fragment = MessageData::decode(&frame.payload)?;
                self.total_data = self
                    .total_data
                    .checked_add(fragment.data.len() as u64)
                    .ok_or(Error::LengthMismatch)?;
                if self.total_data > self.maximum_bytes {
                    return Err(Error::LimitExceeded {
                        declared: self.total_data,
                        maximum: self.maximum_bytes,
                    });
                }
                let complete = self.validator.accept(&fragment)?;
                if fragment.start {
                    self.open.insert(fragment.sequence, fragment.data);
                } else {
                    self.open
                        .get_mut(&fragment.sequence)
                        .ok_or(Error::NonContiguous)?
                        .extend_from_slice(&fragment.data);
                }
                if complete {
                    let message = self
                        .open
                        .remove(&fragment.sequence)
                        .ok_or(Error::NonContiguous)?;
                    if self.messages.len() == self.maximum_messages {
                        return Err(Error::TooManyMessages {
                            actual: self.messages.len() + 1,
                            maximum: self.maximum_messages,
                        });
                    }
                    self.messages.push(message);
                }
                Ok(None)
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = Close::decode(&frame.payload)?;
                let valid_length = close.final_data_bytes == self.total_data
                    && self.validator.open_messages() == 0
                    && self.open.is_empty();
                let messages = core::mem::take(&mut self.messages);
                self.open.clear();
                self.total_data = 0;
                self.closed = true;
                self.receive_lease.release();
                if !valid_length {
                    return Err(Error::LengthMismatch);
                }
                if close.status != Status::Ok.code() {
                    return Err(Error::Closed {
                        status: close.status,
                        detail: String::from_utf8_lossy(&close.detail).into_owned(),
                    });
                }
                Ok(Some(messages))
            }
            yas_wire::transfer::kind::RESET => {
                let reset = Reset::decode(&frame.payload)?;
                self.open.clear();
                self.messages.clear();
                self.total_data = 0;
                self.validator = WireMessageReceiver::new(&self.descriptor)
                    .expect("validated MESSAGE descriptor remains valid");
                self.closed = true;
                self.receive_lease.release();
                Err(Error::Reset {
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                })
            }
            kind => Err(Error::UnexpectedEvent(kind)),
        }
    }

    pub fn cancel(&mut self, client: &mut Client) -> Result<bool, Error> {
        if self.closed {
            return Ok(false);
        }
        if let Err(error) = client.send_typed_terminal_cleanup(
            family::TRANSFER,
            yas_wire::transfer::kind::RESET,
            &Reset {
                transfer_id: self.descriptor.transfer_id,
                status: Status::Cancelled.code(),
                detail: Vec::new(),
            },
            self.descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::RESET)?,
        ) {
            client.poison();
            return Err(error.into());
        }
        client.purge_pending_transfer_ids(&[self.descriptor.transfer_id]);
        self.open.clear();
        self.messages.clear();
        self.total_data = 0;
        self.validator = WireMessageReceiver::new(&self.descriptor)
            .expect("validated MESSAGE descriptor remains valid");
        self.closed = true;
        self.receive_lease.release();
        Ok(true)
    }

    fn retire_after_error(&mut self, client: &mut Client) {
        if self.closed {
            return;
        }
        let sensitivity = self
            .descriptor
            .requires_sensitive_frame(yas_wire::transfer::kind::RESET);
        let result = sensitivity.map_err(Error::from).and_then(|sensitive| {
            client
                .send_typed_terminal_cleanup(
                    family::TRANSFER,
                    yas_wire::transfer::kind::RESET,
                    &Reset {
                        transfer_id: self.descriptor.transfer_id,
                        status: Status::Cancelled.code(),
                        detail: Vec::new(),
                    },
                    sensitive,
                )
                .map_err(Error::from)
        });
        if result.is_err() {
            client.poison();
            return;
        }
        client.purge_pending_transfer_ids(&[self.descriptor.transfer_id]);
        self.open.clear();
        self.messages.clear();
        self.total_data = 0;
        self.validator = WireMessageReceiver::new(&self.descriptor)
            .expect("validated MESSAGE descriptor remains valid");
        self.closed = true;
        self.receive_lease.release();
    }
}

fn reset_resource_exhausted(client: &mut Client, descriptor: &Descriptor) -> Result<(), Error> {
    reset_descriptor(client, descriptor, Status::ResourceExhausted)
}

pub(crate) fn reset_descriptor(
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

pub(crate) fn reject_receive_transfer_with_lease(
    client: &mut Client,
    descriptor: &Descriptor,
    mut receive_lease: ReceiveLease,
) -> Result<(), Error> {
    reset_descriptor(client, descriptor, Status::ResourceExhausted)?;
    receive_lease.release();
    Ok(())
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid Transfer value: {error}"),
            Self::InvalidDescriptor => formatter.write_str("invalid BYTE Transfer descriptor"),
            Self::LimitExceeded { declared, maximum } => {
                write!(
                    formatter,
                    "Transfer declares {declared} bytes; limit is {maximum}"
                )
            }
            Self::LengthMismatch => formatter.write_str("Transfer length mismatch"),
            Self::HashMismatch => formatter.write_str("Transfer content hash mismatch"),
            Self::NonContiguous => formatter.write_str("Transfer data is not contiguous"),
            Self::Sensitivity => formatter.write_str("Transfer sensitivity mismatch"),
            Self::Closed { status, detail } => {
                write!(formatter, "Transfer closed with status {status}: {detail}")
            }
            Self::Reset { status, detail } => {
                write!(formatter, "Transfer reset with status {status}: {detail}")
            }
            Self::UnexpectedEvent(kind) => {
                write!(formatter, "unexpected Transfer event {kind:#06x}")
            }
            Self::TooManyMessages { actual, maximum } => {
                write!(
                    formatter,
                    "Transfer delivered {actual} messages; limit is {maximum}"
                )
            }
        }
    }
}

/// Collect one server-to-client MESSAGE Transfer. Fragments may be interleaved;
/// completed messages retain completion order, while family batch indices remain
/// the authority for semantic ordering.
pub fn receive_message_transfer(
    client: &mut Client,
    descriptor: &Descriptor,
    maximum_bytes: u64,
    maximum_messages: usize,
) -> Result<Vec<Vec<u8>>, Error> {
    let collector =
        MessageCollector::new(client, descriptor.clone(), maximum_bytes, maximum_messages)?;
    collect_message_transfer(client, descriptor, collector)
}

pub(crate) fn receive_message_transfer_with_lease(
    client: &mut Client,
    descriptor: &Descriptor,
    maximum_bytes: u64,
    maximum_messages: usize,
    receive_lease: ReceiveLease,
) -> Result<Vec<Vec<u8>>, Error> {
    let collector = MessageCollector::new_with_lease(
        client,
        descriptor.clone(),
        maximum_bytes,
        maximum_messages,
        receive_lease,
    )?;
    collect_message_transfer(client, descriptor, collector)
}

fn collect_message_transfer(
    client: &mut Client,
    descriptor: &Descriptor,
    mut collector: MessageCollector,
) -> Result<Vec<Vec<u8>>, Error> {
    loop {
        let frame = match client
            .next_matching_event(|frame| frame_transfer_id(frame) == Some(descriptor.transfer_id))
        {
            Ok(frame) => frame,
            Err(error) => {
                collector.retire_after_error(client);
                return Err(error.into());
            }
        };
        match collector.offer_frame(&frame) {
            Ok(Some(messages)) => return Ok(messages),
            Ok(None) => {}
            Err(error) => {
                collector.retire_after_error(client);
                return Err(error);
            }
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

/// Collect an inline value or its server-to-client BYTE Transfer, enforcing
/// the caller's allocation bound and the exact 32-byte BLAKE3 digest.
pub fn receive_inline_or_transfer(
    client: &mut Client,
    value: InlineOrTransfer,
    maximum: u64,
) -> Result<Vec<u8>, Error> {
    if value.byte_len > maximum {
        return Err(Error::LimitExceeded {
            declared: value.byte_len,
            maximum,
        });
    }
    let bytes = match value.delivery {
        Delivery::Inline(bytes) => bytes,
        Delivery::Transfer(descriptor) => {
            receive_byte_transfer(client, &descriptor, Some(value.byte_len), maximum)?
        }
    };
    if bytes.len() as u64 != value.byte_len {
        return Err(Error::LengthMismatch);
    }
    if blake3::hash(&bytes).as_bytes() != &value.content_hash {
        return Err(Error::HashMismatch);
    }
    Ok(bytes)
}

pub(crate) fn receive_inline_or_transfer_with_lease(
    client: &mut Client,
    value: InlineOrTransfer,
    maximum: u64,
    mut receive_lease: ReceiveLease,
) -> Result<Vec<u8>, Error> {
    if value.byte_len > maximum {
        match &value.delivery {
            Delivery::Inline(_) => receive_lease.release(),
            Delivery::Transfer(descriptor) => {
                reject_receive_transfer_with_lease(client, descriptor, receive_lease)?;
            }
        }
        return Err(Error::LimitExceeded {
            declared: value.byte_len,
            maximum,
        });
    }
    let bytes = match value.delivery {
        Delivery::Inline(bytes) => {
            receive_lease.release();
            bytes
        }
        Delivery::Transfer(descriptor) => receive_byte_transfer_with_lease(
            client,
            &descriptor,
            Some(value.byte_len),
            maximum,
            receive_lease,
        )?,
    };
    if bytes.len() as u64 != value.byte_len {
        return Err(Error::LengthMismatch);
    }
    if blake3::hash(&bytes).as_bytes() != &value.content_hash {
        return Err(Error::HashMismatch);
    }
    Ok(bytes)
}

/// Collect one server-to-client BYTE Transfer while preserving unrelated
/// events in the native client's bounded pending queue.
pub fn receive_byte_transfer(
    client: &mut Client,
    descriptor: &Descriptor,
    expected_length: Option<u64>,
    maximum: u64,
) -> Result<Vec<u8>, Error> {
    let receiver = ByteReceiver::new(client, descriptor.clone(), expected_length, maximum)?;
    collect_byte_transfer(client, descriptor, receiver)
}

pub(crate) fn receive_byte_transfer_with_lease(
    client: &mut Client,
    descriptor: &Descriptor,
    expected_length: Option<u64>,
    maximum: u64,
    receive_lease: ReceiveLease,
) -> Result<Vec<u8>, Error> {
    let receiver = ByteReceiver::new_with_lease(
        client,
        descriptor.clone(),
        expected_length,
        maximum,
        receive_lease,
    )?;
    collect_byte_transfer(client, descriptor, receiver)
}

fn collect_byte_transfer(
    client: &mut Client,
    descriptor: &Descriptor,
    mut receiver: ByteReceiver,
) -> Result<Vec<u8>, Error> {
    loop {
        let frame = match client
            .next_matching_event(|frame| frame_transfer_id(frame) == Some(descriptor.transfer_id))
        {
            Ok(frame) => frame,
            Err(error) => {
                receiver.retire_after_error(client);
                return Err(error.into());
            }
        };
        match receiver.offer_frame(&frame) {
            Ok(Some(bytes)) => return Ok(bytes),
            Ok(None) => {}
            Err(error) => {
                receiver.retire_after_error(client);
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use yas_wire::{Extensions, FrameCodec, FrameHeader, FrameLimits};

    use crate::test_support::bootstrap_client;

    const WINDOW: u64 = 4 * 1024 * 1024;

    fn descriptor(transfer_id: u32, mode: Mode) -> Descriptor {
        Descriptor {
            transfer_id,
            mode,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: WINDOW,
            max_item_bytes: if mode == Mode::Message { WINDOW } else { 0 },
            max_chunk_bytes: 64 * 1024,
            content_family: family::EXTENSION,
            content_kind: 0x7fff,
            content_version: 1,
            extensions: Extensions::default(),
        }
    }

    fn sender_descriptor(transfer_id: u32, credit: u64) -> Descriptor {
        Descriptor {
            transfer_id,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: credit,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::EXTENSION,
            content_kind: 0x7fff,
            content_version: 1,
            extensions: Extensions::default(),
        }
    }

    fn fill_pending_headroom(client: &mut Client) {
        client
            .defer_for_test(Frame {
                header: FrameHeader::event(family::EXTENSION, 0x7fff),
                payload: vec![
                    0;
                    yas_wire::schema::transport::RECOMMENDED_BUFFERED as usize
                        - yas_wire::schema::transport::RECOMMENDED_DECODED_FRAME as usize
                        + 1
                ],
            })
            .unwrap();
    }

    fn assert_cancelled_reset(packet: &[u8], transfer_id: u32) {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let (frame, consumed) = codec.decode_stream(packet).unwrap();
        assert_eq!(consumed, packet.len());
        assert_eq!(frame.header.family, family::TRANSFER);
        assert_eq!(frame.header.kind, yas_wire::transfer::kind::RESET);
        let reset = Reset::decode(&frame.payload).unwrap();
        assert_eq!(reset.transfer_id, transfer_id);
        assert_eq!(Status::from_code(reset.status), Status::Cancelled);
    }

    #[test]
    fn blocking_sender_resets_after_read_failure_and_client_remains_reusable() {
        let (mut client, state, _guard) = bootstrap_client();
        fill_pending_headroom(&mut client);

        assert!(matches!(
            send_byte_transfer(&mut client, &sender_descriptor(401, 0), b"blocked"),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(state.borrow().sent.len(), 1);
        assert_cancelled_reset(&state.borrow().sent[0], 401);

        send_byte_transfer(&mut client, &sender_descriptor(402, 2), b"ok").unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let sent = &state.borrow().sent;
        assert_eq!(sent.len(), 3);
        let (data, _) = codec.decode_stream(&sent[1]).unwrap();
        assert_eq!(data.header.kind, yas_wire::transfer::kind::BYTE_DATA);
        assert_eq!(ByteData::decode(&data.payload).unwrap().transfer_id, 402);
        let (close, _) = codec.decode_stream(&sent[2]).unwrap();
        assert_eq!(close.header.kind, yas_wire::transfer::kind::CLOSE);
        assert_eq!(Close::decode(&close.payload).unwrap().transfer_id, 402);
    }

    #[test]
    fn blocking_sender_reset_failure_poisons_client() {
        let (mut client, state, _guard) = bootstrap_client();
        fill_pending_headroom(&mut client);
        state.borrow_mut().fail_sends = 1;

        assert!(matches!(
            send_byte_transfer(&mut client, &sender_descriptor(403, 0), b"blocked"),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert!(state.borrow().sent.is_empty());
        assert!(matches!(
            client.attempt_log("still poisoned"),
            Err(ClientError::Poisoned)
        ));
    }

    #[test]
    fn blocking_collectors_reset_live_authority_after_pending_read_block() {
        {
            let (mut client, state, _guard) = bootstrap_client();
            fill_pending_headroom(&mut client);
            let lease = client.receive_credit_exact(WINDOW).unwrap();
            assert!(matches!(
                receive_byte_transfer_with_lease(
                    &mut client,
                    &descriptor(301, Mode::Byte),
                    None,
                    WINDOW,
                    lease,
                ),
                Err(Error::Client(ClientError::PendingReadBlocked))
            ));
            assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
            assert_eq!(state.borrow().sent.len(), 1);
            assert_cancelled_reset(&state.borrow().sent[0], 301);
        }

        let (mut client, state, _guard) = bootstrap_client();
        fill_pending_headroom(&mut client);
        assert!(matches!(
            receive_message_transfer(&mut client, &descriptor(302, Mode::Message), WINDOW, 8),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert_eq!(state.borrow().sent.len(), 1);
        assert_cancelled_reset(&state.borrow().sent[0], 302);
    }
}
