//! Local accepted-link binding for a reliable YAS stream and its independent
//! optional datagram sideband.
//!
//! This is transport plumbing, not a second protocol. A direct native client
//! still begins with the YAS preface. A trusted edge or WebRTC forwarder uses
//! one explicit binary selector on each of two local streams, with the same
//! random token and path maximum. The server strips those selectors, pairs the
//! halves under bounded state, and only then begins the ordinary YAS HELLO.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::mpsc;

pub const TOKEN_BYTES: usize = 16;
pub type Token = [u8; TOKEN_BYTES];
/// Bytes prepended by the WebTransport uplink to route one physical datagram
/// to its composite YAS session. The token is transport plumbing and is not
/// part of the YAS Event delivered to the server.
pub const ROUTED_DATAGRAM_HEADER: usize = TOKEN_BYTES;

/// The selectors include `YAS`, a composite marker, version 1, and the role.
/// Byte three differs from the zero byte in [`yas_wire::PREFACE`], so no
/// composite ingress can be mistaken for an ordinary YAS link.
pub const MAIN_SELECTOR: [u8; 8] = [0x59, 0x41, 0x53, 0x43, 0x4d, 0x50, 0x01, 0x01];
pub const DATAGRAM_SELECTOR: [u8; 8] = [0x59, 0x41, 0x53, 0x43, 0x4d, 0x50, 0x01, 0x02];
pub const SELECTOR_BYTES: usize = MAIN_SELECTOR.len();
pub const RESERVED_BYTES: usize = 4;
pub const PREAMBLE_BYTES: usize = SELECTOR_BYTES + TOKEN_BYTES + 4 + RESERVED_BYTES;
pub const HARD_MAX_DATAGRAM: u32 = yas_wire::frame::HARD_MAX_DATAGRAM;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Main,
    Datagram,
}

