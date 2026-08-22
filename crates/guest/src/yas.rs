//! Native YAS session bootstrap for extension guests.
//!
//! The host ABI transports arbitrary byte-stream chunks as packets. This
//! module owns YAS stream framing above that packet boundary; no YAS packet
//! is sent during bootstrap or after the session is established.

use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    vec,
    vec::Vec,
};
use core::{
    fmt,
    ops::{Add, Sub},
    time::Duration,
};

pub use yas_wire as wire;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
    core::{
        CatalogStep, ClientHello, FamilyOffer, FamilyUpdate, GoAway, Ping, PingResult,
        ReceiveLimits, ResultPrefix, ServerHello, SessionUpdate, Status,
    },
    extension::{AttemptContext, AttemptOutput, OutputKind},
    family,
};

use crate::{
    host,
    receive::{Budget as ReceiveBudget, Lease as ReceiveLease},
};

const HELLO_REQUEST_ID: u32 = 1;
const RECEIVE_MAX_FRAME: u32 = yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME;
const RECEIVE_MAX_DECODED: u32 = yas_wire::schema::transport::RECOMMENDED_DECODED_FRAME;
const RECEIVE_CREDIT_BUDGET: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const RECEIVE_PENDING_BUDGET: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const RECEIVE_MAX_BUFFERED: u64 = RECEIVE_CREDIT_BUDGET + RECEIVE_PENDING_BUDGET;
const INITIAL_BUFFER: usize = 64 * 1024;
const MAX_PENDING_FRAMES: usize = 1_024;
const MAX_PENDING_BYTES: usize = RECEIVE_PENDING_BUDGET as usize;
const MAX_OUTSTANDING_REQUESTS: usize = 1_024;

/// Exit code returned when the native YAS bootstrap cannot complete.
pub const EXIT_BOOTSTRAP_FAILURE: i32 = 70;

/// A native YAS guest-session failure.
#[derive(Debug)]
pub enum Error {
    Host(host::Error),
    Wire(yas_wire::Error),
    EndpointClosed,
    DeadlineExceeded,
    SendRejected,
    AllocationFailed,
    BufferedLimit,
    HelloRejected(Status),
    UnexpectedHello,
    ExtensionUnavailable,
    ExpectedAttemptContext,
    PendingBufferFull,
    PendingReadBlocked,
    Poisoned,
    ReceiveBudgetExhausted {
        requested: u64,
        available: u64,
    },
    TooManyOutstandingRequests,
    RequestFailed {
        family: u16,
        kind: u16,
        status: Status,
        detail: Extensions,
    },
    Protocol(&'static str),
    GoAway(GoAway),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(f, "host ABI error: {error}"),
            Self::Wire(error) => write!(f, "YAS wire error: {error}"),
            Self::EndpointClosed => f.write_str("YAS extension endpoint is closed"),
            Self::DeadlineExceeded => f.write_str("YAS request deadline elapsed"),
            Self::SendRejected => f.write_str("host rejected a YAS stream chunk"),
            Self::AllocationFailed => f.write_str("YAS receive allocation failed"),
            Self::BufferedLimit => f.write_str("YAS buffered receive limit exceeded"),
            Self::HelloRejected(status) => write!(f, "YAS HELLO rejected: {status:?}"),
            Self::UnexpectedHello => f.write_str("unexpected YAS HELLO response"),
            Self::ExtensionUnavailable => {
                f.write_str("YAS Extension attempt capability was not selected")
            }
            Self::ExpectedAttemptContext => {
                f.write_str("first YAS application frame was not ATTEMPT_CONTEXT")
            }
            Self::PendingBufferFull => f.write_str("pending YAS frame buffer is full"),
            Self::PendingReadBlocked => {
                f.write_str("pending YAS frames leave no room for another decoded frame")
            }
            Self::Poisoned => {
                f.write_str("YAS client is poisoned after protocol state became unsafe")
            }
            Self::ReceiveBudgetExhausted {
                requested,
                available,
            } => write!(
                f,
                "YAS receive budget needs {requested} bytes; {available} available"
            ),
            Self::TooManyOutstandingRequests => {
                f.write_str("too many outstanding YAS guest requests")
            }
            Self::RequestFailed {
                family,
                kind,
                status,
                ..
            } => write!(
                f,
                "YAS request {family:#06x}/{kind:#06x} failed with {status:?}"
            ),
            Self::Protocol(detail) => write!(f, "YAS protocol error: {detail}"),
            Self::GoAway(goaway) => write!(f, "YAS endpoint is closing with {:?}", goaway.status),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::error::Error for Error {}

impl From<host::Error> for Error {
    fn from(value: host::Error) -> Self {
        Self::Host(value)
    }
}

impl From<yas_wire::Error> for Error {
    fn from(value: yas_wire::Error) -> Self {
        Self::Wire(value)
    }
}

/// A bootstrapped native YAS extension session.
pub struct Client {
    hello: ServerHello,
    context: AttemptContext,
    inbound: FrameCodec,
    outbound: FrameCodec,
    receiver: StreamReceiver,
    next_request_id: u32,
    outstanding: BTreeMap<u32, (u16, u16)>,
    pending: VecDeque<PendingFrame>,
    pending_bytes: usize,
    receive_credit: ReceiveBudget,
    receive_pending: ReceiveBudget,
    poisoned: bool,
    negotiated_codecs: Vec<u16>,
}

struct PendingFrame {
    frame: Frame,
    lease: ReceiveLease,
}

/// One SDK-owned request correlation token for internal multiplexed helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestToken {
    request_id: u32,
    family: u16,
    kind: u16,
}

impl RequestToken {
    #[cfg(test)]
    pub(crate) const fn request_id(self) -> u32 {
        self.request_id
    }

    pub(crate) const fn family(self) -> u16 {
        self.family
    }

    pub(crate) const fn kind(self) -> u16 {
        self.kind
    }

    pub(crate) fn matches(self, frame: &Frame) -> bool {
        frame.header.class == Class::Result
            && frame.header.family == self.family
            && frame.header.kind == self.kind
            && frame.header.request_id == Some(self.request_id)
    }
}

/// Signed nanoseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Realtime(i64);

impl Realtime {
    pub const fn unix_timestamp_nanos(self) -> i64 {
        self.0
    }
}

/// Opaque point in the current attempt's host monotonic-clock domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonotonicInstant(i64);

impl MonotonicInstant {
    pub const MAX: Self = Self(i64::MAX);

    pub const fn raw_nanos(self) -> i64 {
        self.0
    }

    pub const fn from_raw_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        let nanos = self.0.checked_sub(earlier.0)?;
        (nanos >= 0).then(|| Duration::from_nanos(nanos as u64))
    }

    fn saturating_add(self, duration: Duration) -> Self {
        let nanos = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
        Self(self.0.saturating_add(nanos))
    }
}

impl Add<Duration> for MonotonicInstant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        self.saturating_add(rhs)
    }
}

impl Sub for MonotonicInstant {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_duration_since(rhs).unwrap_or(Duration::ZERO)
    }
}

impl Client {
    /// Initiate YAS, negotiate the attempt-only Extension capability, and
    /// consume its mandatory first `ATTEMPT_CONTEXT` Event.
    pub fn bootstrap() -> Result<Self, Error> {
        let families = yas_wire::schema::FAMILIES
            .iter()
            .filter(|metadata| {
                !matches!(
                    metadata.id,
                    family::CORE | family::TRANSFER | family::CHANNEL | family::EXTENSION
                )
            })
            .map(|metadata| FamilyOffer {
                family_id: metadata.id,
                versions: vec![metadata.version],
                required: false,
            })
            .collect();
        Self::bootstrap_with_offers(families)
    }

