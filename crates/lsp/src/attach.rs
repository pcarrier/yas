//! One client attachment: the per-connection view of a workspace's
//! backends, owning the paced `LSP_STATE` and `LSP_DIAG` streams
//! (docs/design/lsp.md `LSP_STATE` / `LSP_DIAG`).
//!
//! The pacing thread mirrors fssync's per-sync engine: one in-flight
//! update per stream, coalescing while unacked — a slow client gets
//! fewer, larger updates and never falls behind. The first diagnostics
//! update is a `FULL` cache replay, so a late joiner or one-shot CLI
//! never sees a blank gutter.
//!
//! Backends can be stopped out from under an attachment (`LSP_STOP`, the
//! idle sweep). The attachment holds its backends behind a shared lock:
//! the pacer drops a stopped backend's `SERVER` record from the next
//! snapshot, and a query to a stopped backend respawns it, matching the
//! spec's "subscribers see LSP_STATE lose the record; a later query
//! respawns it".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::model::{
    LSP_CAP_CODE_ACTIONS, LSP_CAP_COMPLETION, LSP_CAP_DEFINITION, LSP_CAP_DOC_SYMBOLS,
    LSP_CAP_FORMATTING, LSP_CAP_HOVER, LSP_CAP_REFERENCES, LSP_CAP_RENAME, LSP_CAP_SIGNATURE,
    LSP_CAP_WS_SYMBOLS, LSP_DIAG_DEPRECATED, LSP_DIAG_UNNECESSARY, LSP_PHASE_FAILED,
    LSP_PHASE_READY, LSP_QUERY_CODE_ACTIONS, LSP_QUERY_COMPLETION, LSP_QUERY_DEFINITION,
    LSP_QUERY_DOC_SYMBOLS, LSP_QUERY_FORMATTING, LSP_QUERY_HOVER, LSP_QUERY_REFERENCES,
    LSP_QUERY_RENAME, LSP_QUERY_SIGNATURE, LSP_QUERY_WS_SYMBOLS, LSP_STATUS_INVALID,
    LSP_STATUS_NOT_FOUND, LSP_STATUS_OTHER, LSP_STATUS_WARMING, LSP_STREAM_DIAG, LSP_STREAM_STATE,
};

use crate::backend::{Backend, Cmd};
use crate::discovery::ServerSpec;
use crate::{Budgets, native};

/// Diagnostics one file may report in a single update. A cascade this long
/// is one broken parse, and its first entries are the informative ones.
fn take_diagnostics(
    diagnostics: Vec<crate::backend::CachedDiagnostic>,
    bytes_used: &mut usize,
    bytes_max: usize,
) -> Vec<native::Diagnostic> {
    diagnostics
        .into_iter()
        .take(crate::DIAG_PROTOCOL_MAX_PER_FILE)
        .take_while(|diagnostic| {
            let size = crate::model::diagnostic_size(diagnostic);
            if bytes_used.saturating_add(size) > bytes_max {
                return false;
            }
            *bytes_used += size;
            true
        })
        .map(|diagnostic| native::Diagnostic {
            severity: diagnostic.severity,
            unnecessary: diagnostic.flags & LSP_DIAG_UNNECESSARY != 0,
            deprecated: diagnostic.flags & LSP_DIAG_DEPRECATED != 0,
            line: diagnostic.line,
            column: diagnostic.col,
            end_line: diagnostic.end_line,
            end_column: diagnostic.end_col,
            code: diagnostic.code,
            source: diagnostic.source,
            message: diagnostic.msg,
        })
        .collect()
}

/// The capability bit a query kind requires of its backend, so routing
/// never sends an unsupported request (which would surface as a bare
/// error). `0` for unknown kinds — no backend advertises it, so the
/// query answers `NOT_FOUND`.
fn required_cap(kind: u8) -> u32 {
    match kind {
        LSP_QUERY_DEFINITION => LSP_CAP_DEFINITION,
        LSP_QUERY_REFERENCES => LSP_CAP_REFERENCES,
        LSP_QUERY_HOVER => LSP_CAP_HOVER,
        LSP_QUERY_DOC_SYMBOLS => LSP_CAP_DOC_SYMBOLS,
        LSP_QUERY_WS_SYMBOLS => LSP_CAP_WS_SYMBOLS,
        LSP_QUERY_RENAME => LSP_CAP_RENAME,
        LSP_QUERY_COMPLETION => LSP_CAP_COMPLETION,
        LSP_QUERY_SIGNATURE => LSP_CAP_SIGNATURE,
        LSP_QUERY_CODE_ACTIONS => LSP_CAP_CODE_ACTIONS,
        LSP_QUERY_FORMATTING => LSP_CAP_FORMATTING,
        _ => 0,
    }
}

