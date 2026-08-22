//! Session-wide receive-budget accounting for the single-threaded guest SDK.

use alloc::rc::Rc;
use core::cell::Cell;

/// Automatic sliding window for metadata/state streams. This is large enough
/// for low-latency batching while allowing dozens of idle resources to share
/// the session aggregate without caller-managed packing.
pub(crate) const DEFAULT_STATE_WINDOW: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug)]
struct State {
    maximum: u64,
    reserved: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Budget {
    state: Rc<Cell<State>>,
}

#[derive(Debug)]
pub(crate) struct Lease {
    state: Rc<Cell<State>>,
    bytes: u64,
    committed: bool,
}

impl Budget {
    pub(crate) fn new(maximum: u64) -> Self {
        Self {
            state: Rc::new(Cell::new(State {
                maximum,
                reserved: 0,
            })),
        }
    }

    pub(crate) fn available(&self) -> u64 {
        let state = self.state.get();
        state.maximum.saturating_sub(state.reserved)
    }

    pub(crate) fn lease_up_to(&self, maximum: u64) -> Lease {
        let bytes = maximum.min(self.available());
        self.reserve(bytes);
        Lease {
            state: self.state.clone(),
            bytes,
            committed: false,
        }
    }

    pub(crate) fn lease_exact(&self, bytes: u64) -> Option<Lease> {
        if bytes > self.available() {
            return None;
        }
        self.reserve(bytes);
        Some(Lease {
            state: self.state.clone(),
            bytes,
            committed: false,
        })
    }

    fn reserve(&self, bytes: u64) {
        let state = self.state.get();
        let reserved = state
            .reserved
            .checked_add(bytes)
            .expect("receive reservation overflow");
        debug_assert!(reserved <= state.maximum);
        self.state.set(State { reserved, ..state });
    }
}

impl Lease {
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn shrink_to(&mut self, bytes: u64) -> bool {
        if bytes == self.bytes {
            return true;
        }
        if self.committed || bytes > self.bytes {
            return false;
        }
        self.release_bytes(self.bytes - bytes);
        true
    }

    /// Settle a provisional committed Request reservation to the exact credit
    /// selected by its successful Result descriptor.
    pub(crate) fn settle_to(&mut self, bytes: u64) -> bool {
        if bytes > self.bytes {
            return false;
        }
        self.release_bytes(self.bytes - bytes);
        true
    }

    pub(crate) fn release(&mut self) {
        self.release_bytes(self.bytes);
    }

    /// Mark bytes as peer send authority. A committed lease is pinned if its
    /// wrapper is abandoned; only explicit protocol retirement may release it.
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }

    pub(crate) fn committed(&self) -> bool {
        self.committed
    }

    fn release_bytes(&mut self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let state = self.state.get();
        debug_assert!(bytes <= self.bytes);
        debug_assert!(bytes <= state.reserved);
        self.bytes -= bytes;
        self.state.set(State {
            reserved: state.reserved - bytes,
            ..state
        });
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.committed {
            self.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_share_one_limit_and_release_once() {
        let budget = Budget::new(10);
        let mut first = budget.lease_up_to(7);
        let second = budget.lease_up_to(7);
        assert_eq!(first.bytes(), 7);
        assert_eq!(second.bytes(), 3);
        assert_eq!(budget.available(), 0);

        assert!(first.shrink_to(4));
        assert_eq!(budget.available(), 3);
        drop(second);
        assert_eq!(budget.available(), 6);
        first.release();
        assert_eq!(budget.available(), 10);
        drop(first);
        assert_eq!(budget.available(), 10);
    }

    #[test]
    fn abandoned_committed_authority_stays_pinned() {
        let budget = Budget::new(10);
        let mut lease = budget.lease_up_to(7);
        lease.commit();
        assert!(lease.committed());
        assert!(!lease.shrink_to(4));
        drop(lease);
        assert_eq!(budget.available(), 3);
    }

    #[test]
    fn state_channel_and_finite_collectors_share_one_aggregate() {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * 1024;
        let budget = Budget::new(16 * MIB);
        let mut terminal_state = budget.lease_exact(DEFAULT_STATE_WINDOW).unwrap();
        let mut surface_state = budget.lease_exact(DEFAULT_STATE_WINDOW).unwrap();
        let mut fs_state = budget.lease_exact(DEFAULT_STATE_WINDOW).unwrap();
        let mut channel = budget
            .lease_exact(crate::channel::DEFAULT_RECEIVE_WINDOW)
            .unwrap();
        for lease in [
            &mut terminal_state,
            &mut surface_state,
            &mut fs_state,
            &mut channel,
        ] {
            lease.commit();
        }
        let finite = budget.lease_up_to(16 * MIB);
        assert_eq!(finite.bytes(), 14 * MIB + 256 * KIB);
        assert!(budget.lease_exact(1).is_none());
        drop(finite);
        assert_eq!(budget.available(), 14 * MIB + 256 * KIB);
        channel.release();
        assert_eq!(budget.available(), 15 * MIB + 256 * KIB);
    }

    #[test]
    fn one_hundred_twenty_eight_channel_attempts_cannot_multiply_credit() {
        const MIB: u64 = 1024 * 1024;
        let budget = Budget::new(16 * MIB);
        let mut granted = alloc::vec::Vec::new();
        for _ in 0..128 {
            if let Some(mut lease) = budget.lease_exact(crate::channel::DEFAULT_RECEIVE_WINDOW) {
                lease.commit();
                granted.push(lease);
            }
        }
        assert_eq!(granted.len(), 16);
        assert_eq!(budget.available(), 0);
        granted[0].release();
        assert!(
            budget
                .lease_exact(crate::channel::DEFAULT_RECEIVE_WINDOW)
                .is_some()
        );
    }
}