    /// Bootstrap while requesting additional native YAS families. Extension
    /// v1 and its Transfer/Channel dependency closure are inserted as required
    /// offers; additional offers remain under the caller's required/optional
    /// policy and must use canonical ordering.
    pub fn bootstrap_with_offers(mut families: Vec<FamilyOffer>) -> Result<Self, Error> {
        if families.iter().any(|offer| {
            matches!(
                offer.family_id,
                family::CORE | family::TRANSFER | family::CHANNEL | family::EXTENSION
            )
        }) {
            return Err(yas_wire::Error::Invalid("guest family offer").into());
        }
        // Extension v1 depends on the common Transfer family and the shared
        // Channel fabric. Insert the entire canonical dependency closure so a
        // default native guest is valid under strict HELLO negotiation.
        families.push(FamilyOffer {
            family_id: family::TRANSFER,
            versions: vec![yas_wire::transfer::VERSION],
            required: true,
        });
        families.push(FamilyOffer {
            family_id: family::CHANNEL,
            versions: vec![yas_wire::channel::VERSION],
            required: true,
        });
        families.push(FamilyOffer {
            family_id: family::EXTENSION,
            versions: vec![yas_wire::extension::VERSION],
            required: true,
        });
        families.sort_by_key(|offer| offer.family_id);

        let mut client_instance = [0; 16];
        host::random(&mut client_instance)?;
        let offer = ClientHello {
            min_minor: 1,
            max_minor: 1,
            receive: ReceiveLimits {
                max_frame: RECEIVE_MAX_FRAME,
                max_decoded: RECEIVE_MAX_DECODED,
                max_datagram: 0,
                max_buffered: RECEIVE_MAX_BUFFERED,
            },
            client_instance,
            client_name: String::from("yas-guest"),
            client_release: String::from(env!("CARGO_PKG_VERSION")),
            families,
            // Guests do not offer compression until the SDK has a streaming
            // decompressor with the same bounded-memory contract.
            codecs: Vec::new(),
            extensions: Default::default(),
        };
        offer.validate()?;
        send_chunk(&yas_wire::PREFACE)?;
        let request = Frame {
            header: FrameHeader::request(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                HELLO_REQUEST_ID,
            ),
            payload: offer.encode()?,
        };
        let pre_hello = FrameCodec::pre_hello();
        send_stream(&pre_hello.encode_stream(&request)?)?;

        let mut receiver = StreamReceiver::new(RECEIVE_MAX_BUFFERED)?;
        let response = receiver.recv(&pre_hello)?.ok_or(Error::EndpointClosed)?;
        if response.header
            != FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                HELLO_REQUEST_ID,
            )
        {
            return Err(Error::UnexpectedHello);
        }
        let result = ResultPrefix::decode(&response.payload)?;
        if !result.status.is_ok() {
            return Err(Error::HelloRejected(result.status));
        }
        let hello = ServerHello::decode(&result.body)?;
        hello.validate_for_client(&offer)?;
        let extension = hello
            .families
            .iter()
            .find(|descriptor| descriptor.family_id == family::EXTENSION)
            .ok_or(Error::ExtensionUnavailable)?;
        let attempt_operation = extension
            .operation(
                Class::Event,
                yas_wire::extension::event_kind::ATTEMPT_CONTEXT,
            )
            .ok_or(Error::ExtensionUnavailable)?;
        if attempt_operation.server_accepts || !attempt_operation.server_sends {
            return Err(Error::ExtensionUnavailable);
        }
        let output_operation = extension
            .operation(
                Class::Event,
                yas_wire::extension::event_kind::ATTEMPT_OUTPUT,
            )
            .ok_or(Error::ExtensionUnavailable)?;
        if !output_operation.server_accepts || output_operation.server_sends {
            return Err(Error::ExtensionUnavailable);
        }

