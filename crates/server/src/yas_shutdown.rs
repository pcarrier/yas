//! Process-wide coordination for native YAS orderly shutdown.
//!
//! Core SHUTDOWN is boot-scoped: the first valid operation schedules one
//! process shutdown and every native session receives the same GOAWAY.  The
//! coordinator is deliberately independent of a wire session so a retry on a
//! different connection can resolve an ambiguously lost Result.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, mpsc, oneshot};
use yas_wire::core::{GoAway, Status};

use super::ConnectionCancellation;

#[derive(Debug)]
pub(crate) struct Notice {
    pub(crate) goaway: GoAway,
    /// Present for sessions in the original broadcast. Late handshakes still
    /// receive GOAWAY, but do not extend the shutdown initiator's drain wait.
    pub(crate) sent: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
struct Endpoint {
    admission_closed: Arc<AtomicBool>,
    cancellation: ConnectionCancellation,
    notices: mpsc::Sender<Notice>,
}

#[derive(Clone)]
struct Replay {
    operation_id: [u8; 16],
    fingerprint: [u8; 32],
    status: Status,
    body: Vec<u8>,
    goaway: GoAway,
}

enum Phase {
    Running,
    Preparing {
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
    },
    Scheduled(Replay),
}

struct Inner {
    phase: Phase,
    endpoints: BTreeMap<[u8; 16], Endpoint>,
}

/// Resolution of one syntactically valid Core SHUTDOWN operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// This caller owns Result delivery and the transition to GOAWAY.
    Start,
    /// Exact boot-scoped replay of the committed Result.
    Replay { status: Status, body: Vec<u8> },
    /// The operation ID was already used with different arguments.
    Conflict,
    /// A different SHUTDOWN operation already owns this server boot.
    Unavailable,
}

/// One server boot's live YAS sessions and Core SHUTDOWN replay state.
pub(crate) struct Coordinator {
    inner: Mutex<Inner>,
    phase_changed: Notify,
}

impl Default for Coordinator {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                phase: Phase::Running,
                endpoints: BTreeMap::new(),
            }),
            phase_changed: Notify::new(),
        }
    }
}

impl Coordinator {
    pub(crate) fn is_scheduled(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
        matches!(&inner.phase, Phase::Scheduled(_))
    }

    /// Register a post-HELLO native session. A handshake which raced with the
    /// process admission seal is immediately put into the existing drain.
    pub(crate) fn register(
        self: &Arc<Self>,
        session_id: [u8; 16],
        admission_closed: Arc<AtomicBool>,
        cancellation: ConnectionCancellation,
        notices: mpsc::Sender<Notice>,
    ) -> Registration {
        let mut inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
        let endpoint = Endpoint {
            admission_closed,
            cancellation,
            notices,
        };
        if let Phase::Scheduled(replay) = &inner.phase {
            endpoint.admission_closed.store(true, Ordering::Release);
            let _ = endpoint.notices.try_send(Notice {
                goaway: replay.goaway.clone(),
                sent: None,
            });
        }
        assert!(
            inner.endpoints.insert(session_id, endpoint).is_none(),
            "native YAS session ID collision"
        );
        Registration {
            coordinator: Arc::clone(self),
            session_id,
        }
    }

    /// Serialize contenders without retaining a mutex across socket writes.
    /// Exact retries wait for the preparing owner to either commit or abort.
    pub(crate) async fn resolve(
        &self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
    ) -> Resolution {
        loop {
            let changed = self.phase_changed.notified();
            let resolution = {
                let mut inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
                match &inner.phase {
                    Phase::Running => {
                        inner.phase = Phase::Preparing {
                            operation_id,
                            fingerprint,
                        };
                        Some(Resolution::Start)
                    }
                    Phase::Preparing {
                        operation_id: known_id,
                        fingerprint: known_fingerprint,
                    } if *known_id == operation_id && *known_fingerprint != fingerprint => {
                        Some(Resolution::Conflict)
                    }
                    Phase::Preparing { .. } => None,
                    Phase::Scheduled(replay) if replay.operation_id == operation_id => {
                        Some(if replay.fingerprint == fingerprint {
                            Resolution::Replay {
                                status: replay.status,
                                body: replay.body.clone(),
                            }
                        } else {
                            Resolution::Conflict
                        })
                    }
                    Phase::Scheduled(_) => Some(Resolution::Unavailable),
                }
            };
            if let Some(resolution) = resolution {
                return resolution;
            }
            changed.await;
        }
    }

    /// Abort a preparation whose initiating Result could not be delivered.
    pub(crate) fn abort(&self, operation_id: [u8; 16], fingerprint: [u8; 32]) {
        let mut inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
        if matches!(
            &inner.phase,
            Phase::Preparing {
                operation_id: known_id,
                fingerprint: known_fingerprint,
            } if *known_id == operation_id && *known_fingerprint == fingerprint
        ) {
            inner.phase = Phase::Running;
            drop(inner);
            self.phase_changed.notify_waiters();
        }
    }

