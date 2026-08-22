//! Direct native Terminal presentation registry.
//!
//! The terminal model is shared with every frontend, but native YAS
//! views register here and receive typed frame states.  No compatibility
//! client, byte stream, opcode, or packet parser sits between the model and
//! the YAS family adapter.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use tokio::sync::mpsc;
use yas_terminal_model::FrameState;

use super::Session;

const INITIAL_FRAME_GRACE: Duration = Duration::from_millis(20);

#[derive(Clone)]
pub(crate) struct FrameGuard {
    epoch: Arc<AtomicU64>,
    expected: u64,
}

/// Keeps every chunk of one logical Terminal frame on the same side of a
/// presentation cutover. Once the writer commits the first chunk, later
/// chunks must finish even if the backend generation changes meanwhile.
#[derive(Clone)]
pub(crate) struct FrameWriteGuard {
    frame: FrameGuard,
    committed: Arc<std::sync::atomic::AtomicBool>,
}

impl FrameWriteGuard {
    pub(crate) fn new(frame: FrameGuard) -> Self {
        Self {
            frame,
            committed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub(crate) fn should_write(&self) -> bool {
        self.committed.load(Ordering::Acquire) || self.frame.is_current()
    }

    pub(crate) fn commit(&self) {
        self.committed.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn current_for_test() -> Self {
        let epoch = Arc::new(AtomicU64::new(1));
        Self::new(FrameGuard { epoch, expected: 1 })
    }
}

impl FrameGuard {
    pub(crate) fn is_current(&self) -> bool {
        self.epoch.load(Ordering::Acquire) == self.expected
    }
}

pub(crate) struct Frame {
    pub(crate) view_handle: u64,
    pub(crate) guard: FrameGuard,
    pub(crate) state: FrameState,
    pub(crate) final_state: bool,
}

struct View {
    pty_id: u16,
    epoch: Arc<AtomicU64>,
    rows: u16,
    cols: u16,
    max_fps: u16,
    scroll_offset: usize,
    next_frame_at: Instant,
    force_frame: bool,
    needs_initial_frame: bool,
    final_sent: bool,
    frames: mpsc::Sender<Frame>,
}

impl View {
    fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    fn guard(&self) -> FrameGuard {
        FrameGuard {
            expected: self.epoch.load(Ordering::Acquire),
            epoch: Arc::clone(&self.epoch),
        }
    }
}

pub(crate) struct Registry {
    next_handle: u64,
    views: HashMap<u64, View>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            next_handle: 1,
            views: HashMap::new(),
        }
    }
}

impl Registry {
    pub(crate) fn register(
        &mut self,
        pty_id: u16,
        rows: u16,
        cols: u16,
        max_fps: u16,
        frames: mpsc::Sender<Frame>,
    ) -> Option<u64> {
        let handle = self.alloc_handle()?;
        self.views.insert(
            handle,
            View {
                pty_id,
                epoch: Arc::new(AtomicU64::new(1)),
                rows,
                cols,
                max_fps: max_fps.max(1),
                scroll_offset: 0,
                // Give a newly spawned shell one short coalescing window to
                // paint before publishing its initial state.  Without this,
                // opening a native view can win the race against the PTY's
                // first output and unnecessarily publish a blank frame.
                next_frame_at: Instant::now() + INITIAL_FRAME_GRACE,
                force_frame: false,
                needs_initial_frame: true,
                final_sent: false,
                frames,
            },
        );
        Some(handle)
    }

    fn alloc_handle(&mut self) -> Option<u64> {
        for _ in 0..u64::MAX {
            let handle = self.next_handle.max(1);
            self.next_handle = self.next_handle.checked_add(1)?;
            if !self.views.contains_key(&handle) {
                return Some(handle);
            }
        }
        None
    }

    pub(crate) fn remove(&mut self, handle: u64) {
        if let Some(view) = self.views.remove(&handle) {
            view.invalidate();
        }
    }