static NEXT_SUB: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct PacerSignals {
    ping: bool,
    close: bool,
    state_ack: Option<u32>,
    diag_ack: Option<u32>,
}

/// Constant-space wake state for one attachment pacer. State changes
/// coalesce into one `ping`, and each one-in-flight stream can have at most
/// one useful ACK. A stalled pacer therefore retains no producer queue.
#[derive(Clone)]
pub(crate) struct PacerControl {
    inner: Arc<(Mutex<PacerSignals>, Condvar)>,
}

struct PacerWake {
    close: bool,
    state_ack: Option<u32>,
    diag_ack: Option<u32>,
}

impl PacerControl {
    fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(PacerSignals::default()), Condvar::new())),
        }
    }

    /// Coalesce any number of producer wakeups into one bit.
    pub(crate) fn ping(&self) {
        let (signals, wake) = &*self.inner;
        signals.lock().unwrap().ping = true;
        wake.notify_one();
    }

    fn ack(&self, stream: u8, update_id: u32) {
        let (signals, wake) = &*self.inner;
        let mut signals = signals.lock().unwrap();
        match stream {
            LSP_STREAM_STATE => signals.state_ack = Some(update_id),
            LSP_STREAM_DIAG => signals.diag_ack = Some(update_id),
            _ => return,
        }
        drop(signals);
        wake.notify_one();
    }

    fn close(&self) {
        let (signals, wake) = &*self.inner;
        signals.lock().unwrap().close = true;
        wake.notify_one();
    }

    fn wait(&self, timeout: Duration) -> PacerWake {
        let (signals, wake) = &*self.inner;
        let mut signals = signals.lock().unwrap();
        if !signals.close
            && !signals.ping
            && signals.state_ack.is_none()
            && signals.diag_ack.is_none()
        {
            signals = wake.wait_timeout(signals, timeout).unwrap().0;
        }
        signals.ping = false;
        PacerWake {
            close: signals.close,
            state_ack: signals.state_ack.take(),
            diag_ack: signals.diag_ack.take(),
        }
    }
}

/// One `lsp_id`: a client's attachment to a workspace.
pub struct Attachment {
    pub root: PathBuf,
    /// Current live backends, shared with the pacer; entries are
    /// replaced in place when a query respawns a stopped backend.
    backends: Arc<Mutex<Vec<Arc<Backend>>>>,
    /// `(spec, root)` parallel to `backends`, for respawn.
    specs: Vec<(ServerSpec, PathBuf)>,
    budgets: Budgets,
    sub: u64,
    ctl: PacerControl,
    /// What backends are attached with: `None` when no stream is wanted
    /// (no pacer thread runs, so nothing may hold a ping sender).
    ping: Option<PacerControl>,
    wants_diags: bool,
}