        let codecs = hello.negotiated_codecs()?.0;
        let inbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: RECEIVE_MAX_FRAME,
                max_decoded_frame: RECEIVE_MAX_DECODED,
            },
            codecs.iter().copied(),
        )?;
        let outbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: hello.receive.max_frame,
                max_decoded_frame: hello.receive.max_decoded,
            },
            codecs.iter().copied(),
        )?;
        let context_frame = receiver.recv(&inbound)?.ok_or(Error::EndpointClosed)?;
        if context_frame.header
            != (FrameHeader {
                sensitive: true,
                ..FrameHeader::event(
                    family::EXTENSION,
                    yas_wire::extension::event_kind::ATTEMPT_CONTEXT,
                )
            })
        {
            return Err(Error::ExpectedAttemptContext);
        }
        let context = AttemptContext::decode(&context_frame.payload)?;

        Ok(Self {
            hello,
            context,
            inbound,
            outbound,
            receiver,
            next_request_id: 2,
            outstanding: BTreeMap::new(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            receive_credit: ReceiveBudget::new(RECEIVE_CREDIT_BUDGET),
            receive_pending: ReceiveBudget::new(RECEIVE_PENDING_BUDGET),
            poisoned: false,
            negotiated_codecs: codecs,
        })
    }

    pub const fn hello(&self) -> &ServerHello {
        &self.hello
    }

    pub const fn context(&self) -> &AttemptContext {
        &self.context
    }

    /// Publish one retained stdout, stderr, or log record for this exact
    /// authenticated Extension attempt.
    pub fn attempt_output(&mut self, kind: OutputKind, data: &[u8]) -> Result<(), Error> {
        self.send_typed_event(
            family::EXTENSION,
            yas_wire::extension::event_kind::ATTEMPT_OUTPUT,
            &AttemptOutput {
                kind,
                data: data.to_vec(),
                extensions: Extensions::default(),
            },
            true,
        )
    }

    pub fn attempt_stdout(&mut self, data: &[u8]) -> Result<(), Error> {
        self.attempt_output(OutputKind::Stdout, data)
    }

    pub fn attempt_stderr(&mut self, data: &[u8]) -> Result<(), Error> {
        self.attempt_output(OutputKind::Stderr, data)
    }

    pub fn attempt_log(&mut self, message: &str) -> Result<(), Error> {
        self.attempt_output(OutputKind::Log, message.as_bytes())
    }

    pub fn family(&self, family_id: u16) -> Option<&yas_wire::core::FamilyDescriptor> {
        self.hello
            .families
            .iter()
            .find(|descriptor| descriptor.family_id == family_id)
    }

    /// Round-trip one bounded Core PING through the negotiated session.
    pub fn ping(&mut self) -> Result<PingResult, Error> {
        let sender_monotonic_ns = u64::try_from(self.monotonic_now().raw_nanos()).unwrap_or(0);
        self.request_typed(
            family::CORE,
            yas_wire::core::request_kind::PING,
            &Ping {
                sender_monotonic_ns,
            },
            false,
        )
    }

    /// Allocate the next client-owned (odd) correlation ID.
    pub fn allocate_request_id(&mut self) -> u32 {
        let id = self.next_request_id | 1;
        self.next_request_id = id.wrapping_add(2);
        if self.next_request_id == 0 {
            self.next_request_id = 3;
        }
        id
    }

    fn allocate_available_request_id(&mut self) -> Result<u32, Error> {
        if self.outstanding.len() >= MAX_OUTSTANDING_REQUESTS {
            return Err(Error::TooManyOutstandingRequests);
        }
        for _ in 0..=MAX_OUTSTANDING_REQUESTS {
            let request_id = self.allocate_request_id();
            if !self.outstanding.contains_key(&request_id) {
                return Ok(request_id);
            }
        }
        Err(Error::TooManyOutstandingRequests)
    }

    /// Send one typed YAS frame using the negotiated server receive limits.
    fn send_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        self.ensure_healthy()?;
        send_stream(&self.outbound.encode_stream(frame)?)
    }

    /// Return whether the negotiated catalogue exposes one exact operation.
    pub fn supports(&self, family_id: u16, class: Class, kind: u16) -> bool {
        self.family(family_id).is_some_and(|descriptor| {
            descriptor
                .operation(class, kind)
                .is_some_and(|operation| match class {
                    Class::Request => operation.server_accepts,
                    Class::Event => operation.server_accepts || operation.server_sends,
                    Class::Result => false,
                })
        })
    }

    /// Send one correlated native YAS Request and return its Result prefix.
    pub(crate) fn request_result(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<ResultPrefix, Error> {
        self.request_result_until(family_id, kind, payload, sensitive, MonotonicInstant::MAX)
    }

    /// Send one correlated Request with an absolute monotonic deadline.
    pub(crate) fn request_result_until(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
        deadline: MonotonicInstant,
    ) -> Result<ResultPrefix, Error> {
        let token = self.begin_request(family_id, kind, payload, sensitive)?;
        match self.await_result_until(&token, deadline) {
            Err(Error::DeadlineExceeded) => {
                // This convenience API does not return its live token. Only
                // an owner which immediately CANCELs and settles may recover
                // from the resumable deadline reported by await_result_until.
                self.poisoned = true;
                self.abandon_request(&token);
                Err(Error::DeadlineExceeded)
            }
            result => result,
        }
    }

    fn await_result_until(
        &mut self,
        token: &RequestToken,
        deadline: MonotonicInstant,
    ) -> Result<ResultPrefix, Error> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| token.matches(&pending.frame))
        {
            let frame = self
                .remove_pending(index)
                .ok_or(Error::Protocol("pending Result disappeared"))?;
            return self
                .offer_result(token, &frame)?
                .ok_or(Error::Protocol("correlated Result was not accepted"));
        }
        loop {
            let frame = match self.read_next_until(deadline) {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    if deadline != MonotonicInstant::MAX {
                        // A finite local deadline does not retire the peer's
                        // Request authority. The owner must either resume this
                        // token or use Core CANCEL and consume the original
                        // terminal Result before releasing any receive lease.
                        return Err(Error::DeadlineExceeded);
                    }
                    self.poisoned = true;
                    self.abandon_request(token);
                    return Err(Error::EndpointClosed);
                }
                Err(error) => {
                    // No non-deadline read failure is resumable through this
                    // synchronous owner. A later Result must not be accepted
                    // as unrelated traffic, nor may committed authority be
                    // recycled while its outcome is unknown.
                    self.poisoned = true;
                    self.abandon_request(token);
                    return Err(error);
                }
            };
            match self.offer_result(token, &frame) {
                Ok(Some(result)) => return Ok(result),
                Ok(None) => {}
                Err(error) => {
                    self.poisoned = true;
                    self.abandon_request(token);
                    return Err(error);
                }
            }
            if frame.header.class == Class::Result && !self.is_outstanding_result(&frame) {
                self.poisoned = true;
                self.abandon_request(token);
                return Err(Error::Protocol("uncorrelated Result"));
            }
            if let Err(error) = self.defer(frame) {
                self.poisoned = true;
                self.abandon_request(token);
                return Err(error);
            }
        }
    }

    /// Start one request without blocking the guest's family event loop.
    pub(crate) fn begin_request(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<RequestToken, Error> {
        self.ensure_read_headroom()?;
        let request_id = self.allocate_available_request_id()?;
        let token = RequestToken {
            request_id,
            family: family_id,
            kind,
        };
        self.outstanding.insert(request_id, (family_id, kind));
        let mut header = FrameHeader::request(family_id, kind, request_id);
        header.sensitive = sensitive;
        if let Err(error) = self.send_frame(&Frame { header, payload }) {
            self.outstanding.remove(&request_id);
            return Err(error);
        }
        Ok(token)
    }

    /// Encode and start one typed request without waiting for its Result.
    pub(crate) fn begin_typed_request<Request: Encode>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
    ) -> Result<RequestToken, Error> {
        self.begin_request(family_id, kind, request.encode()?, sensitive)
    }

    /// Offer a frame to one outstanding request. A matching Result retires the
    /// token and returns its decoded prefix; unrelated frames are untouched.
    pub(crate) fn offer_result(
        &mut self,
        token: &RequestToken,
        frame: &Frame,
    ) -> Result<Option<ResultPrefix>, Error> {
        if !token.matches(frame) {
            return Ok(None);
        }
        if self.outstanding.get(&token.request_id) != Some(&(token.family, token.kind)) {
            return Err(Error::Protocol("Result for retired request"));
        }
        let prefix = ResultPrefix::decode(&frame.payload);
        self.outstanding.remove(&token.request_id);
        match prefix {
            Ok(prefix) => Ok(Some(prefix)),
            Err(error) => {
                self.poisoned = true;
                Err(error.into())
            }
        }
    }

    /// Retire an application-owned request token. This does not send a family
    /// cancellation operation; callers must use one when the family defines it.
    pub(crate) fn abandon_request(&mut self, token: &RequestToken) -> bool {
        let removed = self.outstanding.remove(&token.request_id).is_some();
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| token.matches(&pending.frame))
        {
            let _ = self.remove_pending(index);
        }
        removed
    }

    /// Ask Core to cancel one outstanding Request, then consume the original
    /// terminal Result before returning. The CANCEL Result itself only reports
    /// which side won the race; both OK and conflict/not-found outcomes still
    /// require settlement of the original request ID.
    pub(crate) fn cancel_request_and_wait(
        &mut self,
        token: &RequestToken,
    ) -> Result<ResultPrefix, Error> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| token.matches(&pending.frame))
        {
            let frame = self
                .remove_pending(index)
                .ok_or(Error::Protocol("pending Result disappeared"))?;
            return self
                .offer_result(token, &frame)?
                .ok_or(Error::Protocol("correlated Result was not accepted"));
        }
        let _ = self.request_result(
            family::CORE,
            yas_wire::core::request_kind::CANCEL,
            yas_wire::core::Cancel {
                target_request_id: token.request_id,
            }
            .encode()?,
            false,
        )?;
        self.await_result_until(token, MonotonicInstant::MAX)
    }

    pub(crate) fn request_is_resumable(&self, token: &RequestToken) -> bool {
        !self.poisoned
            && self.outstanding.get(&token.request_id) == Some(&(token.family, token.kind))
    }

    fn is_outstanding_result(&self, frame: &Frame) -> bool {
        frame.header.class == Class::Result
            && frame.header.request_id.is_some_and(|request_id| {
                self.outstanding.get(&request_id) == Some(&(frame.header.family, frame.header.kind))
            })
    }

    fn reject_unsolicited_result(&mut self, frame: &Frame) -> Result<(), Error> {
        if frame.header.class == Class::Result && !self.is_outstanding_result(frame) {
            self.poisoned = true;
            return Err(Error::Protocol("unsolicited Result"));
        }
        Ok(())
    }

    fn reject_pending_unsolicited_result(&mut self) -> Result<(), Error> {
        let Some(index) = self.pending.iter().position(|pending| {
            pending.frame.header.class == Class::Result
                && !self.is_outstanding_result(&pending.frame)
        }) else {
            return Ok(());
        };
        let _ = self.remove_pending(index);
        self.poisoned = true;
        Err(Error::Protocol("unsolicited Result"))
    }

    /// Send one Request and require an OK Result.
    pub(crate) fn request(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<Vec<u8>, Error> {
        let prefix = self.request_result(family_id, kind, payload, sensitive)?;
        if prefix.status == Status::Ok {
            Ok(prefix.body)
        } else {
            Err(Error::RequestFailed {
                family: family_id,
                kind,
                status: prefix.status,
                detail: prefix.detail,
            })
        }
    }

    /// Encode a typed Request and decode its successful Result body.
    pub(crate) fn request_typed<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
    ) -> Result<Response, Error>
    where
        Request: Encode,
        Response: Decode,
    {
        let body = self.request(family_id, kind, request.encode()?, sensitive)?;
        self.decode_result_body(&body)
    }

    /// Send a Request which proposes peer-to-client credit. The reservation is
    /// committed as soon as the Request is accepted by the host; a known
    /// non-OK Result retires it, while transport or decode ambiguity keeps it
    /// pinned until session teardown.
    pub(crate) fn request_typed_with_receive_lease<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
        receive_lease: &mut ReceiveLease,
    ) -> Result<Response, Error>
    where
        Request: Encode,
        Response: Decode,
    {
        self.request_typed_with_receive_leases(
            family_id,
            kind,
            request,
            sensitive,
            &mut [receive_lease],
        )
    }

    pub(crate) fn request_typed_with_receive_lease_until<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
        deadline: MonotonicInstant,
        receive_lease: &mut ReceiveLease,
    ) -> Result<Response, Error>
    where
        Request: Encode,
        Response: Decode,
    {
        let token = self.begin_typed_request(family_id, kind, request, sensitive)?;
        receive_lease.commit();
        let prefix = match self.await_result_until(&token, deadline) {
            Ok(prefix) => prefix,
            Err(Error::DeadlineExceeded) => {
                match self.cancel_request_and_wait(&token) {
                    Ok(prefix) => prefix,
                    Err(error) => {
                        // This blocking helper cannot return the live token to
                        // its caller. If cancellation/settlement is ambiguous,
                        // fail the whole session and leave committed authority
                        // pinned until teardown.
                        self.poisoned = true;
                        return Err(error);
                    }
                }
            }
            Err(error) => return Err(error),
        };
        if prefix.status != Status::Ok {
            receive_lease.release();
            return Err(Error::RequestFailed {
                family: family_id,
                kind,
                status: prefix.status,
                detail: prefix.detail,
            });
        }
        self.decode_result_body(&prefix.body)
    }

    pub(crate) fn request_typed_with_receive_leases<Request, Response>(
        &mut self,
        family_id: u16,
        kind: u16,
        request: &Request,
        sensitive: bool,
        receive_leases: &mut [&mut ReceiveLease],
    ) -> Result<Response, Error>
    where
        Request: Encode,
        Response: Decode,
    {
        let token = self.begin_typed_request(family_id, kind, request, sensitive)?;
        for lease in receive_leases.iter_mut() {
            lease.commit();
        }
        let prefix = self.await_result_until(&token, MonotonicInstant::MAX)?;
        if prefix.status != Status::Ok {
            for lease in receive_leases.iter_mut() {
                lease.release();
            }
            return Err(Error::RequestFailed {
                family: family_id,
                kind,
                status: prefix.status,
                detail: prefix.detail,
            });
        }
        self.decode_result_body(&prefix.body)
    }

    /// Send one native YAS Event.
    pub(crate) fn send_event(
        &mut self,
        family_id: u16,
        kind: u16,
        payload: Vec<u8>,
        sensitive: bool,
    ) -> Result<(), Error> {
        let mut header = FrameHeader::event(family_id, kind);
        header.sensitive = sensitive;
        self.send_frame(&Frame { header, payload })
    }

    /// Encode and send one typed native YAS Event.
    pub(crate) fn send_typed_event<Event: Encode>(
        &mut self,
        family_id: u16,
        kind: u16,
        event: &Event,
        sensitive: bool,
    ) -> Result<(), Error> {
        self.send_event(family_id, kind, event.encode()?, sensitive)
    }

    /// Attempt a terminal compensating Event even after the session has been
    /// poisoned. This is intentionally restricted to internal cleanup paths:
    /// callers may not resume protocol use, but can still ask the peer to
    /// retire authority that an earlier successful Result disclosed.
    pub(crate) fn send_typed_terminal_cleanup<Event: Encode>(
        &mut self,
        family_id: u16,
        kind: u16,
        event: &Event,
        sensitive: bool,
    ) -> Result<(), Error> {
        let frame = Frame {
            header: FrameHeader {
                sensitive,
                ..FrameHeader::event(family_id, kind)
            },
            payload: event.encode()?,
        };
        send_stream(&self.outbound.encode_stream(&frame)?)
    }

    /// Receive the next application Event after servicing Core housekeeping.
    pub fn next_event(&mut self) -> Result<Frame, Error> {
        self.next_event_until(MonotonicInstant::MAX)?
            .ok_or(Error::EndpointClosed)
    }

    /// Receive the next application Event up to an absolute deadline.
    pub fn next_event_until(&mut self, deadline: MonotonicInstant) -> Result<Option<Frame>, Error> {
        self.ensure_healthy()?;
        self.reject_pending_unsolicited_result()?;
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.frame.header.class == Class::Event)
        {
            return Ok(self.remove_pending(index));
        }
        loop {
            let Some(frame) = self.read_next_until(deadline)? else {
                return Ok(None);
            };
            if frame.header.class == Class::Event {
                return Ok(Some(frame));
            }
            self.reject_unsolicited_result(&frame)?;
            self.defer(frame)?;
        }
    }

    /// Receive the next application Event or correlated Result after servicing
    /// Core housekeeping. This is the primitive for multiplexed family loops.
    pub fn next_frame(&mut self) -> Result<Frame, Error> {
        self.next_frame_until(MonotonicInstant::MAX)?
            .ok_or(Error::EndpointClosed)
    }

    /// Receive the next application Event or correlated Result up to an
    /// absolute monotonic deadline.
    pub fn next_frame_until(&mut self, deadline: MonotonicInstant) -> Result<Option<Frame>, Error> {
        self.ensure_healthy()?;
        self.reject_pending_unsolicited_result()?;
        if let Some(frame) = self.remove_pending(0) {
            self.reject_unsolicited_result(&frame)?;
            return Ok(Some(frame));
        }
        let frame = self.read_next_until(deadline)?;
        if let Some(frame) = &frame {
            self.reject_unsolicited_result(frame)?;
        }
        Ok(frame)
    }

    /// Receive the next Event accepted by `predicate`, preserving the rest.
    pub fn next_matching_event(
        &mut self,
        mut predicate: impl FnMut(&Frame) -> bool,
    ) -> Result<Frame, Error> {
        self.ensure_healthy()?;
        self.reject_pending_unsolicited_result()?;
        if let Some(index) = self.pending.iter().position(|pending| {
            pending.frame.header.class == Class::Event && predicate(&pending.frame)
        }) {
            return self
                .remove_pending(index)
                .ok_or(Error::Protocol("pending frame disappeared"));
        }
        loop {
            let frame = self
                .read_next_until(MonotonicInstant::MAX)?
                .ok_or(Error::EndpointClosed)?;
            if frame.header.class == Class::Event && predicate(&frame) {
                return Ok(frame);
            }
            self.reject_unsolicited_result(&frame)?;
            self.defer(frame)?;
        }
    }

    /// Receive the next application Event or correlated Result accepted by
    /// `predicate`, preserving every other frame in the bounded pending queue.
    pub fn next_matching_frame(
        &mut self,
        predicate: impl FnMut(&Frame) -> bool,
    ) -> Result<Frame, Error> {
        self.next_matching_frame_until(MonotonicInstant::MAX, predicate)?
            .ok_or(Error::EndpointClosed)
    }

    /// Receive the next application Event or correlated Result accepted by
    /// `predicate` up to an absolute monotonic deadline, preserving every
    /// other frame in the bounded pending queue.
    pub fn next_matching_frame_until(
        &mut self,
        deadline: MonotonicInstant,
        mut predicate: impl FnMut(&Frame) -> bool,
    ) -> Result<Option<Frame>, Error> {
        self.ensure_healthy()?;
        self.reject_pending_unsolicited_result()?;
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| predicate(&pending.frame))
        {
            return self
                .remove_pending(index)
                .map(Some)
                .ok_or(Error::Protocol("pending frame disappeared"));
        }
        loop {
            let Some(frame) = self.read_next_until(deadline)? else {
                return Ok(None);
            };
            self.reject_unsolicited_result(&frame)?;
            if predicate(&frame) {
                return Ok(Some(frame));
            }
            self.defer(frame)?;
        }
    }

    pub fn realtime_now(&self) -> Realtime {
        Realtime(host::clock(host::ClockKind::Realtime))
    }

    pub fn monotonic_now(&self) -> MonotonicInstant {
        MonotonicInstant(host::clock(host::ClockKind::Monotonic))
    }

    pub fn wait_until(&self, deadline: MonotonicInstant) -> Result<host::WaitOutcome, Error> {
        host::wait(deadline.0).map_err(Into::into)
    }

    pub fn wait(&self) -> Result<host::WaitOutcome, Error> {
        host::wait(i64::MAX).map_err(Into::into)
    }

    pub fn random(&self, destination: &mut [u8]) -> Result<(), Error> {
        host::random(destination).map_err(Into::into)
    }

    pub(crate) fn receive_credit_up_to(&self, maximum: u64) -> Result<ReceiveLease, Error> {
        self.ensure_healthy()?;
        let lease = self.receive_credit.lease_up_to(maximum);
        if lease.bytes() == 0 {
            return Err(Error::ReceiveBudgetExhausted {
                requested: maximum,
                available: 0,
            });
        }
        Ok(lease)
    }

    pub(crate) fn receive_credit_exact(&self, bytes: u64) -> Result<ReceiveLease, Error> {
        self.ensure_healthy()?;
        self.receive_credit
            .lease_exact(bytes)
            .ok_or(Error::ReceiveBudgetExhausted {
                requested: bytes,
                available: self.receive_credit.available(),
            })
    }

    /// Decode a successful Result body. Once correlation has retired its
    /// request, malformed response bytes cannot be resumed safely, and an
    /// initial-credit request may already have created unidentified peer send
    /// authority. Poison the session while preserving the first wire error.
    pub(crate) fn decode_result_body<Response: Decode>(
        &mut self,
        body: &[u8],
    ) -> Result<Response, Error> {
        match Response::decode(body) {
            Ok(response) => Ok(response),
            Err(error) => {
                self.poisoned = true;
                Err(error.into())
            }
        }
    }

    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
    }

    #[cfg(test)]
    pub(crate) fn available_receive_credit(&self) -> u64 {
        self.receive_credit.available()
    }

    /// The id the next request will carry, so a test can stage its Result.
    #[cfg(test)]
    pub(crate) const fn next_request_id(&self) -> u32 {
        self.next_request_id | 1
    }

    #[cfg(test)]
    pub(crate) fn defer_for_test(&mut self, frame: Frame) -> Result<(), Error> {
        self.defer(frame)
    }

    /// Park for at least `duration` while continuing to service native Core
    /// housekeeping and preserving application frames for their family
    /// helpers. This avoids a queued frame turning a sleep into a busy loop.
    pub fn sleep(&mut self, duration: Duration) -> Result<(), Error> {
        let deadline = self.monotonic_now() + duration;
        loop {
            if self.monotonic_now() >= deadline {
                return Ok(());
            }
            match self.read_next_until(deadline)? {
                Some(frame) => self.defer(frame)?,
                None => return Ok(()),
            }
        }
    }

    fn defer(&mut self, frame: Frame) -> Result<(), Error> {
        self.ensure_healthy()?;
        self.reject_unsolicited_result(&frame)?;
        let bytes = self
            .pending_bytes
            .checked_add(frame.payload.len())
            .ok_or_else(|| {
                self.poisoned = true;
                Error::PendingBufferFull
            })?;
        if self.pending.len() >= MAX_PENDING_FRAMES || bytes > MAX_PENDING_BYTES {
            self.poisoned = true;
            return Err(Error::PendingBufferFull);
        }
        let Some(lease) = self.receive_pending.lease_exact(frame.payload.len() as u64) else {
            self.poisoned = true;
            return Err(Error::PendingBufferFull);
        };
        self.pending_bytes = bytes;
        self.pending.push_back(PendingFrame { frame, lease });
        Ok(())
    }

    fn remove_pending(&mut self, index: usize) -> Option<Frame> {
        let PendingFrame { frame, lease } = self.pending.remove(index)?;
        let leased = usize::try_from(lease.bytes()).expect("pending lease fits usize");
        debug_assert_eq!(leased, frame.payload.len());
        self.pending_bytes = self
            .pending_bytes
            .checked_sub(leased)
            .expect("pending byte accounting underflow");
        drop(lease);
        Some(frame)
    }

    /// Drop already-decoded Transfer frames for resources that a terminal
    /// constructor cleanup has explicitly reset. The caller poisons the
    /// session immediately afterwards, so later wire frames cannot be routed
    /// into a replacement resource.
    pub(crate) fn purge_pending_transfer_ids(&mut self, transfer_ids: &[u32]) {
        let mut index = 0;
        while index < self.pending.len() {
            let matches = self.pending.get(index).is_some_and(|pending| {
                pending.frame.header.class == Class::Event
                    && pending.frame.header.family == family::TRANSFER
                    && pending.frame.payload.get(..4).is_some_and(|prefix| {
                        transfer_ids.contains(&u32::from_le_bytes(
                            prefix.try_into().expect("four-byte Transfer ID"),
                        ))
                    })
            });
            if matches {
                let _ = self.remove_pending(index);
            } else {
                index += 1;
            }
        }
    }

    pub(crate) fn await_result(&mut self, token: &RequestToken) -> Result<ResultPrefix, Error> {
        self.await_result_until(token, MonotonicInstant::MAX)
    }

    fn read_next_until(&mut self, deadline: MonotonicInstant) -> Result<Option<Frame>, Error> {
        loop {
            self.ensure_read_headroom()?;
            let outcome = match self.receiver.recv_until(&self.inbound, deadline) {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.poisoned = true;
                    return Err(error);
                }
            };
            let frame = match outcome {
                StreamReceive::Frame(frame) => frame,
                StreamReceive::Deadline if deadline != MonotonicInstant::MAX => return Ok(None),
                StreamReceive::Deadline | StreamReceive::Closed => {
                    self.poisoned = true;
                    return Err(Error::EndpointClosed);
                }
            };
            if frame.header.class == Class::Request
                && frame.header.family == family::CORE
                && frame.header.kind == yas_wire::core::request_kind::PING
            {
                if let Err(error) = self.answer_ping(frame) {
                    self.poisoned = true;
                    return Err(error);
                }
                continue;
            }
            if frame.header.class == Class::Request {
                self.poisoned = true;
                return Err(Error::Protocol("unsupported server Request"));
            }
            if frame.header.class == Class::Event && frame.header.family == family::CORE {
                let update = match frame.header.kind {
                    yas_wire::core::event_kind::GOAWAY => match GoAway::decode(&frame.payload) {
                        Ok(goaway) => Err(Error::GoAway(goaway)),
                        Err(error) => Err(error.into()),
                    },
                    yas_wire::core::event_kind::SESSION_UPDATE => {
                        self.apply_session_update(&frame.payload)
                    }
                    yas_wire::core::event_kind::FAMILY_UPDATE => {
                        self.apply_family_update(&frame.payload)
                    }
                    _ => return Ok(Some(frame)),
                };
                if let Err(error) = update {
                    self.poisoned = true;
                    return Err(error);
                }
                continue;
            }
            return Ok(Some(frame));
        }
    }

    fn ensure_healthy(&self) -> Result<(), Error> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }

    fn ensure_read_headroom(&self) -> Result<(), Error> {
        self.ensure_healthy()?;
        if self.pending.len() >= MAX_PENDING_FRAMES
            || self.receive_pending.available() < u64::from(RECEIVE_MAX_DECODED)
        {
            return Err(Error::PendingReadBlocked);
        }
        Ok(())
    }

    fn answer_ping(&mut self, frame: Frame) -> Result<(), Error> {
        let request_id = frame
            .header
            .request_id
            .ok_or(Error::Protocol("PING has no request ID"))?;
        let _ = Ping::decode(&frame.payload)?;
        let receive_ns = u64::try_from(host::clock(host::ClockKind::Monotonic)).unwrap_or(0);
        let result = PingResult {
            receiver_receive_ns: receive_ns,
            receiver_send_ns: u64::try_from(host::clock(host::ClockKind::Monotonic))
                .unwrap_or(receive_ns)
                .max(receive_ns),
        };
        let prefix = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: result.encode()?,
        };
        self.send_frame(&Frame {
            header: FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::PING,
                request_id,
            ),
            payload: prefix.encode()?,
        })
    }

    fn apply_session_update(&mut self, payload: &[u8]) -> Result<(), Error> {
        let update = SessionUpdate::decode(payload)?;
        if update.validate_after(self.hello.catalog_revision, &self.hello.receive)?
            == CatalogStep::Gap
        {
            return Err(Error::Protocol("SESSION_UPDATE catalogue gap"));
        }
        self.outbound = FrameCodec::new(
            FrameLimits {
                max_wire_frame: update.receive.max_frame,
                max_decoded_frame: update.receive.max_decoded,
            },
            self.negotiated_codecs.iter().copied(),
        )?;
        self.hello.receive = update.receive;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }

    fn apply_family_update(&mut self, payload: &[u8]) -> Result<(), Error> {
        let update = FamilyUpdate::decode(payload)?;
        let descriptor = self
            .hello
            .families
            .iter_mut()
            .find(|descriptor| descriptor.family_id == update.family.family_id)
            .ok_or(Error::Protocol("FAMILY_UPDATE introduced unknown family"))?;
        if update.validate_after(self.hello.catalog_revision, descriptor)? == CatalogStep::Gap {
            return Err(Error::Protocol("FAMILY_UPDATE catalogue gap"));
        }
        *descriptor = update.family;
        self.hello.catalog_revision = update.catalog_revision;
        Ok(())
    }
}

