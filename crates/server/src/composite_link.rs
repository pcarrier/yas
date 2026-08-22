//! Accepted optional-datagram sideband for one native YAS session.
//!
//! Both directions are bounded and use `try_send`: congestion is observable
//! loss on the optional path and can never delay the reliable YAS stream.

use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};

use super::ConnectionCancellation;

const DATAGRAM_QUEUE: usize = 64;

pub(crate) struct DatagramLink {
    max_datagram: u32,
    inbound: mpsc::Receiver<InboundDatagram>,
    outbound: yas_composite_transport::DatagramSender,
    activation: watch::Sender<Option<Arc<super::yas::CreditBudget>>>,
    _outbound_counters: Arc<yas_composite_transport::QueueCounters>,
    #[cfg(test)]
    inbound_budget_drops: Arc<AtomicU64>,
}

struct InboundDatagram {
    frame: Vec<u8>,
    _credit: super::yas::CreditLease,
}

impl DatagramLink {
    pub(crate) fn open<S>(
        stream: S,
        max_datagram: u32,
        cancellation: ConnectionCancellation,
    ) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let (inbound_tx, inbound) = mpsc::channel(DATAGRAM_QUEUE);
        let (activation, mut activated) =
            watch::channel::<Option<Arc<super::yas::CreditBudget>>>(None);
        let (outbound, mut outbound_rx, outbound_counters) =
            yas_composite_transport::bounded_datagrams(DATAGRAM_QUEUE, max_datagram);
        #[cfg(test)]
        let inbound_budget_drops = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let reader_budget_drops = Arc::clone(&inbound_budget_drops);

        let reader_cancel = cancellation.clone();
        let reader_outbound = outbound.clone();
        tokio::spawn(async move {
            let budget = loop {
                if let Some(budget) = activated.borrow().clone() {
                    break budget;
                }
                tokio::select! {
                    changed = activated.changed() => {
                        if changed.is_err() {
                            reader_outbound.disable();
                            return;
                        }
                    }
                    _ = reader_cancel.cancelled() => {
                        reader_outbound.disable();
                        return;
                    }
                }
            };
            loop {
                let frame = tokio::select! {
                    result = yas_composite_transport::read_datagram(&mut reader, max_datagram) => {
                        match result {
                            Ok(frame) => frame,
                            Err(_) => break,
                        }
                    }
                    _ = reader_cancel.cancelled() => break,
                };
                let Some(credit) = budget.try_lease_exact(frame.len() as u64) else {
                    #[cfg(test)]
                    reader_budget_drops.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                // Optional datagrams are allowed to disappear under load.
                let _ = inbound_tx.try_send(InboundDatagram {
                    frame,
                    _credit: credit,
                });
            }
            reader_outbound.disable();
        });

        let writer_cancel = cancellation;
        let writer_outbound = outbound.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    frame = outbound_rx.recv() => match frame {
                        Some(frame) => frame,
                        None => break,
                    },
                    _ = writer_cancel.cancelled() => break,
                };
                if yas_composite_transport::write_datagram(&mut writer, &frame, max_datagram)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            writer_outbound.disable();
        });

        Self {
            max_datagram,
            inbound,
            outbound,
            activation,
            _outbound_counters: outbound_counters,
            #[cfg(test)]
            inbound_budget_drops,
        }
    }

    pub(crate) fn activate(&self, budget: Arc<super::yas::CreditBudget>) {
        self.activation.send_replace(Some(budget));
    }

    pub(crate) const fn max_datagram(&self) -> u32 {
        self.max_datagram
    }

    pub(crate) async fn recv(&mut self) -> Option<(Vec<u8>, super::yas::CreditLease)> {
        self.inbound
            .recv()
            .await
            .map(|queued| (queued.frame, queued._credit))
    }

    #[cfg(test)]
    pub(crate) fn try_send(&self, frame: Vec<u8>) -> Result<(), Vec<u8>> {
        self.outbound.try_send(frame)
    }

    pub(crate) fn sender(&self) -> yas_composite_transport::DatagramSender {
        self.outbound.clone()
    }

    #[cfg(test)]
    pub(crate) fn inbound_budget_drop_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.inbound_budget_drops)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn sideband_round_trips_without_reliable_stream_backpressure() {
        let (server, mut peer) = tokio::io::duplex(4096);
        let cancellation = ConnectionCancellation::default();
        let mut link = DatagramLink::open(server, 1200, cancellation);
        link.activate(Arc::new(super::super::yas::CreditBudget::new(1200)));

        yas_composite_transport::write_datagram(&mut peer, b"inbound", 1200)
            .await
            .unwrap();
        assert_eq!(link.recv().await.unwrap().0, b"inbound");

        link.try_send(b"outbound".to_vec()).unwrap();
        assert_eq!(
            yas_composite_transport::read_datagram(&mut peer, 1200)
                .await
                .unwrap(),
            b"outbound"
        );

        // Saturating the bounded path is loss, never an await on the sender.
        let (blocked_server, mut blocked_peer) = tokio::io::duplex(1);
        let blocked_cancel = ConnectionCancellation::default();
        let blocked = DatagramLink::open(blocked_server, 1200, blocked_cancel.clone());
        blocked.activate(Arc::new(super::super::yas::CreditBudget::new(
            (DATAGRAM_QUEUE * 1200) as u64,
        )));
        let mut dropped = 0;
        for _ in 0..(DATAGRAM_QUEUE * 4) {
            dropped += usize::from(blocked.try_send(vec![1; 1200]).is_err());
        }
        assert!(dropped != 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), blocked_peer.read_u8())
                .await
                .is_ok()
        );
        blocked_cancel.cancel();
    }

    #[tokio::test]
    async fn sideband_eof_disables_datagrams_without_cancelling_reliable_session() {
        let (server, mut peer) = tokio::io::duplex(4096);
        let cancellation = ConnectionCancellation::default();
        let mut link = DatagramLink::open(server, 1200, cancellation.clone());
        link.activate(Arc::new(super::super::yas::CreditBudget::new(1200)));

        peer.shutdown().await.unwrap();
        assert!(link.recv().await.is_none());
        assert!(!cancellation.is_cancelled());
        assert_eq!(
            link.try_send(b"fallback".to_vec()).unwrap_err(),
            b"fallback"
        );
    }

    #[tokio::test]
    async fn inbound_queue_is_charged_to_the_server_receive_budget() {
        let (server, mut peer) = tokio::io::duplex(4096);
        let cancellation = ConnectionCancellation::default();
        let mut link = DatagramLink::open(server, 1200, cancellation.clone());
        let registry = Arc::new(super::super::ServerDiagnosticsRegistry::default());
        let budget = Arc::new(super::super::yas::CreditBudget::new_tracked(
            8,
            8,
            Arc::clone(&registry),
        ));
        link.activate(budget);

        for _ in 0..(DATAGRAM_QUEUE * 2) {
            yas_composite_transport::write_datagram(&mut peer, b"x", 1200)
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = registry.snapshot();
                assert!(snapshot.aggregate_receive_buffered <= snapshot.aggregate_receive_limit);
                if snapshot.aggregate_receive_buffered == 8 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        for _ in 0..8 {
            assert_eq!(link.recv().await.unwrap().0, b"x");
        }
        assert_eq!(registry.snapshot().aggregate_receive_buffered, 0);
        cancellation.cancel();
    }
}
