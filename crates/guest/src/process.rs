//! Native YAS Process and BYTE Transfer helpers for extension guests.

use alloc::{string::String, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame,
    core::Status,
    family, process as wire,
    state::{Cursor, Phase, RecordKind, StateAck, StateEvent, Unwatch, Watch, WatchResult},
    transfer::{ByteData, Close as TransferClose, Credit, Descriptor, Reset as TransferReset},
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    yas::{Client, Error as ClientError},
};

pub const DEFAULT_STREAM_WINDOW: u64 = 4 * 1024 * 1024;
pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Eq, PartialEq)]
#[must_use = "process output must be consumed or deliberately discarded"]
pub struct Delivery {
    process_handle: u64,
    transfer_id: u32,
    kind: StreamKind,
    through: u64,
    data: Vec<u8>,
}

impl Delivery {
    pub fn process_handle(&self) -> u64 {
        self.process_handle
    }

    pub fn kind(&self) -> StreamKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamClosed {
    pub kind: StreamKind,
    pub status: u16,
    pub detail: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    Output(Delivery),
    StdinCredit {
        cumulative_limit: u64,
        available: u64,
    },
    StreamClosed(StreamClosed),
    StdinClosed {
        status: u16,
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateChange {
    Upsert(wire::ProcessRecord),
    Remove(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub phase: Phase,
    pub from_revision: u64,
    pub to_revision: u64,
    pub changes: Vec<StateChange>,
}

/// One bounded native Process catalogue subscription.
pub struct ProcessWatch {
    subscription_id: u32,
    cumulative_credit: u64,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl ProcessWatch {
    pub fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<StateUpdate>, Error> {
        if self.closed
            || frame.header.class != Class::Event
            || frame.header.family != family::PROCESS
            || frame.header.kind != wire::event_kind::STATE
        {
            return Ok(None);
        }
        let event = StateEvent::decode(&frame.payload)?;
        if event.subscription_id != self.subscription_id {
            return Ok(None);
        }
        let mut changes = Vec::with_capacity(event.records.len());
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => changes.push(StateChange::Upsert(
                    wire::ProcessRecord::from_state_record(record)?,
                )),
                RecordKind::Remove => changes.push(StateChange::Remove(
                    wire::RemovedProcess::decode(&record.body)?.process_handle,
                )),
                RecordKind::Patch | RecordKind::Family(_) => {
                    return Err(Error::Protocol("unexpected Process state record kind"));
                }
            }
        }
        self.cumulative_credit = self
            .cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::PROCESS,
            wire::event_kind::STATE_ACK,
            &StateAck {
                subscription_id: self.subscription_id,
                applied_revision: event.to_revision,
                cumulative_byte_limit: self.cumulative_credit,
            },
            false,
        )?;
        Ok(Some(StateUpdate {
            phase: event.phase,
            from_revision: event.from_revision,
            to_revision: event.to_revision,
            changes,
        }))
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::PROCESS,
            wire::request_kind::UNWATCH,
            Unwatch {
                subscription_id: self.subscription_id,
            }
            .encode()?,
            false,
        )?;
        self.closed = true;
        self.receive_lease.release();
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    FeatureMissing,
    NoStdin,
    InvalidStream,
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
            Self::Wire(error) => write!(formatter, "invalid native Process value: {error}"),
            Self::FeatureMissing => formatter.write_str("native Process operation is unavailable"),
            Self::NoStdin => formatter.write_str("native Process has no claimed stdin"),
            Self::InvalidStream => formatter.write_str("invalid native Process stream descriptor"),
            Self::CreditExhausted {
                required,
                available,
            } => write!(
                formatter,
                "native Process stream needs {required} bytes of credit; {available} available"
            ),
            Self::DeliveryPending => formatter.write_str("Process output delivery is pending"),
            Self::StaleDelivery => formatter.write_str("stale Process output delivery"),
            Self::CounterOverflow => formatter.write_str("native Process counter overflow"),
            Self::Protocol(detail) => write!(formatter, "native Process protocol error: {detail}"),
            Self::Closed => formatter.write_str("native Process stream is closed"),
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

struct Input {
    descriptor: Descriptor,
    credit: u64,
    sent: u64,
    closed: bool,
}

struct Output {
    descriptor: Descriptor,
    kind: StreamKind,
    received: u64,
    consumed: u64,
    granted: u64,
    pending: Option<u64>,
    closed: bool,
    receive_window: u64,
    receive_lease: ReceiveLease,
}

/// A spawned or attached native Process stream bundle.
pub struct Process {
    handle: u64,
    stdout_lifetime_offset: u64,
    stderr_lifetime_offset: u64,
    stdin: Option<Input>,
    stdout: Output,
    stderr: Option<Output>,
    merged_stderr: bool,
}

impl fmt::Debug for Process {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Process")
            .field("handle", &self.handle)
            .field("stdout_lifetime_offset", &self.stdout_lifetime_offset)
            .field("stderr_lifetime_offset", &self.stderr_lifetime_offset)
            .field("merged_stderr", &self.merged_stderr)
            .finish_non_exhaustive()
    }
}

impl Process {
    fn new_spawn(
        client: &mut Client,
        mut bundle: wire::StreamBundle,
        requested_flags: u16,
        cleanup_operation_id: [u8; 16],
        stdout_lease: ReceiveLease,
        stderr_lease: Option<ReceiveLease>,
    ) -> Result<Self, Error> {
        if !valid_spawn_bundle(
            &bundle,
            requested_flags,
            &stdout_lease,
            stderr_lease.as_ref(),
        ) {
            terminalize_invalid_bundle(client, &bundle, Some(cleanup_operation_id));
            return Err(Error::InvalidStream);
        }
        if let Err(error) = prepare_output_credit(client, &mut bundle.stdout, stdout_lease.bytes())
        {
            terminalize_invalid_bundle(client, &bundle, Some(cleanup_operation_id));
            return Err(error);
        }
        if let (Some(descriptor), Some(lease)) = (bundle.stderr.as_mut(), stderr_lease.as_ref())
            && let Err(error) = prepare_output_credit(client, descriptor, lease.bytes())
        {
            terminalize_invalid_bundle(client, &bundle, Some(cleanup_operation_id));
            return Err(error);
        }
        let cleanup_bundle = bundle.clone();
        match Self::new(client, bundle, stdout_lease, stderr_lease) {
            Ok(process) => Ok(process),
            Err(error) => {
                terminalize_invalid_bundle(client, &cleanup_bundle, Some(cleanup_operation_id));
                Err(error)
            }
        }
    }

    fn new_attach(
        client: &mut Client,
        mut bundle: wire::StreamBundle,
        requested_flags: u16,
        stdout_lease: ReceiveLease,
        stderr_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        if !valid_attach_bundle(&bundle, requested_flags, &stdout_lease, &stderr_lease) {
            terminalize_invalid_bundle(client, &bundle, None);
            return Err(Error::InvalidStream);
        }
        if let Err(error) = prepare_output_credit(client, &mut bundle.stdout, stdout_lease.bytes())
        {
            terminalize_invalid_bundle(client, &bundle, None);
            return Err(error);
        }
        if let Some(descriptor) = bundle.stderr.as_mut()
            && let Err(error) = prepare_output_credit(client, descriptor, stderr_lease.bytes())
        {
            terminalize_invalid_bundle(client, &bundle, None);
            return Err(error);
        }
        let cleanup_bundle = bundle.clone();
        match Self::new(client, bundle, stdout_lease, Some(stderr_lease)) {
            Ok(process) => Ok(process),
            Err(error) => {
                terminalize_invalid_bundle(client, &cleanup_bundle, None);
                Err(error)
            }
        }
    }

    fn new(
        client: &mut Client,
        bundle: wire::StreamBundle,
        stdout_lease: ReceiveLease,
        mut stderr_lease: Option<ReceiveLease>,
    ) -> Result<Self, Error> {
        if bundle.stderr.is_none()
            && let Some(mut unused) = stderr_lease.take()
        {
            // The successful Result proves that no stderr receive authority
            // was created for this request.
            unused.release();
        }
        if let Some(descriptor) = bundle.stderr.as_ref()
            && stderr_lease.is_none()
        {
            if reset_output(client, descriptor, Status::ResourceExhausted).is_ok() {
                let mut stdout_lease = stdout_lease;
                if reset_output(client, &bundle.stdout, Status::ResourceExhausted).is_ok() {
                    stdout_lease.release();
                }
            }
            return Err(Error::InvalidStream);
        }
        let stdin = bundle.stdin.map(|descriptor| Input {
            credit: descriptor.receiver_send_credit,
            descriptor,
            sent: 0,
            closed: false,
        });
        let mut stdout = match Output::new(client, bundle.stdout, StreamKind::Stdout, stdout_lease)
        {
            Ok(stdout) => stdout,
            Err(error) => {
                // The Result disclosed both output authorities. If stdout is
                // rejected, explicitly retire stderr too before returning;
                // abandoning its committed Request lease would otherwise pin
                // capacity while leaving the client deceptively healthy.
                if let (Some(descriptor), Some(lease)) =
                    (bundle.stderr.as_ref(), stderr_lease.as_mut())
                    && reset_output(client, descriptor, Status::ResourceExhausted).is_ok()
                {
                    lease.release();
                }
                return Err(error);
            }
        };
        let stderr = match (bundle.stderr, stderr_lease) {
            (Some(descriptor), Some(lease)) => {
                match Output::new(client, descriptor, StreamKind::Stderr, lease) {
                    Ok(output) => Some(output),
                    Err(error) => {
                        let _ = stdout.reset(client, Status::ResourceExhausted);
                        return Err(error);
                    }
                }
            }
            (None, None) => None,
            _ => unreachable!("Process stderr Result shape checked above"),
        };
        Ok(Self {
            handle: bundle.process_handle,
            stdout_lifetime_offset: bundle.stdout_lifetime_offset,
            stderr_lifetime_offset: bundle.stderr_lifetime_offset,
            stdin,
            stdout,
            stderr,
            merged_stderr: bundle.merged_stderr,
        })
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Process-lifetime stdout bytes produced before this attachment starts.
    pub fn stdout_lifetime_offset(&self) -> u64 {
        self.stdout_lifetime_offset
    }

    /// Process-lifetime stderr bytes produced before this attachment starts.
    pub fn stderr_lifetime_offset(&self) -> u64 {
        self.stderr_lifetime_offset
    }

    pub fn merged_stderr(&self) -> bool {
        self.merged_stderr
    }

    pub fn stdin_available_credit(&self) -> u64 {
        self.stdin
            .as_ref()
            .map_or(0, |input| input.credit.saturating_sub(input.sent))
    }

    pub fn write_stdin(&mut self, client: &mut Client, bytes: &[u8]) -> Result<(), Error> {
        let input = self.stdin.as_mut().ok_or(Error::NoStdin)?;
        if input.closed {
            return Err(Error::Closed);
        }
        let required = bytes.len() as u64;
        let available = input.credit.saturating_sub(input.sent);
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
                .min(offset.saturating_add(input.descriptor.max_chunk_bytes as usize));
            client.send_typed_event(
                family::TRANSFER,
                yas_wire::transfer::kind::BYTE_DATA,
                &ByteData {
                    transfer_id: input.descriptor.transfer_id,
                    offset: input.sent + offset as u64,
                    data: bytes[offset..end].to_vec(),
                },
                input
                    .descriptor
                    .requires_sensitive_frame(yas_wire::transfer::kind::BYTE_DATA)?,
            )?;
            offset = end;
        }
        input.sent = input
            .sent
            .checked_add(required)
            .ok_or(Error::CounterOverflow)?;
        Ok(())
    }

    pub fn close_stdin(&mut self, client: &mut Client) -> Result<(), Error> {
        let input = self.stdin.as_mut().ok_or(Error::NoStdin)?;
        if input.closed {
            return Ok(());
        }
        let close = TransferClose {
            transfer_id: input.descriptor.transfer_id,
            final_data_bytes: input.sent,
            status: Status::Ok.code(),
            detail: Vec::new(),
        };
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CLOSE,
            &close,
            input
                .descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::CLOSE)?,
        )?;
        input.closed = true;
        Ok(())
    }