fn send_chunk(bytes: &[u8]) -> Result<(), Error> {
    match host::send(bytes)? {
        host::SendOutcome::Accepted => Ok(()),
        host::SendOutcome::Closed => Err(Error::EndpointClosed),
        host::SendOutcome::RejectedSize => Err(Error::SendRejected),
    }
}

fn send_stream(mut bytes: &[u8]) -> Result<(), Error> {
    while !bytes.is_empty() {
        let len = bytes.len().min(host::MAX_PACKET_SIZE);
        send_chunk(&bytes[..len])?;
        bytes = &bytes[len..];
    }
    Ok(())
}

struct StreamReceiver {
    host_buffer: Vec<u8>,
    stream: Vec<u8>,
    maximum: usize,
}

enum StreamReceive {
    Frame(Frame),
    Deadline,
    Closed,
}

impl StreamReceiver {
    fn new(maximum: u64) -> Result<Self, Error> {
        let maximum = usize::try_from(maximum).map_err(|_| Error::BufferedLimit)?;
        let initial = INITIAL_BUFFER.min(maximum);
        Ok(Self {
            host_buffer: vec![0; initial],
            stream: Vec::new(),
            maximum,
        })
    }

    fn recv(&mut self, codec: &FrameCodec) -> Result<Option<Frame>, Error> {
        match self.recv_until(codec, MonotonicInstant::MAX)? {
            StreamReceive::Frame(frame) => Ok(Some(frame)),
            StreamReceive::Deadline | StreamReceive::Closed => Ok(None),
        }
    }