impl Role {
    pub const fn selector(self) -> [u8; SELECTOR_BYTES] {
        match self {
            Self::Main => MAIN_SELECTOR,
            Self::Datagram => DATAGRAM_SELECTOR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub role: Role,
    pub token: Token,
    pub max_datagram: u32,
}

impl Offer {
    pub fn new(role: Role, token: Token, max_datagram: u32) -> io::Result<Self> {
        let value = Self {
            role,
            token,
            max_datagram,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(self) -> io::Result<()> {
        if self.token == [0; TOKEN_BYTES] {
            return Err(invalid("zero composite ingress token"));
        }
        if self.max_datagram < yas_wire::schema::transport::EVENT_HEADER_BYTES as u32
            || self.max_datagram > HARD_MAX_DATAGRAM
        {
            return Err(invalid("invalid composite datagram maximum"));
        }
        Ok(())
    }

    pub fn encode(self) -> io::Result<[u8; PREAMBLE_BYTES]> {
        self.validate()?;
        let mut bytes = [0; PREAMBLE_BYTES];
        bytes[..SELECTOR_BYTES].copy_from_slice(&self.role.selector());
        bytes[SELECTOR_BYTES..SELECTOR_BYTES + TOKEN_BYTES].copy_from_slice(&self.token);
        let maximum = SELECTOR_BYTES + TOKEN_BYTES;
        bytes[maximum..maximum + 4].copy_from_slice(&self.max_datagram.to_le_bytes());
        Ok(bytes)
    }
}

fn invalid(detail: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail)
}

pub async fn write_offer(stream: &mut (impl AsyncWrite + Unpin), offer: Offer) -> io::Result<()> {
    stream.write_all(&offer.encode()?).await
}

/// Result of classifying the first explicit local ingress bytes.
pub enum Ingress<S> {
    /// An ordinary native YAS link. The consumed preface is replayed to the
    /// normal session decoder, so it sees precisely the usual byte stream.
    Direct(ReplayPreface<S>),
    Composite {
        offer: Offer,
        stream: S,
    },
}

/// Classify one local ingress. There is no compatibility fallback: anything
/// other than the exact YAS preface or one exact composite selector is an
/// error and the caller closes the stream.
pub async fn classify<S>(mut stream: S) -> io::Result<Ingress<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut selector = [0; SELECTOR_BYTES];
    stream.read_exact(&mut selector).await?;
    if selector == yas_wire::PREFACE {
        return Ok(Ingress::Direct(ReplayPreface::new(stream)));
    }
    let role = if selector == MAIN_SELECTOR {
        Role::Main
    } else if selector == DATAGRAM_SELECTOR {
        Role::Datagram
    } else {
        return Err(invalid("unknown local YAS ingress selector"));
    };
    let mut tail = [0; PREAMBLE_BYTES - SELECTOR_BYTES];
    stream.read_exact(&mut tail).await?;
    let mut token = [0; TOKEN_BYTES];
    token.copy_from_slice(&tail[..TOKEN_BYTES]);
    let max_datagram = u32::from_le_bytes(
        tail[TOKEN_BYTES..TOKEN_BYTES + 4]
            .try_into()
            .expect("fixed composite maximum"),
    );
    if tail[TOKEN_BYTES + 4..] != [0; RESERVED_BYTES] {
        return Err(invalid("nonzero composite preamble reserved bytes"));
    }
    let offer = Offer::new(role, token, max_datagram)?;
    Ok(Ingress::Composite { offer, stream })
}

/// Stream wrapper that replays the already-classified YAS preface before
/// delegating reads to the native stream. Writes are always passed through.
pub struct ReplayPreface<S> {
    inner: S,
    offset: usize,
}

impl<S> ReplayPreface<S> {
    fn new(inner: S) -> Self {
        Self { inner, offset: 0 }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ReplayPreface<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        target: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.offset < yas_wire::PREFACE.len() && target.remaining() != 0 {
            let remaining = &yas_wire::PREFACE[self.offset..];
            let count = remaining.len().min(target.remaining());
            target.put_slice(&remaining[..count]);
            self.offset += count;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, target)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for ReplayPreface<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pair<T> {
    pub token: Token,
    pub max_datagram: u32,
    pub main: T,
    pub datagram: T,
}

struct Pending<T> {
    max_datagram: u32,
    inserted: Instant,
    main: Option<T>,
    datagram: Option<T>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairError {
    Full,
    DuplicateRole,
    MaximumMismatch,
}

/// Bounded token pairing for local ingress halves. Expiry is explicit so an
/// accept loop can drop late halves from its ordinary timer branch.
pub struct Pairing<T> {
    pending: HashMap<Token, Pending<T>>,
    max_pending: usize,
    timeout: Duration,
}

impl<T> Pairing<T> {
    pub fn new(max_pending: usize, timeout: Duration) -> Self {
        assert!(max_pending != 0, "composite pending cap must be nonzero");
        assert!(!timeout.is_zero(), "composite pair timeout must be nonzero");
        Self {
            pending: HashMap::new(),
            max_pending,
            timeout,
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn insert(
        &mut self,
        offer: Offer,
        stream: T,
        now: Instant,
    ) -> Result<Option<Pair<T>>, PairError> {
        let role = offer.role;
        if let Some(pending) = self.pending.get_mut(&offer.token) {
            if pending.max_datagram != offer.max_datagram {
                return Err(PairError::MaximumMismatch);
            }
            let slot = match role {
                Role::Main => &mut pending.main,
                Role::Datagram => &mut pending.datagram,
            };
            if slot.is_some() {
                return Err(PairError::DuplicateRole);
            }
            *slot = Some(stream);
        } else {
            if self.pending.len() >= self.max_pending {
                return Err(PairError::Full);
            }
            let (main, datagram) = match role {
                Role::Main => (Some(stream), None),
                Role::Datagram => (None, Some(stream)),
            };
            self.pending.insert(
                offer.token,
                Pending {
                    max_datagram: offer.max_datagram,
                    inserted: now,
                    main,
                    datagram,
                },
            );
        }
        let complete = self
            .pending
            .get(&offer.token)
            .is_some_and(|pending| pending.main.is_some() && pending.datagram.is_some());
        if !complete {
            return Ok(None);
        }
        let pending = self.pending.remove(&offer.token).expect("pair exists");
        Ok(Some(Pair {
            token: offer.token,
            max_datagram: pending.max_datagram,
            main: pending.main.expect("complete main half"),
            datagram: pending.datagram.expect("complete datagram half"),
        }))
    }

    /// Drop and return every half whose peer did not arrive in time.
    pub fn expire(&mut self, now: Instant) -> Vec<T> {
        let expired = self
            .pending
            .iter()
            .filter_map(|(token, pending)| {
                (now.saturating_duration_since(pending.inserted) >= self.timeout).then_some(*token)
            })
            .collect::<Vec<_>>();
        let mut streams = Vec::with_capacity(expired.len());
        for token in expired {
            if let Some(pending) = self.pending.remove(&token) {
                streams.extend(pending.main);
                streams.extend(pending.datagram);
            }
        }
        streams
    }
}

/// Write exactly one sideband message. The u32 envelope is local transport
/// framing and is never included in the YAS datagram itself.
pub async fn write_datagram(
    stream: &mut (impl AsyncWrite + Unpin),
    frame: &[u8],
    max_datagram: u32,
) -> io::Result<()> {
    if frame.is_empty() || frame.len() > max_datagram.min(HARD_MAX_DATAGRAM) as usize {
        return Err(invalid("sideband datagram exceeds negotiated maximum"));
    }
    let length = u32::try_from(frame.len()).map_err(|_| invalid("sideband length overflow"))?;
    stream.write_all(&length.to_le_bytes()).await?;
    stream.write_all(frame).await
}

/// Read exactly one sideband message. An invalid declaration closes only the
/// sideband; the owning accepted-link layer decides how to update/fail the
/// reliable session and never allocates the declared hostile length.
pub async fn read_datagram(
    stream: &mut (impl AsyncRead + Unpin),
    max_datagram: u32,
) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).await?;
    let length = u32::from_le_bytes(length);
    if length == 0 || length > max_datagram.min(HARD_MAX_DATAGRAM) {
        return Err(invalid("invalid sideband datagram length"));
    }
    let mut frame = vec![0; length as usize];
    stream.read_exact(&mut frame).await?;
    Ok(frame)
}

#[derive(Default)]
pub struct QueueCounters {
    oversized: AtomicU64,
    congested: AtomicU64,
    closed: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueCounterSnapshot {
    pub oversized: u64,
    pub congested: u64,
    pub closed: u64,
}

impl QueueCounters {
    pub fn snapshot(&self) -> QueueCounterSnapshot {
        QueueCounterSnapshot {
            oversized: self.oversized.load(Ordering::Relaxed),
            congested: self.congested.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone)]
pub struct DatagramSender {
    sender: mpsc::Sender<Vec<u8>>,
    max_datagram: u32,
    counters: Arc<QueueCounters>,
    available: Arc<AtomicBool>,
}

pub struct DatagramReceiver {
    receiver: mpsc::Receiver<Vec<u8>>,
}

pub fn bounded_datagrams(
    capacity: usize,
    max_datagram: u32,
) -> (DatagramSender, DatagramReceiver, Arc<QueueCounters>) {
    assert!(capacity != 0, "datagram queue capacity must be nonzero");
    assert!(max_datagram != 0 && max_datagram <= HARD_MAX_DATAGRAM);
    let (sender, receiver) = mpsc::channel(capacity);
    let counters = Arc::new(QueueCounters::default());
    let available = Arc::new(AtomicBool::new(true));
    (
        DatagramSender {
            sender,
            max_datagram,
            counters: Arc::clone(&counters),
            available,
        },
        DatagramReceiver { receiver },
        counters,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRegistrationError {
    InvalidToken,
    InvalidMaximum,
    Full,
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteDatagramError {
    InvalidEnvelope,
    UnknownRoute,
    Oversized,
    Congested,
    Closed,
}

#[derive(Clone)]
struct RoutedDatagramRoute {
    sender: mpsc::Sender<Vec<u8>>,
    maximum: Arc<AtomicU32>,
}

/// Bounded token router shared by the WebTransport uplink and deterministic
/// transport-adversity tests. Routing is no-wait: a full local sideband is
/// ordinary datagram loss and cannot stall another route or the reliable
/// stream.
#[derive(Clone)]
pub struct RoutedDatagramRoutes {
    routes: Arc<Mutex<HashMap<Token, RoutedDatagramRoute>>>,
    max_routes: usize,
}

impl RoutedDatagramRoutes {
    pub fn new(max_routes: usize) -> Self {
        assert!(max_routes != 0, "routed datagram cap must be nonzero");
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
            max_routes,
        }
    }

    pub fn len(&self) -> usize {
        self.routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Register one independently bounded composite route. The receiver is
    /// owned by that route's sideband writer.
    pub fn register(
        &self,
        token: Token,
        maximum: u32,
        capacity: usize,
    ) -> Result<mpsc::Receiver<Vec<u8>>, RouteRegistrationError> {
        if token == [0; TOKEN_BYTES] {
            return Err(RouteRegistrationError::InvalidToken);
        }
        if !valid_datagram_maximum(maximum) {
            return Err(RouteRegistrationError::InvalidMaximum);
        }
        assert!(capacity != 0, "routed datagram queue must be nonzero");
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes.contains_key(&token) {
            return Err(RouteRegistrationError::Duplicate);
        }
        if routes.len() >= self.max_routes {
            return Err(RouteRegistrationError::Full);
        }
        let (sender, receiver) = mpsc::channel(capacity);
        routes.insert(
            token,
            RoutedDatagramRoute {
                sender,
                maximum: Arc::new(AtomicU32::new(maximum)),
            },
        );
        Ok(receiver)
    }

    /// Update the live physical-path ceiling for a route. A decreasing QUIC
    /// MTU immediately rejects newly oversized datagrams; callers retain the
    /// ordinary reliable YAS fallback.
    pub fn set_maximum(&self, token: Token, maximum: u32) -> Result<(), RouteRegistrationError> {
        if !valid_datagram_maximum(maximum) {
            return Err(RouteRegistrationError::InvalidMaximum);
        }
        let route = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&token)
            .cloned()
            .ok_or(RouteRegistrationError::InvalidToken)?;
        route.maximum.store(maximum, Ordering::Release);
        Ok(())
    }

    pub fn remove(&self, token: Token) -> bool {
        self.routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&token)
            .is_some()
    }

    pub fn clear(&self) {
        self.routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Route one token-prefixed physical datagram without awaiting capacity.
    pub fn route(&self, bytes: &[u8]) -> Result<(), RouteDatagramError> {
        let (token, payload) = split_routed_datagram(bytes)?;
        let route = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&token)
            .cloned()
            .ok_or(RouteDatagramError::UnknownRoute)?;
        if payload.len() > route.maximum.load(Ordering::Acquire) as usize {
            return Err(RouteDatagramError::Oversized);
        }
        route
            .sender
            .try_send(payload.to_vec())
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => RouteDatagramError::Congested,
                mpsc::error::TrySendError::Closed(_) => RouteDatagramError::Closed,
            })
    }
}

fn valid_datagram_maximum(maximum: u32) -> bool {
    maximum >= yas_wire::schema::transport::EVENT_HEADER_BYTES as u32
        && maximum <= HARD_MAX_DATAGRAM
}

/// Add the explicit composite route token used by the uplink's shared
/// WebTransport datagram lane.
pub fn encode_routed_datagram(
    token: Token,
    payload: &[u8],
    maximum: u32,
) -> Result<Vec<u8>, RouteDatagramError> {
    if token == [0; TOKEN_BYTES] || payload.is_empty() {
        return Err(RouteDatagramError::InvalidEnvelope);
    }
    if !valid_datagram_maximum(maximum) || payload.len() > maximum as usize {
        return Err(RouteDatagramError::Oversized);
    }
    let mut routed = Vec::with_capacity(ROUTED_DATAGRAM_HEADER + payload.len());
    routed.extend_from_slice(&token);
    routed.extend_from_slice(payload);
    Ok(routed)
}

/// Strip an uplink route token without interpreting the complete YAS Event.
pub fn split_routed_datagram(bytes: &[u8]) -> Result<(Token, &[u8]), RouteDatagramError> {
    if bytes.len() <= ROUTED_DATAGRAM_HEADER {
        return Err(RouteDatagramError::InvalidEnvelope);
    }
    let mut token = [0; TOKEN_BYTES];
    token.copy_from_slice(&bytes[..ROUTED_DATAGRAM_HEADER]);
    if token == [0; TOKEN_BYTES] {
        return Err(RouteDatagramError::InvalidEnvelope);
    }
    Ok((token, &bytes[ROUTED_DATAGRAM_HEADER..]))
}

impl DatagramSender {
    /// Never waits for capacity. Loss under congestion is a property of the
    /// optional path and cannot backpressure the reliable control stream.
    pub fn try_send(&self, frame: Vec<u8>) -> Result<(), Vec<u8>> {
        if frame.is_empty() || frame.len() > self.max_datagram as usize {
            self.counters.oversized.fetch_add(1, Ordering::Relaxed);
            return Err(frame);
        }
        if !self.available.load(Ordering::Acquire) {
            self.counters.closed.fetch_add(1, Ordering::Relaxed);
            return Err(frame);
        }
        self.sender.try_send(frame).map_err(|error| match error {
            mpsc::error::TrySendError::Full(frame) => {
                self.counters.congested.fetch_add(1, Ordering::Relaxed);
                frame
            }
            mpsc::error::TrySendError::Closed(frame) => {
                self.counters.closed.fetch_add(1, Ordering::Relaxed);
                frame
            }
        })
    }

    pub fn is_closed(&self) -> bool {
        !self.available.load(Ordering::Acquire) || self.sender.is_closed()
    }

    /// Disable the optional path for every clone. This is separate from the
    /// reliable session lifecycle: either sideband half may fail while the
    /// authoritative byte stream remains usable.
    pub fn disable(&self) {
        self.available.store(false, Ordering::Release);
    }
}

impl DatagramReceiver {
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.receiver.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(marker: u8) -> Token {
        let mut value = [marker; TOKEN_BYTES];
        if marker == 0 {
            value[0] = 1;
        }
        value
    }

    #[tokio::test]
    async fn direct_preface_is_replayed_without_sniff_fallback() {
        let (mut client, server) = tokio::io::duplex(128);
        client.write_all(&yas_wire::PREFACE).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let Ingress::Direct(mut direct) = classify(server).await.unwrap() else {
            panic!("direct link was not classified as direct");
        };
        let mut bytes = [0; 13];
        direct.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..8], &yas_wire::PREFACE);
        assert_eq!(&bytes[8..], b"hello");

        let (mut client, server) = tokio::io::duplex(32);
        client.write_all(b"YASbad!!").await.unwrap();
        assert!(classify(server).await.is_err());
    }

    #[tokio::test]
    async fn strict_composite_preamble_round_trips() {
        let offer = Offer::new(Role::Datagram, token(0xa5), 1200).unwrap();
        let (mut client, server) = tokio::io::duplex(64);
        write_offer(&mut client, offer).await.unwrap();
        let Ingress::Composite { offer: decoded, .. } = classify(server).await.unwrap() else {
            panic!("composite link was classified as direct");
        };
        assert_eq!(decoded, offer);

        let mut bad = offer.encode().unwrap();
        *bad.last_mut().unwrap() = 1;
        let (mut client, server) = tokio::io::duplex(64);
        client.write_all(&bad).await.unwrap();
        assert!(classify(server).await.is_err());
    }

    #[test]
    fn pairs_roles_once_and_rejects_collisions() {
        let now = Instant::now();
        let mut pairing = Pairing::new(2, Duration::from_secs(1));
        let main = Offer::new(Role::Main, token(1), 1200).unwrap();
        assert!(pairing.insert(main, "main", now).unwrap().is_none());
        assert_eq!(
            pairing.insert(main, "duplicate", now),
            Err(PairError::DuplicateRole)
        );
        let mismatched = Offer::new(Role::Datagram, token(1), 1400).unwrap();
        assert_eq!(
            pairing.insert(mismatched, "mismatch", now),
            Err(PairError::MaximumMismatch)
        );
        let side = Offer::new(Role::Datagram, token(1), 1200).unwrap();
        let pair = pairing.insert(side, "side", now).unwrap().unwrap();
        assert_eq!(pair.main, "main");
        assert_eq!(pair.datagram, "side");
        assert!(pairing.is_empty());
    }

    #[test]
    fn pending_cap_and_late_half_cleanup_are_bounded() {
        let now = Instant::now();
        let timeout = Duration::from_millis(50);
        let mut pairing = Pairing::new(1, timeout);
        let first = Offer::new(Role::Main, token(1), 1200).unwrap();
        let second = Offer::new(Role::Main, token(2), 1200).unwrap();
        pairing.insert(first, 1, now).unwrap();
        assert_eq!(pairing.insert(second, 2, now), Err(PairError::Full));
        assert_eq!(pairing.expire(now + timeout), vec![1]);
        assert!(pairing.is_empty());
        pairing.insert(second, 2, now + timeout).unwrap();
    }

    #[tokio::test]
    async fn sideband_rejects_oversize_without_allocating_or_draining() {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer.write_all(&1201u32.to_le_bytes()).await.unwrap();
        assert!(read_datagram(&mut reader, 1200).await.is_err());

        let (mut writer, mut reader) = tokio::io::duplex(64);
        assert!(write_datagram(&mut writer, &[0; 1201], 1200).await.is_err());
        let mut byte = [0];
        assert!(
            tokio::time::timeout(Duration::from_millis(10), reader.read_exact(&mut byte))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn bounded_queue_never_backpressures_and_counts_drops() {
        let (sender, mut receiver, counters) = bounded_datagrams(1, 1200);
        sender.try_send(vec![1]).unwrap();
        assert_eq!(sender.try_send(vec![2]).unwrap_err(), vec![2]);
        assert_eq!(sender.try_send(vec![0; 1201]).unwrap_err().len(), 1201);
        assert_eq!(receiver.recv().await.unwrap(), vec![1]);
        assert_eq!(
            counters.snapshot(),
            QueueCounterSnapshot {
                oversized: 1,
                congested: 1,
                closed: 0,
            }
        );
    }

    #[test]
    fn disabling_one_sender_clone_disables_the_optional_path() {
        let (sender, _receiver, counters) = bounded_datagrams(1, 1200);
        let peer = sender.clone();
        peer.disable();
        assert!(sender.is_closed());
        assert_eq!(sender.try_send(vec![1]).unwrap_err(), vec![1]);
        assert_eq!(counters.snapshot().closed, 1);
    }

    fn routed(token: Token, payload: &[u8], maximum: u32) -> Vec<u8> {
        encode_routed_datagram(token, payload, maximum).unwrap()
    }

    #[test]
    fn routed_datagrams_are_strict_bounded_and_mtu_live() {
        let route_token = token(7);
        let routes = RoutedDatagramRoutes::new(1);
        let mut receiver = routes.register(route_token, 32, 1).unwrap();

        assert_eq!(
            routes.route(b"short"),
            Err(RouteDatagramError::InvalidEnvelope)
        );
        let mut zero = vec![0; ROUTED_DATAGRAM_HEADER];
        zero.push(1);
        assert_eq!(
            routes.route(&zero),
            Err(RouteDatagramError::InvalidEnvelope)
        );
        assert_eq!(
            routes.route(&routed(token(8), b"unknown", 32)),
            Err(RouteDatagramError::UnknownRoute)
        );

        routes.route(&routed(route_token, b"first", 32)).unwrap();
        assert_eq!(
            routes.route(&routed(route_token, b"full", 32)),
            Err(RouteDatagramError::Congested)
        );
        assert_eq!(receiver.try_recv().unwrap(), b"first");

        routes.set_maximum(route_token, 5).unwrap();
        assert_eq!(
            routes.route(&routed(route_token, b"123456", 32)),
            Err(RouteDatagramError::Oversized)
        );
        routes.route(&routed(route_token, b"12345", 32)).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), b"12345");
    }

    #[test]
    fn routed_datagram_routes_isolate_congestion_and_registration() {
        let slow = token(1);
        let live = token(2);
        let routes = RoutedDatagramRoutes::new(2);
        let _slow_receiver = routes.register(slow, 32, 1).unwrap();
        let mut live_receiver = routes.register(live, 32, 1).unwrap();
        assert!(matches!(
            routes.register(live, 32, 1),
            Err(RouteRegistrationError::Duplicate)
        ));
        assert!(matches!(
            routes.register(token(3), 32, 1),
            Err(RouteRegistrationError::Full)
        ));

        routes.route(&routed(slow, b"fills", 32)).unwrap();
        assert_eq!(
            routes.route(&routed(slow, b"drops", 32)),
            Err(RouteDatagramError::Congested)
        );
        routes.route(&routed(live, b"fair", 32)).unwrap();
        assert_eq!(live_receiver.try_recv().unwrap(), b"fair");

        assert!(routes.remove(slow));
        assert!(routes.register(token(3), 32, 1).is_ok());
        routes.clear();
        assert!(routes.is_empty());
    }
}
