//! Typed `yas.cli.v1` command-provider support over native YAS Channels.

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};
use core::{fmt, str};

use yas_wire::{Class, core::Status, family};

use crate::{
    channel::{
        Channel, CloseReason, Closed, Error as ChannelError, Event as ChannelEvent, Listener,
        ListenerEvent,
    },
    yas::{Client, Error as ClientError},
};

const INVOKE: u8 = 1;
const STDIN: u8 = 2;
const STDIN_EOF: u8 = 3;
const CANCEL: u8 = 4;
const INVOKE_STDIN: u8 = 1;

const STDOUT: u8 = 1;
const STDERR: u8 = 2;
const LOG: u8 = 3;
const RESULT: u8 = 4;
const EXIT: u8 = 5;

/// Typed command-provider failure.
#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Channel(ChannelError),
    FeatureMissing,
    InvalidContext,
    InvalidDescriptor,
    RegistrationUnavailable,
    InvalidInvocation(&'static str),
    PayloadTooLarge,
    AllocationFailed,
    InvalidContentType,
    DuplicateResult,
    Finished,
    Closed(Closed),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Channel(error) => write!(formatter, "native Channel error: {error}"),
            Self::FeatureMissing => {
                formatter.write_str("native Extension and Channel families are required")
            }
            Self::InvalidContext => formatter
                .write_str("command providers require a named persistent extension attempt"),
            Self::InvalidDescriptor => {
                formatter.write_str("command descriptor is empty or exceeds 64 KiB")
            }
            Self::RegistrationUnavailable => {
                formatter.write_str("server does not advertise native guest command registration")
            }
            Self::InvalidInvocation(detail) => {
                write!(formatter, "invalid yas.cli.v1 invocation: {detail}")
            }
            Self::PayloadTooLarge => {
                formatter.write_str("yas.cli.v1 payload exceeds the Channel limit")
            }
            Self::AllocationFailed => formatter.write_str("could not allocate command payload"),
            Self::InvalidContentType => {
                formatter.write_str("result content type is not a canonical lowercase media type")
            }
            Self::DuplicateResult => {
                formatter.write_str("an invocation may send at most one structured result")
            }
            Self::Finished => formatter.write_str("the invocation already sent EXIT"),
            Self::Closed(closed) => write!(
                formatter,
                "invocation Channel closed with status {}: {}",
                closed.status, closed.detail
            ),
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

impl From<ChannelError> for Error {
    fn from(value: ChannelError) -> Self {
        Self::Channel(value)
    }
}

/// One advertised command listener.
#[derive(Debug)]
pub struct CommandProvider {
    listener: Listener,
    pending: BTreeMap<u32, Channel>,
}

#[derive(Debug)]
pub enum ProviderEvent {
    Invocation(Box<Invocation>),
    Closed(Closed),
}

impl CommandProvider {
    /// Validate and publish a command descriptor for an exact native listener.
    ///
    /// Registration is guarded by the negotiated Extension operation. Older
    /// servers fail explicitly; the SDK never falls back to a private packet
    /// adapter.
    pub fn register(
        client: &mut Client,
        mut listener: Listener,
        descriptor: &str,
    ) -> Result<Self, Error> {
        if descriptor.is_empty()
            || descriptor.len() > yas_wire::extension::MAX_COMMAND_DESCRIPTOR_BYTES
        {
            retire_consumed_listener(client, &mut listener);
            return Err(Error::InvalidDescriptor);
        }
        let context = client.context();
        if context.name.is_empty()
            || context.flags & yas_wire::schema::extension::DEFINITION_PERSISTENT as u16 == 0
        {
            retire_consumed_listener(client, &mut listener);
            return Err(Error::InvalidContext);
        }
        if !client.supports(
            family::EXTENSION,
            Class::Request,
            yas_wire::extension::request_kind::REGISTER_COMMAND,
        ) {
            retire_consumed_listener(client, &mut listener);
            return Err(Error::RegistrationUnavailable);
        }
        let registration: Result<yas_wire::extension::RegisterCommandResult, ClientError> = client
            .request_typed(
                family::EXTENSION,
                yas_wire::extension::request_kind::REGISTER_COMMAND,
                &yas_wire::extension::RegisterCommand {
                    listener_handle: listener.handle(),
                    listener_generation: listener.generation(),
                    descriptor: descriptor.into(),
                    extensions: Default::default(),
                },
                true,
            );
        if let Err(error) = registration {
            retire_consumed_listener(client, &mut listener);
            return Err(error.into());
        }
        Ok(Self {
            listener,
            pending: BTreeMap::new(),
        })
    }

    pub fn listener_handle(&self) -> u64 {
        self.listener.handle()
    }

    pub fn listener_generation(&self) -> u64 {
        self.listener.generation()
    }

    pub fn listener_name(&self) -> &str {
        self.listener.name()
    }

    pub fn update_descriptor(
        &mut self,
        client: &mut Client,
        descriptor: &str,
    ) -> Result<(), Error> {
        if descriptor.is_empty()
            || descriptor.len() > yas_wire::extension::MAX_COMMAND_DESCRIPTOR_BYTES
        {
            return Err(Error::InvalidDescriptor);
        }
        let _: yas_wire::extension::RegisterCommandResult = client.request_typed(
            family::EXTENSION,
            yas_wire::extension::request_kind::REGISTER_COMMAND,
            &yas_wire::extension::RegisterCommand {
                listener_handle: self.listener.handle(),
                listener_generation: self.listener.generation(),
                descriptor: descriptor.into(),
                extensions: Default::default(),
            },
            true,
        )?;
        Ok(())
    }

    pub fn unregister(&mut self, client: &mut Client) -> Result<(), Error> {
        let _: yas_wire::extension::RegisterCommandResult = client.request_typed(
            family::EXTENSION,
            yas_wire::extension::request_kind::REGISTER_COMMAND,
            &yas_wire::extension::RegisterCommand {
                listener_handle: 0,
                listener_generation: 0,
                descriptor: String::new(),
                extensions: Default::default(),
            },
            true,
        )?;
        Ok(())
    }

    pub fn accept(&mut self, client: &mut Client) -> Result<ProviderEvent, Error> {
        match self.listener.accept(client)? {
            ListenerEvent::Accepted(channel) => Invocation::begin(client, *channel)
                .map(Box::new)
                .map(ProviderEvent::Invocation),
            ListenerEvent::Closed(closed) => Ok(ProviderEvent::Closed(closed)),
        }
    }

    /// Offer one already-decoded Event to the listener and any accepted
    /// command Channel without blocking. ACCEPT and fragmented first-message
    /// Transfer events may legitimately return `None`; an Invocation is
    /// returned only after its complete INVOKE message has been acknowledged.
    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &yas_wire::Frame,
    ) -> Result<Option<ProviderEvent>, Error> {
        if let Some(event) = self.listener.offer_frame(client, frame)? {
            return match event {
                ListenerEvent::Accepted(channel) => {
                    let channel = *channel;
                    self.queue_pending_channel(client, channel)?;
                    Ok(None)
                }
                ListenerEvent::Closed(closed) => Ok(Some(ProviderEvent::Closed(closed))),
            };
        }

        let Some(transfer_id) = self
            .pending
            .iter()
            .find_map(|(transfer_id, channel)| channel.owns_frame(frame).then_some(*transfer_id))
        else {
            return Ok(None);
        };
        let mut channel = self
            .pending
            .remove(&transfer_id)
            .ok_or(Error::InvalidInvocation(
                "pending command Channel disappeared",
            ))?;
        let event = match channel.offer_frame(frame) {
            Ok(event) => event,
            Err(error) => {
                let _ = channel.abandon(client, Status::Cancelled);
                return Err(error.into());
            }
        };
        match event {
            Some(ChannelEvent::Data(delivery)) => {
                let invocation = Invocation::from_delivery(client, channel, delivery)?;
                Ok(Some(ProviderEvent::Invocation(Box::new(invocation))))
            }
            Some(ChannelEvent::Closed(_)) => {
                channel.abandon(client, Status::Cancelled)?;
                Ok(None)
            }
            Some(ChannelEvent::Acknowledged { .. }) | None => {
                self.pending.insert(transfer_id, channel);
                Ok(None)
            }
        }
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        let mut first_error = self.unregister(client).err();
        if let Err(error) = self.listener.close(client)
            && first_error.is_none()
        {
            first_error = Some(error.into());
        }
        while let Some((_transfer_id, mut channel)) = self.pending.pop_first() {
            if let Err(error) = channel.abandon(client, Status::Cancelled)
                && first_error.is_none()
            {
                first_error = Some(error.into());
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn queue_pending_channel(
        &mut self,
        client: &mut Client,
        mut channel: Channel,
    ) -> Result<(), Error> {
        let transfer_id = channel.transfer_id();
        if let Some(mut existing) = self.pending.remove(&transfer_id) {
            // Two accepted endpoints cannot legitimately share one Transfer
            // identity. Retire both wrappers and poison even when both RESETs
            // are sent: already-in-flight frames cannot be attributed safely.
            let _ = existing.abandon(client, Status::Cancelled);
            let _ = channel.abandon(client, Status::Cancelled);
            client.poison();
            return Err(Error::InvalidInvocation(
                "duplicate pending command Transfer",
            ));
        }
        self.pending.insert(transfer_id, channel);
        Ok(())
    }
}

fn retire_consumed_listener(client: &mut Client, listener: &mut Listener) {
    if listener.close(client).is_err() {
        // Registration consumed the Listener value. If CLOSE cannot be
        // confirmed, session teardown is the only safe retirement boundary.
        client.poison();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationRequest {
    pub args: Vec<String>,
    pub streams_stdin: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    Stdin(Vec<u8>),
    StdinEof,
    Cancel,
    Closed(Closed),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warning = 3,
    Error = 4,
}

#[derive(Debug)]
pub struct Invocation {
    channel: Channel,
    request: InvocationRequest,
    stdin_done: bool,
    result_sent: bool,
    finished: bool,
}

impl Invocation {
    fn begin(client: &mut Client, mut channel: Channel) -> Result<Self, Error> {
        loop {
            let event = match channel.receive(client) {
                Ok(event) => event,
                Err(error) => {
                    let _ = channel.abandon(client, Status::Cancelled);
                    return Err(error.into());
                }
            };
            match event {
                ChannelEvent::Data(delivery) => {
                    return Self::from_delivery(client, channel, delivery);
                }
                ChannelEvent::Acknowledged { .. } => {}
                ChannelEvent::Closed(closed) => {
                    let _ = channel.abandon(client, Status::Cancelled);
                    return Err(Error::Closed(closed));
                }
            }
        }
    }

    fn from_delivery(
        client: &mut Client,
        mut channel: Channel,
        delivery: crate::channel::Delivery,
    ) -> Result<Self, Error> {
        let request = match decode_invocation(delivery.payload()) {
            Ok(request) => request,
            Err(error) => {
                drop(delivery);
                let _ = channel.abandon(client, Status::Cancelled);
                return Err(error);
            }
        };
        if let Err(error) = channel.discard(client, delivery) {
            let _ = channel.abandon(client, Status::Cancelled);
            return Err(error.into());
        }
        let invocation = Self {
            stdin_done: !request.streams_stdin,
            channel,
            request,
            result_sent: false,
            finished: false,
        };
        Ok(invocation)
    }

    pub fn channel_handle(&self) -> u64 {
        self.channel.handle()
    }

    pub fn peer_session(&self) -> &[u8; 16] {
        self.channel.peer_session()
    }

    pub fn metadata(&self) -> &[u8] {
        self.channel.connector_metadata()
    }

    pub const fn request(&self) -> &InvocationRequest {
        &self.request
    }

    pub fn available_credit(&self) -> u64 {
        self.channel.available_credit()
    }

    pub fn owns_frame(&self, frame: &yas_wire::Frame) -> bool {
        self.channel.owns_frame(frame)
    }

    /// Offer a routed Channel/Transfer Event without blocking. Credit-only
    /// frames return `None`; peer input is decoded and acknowledged here.
    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &yas_wire::Frame,
    ) -> Result<Option<Input>, Error> {
        if self.finished || !self.channel.owns_frame(frame) {
            return Ok(None);
        }
        let result = (|| {
            let event = self.channel.offer_frame(frame)?;
            self.interpret_channel_event(client, event)
        })();
        if result.is_err() {
            self.retire_after_error(client);
        }
        result
    }

    /// Return input already buffered behind a previously consumed Channel
    /// delivery without waiting for another host frame.
    pub fn poll_input(&mut self, client: &mut Client) -> Result<Option<Input>, Error> {
        let result = self
            .channel
            .poll_event()
            .map_err(Error::from)
            .and_then(|event| self.interpret_channel_event(client, event));
        if result.is_err() {
            self.retire_after_error(client);
        }
        result
    }

    pub fn receive_input(&mut self, client: &mut Client) -> Result<Input, Error> {
        if self.finished {
            return Err(Error::Finished);
        }
        loop {
            if let Some(input) = self.poll_input(client)? {
                return Ok(input);
            }
            let event = match self.channel.receive(client) {
                Ok(event) => event,
                Err(error) => {
                    self.retire_after_error(client);
                    return Err(error.into());
                }
            };
            match self.interpret_channel_event(client, Some(event)) {
                Ok(Some(input)) => return Ok(input),
                Ok(None) => {}
                Err(error) => {
                    self.retire_after_error(client);
                    return Err(error);
                }
            }
        }
    }

    fn interpret_channel_event(
        &mut self,
        client: &mut Client,
        event: Option<ChannelEvent>,
    ) -> Result<Option<Input>, Error> {
        match event {
            Some(ChannelEvent::Acknowledged { .. }) | None => Ok(None),
            Some(ChannelEvent::Closed(closed)) => {
                self.channel.close_owned(client, CloseReason::Normal)?;
                self.finished = true;
                Ok(Some(Input::Closed(closed)))
            }
            Some(ChannelEvent::Data(delivery)) => {
                let previous = self.stdin_done;
                let parsed = match self.decode_input(delivery.payload()) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        self.stdin_done = previous;
                        drop(delivery);
                        return Err(error);
                    }
                };
                self.channel.discard(client, delivery)?;
                Ok(Some(parsed))
            }
        }
    }

    fn retire_after_error(&mut self, client: &mut Client) {
        self.finished = true;
        let _ = self.channel.abandon(client, Status::Cancelled);
    }

    pub fn stdout(&mut self, client: &mut Client, data: &[u8]) -> Result<(), Error> {
        self.send_output(client, stdout_payload(data)?)
    }

    pub fn stderr(&mut self, client: &mut Client, data: &[u8]) -> Result<(), Error> {
        self.send_output(client, stderr_payload(data)?)
    }

    pub fn log(
        &mut self,
        client: &mut Client,
        level: LogLevel,
        message: &str,
    ) -> Result<(), Error> {
        self.send_output(client, log_payload(level, message)?)
    }

    pub fn result(
        &mut self,
        client: &mut Client,
        content_type: &str,
        data: &[u8],
    ) -> Result<(), Error> {
        if self.result_sent {
            return Err(Error::DuplicateResult);
        }
        self.send_output(client, result_payload(content_type, data)?)?;
        self.result_sent = true;
        Ok(())
    }

    pub fn exit(&mut self, client: &mut Client, code: i32, detail: &str) -> Result<(), Error> {
        if self.finished {
            return Err(Error::Finished);
        }
        self.send_output(client, exit_payload(code, detail)?)?;
        self.finished = true;
        self.channel.close_owned(client, CloseReason::Normal)?;
        Ok(())
    }

    pub fn cancel(&mut self, client: &mut Client) -> Result<(), Error> {
        self.finished = true;
        self.channel.close_owned(client, CloseReason::Cancelled)?;
        Ok(())
    }

    fn send_output(&mut self, client: &mut Client, payload: Vec<u8>) -> Result<(), Error> {
        if self.finished {
            return Err(Error::Finished);
        }
        match self.channel.send(client, &payload) {
            Ok(()) => Ok(()),
            Err(error @ (ChannelError::InvalidPayload | ChannelError::CreditExhausted { .. })) => {
                Err(error.into())
            }
            Err(error) => {
                self.retire_after_error(client);
                Err(error.into())
            }
        }
    }

    fn decode_input(&mut self, payload: &[u8]) -> Result<Input, Error> {
        let Some((&kind, body)) = payload.split_first() else {
            return Err(Error::InvalidInvocation("empty DATA payload"));
        };
        match kind {
            STDIN if !self.stdin_done && self.request.streams_stdin => {
                Ok(Input::Stdin(body.to_vec()))
            }
            STDIN_EOF if body.is_empty() && !self.stdin_done && self.request.streams_stdin => {
                self.stdin_done = true;
                Ok(Input::StdinEof)
            }
            CANCEL if body.is_empty() => {
                self.stdin_done = true;
                Ok(Input::Cancel)
            }
            STDIN | STDIN_EOF => Err(Error::InvalidInvocation(
                "stdin arrived after EOF or without the stdin flag",
            )),
            CANCEL => Err(Error::InvalidInvocation("CANCEL has a body")),
            _ => Err(Error::InvalidInvocation("unknown input kind")),
        }
    }
}

pub fn stdout_payload(data: &[u8]) -> Result<Vec<u8>, Error> {
    bytes_payload(STDOUT, data)
}

pub fn stderr_payload(data: &[u8]) -> Result<Vec<u8>, Error> {
    bytes_payload(STDERR, data)
}

pub fn log_payload(level: LogLevel, message: &str) -> Result<Vec<u8>, Error> {
    let mut payload = payload_with_capacity(2, message.len())?;
    payload.push(LOG);
    payload.push(level as u8);
    payload.extend_from_slice(message.as_bytes());
    Ok(payload)
}

pub fn result_payload(content_type: &str, data: &[u8]) -> Result<Vec<u8>, Error> {
    if !valid_content_type(content_type) {
        return Err(Error::InvalidContentType);
    }
    let mut payload = payload_with_capacity(3 + content_type.len(), data.len())?;
    payload.push(RESULT);
    payload.extend_from_slice(&(content_type.len() as u16).to_le_bytes());
    payload.extend_from_slice(content_type.as_bytes());
    payload.extend_from_slice(data);
    Ok(payload)
}

pub fn exit_payload(code: i32, detail: &str) -> Result<Vec<u8>, Error> {
    let mut payload = payload_with_capacity(5, detail.len())?;
    payload.push(EXIT);
    payload.extend_from_slice(&code.to_le_bytes());
    payload.extend_from_slice(detail.as_bytes());
    Ok(payload)
}

fn bytes_payload(kind: u8, data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut payload = payload_with_capacity(1, data.len())?;
    payload.push(kind);
    payload.extend_from_slice(data);
    Ok(payload)
}

fn payload_with_capacity(prefix: usize, body: usize) -> Result<Vec<u8>, Error> {
    let total = prefix.checked_add(body).ok_or(Error::PayloadTooLarge)?;
    if total as u64 > yas_wire::channel::MAX_MESSAGE_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(total)
        .map_err(|_| Error::AllocationFailed)?;
    Ok(payload)
}

fn valid_content_type(content_type: &str) -> bool {
    if content_type.is_empty() || content_type.len() > 255 {
        return false;
    }
    let mut components = content_type.split('/');
    let (Some(left), Some(right), None) = (components.next(), components.next(), components.next())
    else {
        return false;
    };
    valid_media_component(left) && valid_media_component(right)
}

fn valid_media_component(component: &str) -> bool {
    let mut bytes = component.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&^_.+-".contains(&byte)
        })
}