    pub fn next_event(&mut self, client: &mut Client) -> Result<Event, Error> {
        let stdin = self
            .stdin
            .as_ref()
            .map(|input| input.descriptor.transfer_id);
        let stdout = self
            .stdout
            .pending
            .is_none()
            .then_some(self.stdout.descriptor.transfer_id);
        let stderr = self.stderr.as_ref().and_then(|output| {
            output
                .pending
                .is_none()
                .then_some(output.descriptor.transfer_id)
        });
        if stdout.is_none() && stderr.is_none() && stdin.is_none() {
            return Err(Error::DeliveryPending);
        }
        let frame = client.next_matching_event(|frame| {
            frame.header.family == family::TRANSFER
                && transfer_id(frame)
                    .is_some_and(|id| Some(id) == stdin || Some(id) == stdout || Some(id) == stderr)
        })?;
        self.interpret(frame)
    }

    /// Offer one already-routed native Event to this process without
    /// blocking. Unrelated family or Transfer identities are left untouched
    /// for another resource in the guest's general event loop.
    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<Event>, Error> {
        let Some(id) = transfer_id(frame) else {
            return Ok(None);
        };
        let belongs = self
            .stdin
            .as_ref()
            .is_some_and(|input| input.descriptor.transfer_id == id)
            || self.stdout.descriptor.transfer_id == id
            || self
                .stderr
                .as_ref()
                .is_some_and(|output| output.descriptor.transfer_id == id);
        if !belongs {
            return Ok(None);
        }
        self.interpret(frame.clone()).map(Some)
    }