    pub(crate) fn restart_backend(&mut self, pty_id: u16) {
        for view in self.views.values_mut().filter(|view| view.pty_id == pty_id) {
            view.invalidate();
            view.scroll_offset = 0;
            view.force_frame = true;
            view.final_sent = false;
        }
    }

    pub(crate) fn refresh_backend(&mut self, pty_id: u16) {
        for view in self.views.values_mut().filter(|view| view.pty_id == pty_id) {
            view.invalidate();
            view.force_frame = true;
        }
    }

    pub(crate) fn remove_backend(&mut self, pty_id: u16) {
        for view in self.views.values().filter(|view| view.pty_id == pty_id) {
            view.invalidate();
        }
        self.views.retain(|_, view| view.pty_id != pty_id);
    }

    pub(crate) fn mediated_size(&self, pty_id: u16) -> Option<(u16, u16)> {
        self.views
            .values()
            .filter(|view| view.pty_id == pty_id)
            .map(|view| (view.rows, view.cols))
            // Render the largest requested logical grid and let each view
            // scale it locally. Component-wise minima combine unrelated
            // portrait/landscape offers into a grid no viewer requested.
            .max_by_key(|(rows, cols)| (u32::from(*rows) * u32::from(*cols), *rows, *cols))
    }

    /// Update one view, returning whether its grid geometry changed.
    ///
    /// `None` means the view disappeared. Repeating an identical configure is
    /// deliberately `Some(false)`: reconnect bookkeeping must not invalidate
    /// a frame and create a render/configure feedback loop.
    pub(crate) fn configure(
        &mut self,
        handle: u64,
        rows: u16,
        cols: u16,
        max_fps: u16,
    ) -> Option<bool> {
        let view = self.views.get_mut(&handle)?;
        let geometry_changed = view.rows != rows || view.cols != cols;
        if geometry_changed {
            view.invalidate();
            view.force_frame = true;
        }
        view.rows = rows;
        view.cols = cols;
        view.max_fps = max_fps.max(1);
        Some(geometry_changed)
    }

    pub(crate) fn reset(&mut self, handle: u64) -> bool {
        let Some(view) = self.views.get_mut(&handle) else {
            return false;
        };
        view.invalidate();
        view.force_frame = true;
        true
    }

    /// Retry a backend frame that the protocol adapter had to coalesce while
    /// peer presentation credit was closed. This does not invalidate an
    /// already committed wire frame. `owed_final_state` distinguishes a final
    /// frame the adapter actually consumed and dropped from one that may
    /// already be queued for delivery; callers must never use this for an
    /// idle ACK.
    pub(crate) fn retry_owed_frame(&mut self, handle: u64, owed_final_state: bool) -> bool {
        let Some(view) = self.views.get_mut(&handle) else {
            return false;
        };
        view.force_frame = true;
        if owed_final_state {
            view.final_sent = false;
        }
        view.next_frame_at = Instant::now();
        true
    }

    /// Let the next PTY output produced in response to input bypass the
    /// ordinary display-rate deadline. Do not force a frame here: input is
    /// written before the child has emitted its response, so forcing the
    /// current state can consume the view's sole frame credit on a no-op.
    pub(crate) fn prioritize_input_response(&mut self, pty_id: u16) {
        let now = Instant::now();
        for view in self
            .views
            .values_mut()
            .filter(|view| view.pty_id == pty_id && !view.final_sent)
        {
            view.next_frame_at = now;
        }
    }

    pub(crate) fn scroll_absolute(&mut self, handle: u64, offset: usize) -> Option<usize> {
        let view = self.views.get_mut(&handle)?;
        view.invalidate();
        view.scroll_offset = offset;
        view.force_frame = true;
        Some(view.scroll_offset)
    }

    fn ptys_due(&self, now: Instant) -> HashSet<u16> {
        self.views
            .values()
            .filter(|view| !view.final_sent && (view.force_frame || view.next_frame_at <= now))
            .map(|view| view.pty_id)
            .collect()
    }
}

impl Session {
    pub(crate) fn native_terminal_ptys_due(&self, now: Instant) -> HashSet<u16> {
        self.native_terminal_views.ptys_due(now)
    }