fn decode_invocation(payload: &[u8]) -> Result<InvocationRequest, Error> {
    if payload.len() as u64 > yas_wire::channel::MAX_MESSAGE_BYTES
        || payload.first() != Some(&INVOKE)
        || payload.len() < 4
    {
        return Err(Error::InvalidInvocation("INVOKE header is malformed"));
    }
    let flags = payload[1];
    if flags & !INVOKE_STDIN != 0 {
        return Err(Error::InvalidInvocation("INVOKE has reserved flags"));
    }
    let count = u16::from_le_bytes([payload[2], payload[3]]) as usize;
    if count > yas_wire::extension::MAX_ARGS {
        return Err(Error::InvalidInvocation("too many arguments"));
    }
    let mut offset = 4usize;
    let mut argument_bytes = 0usize;
    let mut args = Vec::new();
    args.try_reserve_exact(count)
        .map_err(|_| Error::AllocationFailed)?;
    for _ in 0..count {
        let length_end = offset
            .checked_add(4)
            .ok_or(Error::InvalidInvocation("argument length overflow"))?;
        let length = u32::from_le_bytes(
            payload
                .get(offset..length_end)
                .ok_or(Error::InvalidInvocation("truncated argument length"))?
                .try_into()
                .expect("checked argument length"),
        ) as usize;
        if length > yas_wire::extension::MAX_ARG_BYTES {
            return Err(Error::InvalidInvocation("argument is too large"));
        }
        argument_bytes = argument_bytes
            .checked_add(length)
            .ok_or(Error::InvalidInvocation("argument bytes overflow"))?;
        if argument_bytes > yas_wire::extension::MAX_ARGUMENT_BYTES {
            return Err(Error::InvalidInvocation("argument vector is too large"));
        }
        offset = length_end;
        let end = offset
            .checked_add(length)
            .ok_or(Error::InvalidInvocation("argument length overflow"))?;
        let argument = str::from_utf8(
            payload
                .get(offset..end)
                .ok_or(Error::InvalidInvocation("truncated argument"))?,
        )
        .map_err(|_| Error::InvalidInvocation("argument is not UTF-8"))?;
        args.push(argument.into());
        offset = end;
    }
    if offset != payload.len() {
        return Err(Error::InvalidInvocation("INVOKE has trailing bytes"));
    }
    Ok(InvocationRequest {
        args,
        streams_stdin: flags & INVOKE_STDIN != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use yas_wire::{
        Decode, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
        core::ResultPrefix,
        transfer::{Credit, MessageData, Reset as TransferReset},
    };

    use crate::{
        channel::{DEFAULT_RECEIVE_WINDOW, accepted_for_test, listener_for_test},
        test_support::{HostState, bootstrap_client},
    };

    fn queue_result(
        state: &mut HostState,
        family_id: u16,
        kind: u16,
        request_id: u32,
        status: Status,
        body: Vec<u8>,
        sensitive: bool,
    ) {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.incoming.push_back(
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
                .unwrap(),
        );
    }

    fn message(transfer_id: u32, sequence: u64, payload: &[u8]) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::MESSAGE_DATA)
            },
            payload: MessageData {
                transfer_id,
                sequence,
                fragment_offset: 0,
                start: true,
                end: true,
                data: payload.to_vec(),
            }
            .encode()
            .unwrap(),
        }
    }

    fn credit(transfer_id: u32, cumulative_limit: u64) -> Frame {
        Frame {
            header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT),
            payload: Credit {
                transfer_id,
                cumulative_limit,
            }
            .encode()
            .unwrap(),
        }
    }

    fn invoke(streams_stdin: bool) -> Vec<u8> {
        vec![INVOKE, u8::from(streams_stdin), 0, 0]
    }

    fn reset_ids(state: &HostState) -> Vec<u32> {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state
            .sent
            .iter()
            .filter_map(|packet| {
                let (frame, _) = codec.decode_stream(packet).unwrap();
                (frame.header.family == family::TRANSFER
                    && frame.header.kind == yas_wire::transfer::kind::RESET)
                    .then(|| TransferReset::decode(&frame.payload).unwrap().transfer_id)
            })
            .collect()
    }

    #[test]
    fn payload_helpers_preserve_native_command_grammar() {
        assert_eq!(stdout_payload(b"hello").unwrap(), b"\x01hello");
        assert_eq!(stderr_payload(b"bad").unwrap(), b"\x02bad");
        assert_eq!(exit_payload(7, "done").unwrap(), b"\x05\x07\0\0\0done");
        assert!(result_payload("Application/JSON", b"{}").is_err());
    }

    #[test]
    fn invocation_uses_utf8_arguments_and_opaque_stdin_flag() {
        let mut payload = vec![INVOKE, INVOKE_STDIN, 2, 0];
        for argument in ["status", "café"] {
            payload.extend_from_slice(&(argument.len() as u32).to_le_bytes());
            payload.extend_from_slice(argument.as_bytes());
        }
        assert_eq!(
            decode_invocation(&payload).unwrap(),
            InvocationRequest {
                args: vec!["status".into(), "café".into()],
                streams_stdin: true,
            }
        );
    }

    #[test]
    fn invalid_registration_input_closes_consumed_listener() {
        let (mut client, state, _guard) = bootstrap_client();
        queue_result(
            &mut state.borrow_mut(),
            family::CHANNEL,
            yas_wire::channel::request_kind::CLOSE_LISTENER,
            3,
            Status::Ok,
            Vec::new(),
            false,
        );

        assert!(matches!(
            CommandProvider::register(&mut client, listener_for_test(31, 32), ""),
            Err(Error::InvalidDescriptor)
        ));
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let sent = state.borrow();
        assert_eq!(sent.sent.len(), 1);
        let (frame, _) = codec.decode_stream(&sent.sent[0]).unwrap();
        assert_eq!(frame.header.family, family::CHANNEL);
        assert_eq!(
            frame.header.kind,
            yas_wire::channel::request_kind::CLOSE_LISTENER
        );
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn failed_registration_closes_consumed_listener_after_result() {
        let (mut client, state, _guard) = bootstrap_client();
        queue_result(
            &mut state.borrow_mut(),
            family::EXTENSION,
            yas_wire::extension::request_kind::REGISTER_COMMAND,
            3,
            Status::ResourceExhausted,
            Vec::new(),
            true,
        );
        queue_result(
            &mut state.borrow_mut(),
            family::CHANNEL,
            yas_wire::channel::request_kind::CLOSE_LISTENER,
            5,
            Status::Ok,
            Vec::new(),
            false,
        );

        let registration =
            CommandProvider::register(&mut client, listener_for_test(31, 32), "command");
        assert!(
            matches!(
                registration,
                Err(Error::Client(ClientError::RequestFailed {
                    status: Status::ResourceExhausted,
                    ..
                }))
            ),
            "{registration:?}"
        );
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let sent = state.borrow();
        assert_eq!(sent.sent.len(), 2);
        let families = sent
            .sent
            .iter()
            .map(|packet| codec.decode_stream(packet).unwrap().0.header.family)
            .collect::<Vec<_>>();
        assert_eq!(families, vec![family::EXTENSION, family::CHANNEL]);
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn invocation_begin_failure_resets_owned_channel_and_releases_credit() {
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 41;
        let channel = accepted_for_test(&mut client, transfer_id, DEFAULT_RECEIVE_WINDOW).unwrap();
        state.borrow_mut().sent.clear();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&message(transfer_id, 0, b"bad"))
                .unwrap(),
        );

        assert!(matches!(
            Invocation::begin(&mut client, channel),
            Err(Error::InvalidInvocation(_))
        ));
        assert_eq!(reset_ids(&state.borrow()), vec![transfer_id]);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn buffered_input_decode_failure_waits_for_poll_then_retires_channel() {
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 42;
        let mut channel =
            accepted_for_test(&mut client, transfer_id, DEFAULT_RECEIVE_WINDOW).unwrap();
        let Some(ChannelEvent::Data(first)) = channel
            .offer_frame(&message(transfer_id, 0, &invoke(false)))
            .unwrap()
        else {
            panic!("expected INVOKE delivery");
        };
        assert!(
            channel
                .offer_frame(&message(transfer_id, 1, &[STDIN]))
                .unwrap()
                .is_none()
        );
        state.borrow_mut().sent.clear();

        let mut invocation = Invocation::from_delivery(&mut client, channel, first).unwrap();
        assert!(reset_ids(&state.borrow()).is_empty());
        assert_eq!(state.borrow().sent.len(), 1);

        assert!(matches!(
            invocation.poll_input(&mut client),
            Err(Error::InvalidInvocation(_))
        ));
        assert_eq!(reset_ids(&state.borrow()), vec![transfer_id]);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn buffered_stdin_stays_channel_accounted_until_each_poll_boundary() {
        const WINDOW: u64 = 20;
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 47;
        let mut channel = accepted_for_test(&mut client, transfer_id, WINDOW).unwrap();
        state.borrow_mut().sent.clear();
        let Some(ChannelEvent::Data(first)) = channel
            .offer_frame(&message(transfer_id, 0, &invoke(true)))
            .unwrap()
        else {
            panic!("expected INVOKE delivery");
        };
        let stdin_one = [STDIN, b'a', b'b', b'c', b'd', b'e', b'f', b'g'];
        let stdin_two = [STDIN, b'h', b'i', b'j', b'k', b'l', b'm', b'n'];
        assert!(
            channel
                .offer_frame(&message(transfer_id, 2, &stdin_two))
                .unwrap()
                .is_none()
        );
        assert!(
            channel
                .offer_frame(&message(transfer_id, 1, &stdin_one))
                .unwrap()
                .is_none()
        );

        let mut invocation = Invocation::from_delivery(&mut client, channel, first).unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let credit_limits = |state: &HostState| {
            state
                .sent
                .iter()
                .map(|packet| {
                    let (frame, _) = codec.decode_stream(packet).unwrap();
                    Credit::decode(&frame.payload).unwrap().cumulative_limit
                })
                .collect::<Vec<_>>()
        };
        // Only INVOKE crossed the application boundary. Both complete STDIN
        // messages remain inside Channel under the original 20-byte lease.
        assert_eq!(credit_limits(&state.borrow()), vec![24]);

        assert_eq!(
            invocation.poll_input(&mut client).unwrap(),
            Some(Input::Stdin(b"abcdefg".to_vec()))
        );
        assert_eq!(credit_limits(&state.borrow()), vec![24, 32]);
        assert_eq!(
            invocation.poll_input(&mut client).unwrap(),
            Some(Input::Stdin(b"hijklmn".to_vec()))
        );
        assert_eq!(credit_limits(&state.borrow()), vec![24, 32, 40]);
        assert!(invocation.poll_input(&mut client).unwrap().is_none());
        invocation.cancel(&mut client).unwrap();
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn invocation_offer_error_resets_owned_channel() {
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 45;
        let mut channel =
            accepted_for_test(&mut client, transfer_id, DEFAULT_RECEIVE_WINDOW).unwrap();
        let Some(ChannelEvent::Data(first)) = channel
            .offer_frame(&message(transfer_id, 0, &invoke(false)))
            .unwrap()
        else {
            panic!("expected INVOKE delivery");
        };
        let mut invocation = Invocation::from_delivery(&mut client, channel, first).unwrap();
        state.borrow_mut().sent.clear();
        let mut malformed = message(transfer_id, 1, &[CANCEL]);
        malformed.header.sensitive = false;

        assert!(matches!(
            invocation.offer_frame(&mut client, &malformed),
            Err(Error::Channel(ChannelError::Protocol(
                "Transfer sensitivity mismatch"
            )))
        ));
        assert_eq!(reset_ids(&state.borrow()), vec![transfer_id]);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(
            invocation
                .offer_frame(&mut client, &malformed)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn recoverable_output_preflight_errors_preserve_retryable_invocation() {
        const WINDOW: u64 = 8;
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 48;
        let mut channel = accepted_for_test(&mut client, transfer_id, WINDOW).unwrap();
        let Some(ChannelEvent::Data(first)) = channel
            .offer_frame(&message(transfer_id, 0, &invoke(false)))
            .unwrap()
        else {
            panic!("expected INVOKE delivery");
        };
        let mut invocation = Invocation::from_delivery(&mut client, channel, first).unwrap();
        state.borrow_mut().sent.clear();

        invocation.stdout(&mut client, b"1234567").unwrap();
        assert!(matches!(
            invocation.stdout(&mut client, b"x"),
            Err(Error::Channel(ChannelError::CreditExhausted {
                required: 2,
                available: 0,
            }))
        ));
        assert!(!invocation.finished);
        assert!(reset_ids(&state.borrow()).is_empty());
        assert!(
            invocation
                .offer_frame(&mut client, &credit(transfer_id, 10))
                .unwrap()
                .is_none()
        );
        invocation.stdout(&mut client, b"x").unwrap();

        assert!(matches!(
            invocation.stdout(&mut client, b"12345678"),
            Err(Error::Channel(ChannelError::InvalidPayload))
        ));
        assert!(!invocation.finished);
        assert!(reset_ids(&state.borrow()).is_empty());
        // A smaller output remains usable after negotiated-size rejection.
        assert!(
            invocation
                .offer_frame(&mut client, &credit(transfer_id, 18))
                .unwrap()
                .is_none()
        );
        invocation.stdout(&mut client, b"small").unwrap();
    }

    #[test]
    fn duplicate_pending_accepts_retire_both_channels_and_poison() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut provider = CommandProvider {
            listener: listener_for_test(31, 32),
            pending: BTreeMap::new(),
        };
        let first = accepted_for_test(&mut client, 43, DEFAULT_RECEIVE_WINDOW).unwrap();
        let second = accepted_for_test(&mut client, 43, DEFAULT_RECEIVE_WINDOW).unwrap();
        state.borrow_mut().sent.clear();
        provider.queue_pending_channel(&mut client, first).unwrap();

        assert!(matches!(
            provider.queue_pending_channel(&mut client, second),
            Err(Error::InvalidInvocation(
                "duplicate pending command Transfer"
            ))
        ));
        assert!(provider.pending.is_empty());
        assert_eq!(reset_ids(&state.borrow()), vec![43, 43]);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }

    #[test]
    fn provider_close_retires_every_pending_accepted_channel() {
        let (mut client, state, _guard) = bootstrap_client();
        let mut provider = CommandProvider {
            listener: listener_for_test(31, 32),
            pending: BTreeMap::new(),
        };
        let channel = accepted_for_test(&mut client, 46, DEFAULT_RECEIVE_WINDOW).unwrap();
        provider
            .queue_pending_channel(&mut client, channel)
            .unwrap();
        state.borrow_mut().sent.clear();
        let registration = yas_wire::extension::RegisterCommandResult {
            extension_handle: 21,
            generation: 22,
            definition_revision: 23,
            directory_revision: 24,
            changed: true,
            extensions: Extensions::default(),
        };
        queue_result(
            &mut state.borrow_mut(),
            family::EXTENSION,
            yas_wire::extension::request_kind::REGISTER_COMMAND,
            3,
            Status::Ok,
            registration.encode().unwrap(),
            true,
        );
        queue_result(
            &mut state.borrow_mut(),
            family::CHANNEL,
            yas_wire::channel::request_kind::CLOSE_LISTENER,
            5,
            Status::Ok,
            Vec::new(),
            false,
        );

        provider.close(&mut client).unwrap();
        assert!(provider.pending.is_empty());
        assert_eq!(reset_ids(&state.borrow()), vec![46]);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn invocation_exit_discards_buffered_input_then_closes_and_resets() {
        let (mut client, state, _guard) = bootstrap_client();
        let transfer_id = 44;
        let mut channel =
            accepted_for_test(&mut client, transfer_id, DEFAULT_RECEIVE_WINDOW).unwrap();
        let Some(ChannelEvent::Data(first)) = channel
            .offer_frame(&message(transfer_id, 0, &invoke(true)))
            .unwrap()
        else {
            panic!("expected INVOKE delivery");
        };
        assert!(
            channel
                .offer_frame(&message(transfer_id, 1, &[STDIN, b'x']))
                .unwrap()
                .is_none()
        );
        let mut invocation = Invocation::from_delivery(&mut client, channel, first).unwrap();
        assert_eq!(state.borrow().sent.len(), 2);
        state.borrow_mut().sent.clear();

        invocation.exit(&mut client, 0, "").unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let kinds = state
            .borrow()
            .sent
            .iter()
            .map(|packet| codec.decode_stream(packet).unwrap().0.header.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                yas_wire::transfer::kind::MESSAGE_DATA,
                yas_wire::transfer::kind::CLOSE,
                yas_wire::transfer::kind::RESET,
            ]
        );
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }
}
