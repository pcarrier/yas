//! Native Terminal lifecycle, state, input, and bounded query helpers.

use alloc::vec::Vec;
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extension, Extensions, Frame,
    core::Status,
    family,
    state::{
        Cursor, Phase, RecordKind, StateAck, StateEvent, Unwatch, Watch as StateWatch, WatchResult,
    },
    terminal as wire,
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    transfer::{self, ByteReceiver},
    yas::{Client, Error as ClientError, RequestToken},
};

pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;
pub const DEFAULT_QUERY_WINDOW: u64 = 16 * 1024 * 1024;
pub const MAX_RESOURCE_TAG_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    Transfer(transfer::Error),
    FeatureMissing,
    CounterOverflow,
    LimitExceeded { actual: u64, maximum: u64 },
    Protocol(&'static str),
    Completed,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Terminal value: {error}"),
            Self::Transfer(error) => write!(formatter, "native Terminal Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native Terminal operation is unavailable"),
            Self::CounterOverflow => formatter.write_str("native Terminal credit overflow"),
            Self::LimitExceeded { actual, maximum } => write!(
                formatter,
                "native Terminal query returned {actual} bytes; limit is {maximum}"
            ),
            Self::Protocol(detail) => write!(formatter, "native Terminal protocol error: {detail}"),
            Self::Completed => formatter.write_str("native Terminal query is already complete"),
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

impl From<transfer::Error> for Error {
    fn from(value: transfer::Error) -> Self {
        Self::Transfer(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateChange {
    Upsert(wire::TerminalRecord),
    Patch(wire::TerminalPatch),
    Remove(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub phase: Phase,
    pub from_revision: u64,
    pub to_revision: u64,
    pub changes: Vec<StateChange>,
}

/// Return Muster-style resource identity without narrowing or aliasing the
/// terminal handle. Invalid UTF-8 is a protocol error rather than lossy text.
pub fn state_resource_tag(record: &wire::TerminalRecord) -> Result<Option<&str>, Error> {
    let Some(extension) = record.extensions.0.iter().find(|extension| {
        extension.tag == yas_wire::schema::terminal::STATE_RESOURCE_TAG_EXTENSION as u16
    }) else {
        return Ok(None);
    };
    core::str::from_utf8(&extension.value)
        .map(Some)
        .map_err(|_| Error::Protocol("invalid UTF-8 Terminal resource tag"))
}

/// Decode a terminal's exact native exit record when its lifecycle is exited.
pub fn state_exit(record: &wire::TerminalRecord) -> Result<Option<wire::ExitRecord>, Error> {
    record
        .extensions
        .0
        .iter()
        .find(|extension| extension.tag == yas_wire::schema::terminal::STATE_EXIT_EXTENSION as u16)
        .map(|extension| wire::ExitRecord::decode(&extension.value).map_err(Into::into))
        .transpose()
}

/// Extract the optional opaque Surface application handle associated with a
/// terminal. The value remains a full `u64`.
pub fn state_app_handle(record: &wire::TerminalRecord) -> Result<Option<u64>, Error> {
    let Some(extension) = record.extensions.0.iter().find(|extension| {
        extension.tag == yas_wire::schema::terminal::STATE_APP_HANDLE_EXTENSION as u16
    }) else {
        return Ok(None);
    };
    let value: [u8; 8] = extension
        .value
        .as_slice()
        .try_into()
        .map_err(|_| Error::Protocol("invalid Terminal application handle extension"))?;
    let handle = u64::from_le_bytes(value);
    if handle == 0 {
        return Err(Error::Protocol("zero Terminal application handle"));
    }
    Ok(Some(handle))
}

/// Required CREATE extension used by supervisors to adopt their own terminals
/// after a guest restart.
pub fn resource_tag_extension(tag: &str) -> Result<Extension, Error> {
    if tag.is_empty() || tag.len() > MAX_RESOURCE_TAG_BYTES || tag.as_bytes().contains(&0) {
        return Err(Error::Protocol("invalid Terminal resource tag"));
    }
    Ok(Extension {
        tag: yas_wire::schema::terminal::CREATE_RESOURCE_TAG_EXTENSION as u16,
        required: true,
        value: tag.as_bytes().to_vec(),
    })
}

/// Optional LAUNCH extension associating compositor surfaces with a terminal.
pub fn app_handle_extension(app_handle: u64) -> Result<Extension, Error> {
    if app_handle == 0 {
        return Err(Error::Protocol("zero Terminal application handle"));
    }
    Ok(Extension {
        tag: yas_wire::schema::terminal::LAUNCH_APP_HANDLE_EXTENSION as u16,
        required: true,
        value: app_handle.to_le_bytes().to_vec(),
    })
}

/// One bounded Terminal catalogue subscription.
pub struct Watch {
    subscription_id: u32,
    cumulative_credit: u64,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl Watch {
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
            || frame.header.family != family::TERMINAL
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
                RecordKind::Add | RecordKind::Replace => {
                    changes.push(StateChange::Upsert(wire::terminal_from_state_record(
                        record,
                    )?));
                }
                RecordKind::Patch => {
                    changes.push(StateChange::Patch(wire::patch_from_state_record(record)?));
                }
                RecordKind::Remove => changes.push(StateChange::Remove(
                    wire::removal_from_state_record(record)?.terminal_handle,
                )),
                RecordKind::Family(_) => {
                    return Err(Error::Protocol("unexpected Terminal state record kind"));
                }
            }
        }
        self.cumulative_credit = self
            .cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::TERMINAL,
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
            family::TERMINAL,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryMetadata {
    pub content_kind: u8,
    pub encoding: u8,
    pub flags: u16,
    pub next_cursor: Option<wire::QueryNextCursor>,
    pub total_lines: Option<u64>,
    pub satisfying_state_revision: Option<u64>,
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryResult {
    pub metadata: QueryMetadata,
    pub bytes: Vec<u8>,
}

impl QueryResult {
    pub fn decode<T: Decode>(&self) -> Result<T, Error> {
        T::decode(&self.bytes).map_err(Into::into)
    }
}

enum QueryState {
    Result {
        token: RequestToken,
        initial_receive_credit: u64,
        maximum_bytes: u64,
        receive_lease: Option<ReceiveLease>,
    },
    Transfer {
        receiver: ByteReceiver,
        metadata: QueryMetadata,
    },
    Done,
}

/// A Terminal query which can be driven alongside watches and other family
/// streams by repeatedly offering frames from [`Client::next_frame`].
pub struct PendingQuery {
    state: QueryState,
}

impl PendingQuery {
    pub fn owns_frame(&self, frame: &Frame) -> bool {
        match &self.state {
            QueryState::Result { token, .. } => token.matches(frame),
            QueryState::Transfer { receiver, .. } => receiver.owns_frame(frame),
            QueryState::Done => false,
        }
    }

    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<QueryResult>, Error> {
        match &mut self.state {
            QueryState::Result {
                token,
                initial_receive_credit,
                maximum_bytes,
                receive_lease,
            } => {
                let prefix = match client.offer_result(token, frame) {
                    Ok(Some(prefix)) => prefix,
                    Ok(None) => return Ok(None),
                    Err(error) => {
                        self.state = QueryState::Done;
                        return Err(error.into());
                    }
                };
                if prefix.status != Status::Ok {
                    if let Some(mut lease) = receive_lease.take() {
                        lease.release();
                    }
                    let family = token.family();
                    let kind = token.kind();
                    self.state = QueryState::Done;
                    return Err(ClientError::RequestFailed {
                        family,
                        kind,
                        status: prefix.status,
                        detail: prefix.detail,
                    }
                    .into());
                }
                let body = match client.decode_result_body::<wire::QueryBody>(&prefix.body) {
                    Ok(body) => body,
                    Err(error) => {
                        self.state = QueryState::Done;
                        return Err(error.into());
                    }
                };
                if let Err(error) = body.validate_receive_credit(*initial_receive_credit) {
                    if matches!(&body.delivery, wire::QueryDelivery::Inline(_))
                        && let Some(mut lease) = receive_lease.take()
                    {
                        lease.release();
                    }
                    client.poison();
                    self.state = QueryState::Done;
                    return Err(error.into());
                }
                let metadata = query_metadata(&body);
                match body.delivery {
                    wire::QueryDelivery::Inline(bytes) => {
                        let maximum = *maximum_bytes;
                        if bytes.len() as u64 > maximum {
                            if let Some(mut lease) = receive_lease.take() {
                                lease.release();
                            }
                            client.poison();
                            self.state = QueryState::Done;
                            return Err(Error::LimitExceeded {
                                actual: bytes.len() as u64,
                                maximum,
                            });
                        }
                        if let Err(error) = validate_query_content(metadata.content_kind, &bytes) {
                            if let Some(mut lease) = receive_lease.take() {
                                lease.release();
                            }
                            client.poison();
                            self.state = QueryState::Done;
                            return Err(error);
                        }
                        if let Some(mut lease) = receive_lease.take() {
                            lease.release();
                        }
                        self.state = QueryState::Done;
                        Ok(Some(QueryResult { metadata, bytes }))
                    }
                    wire::QueryDelivery::Transfer(descriptor) => {
                        let lease = receive_lease.take().ok_or(ClientError::Protocol(
                            "Terminal query receive lease disappeared",
                        ))?;
                        let receiver = match ByteReceiver::new_with_lease(
                            client,
                            descriptor,
                            None,
                            *initial_receive_credit,
                            lease,
                        ) {
                            Ok(receiver) => receiver,
                            Err(error) => {
                                self.state = QueryState::Done;
                                return Err(error.into());
                            }
                        };
                        self.state = QueryState::Transfer { receiver, metadata };
                        Ok(None)
                    }
                }
            }
            QueryState::Transfer { receiver, metadata } => {
                let Some(bytes) = receiver.offer_frame(frame)? else {
                    return Ok(None);
                };
                let metadata = metadata.clone();
                // The complete receiver has retired its receive authority.
                // Terminalize before content validation so malformed content
                // cannot leave a spent receiver exposed as resumable.
                self.state = QueryState::Done;
                validate_query_content(metadata.content_kind, &bytes)?;
                Ok(Some(QueryResult { metadata, bytes }))
            }
            QueryState::Done => Err(Error::Completed),
        }
    }

    pub fn cancel(&mut self, client: &mut Client) -> Result<bool, Error> {
        match &mut self.state {
            QueryState::Result {
                token,
                initial_receive_credit,
                receive_lease,
                ..
            } => {
                let prefix = match client.cancel_request_and_wait(token) {
                    Ok(prefix) => prefix,
                    Err(error) => {
                        if !client.request_is_resumable(token) {
                            self.state = QueryState::Done;
                        }
                        return Err(error.into());
                    }
                };
                let body = if prefix.status == Status::Ok {
                    let body = match client.decode_result_body::<wire::QueryBody>(&prefix.body) {
                        Ok(body) => body,
                        Err(error) => {
                            self.state = QueryState::Done;
                            return Err(error.into());
                        }
                    };
                    if let Err(error) = body.validate_receive_credit(*initial_receive_credit) {
                        if matches!(&body.delivery, wire::QueryDelivery::Inline(_))
                            && let Some(mut lease) = receive_lease.take()
                        {
                            lease.release();
                        }
                        client.poison();
                        self.state = QueryState::Done;
                        return Err(error.into());
                    }
                    Some(body)
                } else {
                    None
                };
                let mut lease = receive_lease.take().ok_or(ClientError::Protocol(
                    "Terminal query receive lease disappeared",
                ))?;
                let settled = if let Some(body) = body {
                    match body.delivery {
                        wire::QueryDelivery::Inline(_) => {
                            lease.release();
                            Ok(())
                        }
                        wire::QueryDelivery::Transfer(descriptor) => {
                            transfer::reject_receive_transfer_with_lease(client, &descriptor, lease)
                                .map_err(Error::from)
                        }
                    }
                } else {
                    lease.release();
                    Ok(())
                };
                self.state = QueryState::Done;
                settled?;
                Ok(true)
            }
            QueryState::Transfer { receiver, .. } => {
                let cancelled = receiver.cancel(client)?;
                self.state = QueryState::Done;
                Ok(cancelled)
            }
            QueryState::Done => Ok(false),
        }
    }
}

fn query_metadata(body: &wire::QueryBody) -> QueryMetadata {
    QueryMetadata {
        content_kind: body.content_kind,
        encoding: body.encoding,
        flags: body.flags,
        next_cursor: body.next_cursor,
        total_lines: body.total_lines,
        satisfying_state_revision: body.satisfying_state_revision,
        extensions: body.extensions.clone(),
    }
}

fn validate_query_content(content_kind: u8, bytes: &[u8]) -> Result<(), Error> {
    match content_kind {
        kind if kind == yas_wire::schema::terminal::CONTENT_TEXT as u8 => {
            core::str::from_utf8(bytes).map_err(|_| Error::Protocol("invalid UTF-8 text query"))?;
        }
        kind if kind == yas_wire::schema::terminal::CONTENT_PATH as u8 => {}
        kind if kind == yas_wire::schema::terminal::CONTENT_STYLED_LINES as u8 => {
            let _ = wire::StyledLines::decode(bytes)?;
        }
        kind if kind == yas_wire::schema::terminal::CONTENT_SEARCH_RESULTS as u8 => {
            let _ = wire::SearchResults::decode(bytes)?;
        }
        kind if kind == yas_wire::schema::terminal::CONTENT_JOURNAL as u8 => {
            let _ = wire::JournalResult::decode(bytes)?;
        }
        kind if kind == yas_wire::schema::terminal::CONTENT_OUTPUT as u8 => {
            let _ = wire::OutputResult::decode(bytes)?;
        }
        kind if kind == yas_wire::schema::terminal::CONTENT_TEXT_AND_STYLED as u8 => {
            let _ = wire::TextAndStyled::decode(bytes)?;
        }
        _ => return Err(Error::Protocol("unknown Terminal query content kind")),
    }
    Ok(())
}

fn operation_id(client: &Client) -> Result<[u8; 16], Error> {
    let mut operation_id = [0; 16];
    client.random(&mut operation_id)?;
    if operation_id == [0; 16] {
        operation_id[15] = 1;
    }
    Ok(operation_id)
}

impl Client {
    pub fn watch_terminals(&mut self, resume: Option<Cursor>) -> Result<Watch, Error> {
        if !self.supports(family::TERMINAL, Class::Request, wire::request_kind::WATCH) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = self.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let initial_credit = receive_lease.bytes();
        let result: WatchResult = self.request_typed_with_receive_lease(
            family::TERMINAL,
            wire::request_kind::WATCH,
            &StateWatch {
                initial_credit,
                resume,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        Ok(Watch {
            subscription_id: result.subscription_id,
            cumulative_credit: initial_credit,
            closed: false,
            receive_lease,
        })
    }

    pub fn create_terminal(
        &mut self,
        rows: u16,
        cols: u16,
        launch: wire::Launch,
    ) -> Result<wire::CreateResult, Error> {
        self.create_terminal_with_extensions(rows, cols, launch, Extensions::default())
    }

    pub fn create_terminal_with_extensions(
        &mut self,
        rows: u16,
        cols: u16,
        launch: wire::Launch,
        extensions: Extensions,
    ) -> Result<wire::CreateResult, Error> {
        let request = wire::Create {
            rows,
            cols,
            operation_id: operation_id(self)?,
            launch,
            extensions,
        };
        self.request_typed(family::TERMINAL, wire::request_kind::CREATE, &request, true)
            .map_err(Into::into)
    }

    pub fn restart_terminal(
        &mut self,
        terminal_handle: u64,
        launch_mode: wire::LaunchMode,
        cutover_mode: wire::CutoverMode,
        launch: Option<wire::Launch>,
    ) -> Result<wire::RestartResult, Error> {
        self.request_typed(
            family::TERMINAL,
            wire::request_kind::RESTART,
            &wire::Restart {
                terminal_handle,
                operation_id: operation_id(self)?,
                launch_mode,
                cutover_mode,
                launch,
                extensions: Extensions::default(),
            },
            true,
        )
        .map_err(Into::into)
    }

    pub fn signal_terminal(
        &mut self,
        terminal_handle: u64,
        signal: wire::SignalKind,
    ) -> Result<(), Error> {
        self.request(
            family::TERMINAL,
            wire::request_kind::SIGNAL,
            wire::Signal {
                terminal_handle,
                operation_id: operation_id(self)?,
                signal,
                extensions: Extensions::default(),
            }
            .encode()?,
            false,
        )?;
        Ok(())
    }

    pub fn close_terminal(&mut self, terminal_handle: u64) -> Result<(), Error> {
        self.request(
            family::TERMINAL,
            wire::request_kind::CLOSE,
            wire::Close {
                terminal_handle,
                operation_id: operation_id(self)?,
            }
            .encode()?,
            false,
        )?;
        Ok(())
    }

    pub fn set_terminal_deadline(
        &mut self,
        terminal_handle: u64,
        deadline: wire::Deadline,
    ) -> Result<(), Error> {
        self.request(
            family::TERMINAL,
            wire::request_kind::SET_DEADLINE,
            wire::SetDeadline {
                terminal_handle,
                operation_id: operation_id(self)?,
                deadline,
            }
            .encode()?,
            false,
        )?;
        Ok(())
    }

    pub fn resize_terminal(
        &mut self,
        terminal_handle: u64,
        rows: u16,
        cols: u16,
    ) -> Result<wire::ResizeResult, Error> {
        self.request_typed(
            family::TERMINAL,
            wire::request_kind::RESIZE,
            &wire::Resize {
                terminal_handle,
                rows,
                cols,
            },
            false,
        )
        .map_err(Into::into)
    }

    pub fn write_terminal(&mut self, terminal_handle: u64, data: Vec<u8>) -> Result<(), Error> {
        self.send_typed_event(
            family::TERMINAL,
            wire::event_kind::WRITE,
            &wire::Write {
                terminal_handle,
                data,
            },
            true,
        )?;
        Ok(())
    }

    fn begin_terminal_query_with_lease<Request: Encode>(
        &mut self,
        kind: u16,
        request: &Request,
        initial_receive_credit: u64,
        maximum_bytes: u64,
        mut receive_lease: ReceiveLease,
    ) -> Result<PendingQuery, Error> {
        if !self.supports(family::TERMINAL, Class::Request, kind) {
            return Err(Error::FeatureMissing);
        }
        if initial_receive_credit == 0 || maximum_bytes == 0 {
            return Err(Error::Protocol("zero Terminal query receive bound"));
        }
        let token = self.begin_typed_request(family::TERMINAL, kind, request, true)?;
        receive_lease.commit();
        Ok(PendingQuery {
            state: QueryState::Result {
                token,
                initial_receive_credit,
                maximum_bytes,
                receive_lease: Some(receive_lease),
            },
        })
    }

    /// Query one Terminal CWD with a receive grant owned by the SDK budget.
    pub fn query_terminal_cwd(
        &mut self,
        terminal_handle: u64,
        generation: u32,
        maximum_bytes: u64,
    ) -> Result<QueryResult, Error> {
        // CWD has no declared Result length. The server fences completion at
        // this exact advertised grant before returning an inline or Transfer
        // delivery, so a partial aggregate-budget reservation remains live.
        let receive_lease = self.receive_credit_up_to(maximum_bytes)?;
        let initial_receive_credit = receive_lease.bytes();
        let request = wire::CwdQuery {
            terminal_handle,
            generation,
            initial_receive_credit,
            extensions: Extensions::default(),
        };
        let mut query = self.begin_terminal_query_with_lease(
            wire::request_kind::CWD,
            &request,
            initial_receive_credit,
            initial_receive_credit,
            receive_lease,
        )?;
        loop {
            let frame = match self.next_matching_frame(|frame| query.owns_frame(frame)) {
                Ok(frame) => frame,
                Err(error) => {
                    if query.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error.into());
                }
            };
            match query.offer_frame(self, &frame) {
                Ok(Some(result)) => return Ok(result),
                Ok(None) => {}
                Err(error) => {
                    if query.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error);
                }
            }
        }
    }

    /// Query Terminal output with the effective initial credit filled from the
    /// shared receive budget.
    ///
    /// `request.max_bytes` bounds the *content* the server may read; the
    /// delivery that carries it is bounded by the advertised receive credit,
    /// as it is for CWD and JOURNAL, which have no content budget at all.
    /// The two are not interchangeable: an inline delivery also carries the
    /// result envelope, so bounding the delivery by `max_bytes` rejects
    /// well-formed replies whose envelope is larger than the content budget —
    /// a PROBE cursor, which reads no content and asks for `max_bytes = 1`,
    /// always is — and a rejected delivery poisons the client.
    pub fn query_terminal_output(
        &mut self,
        mut request: wire::Output,
        maximum_bytes: u64,
    ) -> Result<QueryResult, Error> {
        let receive_lease = self.receive_credit_up_to(maximum_bytes)?;
        let initial_receive_credit = receive_lease.bytes();
        request.max_bytes = request
            .max_bytes
            .min(u32::try_from(initial_receive_credit).unwrap_or(u32::MAX));
        request.initial_receive_credit = initial_receive_credit;
        let mut query = self.begin_terminal_query_with_lease(
            wire::request_kind::OUTPUT,
            &request,
            initial_receive_credit,
            initial_receive_credit,
            receive_lease,
        )?;
        loop {
            let frame = match self.next_matching_frame(|frame| query.owns_frame(frame)) {
                Ok(frame) => frame,
                Err(error) => {
                    if query.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error.into());
                }
            };
            match query.offer_frame(self, &frame) {
                Ok(Some(result)) => return Ok(result),
                Ok(None) => {}
                Err(error) => {
                    if query.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error);
                }
            }
        }
    }

    /// Begin a multiplexable Terminal WAIT with SDK-owned receive credit.
    ///
    /// The delivery bound is the advertised credit, not `request.max_bytes`,
    /// for the reason spelled out on [`Client::query_terminal_output`].
    pub fn begin_terminal_wait(
        &mut self,
        mut request: wire::Wait,
        maximum_bytes: u64,
    ) -> Result<PendingQuery, Error> {
        let receive_lease = self.receive_credit_up_to(maximum_bytes)?;
        let initial_receive_credit = receive_lease.bytes();
        request.max_bytes = request
            .max_bytes
            .min(u32::try_from(initial_receive_credit).unwrap_or(u32::MAX));
        request.initial_receive_credit = initial_receive_credit;
        self.begin_terminal_query_with_lease(
            wire::request_kind::WAIT,
            &request,
            initial_receive_credit,
            initial_receive_credit,
            receive_lease,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec, vec::Vec};
    use yas_wire::{
        FrameHeader,
        core::ResultPrefix,
        transfer::{ByteData, Close, Descriptor, Direction, Mode},
    };

    use crate::test_support::{bootstrap_client, pending_headroom_burst};

    const QUERY_WINDOW: u64 = 4 * 1024 * 1024;

    fn pending_query(client: &mut Client) -> (PendingQuery, RequestToken) {
        let mut lease = client.receive_credit_exact(QUERY_WINDOW).unwrap();
        let token = client
            .begin_typed_request(
                family::TERMINAL,
                wire::request_kind::CWD,
                &wire::CwdQuery {
                    terminal_handle: 1,
                    generation: 1,
                    initial_receive_credit: QUERY_WINDOW,
                    extensions: Extensions::default(),
                },
                true,
            )
            .unwrap();
        lease.commit();
        (
            PendingQuery {
                state: QueryState::Result {
                    token,
                    initial_receive_credit: QUERY_WINDOW,
                    maximum_bytes: QUERY_WINDOW,
                    receive_lease: Some(lease),
                },
            },
            token,
        )
    }

    fn query_result(token: RequestToken, status: Status, body: Vec<u8>) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::result(token.family(), token.kind(), token.request_id())
            },
            payload: ResultPrefix {
                status,
                detail: Extensions::default(),
                body,
            }
            .encode()
            .unwrap(),
        }
    }

    fn inline_query_body() -> Vec<u8> {
        wire::QueryBody {
            content_kind: yas_wire::schema::terminal::CONTENT_PATH as u8,
            encoding: yas_wire::schema::terminal::QUERY_ENCODING_BYTES as u8,
            flags: 0,
            delivery: wire::QueryDelivery::Inline(b"/tmp".to_vec()),
            next_cursor: None,
            total_lines: None,
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        }
        .encode()
        .unwrap()
    }

    fn transfer_query_descriptor() -> Descriptor {
        Descriptor {
            transfer_id: 101,
            mode: Mode::Byte,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: QUERY_WINDOW,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::TERMINAL,
            content_kind: yas_wire::schema::terminal::QUERY_CONTENT_KIND as u16,
            content_version: wire::VERSION,
            extensions: Extensions(vec![Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn transfer_query_body() -> Vec<u8> {
        wire::QueryBody {
            content_kind: yas_wire::schema::terminal::CONTENT_PATH as u8,
            encoding: yas_wire::schema::terminal::QUERY_ENCODING_BYTES as u8,
            flags: 0,
            delivery: wire::QueryDelivery::Transfer(transfer_query_descriptor()),
            next_cursor: None,
            total_lines: None,
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        }
        .encode()
        .unwrap()
    }

    #[test]
    fn invalid_transferred_content_terminalizes_after_authority_retirement() {
        let (mut client, _state, _guard) = bootstrap_client();
        let lease = client.receive_credit_exact(QUERY_WINDOW).unwrap();
        let receiver = ByteReceiver::new_with_lease(
            &mut client,
            transfer_query_descriptor(),
            None,
            QUERY_WINDOW,
            lease,
        )
        .unwrap();
        let mut query = PendingQuery {
            state: QueryState::Transfer {
                receiver,
                metadata: QueryMetadata {
                    content_kind: yas_wire::schema::terminal::CONTENT_TEXT as u8,
                    encoding: yas_wire::schema::terminal::QUERY_ENCODING_UTF8 as u8,
                    flags: 0,
                    next_cursor: None,
                    total_lines: None,
                    satisfying_state_revision: None,
                    extensions: Extensions::default(),
                },
            },
        };
        let data = Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::BYTE_DATA)
            },
            payload: ByteData {
                transfer_id: 101,
                offset: 0,
                data: vec![0xff],
            }
            .encode()
            .unwrap(),
        };
        assert_eq!(query.offer_frame(&mut client, &data).unwrap(), None);
        let close = Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE)
            },
            payload: Close {
                transfer_id: 101,
                final_data_bytes: 1,
                status: Status::Ok.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        };
        assert!(matches!(
            query.offer_frame(&mut client, &close),
            Err(Error::Protocol("invalid UTF-8 text query"))
        ));
        assert!(!query.owns_frame(&close));
        assert!(!query.cancel(&mut client).unwrap());
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn probe_output_accepts_a_result_larger_than_its_content_budget() {
        // A PROBE cursor reads no output at all and asks for `max_bytes = 1`,
        // yet its reply still carries an `OutputResult`. The delivery bound is
        // the advertised receive credit, so this is an ordinary reply; bounding
        // it by `max_bytes` made every probe a protocol violation, poisoning
        // the client and killing the extension that asked.
        let (mut client, state, _guard) = bootstrap_client();
        let codec = yas_wire::FrameCodec::new(yas_wire::FrameLimits::recommended(), []).unwrap();
        let reply = wire::OutputResult {
            generation: 1,
            flags: 0,
            start_seq: 42,
            start_col: 7,
            next_seq: 42,
            next_col: 7,
            text: Vec::new(),
        }
        .encode()
        .unwrap();
        assert!(reply.len() > 1);
        let body = wire::QueryBody {
            content_kind: yas_wire::schema::terminal::CONTENT_OUTPUT as u8,
            encoding: yas_wire::schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            flags: 0,
            delivery: wire::QueryDelivery::Inline(reply.clone()),
            next_cursor: None,
            total_lines: None,
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        }
        .encode()
        .unwrap();
        let request_id = client.next_request_id();
        state.borrow_mut().responses_after_send.push_back((
            1,
            vec![
                codec
                    .encode_stream(&Frame {
                        header: FrameHeader {
                            sensitive: true,
                            ..FrameHeader::result(
                                family::TERMINAL,
                                wire::request_kind::OUTPUT,
                                request_id,
                            )
                        },
                        payload: ResultPrefix {
                            status: Status::Ok,
                            detail: Extensions::default(),
                            body,
                        }
                        .encode()
                        .unwrap(),
                    })
                    .unwrap(),
            ],
        ));
        let result = client
            .query_terminal_output(
                wire::Output {
                    terminal_handle: 1,
                    generation: 1,
                    cursor_kind: yas_wire::schema::terminal::OUTPUT_CURSOR_PROBE as u8,
                    flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
                    cursor_a: 0,
                    cursor_b: 0,
                    max_bytes: 1,
                    initial_receive_credit: 0,
                    extensions: Extensions::default(),
                },
                QUERY_WINDOW,
            )
            .unwrap();
        assert_eq!(result.bytes, reply);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        // The content budget reaches the server as asked; only the delivery
        // bound comes from the grant.
        let (request_frame, _) = codec.decode_stream(&state.borrow().sent[0]).unwrap();
        let request = wire::Output::decode(&request_frame.payload).unwrap();
        assert_eq!(request.max_bytes, 1);
        assert_eq!(request.initial_receive_credit, QUERY_WINDOW);
    }

    #[test]
    fn wait_uses_effective_partial_grant_as_inline_limit() {
        const KIB: u64 = 1024;
        const BUDGET: u64 = 16 * 1024 * 1024;

        let (mut client, state, _guard) = bootstrap_client();
        let mut held = client.receive_credit_exact(BUDGET - KIB).unwrap();
        held.commit();
        let mut query = client
            .begin_terminal_wait(
                wire::Wait {
                    terminal_handle: 1,
                    generation: 1,
                    wait_kind: yas_wire::schema::terminal::WAIT_LATEST_COMMAND as u8,
                    flags: yas_wire::schema::terminal::WAIT_FLAGS as u8,
                    cursor_a: 0,
                    cursor_b: 0,
                    max_bytes: 32 * 1024,
                    timeout_ns: 1,
                    needle: Vec::new(),
                    initial_receive_credit: 0,
                    extensions: Extensions::default(),
                },
                32 * KIB,
            )
            .unwrap();
        let (token, effective_maximum) = match &query.state {
            QueryState::Result {
                token,
                maximum_bytes,
                ..
            } => (*token, *maximum_bytes),
            _ => panic!("expected pending Terminal Result"),
        };
        assert_eq!(effective_maximum, KIB);
        let codec = yas_wire::FrameCodec::new(yas_wire::FrameLimits::recommended(), []).unwrap();
        let (request_frame, _) = codec
            .decode_stream(state.borrow().sent.last().unwrap())
            .unwrap();
        let request = wire::Wait::decode(&request_frame.payload).unwrap();
        assert_eq!(request.max_bytes, KIB as u32);
        assert_eq!(request.initial_receive_credit, KIB);

        let body = wire::QueryBody {
            content_kind: yas_wire::schema::terminal::CONTENT_PATH as u8,
            encoding: yas_wire::schema::terminal::QUERY_ENCODING_BYTES as u8,
            flags: 0,
            delivery: wire::QueryDelivery::Inline(vec![b'x'; (2 * KIB) as usize]),
            next_cursor: None,
            total_lines: None,
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        }
        .encode()
        .unwrap();
        assert!(matches!(
            query.offer_frame(&mut client, &query_result(token, Status::Ok, body)),
            Err(Error::LimitExceeded { actual, maximum })
                if actual == 2 * KIB && maximum == KIB
        ));
        assert!(!query.owns_frame(&query_result(token, Status::Ok, Vec::new())));
        assert_eq!(client.available_receive_credit(), KIB);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
        held.release();
        assert_eq!(client.available_receive_credit(), BUDGET);
    }

    #[test]
    fn cwd_uses_partial_wire_grant_as_inline_limit() {
        const KIB: u64 = 1024;
        const BUDGET: u64 = 16 * 1024 * 1024;

        let (mut client, state, _guard) = bootstrap_client();
        let mut held = client.receive_credit_exact(BUDGET - KIB).unwrap();
        held.commit();
        let body = wire::QueryBody {
            content_kind: yas_wire::schema::terminal::CONTENT_PATH as u8,
            encoding: yas_wire::schema::terminal::QUERY_ENCODING_BYTES as u8,
            flags: 0,
            delivery: wire::QueryDelivery::Inline(vec![b'x'; (2 * KIB) as usize]),
            next_cursor: None,
            total_lines: None,
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        }
        .encode()
        .unwrap();
        let codec = yas_wire::FrameCodec::new(yas_wire::FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::TERMINAL, wire::request_kind::CWD, 3)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body,
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );

        assert!(matches!(
            client.query_terminal_cwd(1, 1, 32 * KIB),
            Err(Error::LimitExceeded { actual, maximum })
                if actual == 2 * KIB && maximum == KIB
        ));
        let (request_frame, _) = codec
            .decode_stream(state.borrow().sent.last().unwrap())
            .unwrap();
        let request = wire::CwdQuery::decode(&request_frame.payload).unwrap();
        assert_eq!(request.initial_receive_credit, KIB);
        assert_eq!(client.available_receive_credit(), KIB);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
        held.release();
        assert_eq!(client.available_receive_credit(), BUDGET);
    }

    #[test]
    fn terminal_state_helpers_preserve_full_opaque_handles() {
        let exit = wire::ExitRecord::Code {
            code: 23,
            detail: String::from("complete"),
        };
        let record = wire::TerminalRecord {
            terminal_handle: u64::MAX,
            lifecycle: wire::Lifecycle::Exited,
            rows: 24,
            cols: 80,
            generation: 1,
            used_rows: 2,
            extensions: Extensions(vec![
                Extension {
                    tag: yas_wire::schema::terminal::STATE_EXIT_EXTENSION as u16,
                    required: false,
                    value: exit.encode().unwrap(),
                },
                Extension {
                    tag: yas_wire::schema::terminal::STATE_APP_HANDLE_EXTENSION as u16,
                    required: false,
                    value: (u64::MAX - 1).to_le_bytes().to_vec(),
                },
                Extension {
                    tag: yas_wire::schema::terminal::STATE_RESOURCE_TAG_EXTENSION as u16,
                    required: false,
                    value: b"muster:unit:run".to_vec(),
                },
            ]),
        };

        assert_eq!(record.terminal_handle, u64::MAX);
        assert_eq!(state_exit(&record).unwrap(), Some(exit));
        assert_eq!(state_app_handle(&record).unwrap(), Some(u64::MAX - 1));
        assert_eq!(
            state_resource_tag(&record).unwrap(),
            Some("muster:unit:run")
        );
    }

    #[test]
    fn blocking_terminal_owners_poison_and_pin_when_cancel_cannot_preflight() {
        let (mut cwd_client, cwd_state, _guard) = bootstrap_client();
        cwd_state
            .borrow_mut()
            .responses_after_send
            .push_back((1, pending_headroom_burst()));
        assert!(matches!(
            cwd_client.query_terminal_cwd(1, 1, QUERY_WINDOW),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(cwd_state.borrow().sent.len(), 1);
        assert_eq!(cwd_client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(
            cwd_client.next_event(),
            Err(ClientError::Poisoned)
        ));
        drop(_guard);

        let (mut output_client, output_state, _guard) = bootstrap_client();
        output_state
            .borrow_mut()
            .responses_after_send
            .push_back((1, pending_headroom_burst()));
        assert!(matches!(
            output_client.query_terminal_output(
                wire::Output {
                    terminal_handle: 1,
                    generation: 1,
                    cursor_kind: yas_wire::schema::terminal::OUTPUT_CURSOR_LATEST_COMMAND as u8,
                    flags: yas_wire::schema::terminal::OUTPUT_REQUEST_FLAGS as u8,
                    cursor_a: 0,
                    cursor_b: 0,
                    max_bytes: QUERY_WINDOW as u32,
                    initial_receive_credit: 0,
                    extensions: Extensions::default(),
                },
                QUERY_WINDOW,
            ),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(output_state.borrow().sent.len(), 1);
        assert_eq!(output_client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(
            output_client.next_event(),
            Err(ClientError::Poisoned)
        ));
    }

    #[test]
    fn terminal_create_extensions_reject_invalid_or_narrowed_identity() {
        assert!(resource_tag_extension("").is_err());
        assert!(resource_tag_extension("contains\0nul").is_err());
        assert!(resource_tag_extension(&"x".repeat(MAX_RESOURCE_TAG_BYTES + 1)).is_err());
        assert_eq!(
            resource_tag_extension("muster:unit:run").unwrap().value,
            b"muster:unit:run"
        );
        assert!(app_handle_extension(0).is_err());
        assert_eq!(
            app_handle_extension(u64::MAX).unwrap().value,
            u64::MAX.to_le_bytes()
        );
    }

    #[test]
    fn query_cancel_preserves_live_result_when_preflight_is_blocked() {
        let (mut client, state, _guard) = bootstrap_client();
        let (mut query, token) = pending_query(&mut client);
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
        let sent_before = state.borrow().sent.len();

        assert!(matches!(
            query.cancel(&mut client),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert!(client.request_is_resumable(&token));
        assert!(query.owns_frame(&query_result(token, Status::Ok, inline_query_body())));

        client
            .defer_for_test(query_result(token, Status::Ok, inline_query_body()))
            .unwrap();
        assert!(query.cancel(&mut client).unwrap());
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn query_cancel_resets_transfer_and_malformed_result_poisons() {
        let (mut client, state, _guard) = bootstrap_client();
        let (mut transferred, token) = pending_query(&mut client);
        client
            .defer_for_test(query_result(token, Status::Ok, transfer_query_body()))
            .unwrap();
        let sent_before = state.borrow().sent.len();
        assert!(transferred.cancel(&mut client).unwrap());
        assert_eq!(state.borrow().sent.len(), sent_before + 1);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        let (mut malformed, token) = pending_query(&mut client);
        client
            .defer_for_test(query_result(token, Status::Ok, vec![0]))
            .unwrap();
        assert!(matches!(
            malformed.cancel(&mut client),
            Err(Error::Client(ClientError::Wire(_)))
        ));
        assert!(!malformed.owns_frame(&query_result(token, Status::Ok, Vec::new())));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }
}