impl Attachment {
    pub(crate) fn start_native(
        root: PathBuf,
        backends: Vec<Arc<Backend>>,
        specs: Vec<(ServerSpec, PathBuf)>,
        diag_latency_ms: u16,
        sink: native::EventSink,
        budgets: &Budgets,
    ) -> Attachment {
        Self::start_inner(
            root,
            backends,
            specs,
            true,
            true,
            diag_latency_ms,
            sink,
            budgets,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_inner(
        root: PathBuf,
        backends: Vec<Arc<Backend>>,
        specs: Vec<(ServerSpec, PathBuf)>,
        wants_state: bool,
        wants_diags: bool,
        diag_latency_ms: u16,
        sink: native::EventSink,
        budgets: &Budgets,
    ) -> Attachment {
        let sub = NEXT_SUB.fetch_add(1, Ordering::Relaxed);
        let ctl = PacerControl::new();
        // Every attachment registers with every backend regardless of
        // flags, so the idle sweeper counts query-only attachments and
        // never stops a backend that is actively answering queries. A
        // stream-less attachment registers without a ping channel — no
        // pacer thread runs for it, so nothing would drain pings.
        let ping = wants_state.then(|| ctl.clone());
        for backend in &backends {
            backend.send(Cmd::Attach {
                sub,
                ping: ping.clone(),
                wants_diags,
            });
        }
        let backends = Arc::new(Mutex::new(backends));
        if wants_state {
            let latency = if diag_latency_ms == 0 {
                Duration::from_millis(500)
            } else {
                Duration::from_millis(u64::from(diag_latency_ms).clamp(1, 10_000))
            };
            let pacer = Pacer {
                backends: backends.clone(),
                ctl: ctl.clone(),
                sink,
                sub,
                wants_diags,
                latency,
                entries_max: budgets.entries_max,
                bytes_max: budgets.bytes_max,
                state_floors: HashMap::new(),
                diag_floors: HashMap::new(),
                diag_epochs: HashMap::new(),
                state_id: 0,
                diag_id: 0,
                inflight_state: None,
                inflight_diag: None,
                sent_full: false,
                next_diag_at: Instant::now(),
            };
            std::thread::Builder::new()
                .name("yas-lsp-att".into())
                .spawn(move || pacer.run())
                .expect("spawn lsp attachment thread");
        }
        Attachment {
            root,
            backends,
            specs,
            budgets: budgets.clone(),
            sub,
            ctl,
            ping,
            wants_diags,
        }
    }

    fn ack(&self, stream: u8, update_id: u32) {
        self.ctl.ack(stream, update_id);
    }

    pub fn ack_native(&self, stream: native::Stream, update_id: u32) {
        self.ack(
            match stream {
                native::Stream::State => LSP_STREAM_STATE,
                native::Stream::Diagnostics => LSP_STREAM_DIAG,
            },
            update_id,
        );
    }

    /// Route a typed native query and return one owned semantic response.
    pub fn query_native(&self, request: native::QueryRequest<'_>, sink: native::QuerySink) {
        self.route_query(
            request.nonce,
            request.kind.engine_kind(),
            request.flags,
            request.line,
            request.column,
            request.path,
            request.argument,
            sink,
        );
    }

    /// Route a query to the right backend. Immediate statuses (no backend for
    /// the language) answer on the spot. A stopped backend is respawned and
    /// the routing slot updated in place.
    #[allow(clippy::too_many_arguments)]
    fn route_query(
        &self,
        nonce: u16,
        kind: u8,
        flags: u8,
        line: u32,
        col: u32,
        document_path: Option<&Path>,
        arg: &str,
        sink: native::QuerySink,
    ) {
        let refuse = |status: u8| {
            let _ = sink(native::QueryResponse {
                nonce,
                status: native::Status::from_engine(status),
                truncated: false,
                incomplete: false,
                detail: String::new(),
                records: Vec::new(),
            });
        };
        let want = required_cap(kind);
        // Which backends are candidates for this query: any backend for
        // a workspace-wide symbol search, else the ones registered for
        // the queried file's extension.
        let path = if kind == LSP_QUERY_WS_SYMBOLS {
            None
        } else if let Some(path) = document_path.filter(|path| path.is_absolute()) {
            Some(path.to_path_buf())
        } else {
            return refuse(LSP_STATUS_INVALID);
        };
        let ext = path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let candidate = |b: &Arc<Backend>| {
            kind == LSP_QUERY_WS_SYMBOLS
                || ext
                    .as_ref()
                    .is_some_and(|e| b.extensions.iter().any(|x| x == e))
        };

        let mut backends = self.backends.lock().unwrap();
        // Route only to a backend that both applies to the query and
        // advertises the capability — never fall back to an incapable
        // one, or an unsupported request degrades to a bare "error".
        let idx = backends
            .iter()
            .position(|b| candidate(b) && b.caps() & want != 0);
        let Some(idx) = idx else {
            // No capable backend. If a candidate is still warming, the
            // capability may simply be unknown yet — say WARMING (retry)
            // rather than a misleading NOT_FOUND.
            let warming = backends
                .iter()
                .any(|b| candidate(b) && !matches!(b.phase(), LSP_PHASE_READY | LSP_PHASE_FAILED));
            drop(backends);
            return refuse(if warming {
                LSP_STATUS_WARMING
            } else {
                LSP_STATUS_NOT_FOUND
            });
        };
        // Respawn a stopped backend and update the slot in place, so a
        // later query brings the language server back (spec).
        if backends[idx].is_gone()
            && let Some((spec, root)) = self.specs.get(idx)
            && let Some(fresh) = crate::reacquire(spec, root, &self.budgets)
        {
            fresh.send(Cmd::Attach {
                sub: self.sub,
                ping: self.ping.clone(),
                wants_diags: self.wants_diags,
            });
            backends[idx] = fresh;
        }
        let backend = backends[idx].clone();
        drop(backends);
        let sent = backend.send(Cmd::Query {
            sub: self.sub,
            nonce,
            kind,
            flags,
            line,
            col,
            path,
            arg: arg.to_string(),
            sink: sink.clone(),
        });
        if !sent {
            refuse(LSP_STATUS_OTHER);
        }
    }

    pub fn cancel(&self, nonce: u16) {
        for backend in self.backends.lock().unwrap().iter() {
            backend.send(Cmd::Cancel {
                sub: self.sub,
                nonce,
            });
        }
    }

    /// Route one buffer overlay write (`text` `None` releases) to every
    /// backend registered for the path's extension. Fire-and-forget by
    /// design (docs/design/lsp.md "LSP_BUFFER"): a gone backend simply
    /// misses this write and heals on the editor's next debounced send
    /// after a query respawns it.
    pub fn buffer(&self, path: &Path, text: Option<Vec<u8>>) {
        if !path.is_absolute() {
            return;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let Some(ext) = ext else {
            return;
        };
        // Decode once at this boundary and Arc-share: every registered
        // backend gets a handle, not a copy of the buffer. A non-UTF-8
        // buffer degrades to the release the engine would have made of
        // it (docs/design/lsp.md "LSP_BUFFER").
        let text: Option<Arc<String>> =
            text.and_then(|bytes| String::from_utf8(bytes).ok().map(Arc::new));
        for backend in self.backends.lock().unwrap().iter() {
            if backend.extensions.iter().any(|x| x == &ext) {
                backend.send(Cmd::Buffer {
                    sub: self.sub,
                    path: path.to_path_buf(),
                    text: text.clone(),
                });
            }
        }
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.ctl.close();
        for backend in self.backends.lock().unwrap().iter() {
            backend.send(Cmd::Detach { sub: self.sub });
        }
    }
}

struct Pacer {
    backends: Arc<Mutex<Vec<Arc<Backend>>>>,
    ctl: PacerControl,
    sink: native::EventSink,
    /// The attachment's subscriber id, under which acked diag floors
    /// are published for tombstone pruning.
    sub: u64,
    wants_diags: bool,
    latency: Duration,
    entries_max: usize,
    bytes_max: usize,
    /// Per-backend cursors keyed by `server_ref`, so the maps survive a
    /// respawn swapping one backend for another.
    state_floors: HashMap<u16, u64>,
    diag_floors: HashMap<u16, u64>,
    /// Cache-generation cursor per backend. A cache budget reset bumps the
    /// generation and requires a FULL event even when seq floors advanced.
    diag_epochs: HashMap<u16, u64>,
    state_id: u32,
    diag_id: u32,
    inflight_state: Option<(u32, HashMap<u16, u64>)>,
    inflight_diag: Option<DiagCursor>,
    sent_full: bool,
    next_diag_at: Instant,
}

type DiagCursor = (u32, HashMap<u16, u64>, HashMap<u16, u64>);

impl Pacer {
    fn run(mut self) {
        loop {
            let wake = self.ctl.wait(Duration::from_millis(150));
            if wake.close {
                return;
            }
            if let Some(update_id) = wake.state_ack
                && let Some((id, floors)) = &self.inflight_state
                && *id == update_id
            {
                self.state_floors = floors.clone();
                self.inflight_state = None;
            }
            if let Some(update_id) = wake.diag_ack
                && let Some((id, floors, epochs)) = &self.inflight_diag
                && *id == update_id
            {
                self.diag_floors = floors.clone();
                self.diag_epochs = epochs.clone();
                self.inflight_diag = None;
                self.publish_acked_floors();
            }
            if !self.try_send_state() {
                return;
            }
            if self.wants_diags && !self.try_send_diags() {
                return;
            }
        }
    }

    /// Tombstone pruning (backend.rs `prune_diag_tombstones`) waits on
    /// every diag subscriber's acked floor; publish ours after each
    /// diag ack.
    fn publish_acked_floors(&self) {
        let backends = self.backends.lock().unwrap().clone();
        for backend in backends {
            if let Some(floor) = self.diag_floors.get(&backend.server_ref) {
                backend
                    .shared
                    .diag_acked
                    .lock()
                    .unwrap()
                    .insert(self.sub, *floor);
            }
        }
    }

    fn try_send_state(&mut self) -> bool {
        if self.inflight_state.is_some() {
            return true;
        }
        let backends = self.backends.lock().unwrap().clone();
        // Current per-backend sequences, live backends only — matching
        // the floor snapshot below. A stopped backend drops out of both
        // maps, so its departure surfaces once as a floor key with no
        // seq (the second `unchanged` clause) and then goes quiet
        // instead of re-triggering a send on every ack.
        let seqs: HashMap<u16, u64> = backends
            .iter()
            .filter(|b| !b.is_gone())
            .map(|b| (b.server_ref, b.shared.state_seq.load(Ordering::Relaxed)))
            .collect();
        // Nothing to send only if every current backend is at or below
        // its floor AND no backend the client still knows has vanished.
        let unchanged = seqs
            .iter()
            .all(|(r, seq)| self.state_floors.get(r).is_some_and(|f| seq <= f))
            && self.state_floors.keys().all(|r| seqs.contains_key(r));
        if unchanged {
            return true;
        }
        // Whole snapshot: one SERVER record per live backend. A stopped
        // (gone) backend is omitted, so its record disappears.
        let mut native_servers = Vec::new();
        for backend in &backends {
            if backend.is_gone() {
                continue;
            }
            let info = backend.shared.info.lock().unwrap().clone();
            native_servers.push(native::Server {
                server_ref: backend.server_ref,
                phase: info.phase,
                progress_pct: info.progress_pct,
                capabilities: native::Capabilities::from_engine(info.caps),
                epoch: info.epoch,
                refused_edits: info.refused_edits,
                rss_bytes: backend.rss_bytes(),
                id: backend.id.clone(),
                message: info.msg.clone(),
                root: None,
            });
        }
        self.state_id = self.state_id.wrapping_add(1);
        if !(self.sink)(native::Event::State {
            update_id: self.state_id,
            servers: native_servers,
        }) {
            return false;
        }
        // The floor snapshot covers only live backends, so a vanished
        // one drops out of the cursor map too.
        let floors: HashMap<u16, u64> = backends
            .iter()
            .filter(|b| !b.is_gone())
            .map(|b| (b.server_ref, b.shared.state_seq.load(Ordering::Relaxed)))
            .collect();
        self.inflight_state = Some((self.state_id, floors));
        true
    }

    fn try_send_diags(&mut self) -> bool {
        if self.inflight_diag.is_some() || Instant::now() < self.next_diag_at {
            return true;
        }
        let backends = self.backends.lock().unwrap().clone();
        let epochs: HashMap<u16, u64> = backends
            .iter()
            .filter(|backend| !backend.is_gone())
            .map(|backend| {
                (
                    backend.server_ref,
                    backend.shared.diag_epoch.load(Ordering::Relaxed),
                )
            })
            .collect();
        // The first update after subscribe is a FULL cache replay (the
        // drop-everything reset). A cache admission overflow also changes
        // its generation and forces the same reset, so evicted paths never
        // remain stale at the subscriber. Afterwards, incrementals start
        // from the floor. A FULL too large for one message is split: the first
        // chunk carries the FULL flag and advances the floor, so the
        // remainder flows as ordinary incrementals under the same
        // one-in-flight pacing — the payload never trips the receiver's
        // MAX_DECOMPRESSED guard.
        let full = !self.sent_full || epochs != self.diag_epochs;
        let mut native_files = Vec::new();
        let mut bytes_used = 0usize;
        let mut new_floors = self.diag_floors.clone();
        let mut entries = 0usize;
        let mut any = false;
        'outer: for backend in &backends {
            let floor = *self.diag_floors.get(&backend.server_ref).unwrap_or(&0);
            // The seq atomic is the cheap gate: skip the map lock (and
            // the full scan behind it) when this backend published
            // nothing past our floor.
            if !full && backend.shared.diag_seq.load(Ordering::Relaxed) <= floor {
                continue;
            }
            // Snapshot the changed entries and encode after dropping the
            // lock, so record encoding never stalls the engine's
            // publishes.
            let mut changed: Vec<(PathBuf, crate::backend::FileDiags)> = {
                let diags = backend.shared.diags.lock().unwrap();
                diags
                    .iter()
                    .filter(|(_, f)| full || f.seq > floor)
                    .map(|(p, f)| (p.clone(), f.clone()))
                    .collect()
            };
            // Files in seq order so a chunk boundary leaves everything
            // unsent strictly above the new floor.
            changed.sort_by_key(|(_, f)| f.seq);
            for (path, file) in &changed {
                // In a FULL replay, absent files are unknown; empty
                // tombstones carry no information.
                if full && file.is_empty() {
                    let e = new_floors.entry(backend.server_ref).or_insert(0);
                    *e = (*e).max(file.seq);
                    continue;
                }
                if entries >= self.entries_max || bytes_used >= self.bytes_max {
                    break 'outer;
                }
                // One file's diagnostics are bounded too, not only the
                // number of files. The checks above run *between* files, so
                // a single pathological one — a generated bundle, or one
                // syntax error cascading into hundreds of thousands of
                // diagnostics — appended all of them: the payload could pass
                // the client's decompression guard, at which point the
                // client refuses the whole update, never acks, and the
                // one-in-flight pacer never clears, so that client receives
                // no diagnostics again for the life of the attachment. The
                // count also has to fit `n`, which is a `u16`.
                //
                // Built into a scratch buffer so the count in the FILE
                // record is the number of DIAG records that actually
                // follow it, rather than a promise made before the budget
                // was consulted.
                let file_size = 24usize.saturating_add(path.as_os_str().len());
                if bytes_used.saturating_add(file_size) > self.bytes_max {
                    break 'outer;
                }
                bytes_used += file_size;
                // Cold entries decode here, off the cache lock.
                let diagnostics = take_diagnostics(file.diags(), &mut bytes_used, self.bytes_max);
                native_files.push(native::FileDiagnostics {
                    path: path.clone(),
                    hash: file.hash,
                    diagnostics,
                });
                let e = new_floors.entry(backend.server_ref).or_insert(0);
                *e = (*e).max(file.seq);
                entries += 1;
                any = true;
            }
        }
        // Drop cursor entries for backends that are gone.
        new_floors.retain(|r, _| backends.iter().any(|b| b.server_ref == *r));
        if !any && !full {
            return true;
        }
        self.diag_id = self.diag_id.wrapping_add(1);
        if !(self.sink)(native::Event::Diagnostics {
            update_id: self.diag_id,
            full,
            files: native_files,
        }) {
            return false;
        }
        // The reset has now been delivered (possibly as the first of
        // several chunks); the rest flows as incrementals from the
        // advanced floor.
        self.sent_full = true;
        self.next_diag_at = Instant::now() + self.latency;
        self.inflight_diag = Some((self.diag_id, new_floors, epochs));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::CachedDiagnostic;

    fn diag(msg: &str) -> CachedDiagnostic {
        CachedDiagnostic {
            severity: 1,
            flags: 0,
            line: 0,
            col: 0,
            end_line: 0,
            end_col: 1,
            code: String::new(),
            source: "test".into(),
            msg: msg.to_string(),
        }
    }

    /// A single file cannot exceed either its count or byte budget.
    #[test]
    fn one_file_cannot_outgrow_the_payload_budget() {
        let diags: Vec<CachedDiagnostic> =
            (0..500).map(|i| diag(&format!("problem {i}"))).collect();

        let mut used = 0;
        let all = take_diagnostics(diags.clone(), &mut used, 1 << 20);
        assert_eq!(all.len(), diags.len());
        assert!(used <= 1 << 20);

        let mut used = 0;
        let partial = take_diagnostics(diags.clone(), &mut used, 512);
        assert!(
            !partial.is_empty() && partial.len() < diags.len(),
            "expected a partial file, got {}",
            partial.len()
        );
        assert!(used <= 512);

        let mut used = 0;
        assert!(take_diagnostics(diags, &mut used, 0).is_empty());
        assert_eq!(used, 0);
    }

    #[test]
    fn pacer_controls_coalesce_without_a_producer_queue() {
        let control = PacerControl::new();
        for _ in 0..10_000 {
            control.ping();
        }
        control.ack(LSP_STREAM_STATE, 7);
        control.ack(LSP_STREAM_STATE, 8);
        control.ack(LSP_STREAM_DIAG, 9);
        {
            let signals = control.inner.0.lock().unwrap();
            assert!(signals.ping);
            assert_eq!(signals.state_ack, Some(8));
            assert_eq!(signals.diag_ack, Some(9));
        }
        let wake = control.wait(Duration::ZERO);
        assert!(!wake.close);
        assert_eq!(wake.state_ack, Some(8));
        assert_eq!(wake.diag_ack, Some(9));
        control.close();
        assert!(control.wait(Duration::ZERO).close);
    }
}