    /// Commit the exact Result and synchronously admit GOAWAY to every live
    /// session's one-message control queue. The returned acknowledgements are
    /// completed by the individual YAS writers after the frame is written.
    pub(crate) fn commit(
        &self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        status: Status,
        body: Vec<u8>,
        goaway: GoAway,
    ) -> Vec<oneshot::Receiver<()>> {
        let mut inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
        assert!(
            matches!(
                &inner.phase,
                Phase::Preparing {
                    operation_id: known_id,
                    fingerprint: known_fingerprint,
                } if *known_id == operation_id && *known_fingerprint == fingerprint
            ),
            "only the preparing SHUTDOWN owner may commit"
        );
        inner.phase = Phase::Scheduled(Replay {
            operation_id,
            fingerprint,
            status,
            body,
            goaway: goaway.clone(),
        });

        let mut acknowledgements = Vec::with_capacity(inner.endpoints.len());
        for endpoint in inner.endpoints.values() {
            endpoint.admission_closed.store(true, Ordering::Release);
            let (sent, received) = oneshot::channel();
            if endpoint
                .notices
                .try_send(Notice {
                    goaway: goaway.clone(),
                    sent: Some(sent),
                })
                .is_ok()
            {
                acknowledgements.push(received);
            }
        }
        drop(inner);
        self.phase_changed.notify_waiters();
        acknowledgements
    }

    /// End native session drain exactly at the advertised Core deadline.
    /// Common process teardown later repeats this through ConnectionRegistry;
    /// cancellation is level-triggered and idempotent.
    pub(crate) fn cancel_sessions(&self) {
        let cancellations = self
            .inner
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .endpoints
            .values()
            .map(|endpoint| endpoint.cancellation.clone())
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
    }
}

pub(crate) struct Registration {
    coordinator: Arc<Coordinator>,
    session_id: [u8; 16],
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.coordinator
            .inner
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .endpoints
            .remove(&self.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::Extensions;

    fn goaway() -> GoAway {
        GoAway {
            status: Status::Ok,
            close_deadline_server_ns: 99,
            detail: Extensions::default(),
        }
    }

    #[tokio::test]
    async fn replay_is_boot_scoped_and_argument_mismatch_conflicts() {
        let coordinator = Arc::new(Coordinator::default());
        let id = [7; 16];
        let fingerprint = [8; 32];
        assert_eq!(
            coordinator.resolve(id, fingerprint).await,
            Resolution::Start
        );
        let _ = coordinator.commit(id, fingerprint, Status::Ok, vec![1, 2], goaway());
        assert_eq!(
            coordinator.resolve(id, fingerprint).await,
            Resolution::Replay {
                status: Status::Ok,
                body: vec![1, 2],
            }
        );
        assert_eq!(coordinator.resolve(id, [9; 32]).await, Resolution::Conflict);
        assert_eq!(
            coordinator.resolve([10; 16], [11; 32]).await,
            Resolution::Unavailable
        );
    }

    #[tokio::test]
    async fn commit_closes_and_notifies_every_registered_session() {
        let coordinator = Arc::new(Coordinator::default());
        let first_closed = Arc::new(AtomicBool::new(false));
        let second_closed = Arc::new(AtomicBool::new(false));
        let (first_tx, mut first_rx) = mpsc::channel(1);
        let (second_tx, mut second_rx) = mpsc::channel(1);
        let first_cancellation = ConnectionCancellation::default();
        let second_cancellation = ConnectionCancellation::default();
        let _first = coordinator.register(
            [1; 16],
            Arc::clone(&first_closed),
            first_cancellation.clone(),
            first_tx,
        );
        let _second = coordinator.register(
            [2; 16],
            Arc::clone(&second_closed),
            second_cancellation.clone(),
            second_tx,
        );
        let id = [3; 16];
        let fingerprint = [4; 32];
        assert_eq!(
            coordinator.resolve(id, fingerprint).await,
            Resolution::Start
        );
        let acknowledgements =
            coordinator.commit(id, fingerprint, Status::Ok, Vec::new(), goaway());
        assert_eq!(acknowledgements.len(), 2);
        assert!(first_closed.load(Ordering::Acquire));
        assert!(second_closed.load(Ordering::Acquire));
        assert_eq!(
            first_rx
                .recv()
                .await
                .unwrap()
                .goaway
                .close_deadline_server_ns,
            99
        );
        assert_eq!(
            second_rx
                .recv()
                .await
                .unwrap()
                .goaway
                .close_deadline_server_ns,
            99
        );

        let late_closed = Arc::new(AtomicBool::new(false));
        let late_cancellation = ConnectionCancellation::default();
        let (late_tx, mut late_rx) = mpsc::channel(1);
        let _late = coordinator.register(
            [5; 16],
            Arc::clone(&late_closed),
            late_cancellation.clone(),
            late_tx,
        );
        assert!(late_closed.load(Ordering::Acquire));
        let late_notice = late_rx.recv().await.unwrap();
        assert_eq!(late_notice.goaway.close_deadline_server_ns, 99);
        assert!(late_notice.sent.is_none());

        coordinator.cancel_sessions();
        assert!(first_cancellation.is_cancelled());
        assert!(second_cancellation.is_cancelled());
        assert!(late_cancellation.is_cancelled());
    }
}