    /// Whether an Event carries one of this process's stream identities.
    pub fn owns_frame(&self, frame: &Frame) -> bool {
        let Some(id) = transfer_id(frame) else {
            return false;
        };
        if self
            .stdin
            .as_ref()
            .is_some_and(|input| input.descriptor.transfer_id == id)
        {
            return true;
        }
        if self.stdout.descriptor.transfer_id == id {
            return self.stdout.pending.is_none();
        }
        self.stderr
            .as_ref()
            .is_some_and(|output| output.descriptor.transfer_id == id && output.pending.is_none())
    }

    pub fn consume(&mut self, client: &mut Client, delivery: Delivery) -> Result<Vec<u8>, Error> {
        self.finish_delivery(client, &delivery)?;
        Ok(delivery.data)
    }

    pub fn discard(&mut self, client: &mut Client, delivery: Delivery) -> Result<(), Error> {
        self.finish_delivery(client, &delivery)
    }

    pub fn discard_pending(&mut self, client: &mut Client, kind: StreamKind) -> Result<(), Error> {
        let output = self.output_mut(kind).ok_or(Error::InvalidStream)?;
        let through = output.pending.ok_or(Error::StaleDelivery)?;
        output.acknowledge(client, through)
    }

    pub fn control(
        &mut self,
        client: &mut Client,
        action: wire::ControlAction,
        value: u16,
    ) -> Result<wire::ControlResult, Error> {
        let mut operation_id = [0; 16];
        client.random(&mut operation_id)?;
        client
            .request_typed(
                family::PROCESS,
                wire::request_kind::CONTROL,
                &wire::Control {
                    process_handle: self.handle,
                    operation_id,
                    action,
                    value,
                    extensions: Extensions::default(),
                },
                true,
            )
            .map_err(Into::into)
    }

    pub fn wait(
        &mut self,
        client: &mut Client,
        timeout_ns: u64,
    ) -> Result<wire::ExitRecord, Error> {
        client
            .request_typed(
                family::PROCESS,
                wire::request_kind::WAIT,
                &wire::Wait {
                    process_handle: self.handle,
                    timeout_ns,
                    extensions: Extensions::default(),
                },
                false,
            )
            .map_err(Into::into)
    }