    fn recv_until(
        &mut self,
        codec: &FrameCodec,
        deadline: MonotonicInstant,
    ) -> Result<StreamReceive, Error> {
        loop {
            if self.stream.len() >= 4 {
                let frame_len = u32::from_le_bytes(
                    self.stream[..4]
                        .try_into()
                        .expect("checked stream prefix length"),
                ) as usize;
                let total = frame_len.checked_add(4).ok_or(Error::BufferedLimit)?;
                if total > self.maximum {
                    return Err(Error::BufferedLimit);
                }
                if self.stream.len() >= total {
                    let (frame, consumed) = codec.decode_stream(&self.stream[..total])?;
                    debug_assert_eq!(consumed, total);
                    self.stream.drain(..total);
                    return Ok(StreamReceive::Frame(frame));
                }
            }

            match host::wait(deadline.0)? {
                host::WaitOutcome::Deadline => return Ok(StreamReceive::Deadline),
                host::WaitOutcome::Closed => return Ok(StreamReceive::Closed),
                host::WaitOutcome::Packet => {}
            }
            let chunk = loop {
                match host::recv(&mut self.host_buffer)? {
                    host::RecvOutcome::Closed => return Ok(StreamReceive::Closed),
                    host::RecvOutcome::NeedsCapacity(required) => {
                        if required > self.maximum {
                            return Err(Error::BufferedLimit);
                        }
                        self.host_buffer
                            .try_reserve_exact(required.saturating_sub(self.host_buffer.len()))
                            .map_err(|_| Error::AllocationFailed)?;
                        self.host_buffer.resize(required, 0);
                    }
                    host::RecvOutcome::Copied(len) => break &self.host_buffer[..len],
                }
            };
            let total = self
                .stream
                .len()
                .checked_add(chunk.len())
                .ok_or(Error::BufferedLimit)?;
            if total > self.maximum {
                return Err(Error::BufferedLimit);
            }
            self.stream
                .try_reserve_exact(chunk.len())
                .map_err(|_| Error::AllocationFailed)?;
            self.stream.extend_from_slice(chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::VecDeque, rc::Rc};
    use std::cell::RefCell;
    use yas_wire::{
        Extensions,
        core::{FamilyDescriptor, Operation, RuntimeState},
        extension::{Runtime, event_kind},
        transfer::{ByteData, Close, Credit, Descriptor, Direction, Mode},
    };

    use crate::{native_host, transfer::ByteReceiver};

    #[derive(Default)]
    struct State {
        incoming: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
    }

    struct MockHost(Rc<RefCell<State>>);

    impl native_host::Host for MockHost {
        fn send(&mut self, packet: &[u8]) -> i32 {
            self.0.borrow_mut().sent.push(packet.to_vec());
            0
        }

        fn recv(&mut self, buffer: &mut [u8]) -> i32 {
            let mut state = self.0.borrow_mut();
            let Some(packet) = state.incoming.front() else {
                return 0;
            };
            let len = packet.len();
            if len <= buffer.len() {
                buffer[..len].copy_from_slice(packet);
                state.incoming.pop_front();
            }
            len as i32
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
                    version: yas_wire::channel::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: yas_wire::channel::Limits::HARD.to_extensions().unwrap(),
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
            extension_handle: 11,
            generation: 12,
            definition_revision: 13,
            attempt: 14,
            task_id: 15,
            flags: (yas_wire::schema::extension::DEFINITION_ENABLED
                | yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING)
                as u16,
            runtime: Runtime::Wasmi,
            content_hash: [4; 32],
            name: String::from("native"),
            argv: vec![b"--yas".to_vec()],
            extensions: Extensions::default(),
        }
    }

    fn bootstrap_client() -> (Client, Rc<RefCell<State>>, native_host::Guard) {
        let hello = server_hello();
        let hello_result = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: hello.encode().unwrap(),
        };
        let pre = FrameCodec::pre_hello();
        let hello_frame = pre
            .encode_stream(&Frame {
                header: FrameHeader::result(
                    family::CORE,
                    yas_wire::core::request_kind::HELLO,
                    HELLO_REQUEST_ID,
                ),
                payload: hello_result.encode().unwrap(),
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
        let state = Rc::new(RefCell::new(State {
            incoming: [hello_frame, context_frame].into(),
            sent: Vec::new(),
        }));
        let guard = native_host::install(MockHost(state.clone()));
        let client = Client::bootstrap().unwrap();
        (client, state, guard)
    }

    #[test]
    fn native_bootstrap_sends_preface_and_typed_hello() {
        let hello = server_hello();
        let hello_result = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: hello.encode().unwrap(),
        };
        let pre = FrameCodec::pre_hello();
        let hello_frame = pre
            .encode_stream(&Frame {
                header: FrameHeader::result(
                    family::CORE,
                    yas_wire::core::request_kind::HELLO,
                    HELLO_REQUEST_ID,
                ),
                payload: hello_result.encode().unwrap(),
            })
            .unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let expected_context = attempt_context();
        let context_frame = codec
            .encode_stream(&Frame {
                header: FrameHeader {
                    sensitive: true,
                    ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT)
                },
                payload: expected_context.encode().unwrap(),
            })
            .unwrap();

        // Deliberately split one frame and coalesce its tail with the next to
        // prove host packet boundaries are not protocol boundaries.
        let split = hello_frame.len() / 2;
        let mut second = hello_frame[split..].to_vec();
        second.extend_from_slice(&context_frame);
        let state = Rc::new(RefCell::new(State {
            incoming: [hello_frame[..split].to_vec(), second].into(),
            sent: Vec::new(),
        }));
        let _guard = native_host::install(MockHost(state.clone()));

        let client = Client::bootstrap().unwrap();
        assert_eq!(client.context(), &expected_context);
        assert_eq!(client.hello(), &hello);
        let sent = &state.borrow().sent;
        assert_eq!(sent[0], yas_wire::PREFACE);
        let (request, consumed) = pre.decode_stream(&sent[1]).unwrap();
        assert_eq!(consumed, sent[1].len());
        assert_eq!(
            request.header,
            FrameHeader::request(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                HELLO_REQUEST_ID,
            )
        );
        let offer = ClientHello::decode(&request.payload).unwrap();
        assert_eq!(offer.client_instance, [7; 16]);
        assert_eq!(offer.receive.max_buffered, RECEIVE_MAX_BUFFERED);
        assert_eq!(
            offer.receive.max_buffered,
            RECEIVE_CREDIT_BUDGET + RECEIVE_PENDING_BUDGET
        );
        assert_eq!(
            offer
                .families
                .iter()
                .map(|family| family.family_id)
                .collect::<Vec<_>>(),
            yas_wire::schema::FAMILIES
                .iter()
                .filter(|metadata| metadata.id != family::CORE)
                .map(|metadata| metadata.id)
                .collect::<Vec<_>>()
        );
        assert!(offer.families.iter().all(|offered| {
            offered.required
                == matches!(
                    offered.family_id,
                    family::TRANSFER | family::CHANNEL | family::EXTENSION
                )
        }));
    }

    #[test]
    fn attempt_output_is_sensitive_typed_native_event() {
        let (mut client, state, _guard) = bootstrap_client();
        client.attempt_stdout(b"hello").unwrap();
        client.attempt_stderr(b"oops").unwrap();
        client.attempt_log("ready").unwrap();

        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let sent = &state.borrow().sent;
        let expected = [
            (OutputKind::Stdout, b"hello".as_slice()),
            (OutputKind::Stderr, b"oops".as_slice()),
            (OutputKind::Log, b"ready".as_slice()),
        ];
        for (encoded, (kind, data)) in sent[sent.len() - expected.len()..].iter().zip(expected) {
            let (frame, consumed) = codec.decode_stream(encoded).unwrap();
            assert_eq!(consumed, encoded.len());
            assert_eq!(
                frame.header,
                FrameHeader {
                    sensitive: true,
                    ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_OUTPUT)
                }
            );
            assert_eq!(
                AttemptOutput::decode(&frame.payload).unwrap(),
                AttemptOutput {
                    kind,
                    data: data.to_vec(),
                    extensions: Extensions::default(),
                }
            );
        }
    }

