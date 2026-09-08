use std::collections::HashMap;

use tokio::time::Instant;

use crate::{Access, MAX_SIGNAL_PEERS, PeerState, ProducerKeys};

const MAX_PENDING_PEERS: usize = 64;
const OFFER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) struct AuthenticatedOffer {
    pub sdp: str0m::change::SdpOffer,
    pub access: Access,
}

#[derive(Default)]
pub(crate) struct Peers {
    pub active: HashMap<String, PeerState>,
    pending: HashMap<String, Instant>,
}

impl Peers {
    pub fn expire(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, deadline| *deadline > now);
        self.active.retain(|_, state| !state.handle.is_finished());
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.pending.values().copied().min()
    }

    pub async fn wait_for_expiry(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    pub fn joined(&mut self, session_id: String) {
        if uuid::Uuid::parse_str(&session_id).is_err()
            || self.active.contains_key(&session_id)
            || self.pending.len() >= MAX_PENDING_PEERS
        {
            return;
        }
        // Presence is not proof of possession. Keep only bounded metadata,
        // never RTC state, sockets, relay allocations, queues, or peer tasks.
        self.pending
            .entry(session_id)
            .or_insert_with(|| Instant::now() + OFFER_TIMEOUT);
    }

    pub fn left(&mut self, session_id: &str) {
        self.pending.remove(session_id);
        if let Some(state) = self.active.get_mut(session_id) {
            if state.established.load(std::sync::atomic::Ordering::Relaxed) {
                // WebRTC survives signaling reconnects independently.
                state.signal_tx = None;
            } else {
                self.active.remove(session_id);
            }
        }
    }

    pub fn signal(
        &mut self,
        from: String,
        raw: serde_json::Value,
        keys: &ProducerKeys,
        start: impl FnOnce(AuthenticatedOffer) -> PeerState,
    ) {
        if uuid::Uuid::parse_str(&from).is_err() {
            return;
        }
        let Some((data, access)) = keys.open_sealed(&raw) else {
            return;
        };
        if let Some(state) = self.active.get(&from) {
            if access != state.access {
                return;
            }
            if !data
                .get("sdp")
                .is_some_and(|_| !state.established.load(std::sync::atomic::Ordering::Relaxed))
            {
                if state
                    .signal_tx
                    .as_ref()
                    .is_some_and(|tx| tx.try_send(data).is_err())
                {
                    self.active.remove(&from);
                }
                return;
            }
        }
        let Some(sdp) = data
            .get("sdp")
            .and_then(|sdp| serde_json::from_value(sdp.clone()).ok())
        else {
            return;
        };
        if !self.active.contains_key(&from) && self.active.len() >= MAX_SIGNAL_PEERS {
            return;
        }
        // Authenticate even offers with missing/expired presence: a saturated
        // pending pool must not prevent a legitimate consumer from connecting.
        self.pending.remove(&from);
        self.active.remove(&from);
        self.active
            .insert(from, start(AuthenticatedOffer { sdp, access }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::mpsc;

    fn id(n: u128) -> String {
        uuid::Uuid::from_u128(n).to_string()
    }

    fn keys() -> ProducerKeys {
        ProducerKeys {
            signing: ed25519_dalek::SigningKey::from_bytes(&[1; 32]),
            our_secret: crypto_box::SecretKey::from([2; 32]),
            consumer_rw_pk: crypto_box::SecretKey::from([3; 32]).public_key(),
            consumer_ro_pk: crypto_box::SecretKey::from([4; 32]).public_key(),
        }
    }

    fn sealed(keys: &ProducerKeys, access: Access, data: serde_json::Value) -> serde_json::Value {
        let consumer = crate::BoxKeys {
            our_secret: crypto_box::SecretKey::from(match access {
                Access::ReadWrite => [3; 32],
                Access::ReadOnly => [4; 32],
            }),
            their_public: keys.our_secret.public_key(),
        };
        let wire = crate::signaling::build_sealed_message(&keys.signing, &id(0), &data, &consumer);
        let wire: serde_json::Value = serde_json::from_str(&wire).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(wire["signed"].as_str().unwrap())
            .unwrap();
        serde_json::from_slice(&bytes[64..]).unwrap()
    }

    fn offer(keys: &ProducerKeys, access: Access) -> serde_json::Value {
        let mut rtc = str0m::Rtc::new(std::time::Instant::now());
        let mut changes = rtc.sdp_api();
        changes.add_channel(crate::DATA_CHANNEL_LABEL.to_owned());
        let (offer, _) = changes.apply().unwrap();
        sealed(keys, access, serde_json::json!({ "sdp": offer }))
    }

    fn start(offer: AuthenticatedOffer) -> PeerState {
        let (tx, mut rx) = mpsc::channel(crate::SIGNAL_PER_PEER_QUEUE);
        PeerState {
            handle: tokio::spawn(async move {
                while rx.recv().await.is_some() {}
                std::future::pending::<()>().await;
            }),
            signal_tx: Some(tx),
            established: Arc::new(AtomicBool::new(false)),
            access: offer.access,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn idle_presence_expires_without_more_signaling() {
        let mut peers = Peers::default();
        peers.joined(id(1));
        assert!(peers.active.is_empty());
        let deadline = peers.deadline().unwrap();
        Peers::wait_for_expiry(Some(deadline)).await;
        assert_eq!(Instant::now(), deadline);
        peers.expire();
        assert!(peers.pending.is_empty());
        assert!(peers.deadline().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn duplicate_presence_and_invalid_signals_do_not_extend_deadline() {
        let mut peers = Peers::default();
        let keys = keys();
        peers.joined(id(1));
        let deadline = peers.deadline();
        for _ in 0..9 {
            tokio::time::advance(std::time::Duration::from_secs(1)).await;
            peers.joined(id(1));
            peers.signal(id(1), serde_json::json!({"sdp": "unsealed"}), &keys, |_| {
                panic!("unauthenticated allocation")
            });
            assert_eq!(peers.deadline(), deadline);
        }
        Peers::wait_for_expiry(deadline).await;
        peers.expire();
        assert!(peers.pending.is_empty());
        assert!(peers.active.is_empty());
    }

    #[tokio::test]
    async fn authenticated_consumers_bypass_saturated_pending_pool() {
        let keys = keys();
        let mut peers = Peers::default();
        for n in 0..(MAX_PENDING_PEERS * 2) {
            peers.joined(id(n as u128));
        }
        assert_eq!(peers.pending.len(), MAX_PENDING_PEERS);
        assert!(peers.active.is_empty());
        for (n, access) in [(1000, Access::ReadWrite), (1001, Access::ReadOnly)] {
            peers.joined(id(n));
            assert!(!peers.pending.contains_key(&id(n)));
            peers.signal(id(n), offer(&keys, access), &keys, start);
            assert_eq!(peers.active[&id(n)].access, access);
        }
        assert_eq!(peers.active.len(), 2);
        assert_eq!(peers.pending.len(), MAX_PENDING_PEERS);
    }

    #[tokio::test(start_paused = true)]
    async fn expired_presence_does_not_block_authenticated_offer() {
        let mut peers = Peers::default();
        let keys = keys();
        peers.joined(id(1));
        Peers::wait_for_expiry(peers.deadline()).await;
        peers.expire();
        peers.signal(id(1), offer(&keys, Access::ReadWrite), &keys, start);
        assert_eq!(peers.active.len(), 1);
    }

    #[tokio::test]
    async fn unverified_signals_and_presence_cannot_displace_authenticated_peer() {
        let keys = keys();
        let mut peers = Peers::default();
        peers.signal(id(1), offer(&keys, Access::ReadWrite), &keys, start);
        let task = peers.active[&id(1)].handle.id();
        for _ in 0..(crate::SIGNAL_PER_PEER_QUEUE * 2) {
            peers.joined(id(1));
            peers.signal(id(1), serde_json::json!({"box": "invalid"}), &keys, |_| {
                panic!("unauthenticated restart")
            });
        }
        assert_eq!(peers.active[&id(1)].handle.id(), task);
        assert!(peers.pending.is_empty());
        let invalid_offer = sealed(
            &keys,
            Access::ReadWrite,
            serde_json::json!({"sdp": "invalid"}),
        );
        peers.signal(id(1), invalid_offer.clone(), &keys, |_| {
            panic!("invalid restart")
        });
        peers.signal(id(2), invalid_offer, &keys, |_| {
            panic!("invalid allocation")
        });
        assert_eq!(peers.active.len(), 1);
        assert_eq!(peers.active[&id(1)].handle.id(), task);
    }

    #[tokio::test]
    async fn authenticated_capacity_is_bounded_and_not_evicted_by_presence() {
        let keys = keys();
        let mut peers = Peers::default();
        let offer = offer(&keys, Access::ReadWrite);
        for n in 0..MAX_SIGNAL_PEERS {
            peers.signal(id(n as u128), offer.clone(), &keys, start);
        }
        peers.joined(id(1000));
        peers.signal(id(1000), offer, &keys, |_| panic!("active budget exceeded"));
        assert_eq!(peers.active.len(), MAX_SIGNAL_PEERS);
        assert_eq!(peers.pending.len(), 1);
    }

    #[tokio::test]
    async fn reconnect_restarts_only_on_authenticated_offer_and_preserves_established_peer() {
        let keys = keys();
        let mut peers = Peers::default();
        let offer = offer(&keys, Access::ReadWrite);
        peers.signal(id(1), offer.clone(), &keys, start);
        let first = peers.active[&id(1)].handle.abort_handle();
        peers.joined(id(1));
        assert_eq!(peers.active[&id(1)].handle.id(), first.id());
        peers.signal(id(1), offer.clone(), &keys, start);
        assert_ne!(peers.active[&id(1)].handle.id(), first.id());
        tokio::task::yield_now().await;
        assert!(first.is_finished());
        let established = &peers.active[&id(1)];
        established.established.store(true, Ordering::Relaxed);
        let task = established.handle.id();
        peers.left(&id(1));
        peers.joined(id(1));
        peers.signal(id(1), offer, &keys, |_| panic!("established restart"));
        assert_eq!(peers.active[&id(1)].handle.id(), task);
        assert!(peers.active[&id(1)].signal_tx.is_none());
    }

    #[tokio::test]
    async fn leaving_and_shutdown_drop_pending_and_abort_peer_tasks() {
        let keys = keys();
        let mut peers = Peers::default();
        peers.joined(id(1));
        peers.left(&id(1));
        assert!(peers.pending.is_empty());
        peers.signal(id(2), offer(&keys, Access::ReadOnly), &keys, start);
        let leaving = peers.active[&id(2)].handle.abort_handle();
        peers.left(&id(2));
        tokio::task::yield_now().await;
        assert!(leaving.is_finished());
        peers.signal(id(3), offer(&keys, Access::ReadWrite), &keys, start);
        let shutdown = peers.active[&id(3)].handle.abort_handle();
        peers.joined(id(4));
        drop(peers);
        tokio::task::yield_now().await;
        assert!(shutdown.is_finished());
    }
}