    fn interpret(&mut self, frame: Frame) -> Result<Event, Error> {
        let id = transfer_id(&frame).ok_or(Error::Protocol("missing Transfer ID"))?;
        if self
            .stdin
            .as_ref()
            .is_some_and(|input| input.descriptor.transfer_id == id)
        {
            return self.interpret_stdin(frame);
        }
        if self.stdout.descriptor.transfer_id == id {
            return Self::interpret_output(self.handle, &mut self.stdout, frame);
        }
        if let Some(stderr) = self.stderr.as_mut()
            && stderr.descriptor.transfer_id == id
        {
            return Self::interpret_output(self.handle, stderr, frame);
        }
        Err(Error::Protocol("unknown Process Transfer"))
    }

    fn interpret_stdin(&mut self, frame: Frame) -> Result<Event, Error> {
        let input = self.stdin.as_mut().ok_or(Error::NoStdin)?;
        input.check_sensitivity(&frame)?;
        match frame.header.kind {
            yas_wire::transfer::kind::CREDIT => {
                let credit = Credit::decode(&frame.payload)?;
                if credit.cumulative_limit < input.credit || credit.cumulative_limit < input.sent {
                    return Err(Error::Protocol("Process stdin credit moved backwards"));
                }
                input.credit = credit.cumulative_limit;
                Ok(Event::StdinCredit {
                    cumulative_limit: input.credit,
                    available: input.credit.saturating_sub(input.sent),
                })
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload)?;
                input.closed = true;
                Ok(Event::StdinClosed {
                    status: close.status,
                    detail: String::from_utf8_lossy(&close.detail).into_owned(),
                })
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload)?;
                input.closed = true;
                Ok(Event::StdinClosed {
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                })
            }
            _ => Err(Error::Protocol("unexpected Process stdin Transfer event")),
        }
    }

    fn interpret_output(
        process_handle: u64,
        output: &mut Output,
        frame: Frame,
    ) -> Result<Event, Error> {
        output.check_sensitivity(&frame)?;
        match frame.header.kind {
            yas_wire::transfer::kind::BYTE_DATA => {
                if output.pending.is_some() {
                    return Err(Error::DeliveryPending);
                }
                let data = ByteData::decode(&frame.payload)?;
                if data.offset != output.received {
                    return Err(Error::Protocol("Process output offset is not contiguous"));
                }
                let through = output
                    .received
                    .checked_add(data.data.len() as u64)
                    .ok_or(Error::CounterOverflow)?;
                if through > output.granted {
                    return Err(Error::Protocol("Process output exceeded receive credit"));
                }
                output.received = through;
                output.pending = Some(through);
                Ok(Event::Output(Delivery {
                    process_handle,
                    transfer_id: output.descriptor.transfer_id,
                    kind: output.kind,
                    through,
                    data: data.data,
                }))
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload)?;
                output.closed = true;
                if output.pending.is_none() {
                    output.receive_lease.release();
                }
                if close.final_data_bytes != output.received {
                    return Err(Error::Protocol("Process output CLOSE length mismatch"));
                }
                Ok(Event::StreamClosed(StreamClosed {
                    kind: output.kind,
                    status: close.status,
                    detail: String::from_utf8_lossy(&close.detail).into_owned(),
                }))
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload)?;
                output.closed = true;
                if output.pending.is_none() {
                    output.receive_lease.release();
                }
                Ok(Event::StreamClosed(StreamClosed {
                    kind: output.kind,
                    status: reset.status,
                    detail: String::from_utf8_lossy(&reset.detail).into_owned(),
                }))
            }
            _ => Err(Error::Protocol("unexpected Process output Transfer event")),
        }
    }

    fn finish_delivery(&mut self, client: &mut Client, delivery: &Delivery) -> Result<(), Error> {
        if delivery.process_handle != self.handle {
            return Err(Error::StaleDelivery);
        }
        let output = self.output_mut(delivery.kind).ok_or(Error::StaleDelivery)?;
        if output.descriptor.transfer_id != delivery.transfer_id
            || output.pending != Some(delivery.through)
        {
            return Err(Error::StaleDelivery);
        }
        output.acknowledge(client, delivery.through)
    }

    fn output_mut(&mut self, kind: StreamKind) -> Option<&mut Output> {
        match kind {
            StreamKind::Stdout => Some(&mut self.stdout),
            StreamKind::Stderr => self.stderr.as_mut(),
        }
    }
}

impl Input {
    fn check_sensitivity(&self, frame: &Frame) -> Result<(), Error> {
        if frame.header.sensitive
            != self
                .descriptor
                .requires_sensitive_frame(frame.header.kind)?
        {
            return Err(Error::Protocol("Process stdin sensitivity mismatch"));
        }
        Ok(())
    }
}