    #[test]
    fn multiplexed_results_and_events_preserve_correlation_and_order() {
        let (mut client, state, _guard) = bootstrap_client();
        let first = client
            .begin_request(family::CHANNEL, 0x0101, Vec::new(), false)
            .unwrap();
        let second = client
            .begin_request(family::CHANNEL, 0x0102, Vec::new(), false)
            .unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let result_frame = |token: RequestToken, body: &[u8]| {
            codec
                .encode_stream(&Frame {
                    header: FrameHeader::result(token.family(), token.kind(), token.request_id()),
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: body.to_vec(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap()
        };
        let event = codec
            .encode_stream(&Frame {
                header: FrameHeader {
                    sensitive: true,
                    ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT)
                },
                payload: attempt_context().encode().unwrap(),
            })
            .unwrap();
        state.borrow_mut().incoming.extend([
            result_frame(second, b"second"),
            event,
            result_frame(first, b"first"),
        ]);

        let frame = client
            .next_matching_frame(|frame| first.matches(frame))
            .unwrap();
        let result = client.offer_result(&first, &frame).unwrap().unwrap();
        assert_eq!(result.body, b"first");

        let frame = client
            .next_matching_frame(|frame| second.matches(frame))
            .unwrap();
        let result = client.offer_result(&second, &frame).unwrap().unwrap();
        assert_eq!(result.body, b"second");

        let frame = client.next_event().unwrap();
        assert_eq!(frame.header.family, family::EXTENSION);
        assert_eq!(frame.header.kind, event_kind::ATTEMPT_CONTEXT);
    }

    #[test]
    fn byte_transfer_receiver_is_credited_bounded_and_contiguous() {
        let (mut client, state, _guard) = bootstrap_client();
        let descriptor = Descriptor {
            transfer_id: 17,
            mode: Mode::Byte,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 2,
            max_item_bytes: 0,
            max_chunk_bytes: 2,
            content_family: family::TERMINAL,
            content_kind: yas_wire::terminal::request_kind::OUTPUT,
            content_version: yas_wire::terminal::VERSION,
            extensions: Extensions::default(),
        };
        let mut receiver = ByteReceiver::new(&mut client, descriptor, Some(4), 4).unwrap();

        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let sent = state.borrow().sent.last().cloned().unwrap();
        let (credit_frame, consumed) = codec.decode_stream(&sent).unwrap();
        assert_eq!(consumed, sent.len());
        assert_eq!(credit_frame.header.family, family::TRANSFER);
        assert_eq!(credit_frame.header.kind, yas_wire::transfer::kind::CREDIT);
        assert_eq!(
            Credit::decode(&credit_frame.payload)
                .unwrap()
                .cumulative_limit,
            4
        );

        let data_frame = |offset, data: &[u8]| Frame {
            header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::BYTE_DATA),
            payload: ByteData {
                transfer_id: 17,
                offset,
                data: data.to_vec(),
            }
            .encode()
            .unwrap(),
        };
        assert!(
            receiver
                .offer_frame(&data_frame(0, b"ab"))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            receiver.offer_frame(&data_frame(3, b"x")),
            Err(crate::transfer::Error::NonContiguous)
        ));
        assert!(
            receiver
                .offer_frame(&data_frame(2, b"cd"))
                .unwrap()
                .is_none()
        );
        let close = Frame {
            header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE),
            payload: Close {
                transfer_id: 17,
                final_data_bytes: 4,
                status: Status::Ok.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        };
        assert_eq!(
            receiver.offer_frame(&close).unwrap(),
            Some(b"abcd".to_vec())
        );
    }