    /// Publish one typed full state per due native view. Full native states
    /// deliberately do not inherit the terminal model's per-client delta
    /// baseline.
    pub(crate) fn publish_native_terminal_frames(
        &mut self,
        snapshots: &FxHashMap<u16, FrameState>,
        now: Instant,
    ) -> Option<Instant> {
        let handles = self
            .native_terminal_views
            .views
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut next_deadline = None;
        for handle in handles {
            let Some(view) = self.native_terminal_views.views.get_mut(&handle) else {
                continue;
            };
            if view.final_sent {
                continue;
            }
            if !view.force_frame && view.next_frame_at > now {
                next_deadline = Some(next_deadline.map_or(view.next_frame_at, |known: Instant| {
                    known.min(view.next_frame_at)
                }));
                continue;
            }
            let Some(pty) = self.ptys.get_mut(&view.pty_id) else {
                continue;
            };
            let state = if view.scroll_offset == 0 {
                snapshots.get(&view.pty_id).cloned()
            } else {
                let rows = usize::from(pty.driver.size().0);
                let maximum = pty.driver.total_lines().saturating_sub(rows as u32) as usize;
                view.scroll_offset = view.scroll_offset.min(maximum);
                Some(pty.driver.scrollback_frame(view.scroll_offset))
            };
            let Some(state) = state else {
                // Siblings can have different pacing deadlines but share one
                // ephemeral PTY snapshot. Re-arm the PTY while this view is
                // still owed an initial or explicitly forced frame.
                if view.force_frame || view.needs_initial_frame {
                    pty.mark_dirty();
                    next_deadline = Some(next_deadline.map_or(now, |known| known.min(now)));
                }
                continue;
            };
            let final_state = pty.exited;
            match view.frames.try_send(Frame {
                view_handle: handle,
                guard: view.guard(),
                state,
                final_state,
            }) {
                Ok(()) => {
                    view.force_frame = false;
                    view.needs_initial_frame = false;
                    view.final_sent = final_state;
                    let period = Duration::from_secs_f64(1.0 / f64::from(view.max_fps.max(1)));
                    view.next_frame_at = now + period;
                    if !final_state {
                        next_deadline =
                            Some(next_deadline.map_or(view.next_frame_at, |known: Instant| {
                                known.min(view.next_frame_at)
                            }));
                    }
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    view.force_frame = true;
                    next_deadline = Some(now + Duration::from_millis(1));
                    pty.mark_dirty();
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.native_terminal_views.views.remove(&handle);
                }
            }
        }
        next_deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_guard(registry: &Registry, handle: u64) -> FrameGuard {
        registry
            .views
            .get(&handle)
            .expect("registered Terminal view")
            .guard()
    }

    #[test]
    fn presentation_and_lifecycle_cutovers_invalidate_prior_frame_guards() {
        let (frames, _frame_rx) = mpsc::channel(1);
        let mut registry = Registry::default();
        let view = registry
            .register(7, 24, 80, 60, frames)
            .expect("Terminal view handle");

        let before_configure = current_guard(&registry, view);
        assert_eq!(registry.configure(view, 30, 100, 30), Some(true));
        assert!(!before_configure.is_current());

        let before_reset = current_guard(&registry, view);
        assert!(registry.reset(view));
        assert!(!before_reset.is_current());

        let before_scroll = current_guard(&registry, view);
        assert_eq!(registry.scroll_absolute(view, 4), Some(4));
        assert!(!before_scroll.is_current());

        let before_restart = current_guard(&registry, view);
        registry.restart_backend(7);
        assert!(!before_restart.is_current());

        let before_failed_restart_refresh = current_guard(&registry, view);
        registry.refresh_backend(7);
        assert!(!before_failed_restart_refresh.is_current());

        let before_close = current_guard(&registry, view);
        registry.remove_backend(7);
        assert!(!before_close.is_current());
        assert!(!registry.views.contains_key(&view));
    }

    #[test]
    fn close_view_invalidates_its_prior_frame_guard() {
        let (frames, _frame_rx) = mpsc::channel(1);
        let mut registry = Registry::default();
        let view = registry
            .register(7, 24, 80, 60, frames)
            .expect("Terminal view handle");
        let before_close = current_guard(&registry, view);

        registry.remove(view);

        assert!(!before_close.is_current());
        assert!(!registry.views.contains_key(&view));
    }

    #[test]
    fn every_new_view_retains_an_initial_frame_retry() {
        let (frames, _frame_rx) = mpsc::channel(2);
        let mut registry = Registry::default();
        let first = registry
            .register(7, 24, 80, 60, frames.clone())
            .expect("first Terminal view handle");
        let second = registry
            .register(7, 12, 40, 60, frames)
            .expect("second Terminal view handle");

        assert!(registry.views[&first].needs_initial_frame);
        assert!(registry.views[&second].needs_initial_frame);
    }

    #[test]
    fn terminal_size_uses_the_largest_native_view() {
        let (frames, _frame_rx) = mpsc::channel(2);
        let mut registry = Registry::default();
        let large = registry
            .register(7, 40, 160, 60, frames.clone())
            .expect("large Terminal view");
        let small = registry
            .register(7, 24, 100, 60, frames)
            .expect("small Terminal view");

        assert_eq!(registry.mediated_size(7), Some((40, 160)));
        assert_eq!(registry.configure(large, 20, 120, 60), Some(true));
        assert_eq!(registry.mediated_size(7), Some((24, 100)));
        let unchanged_guard = current_guard(&registry, large);
        assert_eq!(registry.configure(large, 20, 120, 60), Some(false));
        assert!(unchanged_guard.is_current());
        registry.remove(small);
        assert_eq!(registry.mediated_size(7), Some((20, 120)));
        registry.remove(large);
        assert_eq!(registry.mediated_size(7), None);
    }

    #[test]
    fn final_view_stays_quiescent_across_presentation_updates_until_restart() {
        let (frames, _frame_rx) = mpsc::channel(1);
        let mut registry = Registry::default();
        let view = registry
            .register(7, 24, 80, 60, frames)
            .expect("Terminal view handle");
        registry.views.get_mut(&view).unwrap().final_sent = true;

        assert_eq!(registry.configure(view, 30, 100, 30), Some(true));
        assert!(registry.reset(view));
        assert_eq!(registry.scroll_absolute(view, 4), Some(4));
        registry.refresh_backend(7);
        assert!(registry.views[&view].final_sent);

        registry.restart_backend(7);
        assert!(!registry.views[&view].final_sent);
    }

    #[test]
    fn owed_retry_reopens_only_a_final_frame_consumed_by_the_adapter() {
        let (frames, _frame_rx) = mpsc::channel(1);
        let mut registry = Registry::default();
        let view = registry
            .register(7, 24, 80, 60, frames)
            .expect("Terminal view handle");
        registry.views.get_mut(&view).unwrap().final_sent = true;

        assert!(registry.retry_owed_frame(view, false));
        assert!(registry.views[&view].final_sent);
        assert!(!registry.ptys_due(Instant::now()).contains(&7));

        assert!(registry.retry_owed_frame(view, true));
        assert!(!registry.views[&view].final_sent);
        assert!(registry.views[&view].force_frame);
        assert!(registry.ptys_due(Instant::now()).contains(&7));
    }

    #[test]
    fn input_prioritizes_the_next_output_without_forcing_a_stale_frame() {
        let (frames, _frame_rx) = mpsc::channel(1);
        let mut registry = Registry::default();
        let handle = registry
            .register(7, 24, 80, 60, frames)
            .expect("Terminal view handle");
        let view = registry.views.get_mut(&handle).expect("registered view");
        view.needs_initial_frame = false;
        view.next_frame_at = Instant::now() + Duration::from_secs(1);

        registry.prioritize_input_response(7);

        let view = registry.views.get(&handle).expect("registered view");
        assert!(!view.force_frame);
        assert!(registry.ptys_due(Instant::now()).contains(&7));
    }
}