impl Output {
    fn new(
        client: &mut Client,
        descriptor: Descriptor,
        kind: StreamKind,
        mut receive_lease: ReceiveLease,
    ) -> Result<Self, Error> {
        receive_lease.commit();
        let content_kind = match kind {
            StreamKind::Stdout => yas_wire::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
            StreamKind::Stderr => yas_wire::schema::process::STREAM_STDERR_CONTENT_KIND as u16,
        };
        let valid = descriptor.validate().is_ok()
            && descriptor.sender_send_credit <= receive_lease.bytes()
            && descriptor.mode == yas_wire::transfer::Mode::Byte
            && descriptor.direction == yas_wire::transfer::Direction::SENDER_TO_RECEIVER
            && descriptor.content_family == family::PROCESS
            && descriptor.content_kind == content_kind
            && descriptor.content_version == wire::VERSION
            && descriptor
                .sensitive_content()
                .is_ok_and(|sensitive| sensitive);
        if !valid {
            if reset_output(client, &descriptor, Status::ResourceExhausted).is_ok() {
                receive_lease.release();
            }
            return Err(Error::InvalidStream);
        }
        let receive_window = receive_lease.bytes();
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
            granted: receive_window,
            descriptor,
            kind,
            received: 0,
            consumed: 0,
            pending: None,
            closed: false,
            receive_window,
            receive_lease,
        })
    }

    fn check_sensitivity(&self, frame: &Frame) -> Result<(), Error> {
        if frame.header.sensitive
            != self
                .descriptor
                .requires_sensitive_frame(frame.header.kind)?
        {
            return Err(Error::Protocol("Process output sensitivity mismatch"));
        }
        Ok(())
    }

    fn acknowledge(&mut self, client: &mut Client, through: u64) -> Result<(), Error> {
        if self.pending != Some(through) {
            return Err(Error::StaleDelivery);
        }
        self.consumed = through;
        if self.closed {
            self.pending = None;
            self.receive_lease.release();
            return Ok(());
        }
        let cumulative_limit = self
            .consumed
            .checked_add(self.receive_window)
            .ok_or(Error::CounterOverflow)?;
        if cumulative_limit > self.granted {
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
        }
        self.pending = None;
        Ok(())
    }

    fn reset(&mut self, client: &mut Client, status: Status) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        reset_output(client, &self.descriptor, status)?;
        self.pending = None;
        self.closed = true;
        self.receive_lease.release();
        Ok(())
    }
}