    #[test]
    fn pending_partition_rejects_without_corrupting_accounting() {
        let (mut client, _state, _guard) = bootstrap_client();
        let full = Frame {
            header: FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT),
            payload: vec![0; MAX_PENDING_BYTES],
        };
        client.defer(full).unwrap();
        assert_eq!(client.pending.len(), 1);
        assert_eq!(client.pending_bytes, MAX_PENDING_BYTES);
        assert_eq!(client.receive_pending.available(), 0);

        let rejected = Frame {
            header: FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT),
            payload: vec![1],
        };
        assert!(matches!(
            client.defer(rejected),
            Err(Error::PendingBufferFull)
        ));
        assert_eq!(client.pending.len(), 1);
        assert_eq!(client.pending_bytes, MAX_PENDING_BYTES);
        assert_eq!(client.receive_pending.available(), 0);
        assert!(client.poisoned);

        assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        assert!(matches!(
            client.begin_request(family::CHANNEL, 1, Vec::new(), false),
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(Error::Poisoned)
        ));

        let removed = client.remove_pending(0).unwrap();
        assert_eq!(removed.payload.len(), MAX_PENDING_BYTES);
        assert!(client.pending.is_empty());
        assert_eq!(client.pending_bytes, 0);
        assert_eq!(client.receive_pending.available(), RECEIVE_PENDING_BUDGET);
        assert!(matches!(
            client.attempt_log("still poisoned"),
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(Error::Poisoned)
        ));
    }

    #[test]
    fn request_preflight_rejects_before_wire_send_when_pending_headroom_is_low() {
        let (mut client, state, _guard) = bootstrap_client();
        client
            .defer(Frame {
                header: FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT),
                payload: vec![
                    0;
                    MAX_PENDING_BYTES - usize::try_from(RECEIVE_MAX_DECODED).unwrap() + 1
                ],
            })
            .unwrap();
        let sent_before = state.borrow().sent.len();

        assert!(matches!(
            client.begin_request(family::CHANNEL, 1, Vec::new(), false),
            Err(Error::PendingReadBlocked)
        ));
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert!(!client.poisoned);
        assert!(client.outstanding.is_empty());
    }

    #[test]
    fn request_poisoned_when_pending_burst_exhausts_headroom_after_send() {
        let (mut client, state, _guard) = bootstrap_client();
        client
            .defer(Frame {
                header: FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT),
                payload: vec![0; MAX_PENDING_BYTES - usize::try_from(RECEIVE_MAX_DECODED).unwrap()],
            })
            .unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT)
                    },
                    payload: vec![1],
                })
                .unwrap(),
        );
        let sent_before = state.borrow().sent.len();

        assert!(matches!(
            client.request_result(family::CHANNEL, 1, Vec::new(), false),
            Err(Error::PendingReadBlocked)
        ));
        assert_eq!(state.borrow().sent.len(), sent_before + 1);
        assert!(client.poisoned);
        assert!(client.outstanding.is_empty());
        assert_eq!(
            client.pending_bytes,
            MAX_PENDING_BYTES - usize::try_from(RECEIVE_MAX_DECODED).unwrap() + 1
        );
        assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(Error::Poisoned)
        ));
    }

    #[test]
    fn malformed_result_prefix_reports_wire_error_then_poisoned() {
        let (mut client, state, _guard) = bootstrap_client();
        let token = client
            .begin_request(family::CHANNEL, 0x0101, Vec::new(), false)
            .unwrap();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader::result(token.family(), token.kind(), token.request_id()),
                    payload: vec![0],
                })
                .unwrap(),
        );

        assert!(matches!(
            client.await_result_until(&token, MonotonicInstant::MAX),
            Err(Error::Wire(_))
        ));
        assert!(client.poisoned);
        assert!(client.outstanding.is_empty());
        assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        assert!(matches!(
            client.attempt_log("blocked"),
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(Error::Poisoned)
        ));
    }

    #[test]
    fn malformed_initial_credit_result_body_pins_authority_and_poisons() {
        let (mut client, state, _guard) = bootstrap_client();
        let request_id = client.next_request_id | 1;
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::CHANNEL, 0x0101, request_id)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: vec![0],
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );
        let mut lease = client.receive_credit_exact(4 * 1024 * 1024).unwrap();

        let result: Result<PingResult, Error> = client.request_typed_with_receive_lease(
            family::CHANNEL,
            0x0101,
            &Ping {
                sender_monotonic_ns: 1,
            },
            true,
            &mut lease,
        );
        assert!(matches!(result, Err(Error::Wire(_))), "got {result:?}");
        assert!(client.poisoned);
        assert!(lease.committed());
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        drop(lease);
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        assert!(matches!(
            client.attempt_log("blocked"),
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(Error::Poisoned)
        ));
    }

    #[test]
    fn nondeadline_await_failures_poison_and_pin_committed_authority() {
        for case in 0..3 {
            let (mut client, state, _guard) = bootstrap_client();
            let mut lease = client.receive_credit_exact(4 * 1024 * 1024).unwrap();
            let token = client
                .begin_request(family::CHANNEL, 0x0101, Vec::new(), false)
                .unwrap();
            lease.commit();
            let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
            match case {
                0 => {}
                1 => state.borrow_mut().incoming.push_back(
                    codec
                        .encode_stream(&Frame {
                            header: FrameHeader::request(family::CHANNEL, 0x0202, 2),
                            payload: Vec::new(),
                        })
                        .unwrap(),
                ),
                2 => state.borrow_mut().incoming.push_back(
                    codec
                        .encode_stream(&Frame {
                            header: FrameHeader::result(family::CHANNEL, 0x0202, 99),
                            payload: ResultPrefix {
                                status: Status::Ok,
                                detail: Extensions::default(),
                                body: Vec::new(),
                            }
                            .encode()
                            .unwrap(),
                        })
                        .unwrap(),
                ),
                _ => unreachable!(),
            }

            let result = client.await_result_until(&token, MonotonicInstant::MAX);
            match case {
                0 => assert!(matches!(result, Err(Error::EndpointClosed))),
                1 => assert!(matches!(
                    result,
                    Err(Error::Protocol("unsupported server Request"))
                )),
                2 => assert!(matches!(
                    result,
                    Err(Error::Protocol("uncorrelated Result"))
                )),
                _ => unreachable!(),
            }
            assert!(client.poisoned);
            assert!(client.outstanding.is_empty());
            drop(lease);
            assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
            assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        }
    }

    #[test]
    fn public_routers_poison_on_wire_or_pending_unsolicited_result() {
        for case in 0..7 {
            let (mut client, state, _guard) = bootstrap_client();
            let frame = Frame {
                header: FrameHeader::result(family::CHANNEL, 0x0202, 99),
                payload: ResultPrefix {
                    status: Status::Ok,
                    detail: Extensions::default(),
                    body: Vec::new(),
                }
                .encode()
                .unwrap(),
            };
            if case % 2 == 0 {
                let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
                state
                    .borrow_mut()
                    .incoming
                    .push_back(codec.encode_stream(&frame).unwrap());
            } else {
                client.outstanding.insert(99, (family::CHANNEL, 0x0202));
                client.defer(frame).unwrap();
                client.outstanding.remove(&99);
            }

            let result = match case {
                0 => client.next_event().map(drop),
                1 => client.next_event_until(MonotonicInstant::MAX).map(drop),
                2 => client.next_frame().map(drop),
                3 => client.next_frame_until(MonotonicInstant::MAX).map(drop),
                4 => client.next_matching_event(|_| true).map(drop),
                5 => client.next_matching_frame(|_| true).map(drop),
                6 => client
                    .next_matching_frame_until(MonotonicInstant::MAX, |_| true)
                    .map(drop),
                _ => unreachable!(),
            };
            assert!(matches!(result, Err(Error::Protocol("unsolicited Result"))));
            assert!(client.poisoned);
            assert!(matches!(client.next_event(), Err(Error::Poisoned)));
            assert!(matches!(
                client.receive_credit_exact(1),
                Err(Error::Poisoned)
            ));
        }
    }

    #[test]
    fn sleep_poisoned_by_unsolicited_result() {
        let (mut client, state, _guard) = bootstrap_client();
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader::result(family::CHANNEL, 0x0202, 99),
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: Vec::new(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );

        assert!(matches!(
            client.sleep(Duration::from_nanos(1)),
            Err(Error::Protocol("unsolicited Result"))
        ));
        assert!(matches!(client.next_event(), Err(Error::Poisoned)));
    }

    #[test]
    fn permissive_matching_event_preserves_correlated_pending_result() {
        let (mut client, _state, _guard) = bootstrap_client();
        let token = client
            .begin_request(family::CHANNEL, 0x0101, Vec::new(), false)
            .unwrap();
        client
            .defer(Frame {
                header: FrameHeader::result(token.family(), token.kind(), token.request_id()),
                payload: ResultPrefix {
                    status: Status::Ok,
                    detail: Extensions::default(),
                    body: Vec::new(),
                }
                .encode()
                .unwrap(),
            })
            .unwrap();
        client
            .defer(Frame {
                header: FrameHeader::event(family::EXTENSION, 0x7fff),
                payload: Vec::new(),
            })
            .unwrap();

        let event = client.next_matching_event(|_| true).unwrap();
        assert_eq!(event.header.class, Class::Event);
        let result = client
            .next_matching_frame(|frame| token.matches(frame))
            .unwrap();
        assert_eq!(
            client
                .offer_result(&token, &result)
                .unwrap()
                .unwrap()
                .status,
            Status::Ok
        );
        assert!(client.outstanding.is_empty());
        assert!(!client.poisoned);
    }

    #[test]
    fn consumed_core_terminal_or_malformed_update_poison_read_boundary() {
        for goaway in [false, true] {
            let (mut client, state, _guard) = bootstrap_client();
            let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
            let (kind, payload) = if goaway {
                (
                    yas_wire::core::event_kind::GOAWAY,
                    GoAway {
                        status: Status::Ok,
                        close_deadline_server_ns: 1,
                        detail: Extensions::default(),
                    }
                    .encode()
                    .unwrap(),
                )
            } else {
                (yas_wire::core::event_kind::SESSION_UPDATE, vec![0])
            };
            state.borrow_mut().incoming.push_back(
                codec
                    .encode_stream(&Frame {
                        header: FrameHeader::event(family::CORE, kind),
                        payload,
                    })
                    .unwrap(),
            );

            let first = client.next_event();
            if goaway {
                assert!(matches!(first, Err(Error::GoAway(_))));
            } else {
                assert!(matches!(first, Err(Error::Wire(_))));
            }
            assert!(matches!(client.next_event(), Err(Error::Poisoned)));
        }
    }

    #[test]
    fn cancel_result_and_original_result_settle_in_either_wire_order() {
        for original_first in [false, true] {
            let (mut client, state, _guard) = bootstrap_client();
            let original = client
                .begin_request(family::CHANNEL, 0x0101, Vec::new(), true)
                .unwrap();
            let cancel_request_id = 5;
            let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
            let encoded_result = |header: FrameHeader, status: Status| {
                codec
                    .encode_stream(&Frame {
                        header,
                        payload: ResultPrefix {
                            status,
                            detail: Extensions::default(),
                            body: Vec::new(),
                        }
                        .encode()
                        .unwrap(),
                    })
                    .unwrap()
            };
            let original_result = encoded_result(
                FrameHeader {
                    sensitive: true,
                    ..FrameHeader::result(original.family(), original.kind(), original.request_id())
                },
                if original_first {
                    Status::Cancelled
                } else {
                    Status::Ok
                },
            );
            let cancel_result = encoded_result(
                FrameHeader::result(
                    family::CORE,
                    yas_wire::core::request_kind::CANCEL,
                    cancel_request_id,
                ),
                if original_first {
                    Status::Conflict
                } else {
                    Status::Ok
                },
            );
            if original_first {
                state
                    .borrow_mut()
                    .incoming
                    .extend([original_result, cancel_result]);
            } else {
                state
                    .borrow_mut()
                    .incoming
                    .extend([cancel_result, original_result]);
            }

            let settled = client.cancel_request_and_wait(&original).unwrap();
            assert_eq!(
                settled.status,
                if original_first {
                    Status::Cancelled
                } else {
                    Status::Ok
                }
            );
            assert!(client.outstanding.is_empty());
            assert!(client.pending.is_empty());
            assert!(!client.poisoned);
        }
    }
}