fn reset_output(client: &mut Client, descriptor: &Descriptor, status: Status) -> Result<(), Error> {
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

fn valid_spawn_bundle(
    bundle: &wire::StreamBundle,
    requested_flags: u16,
    stdout_lease: &ReceiveLease,
    stderr_lease: Option<&ReceiveLease>,
) -> bool {
    let requested_merged =
        requested_flags & yas_wire::schema::process::SPAWN_MERGE_STDERR as u16 != 0;
    bundle.encode().is_ok()
        && bundle.stdin.is_some()
        && bundle.merged_stderr == requested_merged
        && bundle.stdout.sender_send_credit <= stdout_lease.bytes()
        && match (bundle.stderr.as_ref(), stderr_lease) {
            (Some(descriptor), Some(lease)) => descriptor.sender_send_credit <= lease.bytes(),
            (None, None) => true,
            _ => false,
        }
}

fn valid_attach_bundle(
    bundle: &wire::StreamBundle,
    requested_flags: u16,
    stdout_lease: &ReceiveLease,
    stderr_lease: &ReceiveLease,
) -> bool {
    let requested_stdin = requested_flags & yas_wire::schema::process::ATTACH_STDIN as u16 != 0;
    bundle.encode().is_ok()
        && bundle.stdin.is_some() == requested_stdin
        && bundle.stdout.sender_send_credit <= stdout_lease.bytes()
        && bundle
            .stderr
            .as_ref()
            .is_none_or(|descriptor| descriptor.sender_send_credit <= stderr_lease.bytes())
}

fn prepare_output_credit(
    client: &mut Client,
    descriptor: &mut Descriptor,
    receive_window: u64,
) -> Result<(), Error> {
    if descriptor.sender_send_credit < receive_window {
        client.send_typed_event(
            family::TRANSFER,
            yas_wire::transfer::kind::CREDIT,
            &Credit {
                transfer_id: descriptor.transfer_id,
                cumulative_limit: receive_window,
            },
            false,
        )?;
        descriptor.sender_send_credit = receive_window;
    }
    Ok(())
}

fn terminalize_invalid_bundle(
    client: &mut Client,
    bundle: &wire::StreamBundle,
    kill_operation_id: Option<[u8; 16]>,
) {
    // Fully settle KILL before any RESET: RESET detaches the server-side
    // attachment, so merely putting CONTROL on the wire first would still let
    // the two operations race and could leave a detachable child running.
    let kill = kill_operation_id.and_then(|operation_id| {
        client
            .begin_typed_request(
                family::PROCESS,
                wire::request_kind::CONTROL,
                &wire::Control {
                    process_handle: bundle.process_handle,
                    operation_id,
                    action: wire::ControlAction::Kill,
                    value: 0,
                    extensions: Extensions::default(),
                },
                true,
            )
            .ok()
    });

    if let Some(token) = kill {
        // Correlation settlement is the retirement boundary. Status::Ok
        // proves KILL ran and Conflict proves the generation was already
        // terminal; every other status/error is still fully observed before
        // attachment cleanup is allowed to race it.
        let _ = client.await_result(&token);
    }

    let mut transfer_ids = Vec::with_capacity(3);
    for descriptor in bundle
        .stdin
        .iter()
        .chain(core::iter::once(&bundle.stdout))
        .chain(bundle.stderr.iter())
    {
        transfer_ids.push(descriptor.transfer_id);
        let _ = client.send_typed_terminal_cleanup(
            family::TRANSFER,
            yas_wire::transfer::kind::RESET,
            &TransferReset {
                transfer_id: descriptor.transfer_id,
                status: Status::ResourceExhausted.code(),
                detail: Vec::new(),
            },
            descriptor
                .requires_sensitive_frame(yas_wire::transfer::kind::RESET)
                .unwrap_or(false),
        );
    }
    client.purge_pending_transfer_ids(&transfer_ids);
    // A post-OK shape/policy failure is terminal even if every cleanup frame
    // succeeded: already-in-flight Transfer data must never reach a future
    // resource, and committed Request leases remain pinned until teardown.
    client.poison();
}

fn transfer_id(frame: &Frame) -> Option<u32> {
    (frame.header.family == family::TRANSFER && frame.payload.len() >= 4).then(|| {
        u32::from_le_bytes(
            frame.payload[..4]
                .try_into()
                .expect("four-byte Transfer ID"),
        )
    })
}

impl Client {
    /// Subscribe to the complete boot-scoped Process catalogue.
    pub fn watch_processes(&mut self, resume: Option<Cursor>) -> Result<ProcessWatch, Error> {
        if !self.supports(family::PROCESS, Class::Request, wire::request_kind::WATCH) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = self.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let initial_credit = receive_lease.bytes();
        let result: WatchResult = self.request_typed_with_receive_lease(
            family::PROCESS,
            wire::request_kind::WATCH,
            &Watch {
                initial_credit,
                resume,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        Ok(ProcessWatch {
            subscription_id: result.subscription_id,
            cumulative_credit: initial_credit,
            closed: false,
            receive_lease,
        })
    }

    /// Spawn one process with native Process semantics. The operation ID and
    /// bounded receive credits are filled by the SDK.
    pub fn spawn_process(
        &mut self,
        flags: u16,
        environment_kind: wire::EnvironmentKind,
        cwd: wire::Cwd,
        argv: Vec<Vec<u8>>,
        env: Vec<wire::EnvEntry>,
        extensions: Extensions,
    ) -> Result<Process, Error> {
        self.spawn_process_with_window(
            flags,
            environment_kind,
            cwd,
            argv,
            env,
            extensions,
            DEFAULT_STREAM_WINDOW,
        )
    }

    /// Spawn with an explicit replenished output window.
    ///
    /// Long-lived consumers normally want [`DEFAULT_STREAM_WINDOW`]. A guest
    /// which continuously discards GUI child diagnostics can use a much
    /// smaller window without reserving several MiB for every open window.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_process_with_window(
        &mut self,
        flags: u16,
        environment_kind: wire::EnvironmentKind,
        cwd: wire::Cwd,
        argv: Vec<Vec<u8>>,
        env: Vec<wire::EnvEntry>,
        extensions: Extensions,
        stream_window: u64,
    ) -> Result<Process, Error> {
        if !self.supports(family::PROCESS, Class::Request, wire::request_kind::SPAWN) {
            return Err(Error::FeatureMissing);
        }
        if stream_window == 0 {
            return Err(Error::Protocol("Process stream window is zero"));
        }
        let operation_id = operation_id(self)?;
        let cleanup_operation_id = distinct_operation_id(self, operation_id)?;
        let merged = flags & yas_wire::schema::process::SPAWN_MERGE_STDERR as u16 != 0;
        let mut stdout_lease = self.receive_credit_exact(stream_window)?;
        let mut stderr_lease = if merged {
            None
        } else {
            Some(self.receive_credit_exact(stream_window)?)
        };
        let request = wire::Spawn {
            operation_id,
            flags,
            environment_kind,
            cwd,
            argv,
            env,
            stdout_receive_credit: stream_window,
            stderr_receive_credit: if merged { 0 } else { stream_window },
            extensions,
        };
        let bundle: wire::StreamBundle = if let Some(stderr) = stderr_lease.as_mut() {
            self.request_typed_with_receive_leases(
                family::PROCESS,
                wire::request_kind::SPAWN,
                &request,
                true,
                &mut [&mut stdout_lease, stderr],
            )?
        } else {
            self.request_typed_with_receive_lease(
                family::PROCESS,
                wire::request_kind::SPAWN,
                &request,
                true,
                &mut stdout_lease,
            )?
        };
        Process::new_spawn(
            self,
            bundle,
            flags,
            cleanup_operation_id,
            stdout_lease,
            stderr_lease,
        )
    }

    /// Attach native streams to one exact boot-scoped Process handle.
    pub fn attach_process(&mut self, process_handle: u64, flags: u16) -> Result<Process, Error> {
        if !self.supports(family::PROCESS, Class::Request, wire::request_kind::ATTACH) {
            return Err(Error::FeatureMissing);
        }
        let mut stdout_lease = self.receive_credit_exact(DEFAULT_STREAM_WINDOW)?;
        let mut stderr_lease = self.receive_credit_exact(DEFAULT_STREAM_WINDOW)?;
        let bundle: wire::StreamBundle = self.request_typed_with_receive_leases(
            family::PROCESS,
            wire::request_kind::ATTACH,
            &wire::Attach {
                process_handle,
                flags,
                stdout_receive_credit: DEFAULT_STREAM_WINDOW,
                stderr_receive_credit: DEFAULT_STREAM_WINDOW,
                extensions: Extensions::default(),
            },
            true,
            &mut [&mut stdout_lease, &mut stderr_lease],
        )?;
        Process::new_attach(self, bundle, flags, stdout_lease, stderr_lease)
    }
}

fn distinct_operation_id(client: &Client, existing: [u8; 16]) -> Result<[u8; 16], Error> {
    let mut candidate = operation_id(client)?;
    if candidate == existing {
        let next = u128::from_le_bytes(candidate).wrapping_add(1).max(1);
        candidate = next.to_le_bytes();
    }
    Ok(candidate)
}

fn operation_id(client: &Client) -> Result<[u8; 16], Error> {
    let mut operation_id = [0; 16];
    client.random(&mut operation_id)?;
    if operation_id == [0; 16] {
        operation_id[15] = 1;
    }
    Ok(operation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::{
        Extension, FrameCodec, FrameHeader, FrameLimits, core::ResultPrefix, transfer::Direction,
    };

    use crate::test_support::{HostState, bootstrap_client};

    fn stream_descriptor(
        transfer_id: u32,
        content_kind: u16,
        direction: Direction,
        sender_send_credit: u64,
    ) -> Descriptor {
        Descriptor {
            transfer_id,
            mode: yas_wire::transfer::Mode::Byte,
            direction,
            receiver_send_credit: if direction == Direction::RECEIVER_TO_SENDER {
                64 * 1024
            } else {
                0
            },
            sender_send_credit,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::PROCESS,
            content_kind,
            content_version: wire::VERSION,
            extensions: Extensions(alloc::vec![Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn input_descriptor(transfer_id: u32) -> Descriptor {
        stream_descriptor(
            transfer_id,
            yas_wire::schema::process::STREAM_STDIN_CONTENT_KIND as u16,
            Direction::RECEIVER_TO_SENDER,
            0,
        )
    }

    fn output_descriptor(transfer_id: u32, kind: StreamKind) -> Descriptor {
        let content_kind = match kind {
            StreamKind::Stdout => yas_wire::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
            StreamKind::Stderr => yas_wire::schema::process::STREAM_STDERR_CONTENT_KIND as u16,
        };
        stream_descriptor(
            transfer_id,
            content_kind,
            Direction::SENDER_TO_RECEIVER,
            DEFAULT_STREAM_WINDOW,
        )
    }

    fn stream_bundle(merged_stderr: bool, stdin: bool) -> wire::StreamBundle {
        wire::StreamBundle {
            process_handle: 77,
            stdout_lifetime_offset: 0,
            stderr_lifetime_offset: 0,
            stdin: stdin.then(|| input_descriptor(70)),
            stdout: output_descriptor(71, StreamKind::Stdout),
            stderr: (!merged_stderr).then(|| output_descriptor(72, StreamKind::Stderr)),
            merged_stderr,
            extensions: Extensions::default(),
        }
    }

    fn queue_control_result(state: &mut HostState, status: Status) {
        let body = if status == Status::Ok {
            wire::ControlResult { state_revision: 1 }.encode().unwrap()
        } else {
            Vec::new()
        };
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::PROCESS, wire::request_kind::CONTROL, 3)
                    },
                    payload: ResultPrefix {
                        status,
                        detail: Extensions::default(),
                        body,
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );
    }

    fn sent_frames(state: &HostState) -> Vec<Frame> {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state
            .sent
            .iter()
            .map(|packet| {
                let (frame, consumed) = codec.decode_stream(packet).unwrap();
                assert_eq!(consumed, packet.len());
                frame
            })
            .collect()
    }

    fn process(client: &mut Client, transfer_id: u32) -> Process {
        let lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        Process {
            handle: u64::from(transfer_id) + 100,
            stdout_lifetime_offset: 0,
            stderr_lifetime_offset: 0,
            stdin: None,
            stdout: Output::new(
                client,
                output_descriptor(transfer_id, StreamKind::Stdout),
                StreamKind::Stdout,
                lease,
            )
            .unwrap(),
            stderr: None,
            merged_stderr: true,
        }
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
            payload: TransferReset {
                transfer_id,
                status: Status::Cancelled.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        }
    }

    fn delivery(event: Option<Event>) -> Delivery {
        let Some(Event::Output(delivery)) = event else {
            panic!("expected Process output delivery");
        };
        delivery
    }

    #[test]
    fn invalid_spawn_kill_settles_before_resets_for_ordinary_and_detachable_children() {
        for (flags, kill_status) in [
            (0, Status::Ok),
            (
                yas_wire::schema::process::SPAWN_DETACHABLE as u16,
                Status::Conflict,
            ),
        ] {
            let (mut client, state, _guard) = bootstrap_client();
            queue_control_result(&mut state.borrow_mut(), kill_status);
            let mut stdout_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
            stdout_lease.commit();
            let mut stderr_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
            stderr_lease.commit();

            // The body is wire-valid but contradicts the non-merged SPAWN
            // policy. Both ordinary and detachable children require terminal
            // cleanup; Conflict means the KILL target was already terminal.
            assert!(matches!(
                Process::new_spawn(
                    &mut client,
                    stream_bundle(true, true),
                    flags,
                    [9; 16],
                    stdout_lease,
                    Some(stderr_lease),
                ),
                Err(Error::InvalidStream)
            ));

            let host = state.borrow();
            let frames = sent_frames(&host);
            assert_eq!(frames.len(), 3);
            assert_eq!(frames[0].header.family, family::PROCESS);
            assert_eq!(frames[0].header.kind, wire::request_kind::CONTROL);
            let control = wire::Control::decode(&frames[0].payload).unwrap();
            assert_eq!(control.process_handle, 77);
            assert_eq!(control.operation_id, [9; 16]);
            assert_eq!(control.action, wire::ControlAction::Kill);
            assert_eq!(
                frames[1..]
                    .iter()
                    .map(|frame| TransferReset::decode(&frame.payload).unwrap().transfer_id)
                    .collect::<Vec<_>>(),
                alloc::vec![70, 71]
            );
            // The KILL Result was actually consumed before either RESET was
            // sent; this catches the send-first-but-racy implementation.
            assert_eq!(host.sent_after_receives, alloc::vec![0, 1, 1]);
            drop(host);
            assert_eq!(client.available_receive_credit(), 8 * 1024 * 1024);
            assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
            assert!(matches!(
                client.receive_credit_exact(1),
                Err(ClientError::Poisoned)
            ));
        }
    }

    #[test]
    fn invalid_spawn_kill_failure_still_resets_every_disclosed_stream_and_pins() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut stdout_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stdout_lease.commit();
        let mut stderr_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stderr_lease.commit();
        let mut bundle = stream_bundle(false, true);
        bundle.stdout.sender_send_credit = DEFAULT_STREAM_WINDOW + 1;

        assert!(matches!(
            Process::new_spawn(
                &mut client,
                bundle,
                0,
                [9; 16],
                stdout_lease,
                Some(stderr_lease),
            ),
            Err(Error::InvalidStream)
        ));
        let host = state.borrow();
        let frames = sent_frames(&host);
        assert_eq!(frames[0].header.family, family::PROCESS);
        assert_eq!(frames[0].header.kind, wire::request_kind::CONTROL);
        assert_eq!(
            frames[1..]
                .iter()
                .map(|frame| TransferReset::decode(&frame.payload).unwrap().transfer_id)
                .collect::<Vec<_>>(),
            alloc::vec![70, 71, 72]
        );
        assert_eq!(host.sent_after_receives, alloc::vec![0, 0, 0, 0]);
        drop(host);
        assert_eq!(client.available_receive_credit(), 8 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }

    #[test]
    fn attach_stdin_shape_mismatch_resets_without_kill_and_poisons() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut stdout_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stdout_lease.commit();
        let mut stderr_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stderr_lease.commit();

        // ATTACH_STDIN is absent, but the successful body disclosed stdin.
        assert!(matches!(
            Process::new_attach(
                &mut client,
                stream_bundle(false, true),
                0,
                stdout_lease,
                stderr_lease,
            ),
            Err(Error::InvalidStream)
        ));
        let host = state.borrow();
        let frames = sent_frames(&host);
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|frame| {
            frame.header.family == family::TRANSFER
                && frame.header.kind == yas_wire::transfer::kind::RESET
        }));
        assert_eq!(
            frames
                .iter()
                .map(|frame| TransferReset::decode(&frame.payload).unwrap().transfer_id)
                .collect::<Vec<_>>(),
            alloc::vec![70, 71, 72]
        );
        assert_eq!(host.sent_after_receives, alloc::vec![0, 0, 0]);
        drop(host);
        assert_eq!(client.available_receive_credit(), 8 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }

    #[test]
    fn attach_preserves_nonzero_process_lifetime_offsets() {
        let (mut client, _state, _guard) = bootstrap_client();
        let mut stdout_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stdout_lease.commit();
        let mut stderr_lease = client.receive_credit_exact(DEFAULT_STREAM_WINDOW).unwrap();
        stderr_lease.commit();
        let mut bundle = stream_bundle(false, true);
        bundle.stdout_lifetime_offset = 123;
        bundle.stderr_lifetime_offset = 456;

        let process = Process::new_attach(
            &mut client,
            bundle,
            yas_wire::schema::process::ATTACH_STDIN as u16,
            stdout_lease,
            stderr_lease,
        )
        .unwrap();
        assert_eq!(process.stdout_lifetime_offset(), 123);
        assert_eq!(process.stderr_lifetime_offset(), 456);
    }

    #[test]
    fn held_delivery_retires_credit_only_after_close_or_reset_and_consume() {
        let (mut client, state, _guard) = bootstrap_client();

        let mut closed = process(&mut client, 41);
        let first = delivery(closed.offer_frame(&data(41, 0, b"a")).unwrap());
        let sent_before_terminal = state.borrow().sent.len();
        assert!(matches!(
            closed.offer_frame(&close(41, 1)).unwrap(),
            Some(Event::StreamClosed(_))
        ));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert_eq!(closed.consume(&mut client, first).unwrap(), b"a");
        assert_eq!(state.borrow().sent.len(), sent_before_terminal);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        let mut reset_stream = process(&mut client, 42);
        let second = delivery(reset_stream.offer_frame(&data(42, 0, b"b")).unwrap());
        let sent_before_terminal = state.borrow().sent.len();
        assert!(matches!(
            reset_stream.offer_frame(&reset(42)).unwrap(),
            Some(Event::StreamClosed(_))
        ));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        reset_stream.discard(&mut client, second).unwrap();
        assert_eq!(state.borrow().sent.len(), sent_before_terminal);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn blocking_receive_does_not_skip_second_data_to_later_close() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut process = process(&mut client, 51);
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.extend([
            codec.encode_stream(&data(51, 0, b"a")).unwrap(),
            codec.encode_stream(&data(51, 1, b"b")).unwrap(),
            codec.encode_stream(&close(51, 2)).unwrap(),
        ]);

        let Event::Output(first) = process.next_event(&mut client).unwrap() else {
            panic!("expected first Process output");
        };
        assert!(!process.owns_frame(&data(51, 1, b"b")));
        assert!(!process.owns_frame(&close(51, 2)));
        assert!(matches!(
            process.next_event(&mut client),
            Err(Error::DeliveryPending)
        ));
        assert_eq!(state.borrow().incoming.len(), 2);
        process.consume(&mut client, first).unwrap();

        let Event::Output(second) = process.next_event(&mut client).unwrap() else {
            panic!("expected second Process output");
        };
        assert!(matches!(
            process.next_event(&mut client),
            Err(Error::DeliveryPending)
        ));
        assert_eq!(state.borrow().incoming.len(), 1);
        process.consume(&mut client, second).unwrap();
        assert!(matches!(
            process.next_event(&mut client).unwrap(),
            Event::StreamClosed(_)
        ));
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }
}
