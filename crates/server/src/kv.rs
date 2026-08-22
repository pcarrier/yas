//! Server KV store (docs/design/kv.md): a host-local key→value map with
//! CAS puts/deletes, prefix-watch subscriptions, and a redb write-behind.
//!
//! The in-memory map is the source of truth for CAS and watches; redb is
//! its write-behind on a dedicated writer thread, fed in mutation order
//! from under the store lock. Jobs queued behind one another batch into a
//! single transaction, so a durable mutation's fsynced commit covers every
//! mutation ordered before it and crash durability stays prefix-consistent
//! with the mutation order. Non-durable mutations complete as soon as the
//! in-memory commit lands; durable mutations wait for the writer to confirm.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use redb::ReadableTable;
#[cfg(test)]
use redb::TableHandle;
use tokio::sync::{broadcast, oneshot};

/// The standalone YAS table is byte-keyed, so keys never pass through UTF-8
/// or a lossy conversion.
const TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("kv_v1");
const OPERATION_TABLE: redb::TableDefinition<&[u8], &[u8]> =
    redb::TableDefinition::new("kv_operations_v1");
const NATIVE_CHANGE_QUEUE: usize = 1024;
/// Process-global write-behind admission. This bounds both queued jobs and
/// durable Request waiters, because the permit lives through the redb commit.
pub(crate) const MAX_PENDING_WRITES: usize = 64;
/// In addition to the mutation that established every currently-live entry,
/// keep this many most-recent settlements. This covers deletes, conflicts, and
/// overwritten values without letting durable operation rows grow forever.
pub(crate) const MAX_RECENT_OPERATION_REPLAYS: usize = 1024;
const OPERATION_RECORD_MAGIC: &[u8; 4] = b"YKO1";
const OPERATION_RECORD_HEADER_BYTES: usize = 56;
const OPERATION_RESULT_ENCODED_BYTES: usize = 58;
const OPERATION_PRUNE_BATCH: usize = 1024;
const STAGE_WITNESS_ENCODED_BYTES: usize = 50;
const MAX_STAGE_WITNESSES_PER_OPERATION: usize =
    yas_wire::kv::Limits::HARD.max_stages_per_session as usize;
#[cfg(test)]
const MAX_RETAINED_STAGE_WITNESSES: usize = (yas_wire::kv::Limits::HARD.max_entries as usize
    + MAX_RECENT_OPERATION_REPLAYS)
    * MAX_STAGE_WITNESSES_PER_OPERATION;
#[cfg(test)]
const MAX_RETAINED_STAGE_WITNESS_BYTES: usize =
    MAX_RETAINED_STAGE_WITNESSES * STAGE_WITNESS_ENCODED_BYTES;

fn env_budget(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// Budgets (docs/design/kv.md § Budgets), read once: these sit on the
// per-message hot path, where a getenv per PUT is pure overhead.
fn value_max() -> u64 {
    static V: LazyLock<u64> = LazyLock::new(|| env_budget("YAS_KV_VALUE_MAX", 4 * 1024 * 1024));
    *V
}
fn total_max() -> u64 {
    static V: LazyLock<u64> = LazyLock::new(|| env_budget("YAS_KV_TOTAL_MAX", 256 * 1024 * 1024));
    *V
}
fn max_entries() -> u64 {
    static V: LazyLock<u64> = LazyLock::new(|| env_budget("YAS_KV_MAX_ENTRIES", 16384));
    *V
}

fn writer_queue_max_bytes() -> usize {
    let hard = yas_wire::kv::Limits::HARD;
    let values = total_max().min(hard.max_store_bytes);
    let metadata = u64::from(hard.max_key_bytes)
        .saturating_mul(u64::try_from(yas_wire::kv::MAX_BATCH_ITEMS).unwrap_or(u64::MAX))
        .saturating_add(
            (OPERATION_RECORD_HEADER_BYTES as u64)
                .saturating_add((OPERATION_RESULT_ENCODED_BYTES as u64).saturating_mul(
                    u64::try_from(yas_wire::kv::MAX_BATCH_ITEMS).unwrap_or(u64::MAX),
                ))
                .saturating_add(
                    u64::try_from(MAX_STAGE_WITNESSES_PER_OPERATION)
                        .unwrap_or(u64::MAX)
                        .saturating_mul(STAGE_WITNESS_ENCODED_BYTES as u64),
                ),
        );
    usize::try_from(values.saturating_add(metadata)).unwrap_or(usize::MAX)
}
fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// `$YAS_KV_PATH`, else the platform state path (docs/design/kv.md
/// § Storage) followed by `yas/instances/NAME/kv.redb`. `None` means no
/// resolvable home.
fn resolve_db_path(name: &crate::ServerName) -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("YAS_KV_PATH") {
        return Some(std::path::PathBuf::from(p));
    }
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        });
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    base.map(|base| crate::server_name::server_path(&base, name, "kv.redb"))
}

static DB_PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();

#[cfg(test)]
fn ensure_test_db_path() {
    static CONFIGURED: OnceLock<()> = OnceLock::new();
    CONFIGURED.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("yas-kv-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = DB_PATH.set(Some(dir.join("kv.redb")));
    });
}

/// Freeze the database path before the process-global store is first opened.
/// A yas process owns one server and one KV store; separate named instances
/// run in separate processes.
#[cfg(not(test))]
pub(crate) fn configure_server_name(name: &crate::ServerName) {
    let _ = DB_PATH.set(resolve_db_path(name));
}

#[cfg(test)]
pub(crate) fn configure_server_name(_name: &crate::ServerName) {
    ensure_test_db_path();
}

fn db_path() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    ensure_test_db_path();
    DB_PATH
        .get_or_init(|| resolve_db_path(&crate::ServerName::default()))
        .clone()
}

#[derive(Clone)]
struct Entry {
    value: Arc<Vec<u8>>,
    hash: [u8; 32],
    mtime_ns: u64,
    modification_revision: u64,
}

/// One mutation for the writer thread. Enqueued under the store lock, so
/// the channel order is the mutation order.
struct PersistedMutation {
    key: Vec<u8>,
    /// `None` = delete; the `Arc` shares bytes with the live entry.
    value: Option<(Arc<Vec<u8>>, u64, u64)>,
}

#[derive(Default)]
struct WriterUsage {
    jobs: usize,
    bytes: usize,
}

struct WriterBudget {
    usage: Mutex<WriterUsage>,
    max_jobs: usize,
    max_bytes: usize,
}

impl WriterBudget {
    fn new(max_jobs: usize, max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            usage: Mutex::new(WriterUsage::default()),
            max_jobs,
            max_bytes,
        })
    }

    fn try_reserve(self: &Arc<Self>, bytes: usize) -> Option<WriterPermit> {
        let mut usage = self.usage.lock().unwrap();
        let next_jobs = usage.jobs.checked_add(1)?;
        let next_bytes = usage.bytes.checked_add(bytes)?;
        if next_jobs > self.max_jobs || next_bytes > self.max_bytes {
            return None;
        }
        usage.jobs = next_jobs;
        usage.bytes = next_bytes;
        Some(WriterPermit {
            budget: Arc::clone(self),
            bytes,
        })
    }
}

struct WriterPermit {
    budget: Arc<WriterBudget>,
    bytes: usize,
}

impl Drop for WriterPermit {
    fn drop(&mut self) {
        let mut usage = self.budget.usage.lock().unwrap();
        usage.jobs = usage.jobs.saturating_sub(1);
        usage.bytes = usage.bytes.saturating_sub(self.bytes);
    }
}

#[derive(Clone)]
struct WriterHandle {
    sender: std::sync::mpsc::SyncSender<WriteJob>,
    budget: Arc<WriterBudget>,
}

struct WriteJob {
    /// One job is one logical store transaction.  The writer may commit
    /// several ordered jobs together, but never splits one native BATCH.
    mutations: Vec<PersistedMutation>,
    operation: Option<([u8; 16], Arc<Vec<u8>>)>,
    evicted_operations: Vec<[u8; 16]>,
    durable: bool,
    native_reply: Option<oneshot::Sender<bool>>,
    /// Admission is released only after the writer has committed or failed the
    /// complete ordered batch, not when a client cancels its durable wait.
    _permit: WriterPermit,
}

/// Byte-exact store view used by the YAS KV family.
#[derive(Clone, Debug)]
pub(crate) struct NativeEntry {
    pub key: Vec<u8>,
    pub value: Arc<Vec<u8>>,
    pub content_hash: [u8; 32],
    pub mtime_ns: u64,
    pub modification_revision: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum NativeChangeRecord {
    Upsert {
        entry: NativeEntry,
        added: bool,
    },
    Remove {
        key: Vec<u8>,
        modification_revision: u64,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct NativeChange {
    pub from_revision: u64,
    pub to_revision: u64,
    pub records: Vec<NativeChangeRecord>,
}

pub(crate) struct NativeWatch {
    pub revision: u64,
    pub entries: Vec<NativeEntry>,
    pub changes: broadcast::Receiver<NativeChange>,
}

#[derive(Clone, Debug)]
pub(crate) enum NativeMutation {
    Put {
        key: Vec<u8>,
        precondition: yas_wire::kv::Precondition,
        value: Arc<Vec<u8>>,
        content_hash: [u8; 32],
    },
    Delete {
        key: Vec<u8>,
        precondition: yas_wire::kv::Precondition,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeMutationResult {
    pub status: yas_wire::core::Status,
    pub modification_revision: u64,
    pub mtime_ns: u64,
    pub content_hash: [u8; 32],
    pub byte_len: u64,
}

#[derive(Clone, Debug)]
struct NativeDedupResult {
    fingerprint: [u8; 32],
    store_revision: u64,
    settlement_sequence: u64,
    results: Vec<NativeMutationResult>,
    stage_witnesses: Vec<NativeStageWitness>,
}

/// Canonical metadata for one staged value used by a retained operation.
/// The ordinal is among staged values (not all mutations). The original
/// handle allows an exact retry after its bounded session tombstone expires;
/// a newly staged handle instead supplies its own live metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeStageWitness {
    pub(crate) ordinal: u16,
    pub(crate) original_handle: u64,
    pub(crate) byte_len: u64,
    pub(crate) content_hash: [u8; 32],
}

struct RetainedOperationSet {
    values: BTreeMap<[u8; 16], NativeDedupResult>,
    order: VecDeque<[u8; 16]>,
    maximum_store_revision: u64,
    maximum_sequence: u64,
}

struct OperationReplayPlan {
    values: BTreeMap<[u8; 16], NativeDedupResult>,
    order: VecDeque<[u8; 16]>,
    evicted: Vec<[u8; 16]>,
}

pub(crate) struct NativeCommit {
    pub store_revision: u64,
    pub results: Vec<NativeMutationResult>,
    /// Present for a newly-admitted durable mutation.  A replay returns the
    /// already-authoritative result and therefore needs no second fsync.
    pub durable: Option<oneshot::Receiver<bool>>,
    pub persistence_failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeMutationError {
    Invalid,
    ResourceExhausted,
    OperationConflict,
}

struct Store {
    entries: BTreeMap<Vec<u8>, Entry>,
    native_changes: broadcast::Sender<NativeChange>,
    operations: BTreeMap<[u8; 16], NativeDedupResult>,
    operation_order: VecDeque<[u8; 16]>,
    next_operation_sequence: u64,
    /// Ordered queue to the writer thread; `None` = memory-only store.
    writer: Option<WriterHandle>,
    total_bytes: u64,
    revision: u64,
}

impl Store {
    /// Open (or create) the database, load every entry — computing hashes
    /// at memory speed (hashes are not persisted; docs/design/kv.md
    /// § Storage) — and hand the database to the writer thread. Any
    /// failure degrades to a memory-only store with a warning — the native
    /// KV contract holds, durability does not.
    fn open() -> Store {
        let (native_changes, _) = broadcast::channel(NATIVE_CHANGE_QUEUE);
        let mut store = Store {
            entries: BTreeMap::new(),
            native_changes,
            operations: BTreeMap::new(),
            operation_order: VecDeque::new(),
            next_operation_sequence: 1,
            writer: None,
            total_bytes: 0,
            // Revisions are never zero, including for an empty store.
            revision: 1,
        };
        let Some(path) = db_path() else {
            eprintln!("kv: no resolvable state dir; store is memory-only");
            return store;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        match redb::Database::create(&path) {
            Ok(db) => {
                // 0600, as docs/design/kv.md states. `Database::create` uses
                // the umask (0644 by default), and the 0700 parent is only
                // defense in depth if the parent is ours — it may predate
                // this process with looser modes.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
                #[allow(clippy::result_large_err)] // redb::Error is large and local
                let load = || -> Result<_, redb::Error> {
                    let txn = db.begin_read()?;
                    let mut rows = Vec::new();
                    match txn.open_table(TABLE) {
                        Ok(table) => {
                            for item in table.iter()? {
                                let (k, v) = item?;
                                rows.push((k.value().to_vec(), v.value().to_vec()));
                            }
                        }
                        Err(redb::TableError::TableDoesNotExist(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                    Ok(rows)
                };
                match load() {
                    Ok(rows) => {
                        for (key, raw) in rows {
                            if raw.len() < 16 {
                                continue;
                            }
                            let mtime_ns = u64::from_le_bytes(raw[0..8].try_into().unwrap());
                            let modification_revision =
                                u64::from_le_bytes(raw[8..16].try_into().unwrap()).max(1);
                            let value = raw[16..].to_vec();
                            store.revision = store.revision.max(modification_revision);
                            store.total_bytes += (key.len() + value.len()) as u64;
                            let hash = *blake3::hash(&value).as_bytes();
                            store.entries.insert(
                                key.clone(),
                                Entry {
                                    hash,
                                    value: Arc::new(value),
                                    mtime_ns,
                                    modification_revision,
                                },
                            );
                        }
                        let live_revisions = store
                            .entries
                            .values()
                            .map(|entry| entry.modification_revision)
                            .collect::<BTreeSet<_>>();
                        match load_operation_replays(&db, &live_revisions) {
                            Ok(retained) => {
                                store.operations = retained.values;
                                store.operation_order = retained.order;
                                store.revision =
                                    store.revision.max(retained.maximum_store_revision);
                                store.next_operation_sequence =
                                    retained.maximum_sequence.saturating_add(1);
                                if let Err(error) =
                                    prune_persisted_operation_replays(&db, &store.operations)
                                {
                                    eprintln!(
                                        "kv: operation replay pruning failed ({error}); continuing"
                                    );
                                }
                            }
                            Err(error) => {
                                eprintln!(
                                    "kv: operation replay load failed ({error}); continuing without replays"
                                );
                            }
                        }
                        let (tx, rx) =
                            std::sync::mpsc::sync_channel::<WriteJob>(MAX_PENDING_WRITES);
                        let budget =
                            WriterBudget::new(MAX_PENDING_WRITES, writer_queue_max_bytes());
                        let spawned = std::thread::Builder::new()
                            .name("kv-writer".into())
                            .spawn(move || writer_loop(db, rx));
                        match spawned {
                            Ok(_) => {
                                store.writer = Some(WriterHandle { sender: tx, budget });
                            }
                            Err(e) => {
                                eprintln!("kv: writer thread failed ({e}); store is memory-only");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("kv: load failed ({e}); store is memory-only");
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "kv: cannot open {} ({e}); store is memory-only",
                    path.display()
                );
            }
        }
        store
    }

    fn reserve_write(&self, bytes: usize) -> Result<Option<WriterPermit>, NativeMutationError> {
        self.writer
            .as_ref()
            .map(|writer| {
                writer
                    .budget
                    .try_reserve(bytes)
                    .ok_or(NativeMutationError::ResourceExhausted)
            })
            .transpose()
    }

    fn enqueue_job(&mut self, job: WriteJob) -> bool {
        let Some(writer) = &self.writer else {
            return false;
        };
        match writer.sender.try_send(job) {
            Ok(()) => true,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                self.writer = None;
                eprintln!("kv: writer thread gone; store is memory-only");
                false
            }
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                // A job owns one of exactly `MAX_PENDING_WRITES` permits before
                // it reaches the equally-sized channel, so Full is impossible
                // unless the admission invariant is broken. Disable the writer
                // rather than allow a later job to create a persistence gap.
                self.writer = None;
                eprintln!("kv: writer admission invariant failed; store is memory-only");
                false
            }
        }
    }
}

fn encode_native_dedup(result: &NativeDedupResult) -> Vec<u8> {
    let mut raw = Vec::with_capacity(
        OPERATION_RECORD_HEADER_BYTES
            + result.results.len() * OPERATION_RESULT_ENCODED_BYTES
            + result.stage_witnesses.len() * STAGE_WITNESS_ENCODED_BYTES,
    );
    raw.extend_from_slice(OPERATION_RECORD_MAGIC);
    raw.extend_from_slice(&result.settlement_sequence.to_le_bytes());
    raw.extend_from_slice(&result.fingerprint);
    raw.extend_from_slice(&result.store_revision.to_le_bytes());
    raw.extend_from_slice(&(result.results.len() as u16).to_le_bytes());
    raw.extend_from_slice(&(result.stage_witnesses.len() as u16).to_le_bytes());
    for item in &result.results {
        raw.extend_from_slice(&item.status.code().to_le_bytes());
        raw.extend_from_slice(&item.modification_revision.to_le_bytes());
        raw.extend_from_slice(&item.mtime_ns.to_le_bytes());
        raw.extend_from_slice(&item.content_hash);
        raw.extend_from_slice(&item.byte_len.to_le_bytes());
    }
    for witness in &result.stage_witnesses {
        raw.extend_from_slice(&witness.ordinal.to_le_bytes());
        raw.extend_from_slice(&witness.original_handle.to_le_bytes());
        raw.extend_from_slice(&witness.byte_len.to_le_bytes());
        raw.extend_from_slice(&witness.content_hash);
    }
    raw
}

fn decode_native_dedup(raw: &[u8]) -> Option<NativeDedupResult> {
    if raw.len() < OPERATION_RECORD_HEADER_BYTES || !raw.starts_with(OPERATION_RECORD_MAGIC) {
        return None;
    }
    let settlement_sequence = u64::from_le_bytes(raw[4..12].try_into().ok()?);
    let fingerprint = raw[12..44].try_into().ok()?;
    let store_revision = u64::from_le_bytes(raw[44..52].try_into().ok()?);
    let count = u16::from_le_bytes(raw[52..54].try_into().ok()?) as usize;
    let witness_count = u16::from_le_bytes(raw[54..56].try_into().ok()?) as usize;
    if count > yas_wire::kv::MAX_BATCH_ITEMS || witness_count > MAX_STAGE_WITNESSES_PER_OPERATION {
        return None;
    }
    let result_bytes = count.checked_mul(OPERATION_RESULT_ENCODED_BYTES)?;
    let witness_bytes = witness_count.checked_mul(STAGE_WITNESS_ENCODED_BYTES)?;
    let expected_len = OPERATION_RECORD_HEADER_BYTES
        .checked_add(result_bytes)?
        .checked_add(witness_bytes)?;
    if raw.len() != expected_len || settlement_sequence == 0 || store_revision == 0 {
        return None;
    }
    let mut results = Vec::with_capacity(count);
    let mut offset = OPERATION_RECORD_HEADER_BYTES;
    for _ in 0..count {
        let status = yas_wire::core::Status::from_code(u16::from_le_bytes(
            raw[offset..offset + 2].try_into().ok()?,
        ));
        offset += 2;
        let modification_revision = u64::from_le_bytes(raw[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let mtime_ns = u64::from_le_bytes(raw[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let content_hash = raw[offset..offset + 32].try_into().ok()?;
        offset += 32;
        let byte_len = u64::from_le_bytes(raw[offset..offset + 8].try_into().ok()?);
        offset += 8;
        results.push(NativeMutationResult {
            status,
            modification_revision,
            mtime_ns,
            content_hash,
            byte_len,
        });
    }
    let mut stage_witnesses = Vec::with_capacity(witness_count);
    for ordinal in 0..witness_count {
        let encoded_ordinal = u16::from_le_bytes(raw[offset..offset + 2].try_into().ok()?);
        offset += 2;
        if usize::from(encoded_ordinal) != ordinal {
            return None;
        }
        let original_handle = u64::from_le_bytes(raw[offset..offset + 8].try_into().ok()?);
        offset += 8;
        if original_handle == 0 {
            return None;
        }
        let byte_len = u64::from_le_bytes(raw[offset..offset + 8].try_into().ok()?);
        offset += 8;
        if byte_len > yas_wire::kv::Limits::HARD.max_value_bytes {
            return None;
        }
        let content_hash = raw[offset..offset + 32].try_into().ok()?;
        offset += 32;
        stage_witnesses.push(NativeStageWitness {
            ordinal: encoded_ordinal,
            original_handle,
            byte_len,
            content_hash,
        });
    }
    debug_assert_eq!(offset, raw.len());
    Some(NativeDedupResult {
        fingerprint,
        store_revision,
        settlement_sequence,
        results,
        stage_witnesses,
    })
}

fn operation_pins_live_entry(result: &NativeDedupResult, live_revisions: &BTreeSet<u64>) -> bool {
    live_revisions.contains(&result.store_revision)
        && result
            .results
            .iter()
            .any(|item| item.status == yas_wire::core::Status::Ok)
}

#[cfg(test)]
fn retained_operations(
    rows: impl IntoIterator<Item = ([u8; 16], NativeDedupResult)>,
    live_revisions: &BTreeSet<u64>,
) -> RetainedOperationSet {
    let mut pinned = BTreeMap::new();
    let mut recent = BTreeMap::<(u64, [u8; 16]), NativeDedupResult>::new();
    let mut maximum_store_revision = 1;
    let mut maximum_sequence = 0;
    for (operation_id, result) in rows {
        retain_operation_candidate(
            operation_id,
            result,
            live_revisions,
            &mut pinned,
            &mut recent,
            &mut maximum_store_revision,
            &mut maximum_sequence,
        );
    }
    finish_retained_operations(pinned, recent, maximum_store_revision, maximum_sequence)
}

#[allow(clippy::too_many_arguments)]
fn retain_operation_candidate(
    operation_id: [u8; 16],
    result: NativeDedupResult,
    live_revisions: &BTreeSet<u64>,
    pinned: &mut BTreeMap<u64, ([u8; 16], NativeDedupResult)>,
    recent: &mut BTreeMap<(u64, [u8; 16]), NativeDedupResult>,
    maximum_store_revision: &mut u64,
    maximum_sequence: &mut u64,
) {
    *maximum_store_revision = (*maximum_store_revision).max(result.store_revision);
    *maximum_sequence = (*maximum_sequence).max(result.settlement_sequence);
    if operation_pins_live_entry(&result, live_revisions) {
        let replace = pinned
            .get(&result.store_revision)
            .is_none_or(|(current_id, current)| {
                (result.settlement_sequence, operation_id)
                    > (current.settlement_sequence, *current_id)
            });
        if replace {
            pinned.insert(result.store_revision, (operation_id, result.clone()));
        }
    }
    recent.insert((result.settlement_sequence, operation_id), result);
    if recent.len() > MAX_RECENT_OPERATION_REPLAYS {
        recent.pop_first();
    }
}

fn finish_retained_operations(
    pinned: BTreeMap<u64, ([u8; 16], NativeDedupResult)>,
    recent: BTreeMap<(u64, [u8; 16]), NativeDedupResult>,
    maximum_store_revision: u64,
    maximum_sequence: u64,
) -> RetainedOperationSet {
    let mut pinned = pinned
        .into_values()
        .collect::<BTreeMap<[u8; 16], NativeDedupResult>>();
    for ((_, operation_id), result) in recent {
        pinned.entry(operation_id).or_insert(result);
    }
    let mut ordered = pinned
        .iter()
        .map(|(operation_id, result)| (result.settlement_sequence, *operation_id))
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    RetainedOperationSet {
        values: pinned,
        order: ordered
            .into_iter()
            .map(|(_, operation_id)| operation_id)
            .collect(),
        maximum_store_revision,
        maximum_sequence,
    }
}

fn prune_operation_replays(
    entries: &BTreeMap<Vec<u8>, Entry>,
    operations: &mut BTreeMap<[u8; 16], NativeDedupResult>,
    order: &mut VecDeque<[u8; 16]>,
) -> Vec<[u8; 16]> {
    let live_revisions = entries
        .values()
        .map(|entry| entry.modification_revision)
        .collect::<BTreeSet<_>>();
    let recent = order
        .iter()
        .rev()
        .take(MAX_RECENT_OPERATION_REPLAYS)
        .copied()
        .collect::<HashSet<_>>();
    let mut pinned_by_revision = BTreeMap::<u64, [u8; 16]>::new();
    for operation_id in order.iter() {
        if let Some(result) = operations.get(operation_id)
            && operation_pins_live_entry(result, &live_revisions)
        {
            pinned_by_revision.insert(result.store_revision, *operation_id);
        }
    }
    let pinned = pinned_by_revision.into_values().collect::<HashSet<_>>();
    let evicted = order
        .iter()
        .filter_map(|operation_id| {
            (!pinned.contains(operation_id) && !recent.contains(operation_id))
                .then_some(*operation_id)
        })
        .collect::<HashSet<_>>();
    order.retain(|operation_id| !evicted.contains(operation_id));
    for operation_id in &evicted {
        operations.remove(operation_id);
    }
    let mut evicted = evicted.into_iter().collect::<Vec<_>>();
    evicted.sort_unstable();
    evicted
}

fn plan_operation_replay(
    entries: &BTreeMap<Vec<u8>, Entry>,
    operations: &BTreeMap<[u8; 16], NativeDedupResult>,
    order: &VecDeque<[u8; 16]>,
    operation_id: [u8; 16],
    result: NativeDedupResult,
) -> OperationReplayPlan {
    let mut operations = operations.clone();
    let mut order = order.clone();
    operations.insert(operation_id, result);
    order.push_back(operation_id);
    let evicted = prune_operation_replays(entries, &mut operations, &mut order);
    OperationReplayPlan {
        values: operations,
        order,
        evicted,
    }
}

fn write_job_bytes(
    mutations: &[PersistedMutation],
    encoded_operation_bytes: usize,
    evicted_operations: usize,
) -> Option<usize> {
    let mutation_bytes = mutations.iter().try_fold(0usize, |total, mutation| {
        total.checked_add(mutation.key.len())?.checked_add(
            mutation
                .value
                .as_ref()
                .map_or(0, |(value, _, _)| value.len()),
        )
    })?;
    mutation_bytes
        .checked_add(encoded_operation_bytes)?
        .checked_add(evicted_operations.checked_mul(16)?)
}

#[allow(clippy::result_large_err)]
fn load_operation_replays(
    db: &redb::Database,
    live_revisions: &BTreeSet<u64>,
) -> Result<RetainedOperationSet, redb::Error> {
    let txn = db.begin_read()?;
    let table = match txn.open_table(OPERATION_TABLE) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => {
            return Ok(RetainedOperationSet {
                values: BTreeMap::new(),
                order: VecDeque::new(),
                maximum_store_revision: 1,
                maximum_sequence: 0,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut pinned = BTreeMap::new();
    let mut recent = BTreeMap::new();
    let mut maximum_store_revision = 1;
    let mut maximum_sequence = 0;
    for item in table.iter()? {
        let (key, value) = item?;
        let Ok(operation_id) = <[u8; 16]>::try_from(key.value()) else {
            continue;
        };
        if let Some(result) = decode_native_dedup(value.value()) {
            retain_operation_candidate(
                operation_id,
                result,
                live_revisions,
                &mut pinned,
                &mut recent,
                &mut maximum_store_revision,
                &mut maximum_sequence,
            );
        }
    }
    Ok(finish_retained_operations(
        pinned,
        recent,
        maximum_store_revision,
        maximum_sequence,
    ))
}

#[allow(clippy::result_large_err)]
fn prune_persisted_operation_replays(
    db: &redb::Database,
    retained: &BTreeMap<[u8; 16], NativeDedupResult>,
) -> Result<(), redb::Error> {
    loop {
        let delete = {
            let txn = db.begin_read()?;
            let table = match txn.open_table(OPERATION_TABLE) {
                Ok(table) => table,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            let mut delete = Vec::new();
            for item in table.iter()? {
                let (key, _) = item?;
                let raw = key.value();
                let keep = <[u8; 16]>::try_from(raw)
                    .ok()
                    .is_some_and(|operation_id| retained.contains_key(&operation_id));
                if !keep {
                    delete.push(raw.to_vec());
                    if delete.len() == OPERATION_PRUNE_BATCH {
                        break;
                    }
                }
            }
            delete
        };
        if delete.is_empty() {
            return Ok(());
        }
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(OPERATION_TABLE)?;
            for operation_id in &delete {
                table.remove(operation_id.as_slice())?;
            }
        }
        txn.commit()?;
    }
}

/// The writer thread: drains queued mutations into one transaction per
/// wakeup, `Immediate` (fsynced) when any job in the batch is `DURABLE`,
/// `Eventual` otherwise — so a `DURABLE` commit also hardens everything
/// ordered before it. Failures degrade (memory truth holds, durability is
/// lost) and are reported to the native mutation waiter.
fn writer_loop(db: redb::Database, rx: std::sync::mpsc::Receiver<WriteJob>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(job) = rx.try_recv() {
            batch.push(job);
        }
        let durable = batch.iter().any(|j| j.durable);
        #[allow(clippy::result_large_err)] // redb::Error is big; local + immediately consumed
        let run = || -> Result<(), redb::Error> {
            let mut txn = db.begin_write()?;
            txn.set_durability(if durable {
                redb::Durability::Immediate
            } else {
                redb::Durability::Eventual
            });
            {
                let mut table = txn.open_table(TABLE)?;
                for job in &batch {
                    for mutation in &job.mutations {
                        match &mutation.value {
                            Some((bytes, mtime_ns, modification_revision)) => {
                                let mut raw = Vec::with_capacity(16 + bytes.len());
                                raw.extend_from_slice(&mtime_ns.to_le_bytes());
                                raw.extend_from_slice(&modification_revision.to_le_bytes());
                                raw.extend_from_slice(bytes);
                                table.insert(mutation.key.as_slice(), raw.as_slice())?;
                            }
                            None => {
                                table.remove(mutation.key.as_slice())?;
                            }
                        }
                    }
                }
            }
            {
                let mut operations = txn.open_table(OPERATION_TABLE)?;
                for job in &batch {
                    if let Some((operation_id, result)) = &job.operation {
                        operations.insert(operation_id.as_slice(), result.as_slice())?;
                    }
                    for operation_id in &job.evicted_operations {
                        operations.remove(operation_id.as_slice())?;
                    }
                }
            }
            txn.commit()?;
            Ok(())
        };
        let ok = match run() {
            Ok(()) => true,
            Err(e) => {
                eprintln!("kv: persist failed ({e})");
                false
            }
        };
        for mut job in batch {
            if let Some(reply) = job.native_reply.take() {
                let _ = reply.send(ok);
            }
        }
        if !ok {
            // A failed transaction leaves the database at the preceding
            // prefix. Never admit a later transaction after that gap: fail
            // every already-queued durable waiter and drop the receiver so
            // subsequent nonblocking sends disable persistence immediately.
            while let Ok(mut job) = rx.try_recv() {
                if let Some(reply) = job.native_reply.take() {
                    let _ = reply.send(false);
                }
            }
            return;
        }
    }
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Store::open()))
}

/// Load and hash the store off the serving paths, so the first KV frame
/// of the first connection doesn't pay the whole-database load+hash
/// (≤ 256 MiB of BLAKE3) inline.
pub fn warm() {
    let _ = std::thread::Builder::new()
        .name("kv-warm".into())
        .spawn(|| {
            let _ = store();
        });
}

pub(crate) fn native_limits() -> yas_wire::kv::Limits {
    let hard = yas_wire::kv::Limits::HARD;
    yas_wire::kv::Limits {
        max_key_bytes: hard.max_key_bytes,
        max_value_bytes: value_max().min(hard.max_value_bytes),
        max_inline_bytes: hard.max_inline_bytes,
        max_entries: u32::try_from(max_entries())
            .unwrap_or(u32::MAX)
            .min(hard.max_entries),
        max_store_bytes: total_max().min(hard.max_store_bytes),
        max_namespaces_per_session: hard.max_namespaces_per_session,
        max_stages_per_session: hard.max_stages_per_session,
        max_staged_bytes_per_session: hard.max_staged_bytes_per_session,
        max_batch_items: hard.max_batch_items,
    }
}

pub(crate) fn native_revision() -> u64 {
    store().lock().unwrap().revision
}

pub(crate) fn native_get(key: &[u8]) -> Option<NativeEntry> {
    let st = store().lock().unwrap();
    st.entries.get(key).map(|entry| NativeEntry {
        key: key.to_vec(),
        value: entry.value.clone(),
        content_hash: entry.hash,
        mtime_ns: entry.mtime_ns,
        modification_revision: entry.modification_revision,
    })
}

pub(crate) fn native_replay(
    operation_id: [u8; 16],
    fingerprint: [u8; 32],
) -> Result<Option<NativeCommit>, NativeMutationError> {
    let st = store().lock().unwrap();
    let Some(previous) = st.operations.get(&operation_id) else {
        return Ok(None);
    };
    if previous.fingerprint != fingerprint {
        return Err(NativeMutationError::OperationConflict);
    }
    Ok(Some(NativeCommit {
        store_revision: previous.store_revision,
        results: previous.results.clone(),
        durable: None,
        persistence_failed: false,
    }))
}

/// Return staged-value metadata retained atomically with an operation replay.
/// The native adapter uses this only when an exact consumed stage has aged out
/// of its much smaller per-session recent tombstone ring.
pub(crate) fn native_stage_witnesses(operation_id: [u8; 16]) -> Option<Vec<NativeStageWitness>> {
    store()
        .lock()
        .unwrap()
        .operations
        .get(&operation_id)
        .map(|result| result.stage_witnesses.clone())
}

/// Subscribe and clone the matching snapshot under the same lock. Any commit
/// after `revision` is retained by the bounded receiver, so the YAS adapter
/// can serialize the snapshot off-lock without a snapshot/live race.
pub(crate) fn native_watch(prefix: &[u8]) -> NativeWatch {
    let st = store().lock().unwrap();
    let changes = st.native_changes.subscribe();
    let revision = st.revision;
    let prefix = prefix.to_vec();
    let entries = st
        .entries
        .range(prefix.clone()..)
        .take_while(|(key, _)| key.starts_with(&prefix))
        .map(|(key, entry)| NativeEntry {
            key: key.clone(),
            value: entry.value.clone(),
            content_hash: entry.hash,
            mtime_ns: entry.mtime_ns,
            modification_revision: entry.modification_revision,
        })
        .collect();
    NativeWatch {
        revision,
        entries,
        changes,
    }
}

fn mutation_key(mutation: &NativeMutation) -> &[u8] {
    match mutation {
        NativeMutation::Put { key, .. } | NativeMutation::Delete { key, .. } => key,
    }
}

fn precondition_matches(
    precondition: &yas_wire::kv::Precondition,
    current: Option<&Entry>,
) -> bool {
    use yas_wire::kv::Precondition;
    match precondition {
        Precondition::Any => true,
        Precondition::Absent => current.is_none(),
        Precondition::Hash(hash) => current.is_some_and(|entry| &entry.hash == hash),
        Precondition::Revision(revision) => {
            current.is_some_and(|entry| entry.modification_revision == *revision)
        }
        Precondition::HashAndRevision {
            content_hash,
            modification_revision,
        } => current.is_some_and(|entry| {
            entry.hash == *content_hash && entry.modification_revision == *modification_revision
        }),
    }
}

fn result_for(status: yas_wire::core::Status, current: Option<&Entry>) -> NativeMutationResult {
    NativeMutationResult {
        status,
        modification_revision: current.map_or(0, |entry| entry.modification_revision),
        mtime_ns: current.map_or(0, |entry| entry.mtime_ns),
        content_hash: current.map_or([0; 32], |entry| entry.hash),
        byte_len: current.map_or(0, |entry| entry.value.len() as u64),
    }
}

/// Apply one PUT/DELETE or an atomic BATCH. `fingerprint` is BLAKE3 over the
/// canonical request family/kind/body and makes operation-ID reuse with
/// different arguments a request-level conflict. The outcome is persisted in
/// the same redb transaction as the data mutations.
pub(crate) fn native_mutate(
    operation_id: [u8; 16],
    fingerprint: [u8; 32],
    durable: bool,
    stage_witnesses: Vec<NativeStageWitness>,
    mutations: Vec<NativeMutation>,
) -> Result<NativeCommit, NativeMutationError> {
    if mutations.is_empty() || mutations.len() > yas_wire::kv::MAX_BATCH_ITEMS {
        return Err(NativeMutationError::Invalid);
    }
    if stage_witnesses.len() > MAX_STAGE_WITNESSES_PER_OPERATION
        || stage_witnesses
            .iter()
            .enumerate()
            .any(|(ordinal, witness)| {
                usize::from(witness.ordinal) != ordinal
                    || witness.original_handle == 0
                    || witness.byte_len > value_max()
            })
    {
        return Err(NativeMutationError::Invalid);
    }
    let limits = native_limits();
    for mutation in &mutations {
        let key = mutation_key(mutation);
        if key.is_empty()
            || key.len() > limits.max_key_bytes as usize
            || key.contains(&0)
            || matches!(
                mutation,
                NativeMutation::Put { value, .. } if value.len() as u64 > limits.max_value_bytes
            )
        {
            return Err(NativeMutationError::Invalid);
        }
    }

    let mut st = store().lock().unwrap();
    if let Some(previous) = st.operations.get(&operation_id) {
        if previous.fingerprint != fingerprint {
            return Err(NativeMutationError::OperationConflict);
        }
        return Ok(NativeCommit {
            store_revision: previous.store_revision,
            results: previous.results.clone(),
            durable: None,
            persistence_failed: false,
        });
    }

    let from_revision = st.revision;
    let commit_revision = from_revision.saturating_add(1);
    let mtime_ns = now_ns();
    let mut working = st.entries.clone();
    let mut results = Vec::with_capacity(mutations.len());
    let mut conflict = false;
    for mutation in &mutations {
        match mutation {
            NativeMutation::Put {
                key,
                precondition,
                value,
                content_hash,
            } => {
                if !precondition_matches(precondition, working.get(key)) {
                    conflict = true;
                    break;
                }
                working.insert(
                    key.clone(),
                    Entry {
                        value: value.clone(),
                        hash: *content_hash,
                        mtime_ns,
                        modification_revision: commit_revision,
                    },
                );
                results.push(result_for(yas_wire::core::Status::Ok, working.get(key)));
            }
            NativeMutation::Delete { key, precondition } => {
                if !precondition_matches(precondition, working.get(key)) {
                    conflict = true;
                    break;
                }
                working.remove(key);
                results.push(NativeMutationResult {
                    status: yas_wire::core::Status::Ok,
                    modification_revision: commit_revision,
                    mtime_ns,
                    content_hash: [0; 32],
                    byte_len: 0,
                });
            }
        }
    }

    if conflict {
        results = mutations
            .iter()
            .map(|mutation| {
                result_for(
                    yas_wire::core::Status::Conflict,
                    st.entries.get(mutation_key(mutation)),
                )
            })
            .collect();
        let dedup = NativeDedupResult {
            fingerprint,
            store_revision: st.revision,
            settlement_sequence: st.next_operation_sequence,
            results: results.clone(),
            stage_witnesses,
        };
        let encoded = Arc::new(encode_native_dedup(&dedup));
        let replay_plan = plan_operation_replay(
            &st.entries,
            &st.operations,
            &st.operation_order,
            operation_id,
            dedup,
        );
        let queued_bytes = write_job_bytes(&[], encoded.len(), replay_plan.evicted.len())
            .ok_or(NativeMutationError::ResourceExhausted)?;
        let permit = st.reserve_write(queued_bytes)?;
        let (reply, durable_result) = if durable && permit.is_some() {
            let (tx, rx) = oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        st.operations = replay_plan.values;
        st.operation_order = replay_plan.order;
        st.next_operation_sequence = st.next_operation_sequence.saturating_add(1);
        let queued = permit.is_some_and(|permit| {
            st.enqueue_job(WriteJob {
                mutations: Vec::new(),
                operation: Some((operation_id, encoded)),
                evicted_operations: replay_plan.evicted,
                durable,
                native_reply: reply,
                _permit: permit,
            })
        });
        if durable && !queued {
            return Ok(NativeCommit {
                store_revision: st.revision,
                results,
                durable: None,
                persistence_failed: true,
            });
        }
        return Ok(NativeCommit {
            store_revision: st.revision,
            results,
            durable: durable_result,
            persistence_failed: false,
        });
    }

    let total_bytes = working.iter().try_fold(0u64, |total, (key, entry)| {
        total.checked_add((key.len() + entry.value.len()) as u64)
    });
    let Some(total_bytes) = total_bytes else {
        return Err(NativeMutationError::ResourceExhausted);
    };
    if working.len() as u64 > u64::from(limits.max_entries) || total_bytes > limits.max_store_bytes
    {
        return Err(NativeMutationError::ResourceExhausted);
    }

    let mut changed = BTreeMap::<Vec<u8>, bool>::new();
    for mutation in &mutations {
        let key = mutation_key(mutation).to_vec();
        let added = !st.entries.contains_key(&key);
        changed.insert(key, added);
    }
    let mut records = Vec::with_capacity(changed.len());
    let mut persisted = Vec::with_capacity(changed.len());
    for (key, added) in changed {
        if let Some(entry) = working.get(&key) {
            records.push(NativeChangeRecord::Upsert {
                entry: NativeEntry {
                    key: key.clone(),
                    value: entry.value.clone(),
                    content_hash: entry.hash,
                    mtime_ns: entry.mtime_ns,
                    modification_revision: entry.modification_revision,
                },
                added,
            });
            persisted.push(PersistedMutation {
                key,
                value: Some((
                    entry.value.clone(),
                    entry.mtime_ns,
                    entry.modification_revision,
                )),
            });
        } else {
            records.push(NativeChangeRecord::Remove {
                key: key.clone(),
                modification_revision: commit_revision,
            });
            persisted.push(PersistedMutation { key, value: None });
        }
    }
    let change = NativeChange {
        from_revision,
        to_revision: commit_revision,
        records,
    };
    let dedup = NativeDedupResult {
        fingerprint,
        store_revision: commit_revision,
        settlement_sequence: st.next_operation_sequence,
        results: results.clone(),
        stage_witnesses,
    };
    let encoded = Arc::new(encode_native_dedup(&dedup));
    let replay_plan = plan_operation_replay(
        &working,
        &st.operations,
        &st.operation_order,
        operation_id,
        dedup,
    );
    let queued_bytes = write_job_bytes(&persisted, encoded.len(), replay_plan.evicted.len())
        .ok_or(NativeMutationError::ResourceExhausted)?;
    let permit = st.reserve_write(queued_bytes)?;
    let (reply, durable_result) = if durable && permit.is_some() {
        let (tx, rx) = oneshot::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    st.entries = working;
    st.total_bytes = total_bytes;
    st.revision = commit_revision;
    st.operations = replay_plan.values;
    st.operation_order = replay_plan.order;
    st.next_operation_sequence = st.next_operation_sequence.saturating_add(1);
    let queued = permit.is_some_and(|permit| {
        st.enqueue_job(WriteJob {
            mutations: persisted,
            operation: Some((operation_id, encoded)),
            evicted_operations: replay_plan.evicted,
            durable,
            native_reply: reply,
            _permit: permit,
        })
    });
    let _ = st.native_changes.send(change.clone());
    Ok(NativeCommit {
        store_revision: commit_revision,
        results,
        durable: if queued { durable_result } else { None },
        persistence_failed: durable && !queued,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(sequence: u64, revision: u64, status: yas_wire::core::Status) -> NativeDedupResult {
        NativeDedupResult {
            fingerprint: [sequence as u8; 32],
            store_revision: revision,
            settlement_sequence: sequence,
            results: vec![NativeMutationResult {
                status,
                modification_revision: revision,
                mtime_ns: sequence,
                content_hash: [revision as u8; 32],
                byte_len: sequence,
            }],
            stage_witnesses: Vec::new(),
        }
    }

    fn maximum_stage_witnesses() -> Vec<NativeStageWitness> {
        (0..MAX_STAGE_WITNESSES_PER_OPERATION)
            .map(|ordinal| NativeStageWitness {
                ordinal: ordinal as u16,
                original_handle: ordinal as u64 + 1,
                byte_len: ordinal as u64,
                content_hash: [ordinal as u8; 32],
            })
            .collect()
    }

    fn empty_job(permit: WriterPermit) -> WriteJob {
        WriteJob {
            mutations: Vec::new(),
            operation: None,
            evicted_operations: Vec::new(),
            durable: false,
            native_reply: None,
            _permit: permit,
        }
    }

    #[test]
    fn native_dedup_persistence_uses_exact_yko1_layout() {
        let expected = NativeDedupResult {
            fingerprint: [0xa5; 32],
            store_revision: u64::MAX - 1,
            settlement_sequence: u64::MAX - 5,
            results: vec![NativeMutationResult {
                status: yas_wire::core::Status::Conflict,
                modification_revision: u64::MAX - 2,
                mtime_ns: u64::MAX - 3,
                content_hash: [0x5a; 32],
                byte_len: u64::MAX - 4,
            }],
            stage_witnesses: vec![NativeStageWitness {
                ordinal: 0,
                original_handle: 0x1234,
                byte_len: 0x5678,
                content_hash: [0x9a; 32],
            }],
        };

        let encoded = encode_native_dedup(&expected);
        assert_eq!(
            encoded.len(),
            OPERATION_RECORD_HEADER_BYTES
                + OPERATION_RESULT_ENCODED_BYTES
                + STAGE_WITNESS_ENCODED_BYTES
        );
        assert_eq!(&encoded[0..4], b"YKO1");
        assert_eq!(&encoded[4..12], &expected.settlement_sequence.to_le_bytes());
        assert_eq!(&encoded[12..44], &expected.fingerprint);
        assert_eq!(&encoded[44..52], &expected.store_revision.to_le_bytes());
        assert_eq!(&encoded[52..54], &1_u16.to_le_bytes());
        assert_eq!(&encoded[54..56], &1_u16.to_le_bytes());
        assert_eq!(
            &encoded[56..58],
            &expected.results[0].status.code().to_le_bytes()
        );
        assert_eq!(
            &encoded[58..66],
            &expected.results[0].modification_revision.to_le_bytes()
        );
        assert_eq!(
            &encoded[66..74],
            &expected.results[0].mtime_ns.to_le_bytes()
        );
        assert_eq!(&encoded[74..106], &expected.results[0].content_hash);
        assert_eq!(
            &encoded[106..114],
            &expected.results[0].byte_len.to_le_bytes()
        );
        assert_eq!(&encoded[114..116], &0_u16.to_le_bytes());
        assert_eq!(
            &encoded[116..124],
            &expected.stage_witnesses[0].original_handle.to_le_bytes()
        );
        assert_eq!(
            &encoded[124..132],
            &expected.stage_witnesses[0].byte_len.to_le_bytes()
        );
        assert_eq!(
            &encoded[132..164],
            &expected.stage_witnesses[0].content_hash
        );
        let decoded = decode_native_dedup(&encoded).unwrap();
        assert_eq!(decoded.fingerprint, expected.fingerprint);
        assert_eq!(decoded.store_revision, expected.store_revision);
        assert_eq!(decoded.settlement_sequence, expected.settlement_sequence);
        assert_eq!(decoded.results, expected.results);
        assert_eq!(decoded.stage_witnesses, expected.stage_witnesses);
    }

    #[test]
    fn native_dedup_persistence_rejects_non_yko1_and_malformed_rows() {
        let expected = NativeDedupResult {
            fingerprint: [1; 32],
            store_revision: 1,
            settlement_sequence: 2,
            results: Vec::new(),
            stage_witnesses: Vec::new(),
        };
        let encoded = encode_native_dedup(&expected);
        for end in 0..encoded.len() {
            assert!(decode_native_dedup(&encoded[..end]).is_none(), "end={end}");
        }

        let mut wrong_magic = encoded.clone();
        wrong_magic[0..4].copy_from_slice(b"NOPE");
        assert!(decode_native_dedup(&wrong_magic).is_none());

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_native_dedup(&trailing).is_none());

        let mut zero_sequence = encoded.clone();
        zero_sequence[4..12].fill(0);
        assert!(decode_native_dedup(&zero_sequence).is_none());

        let mut zero_revision = encoded;
        zero_revision[44..52].fill(0);
        assert!(decode_native_dedup(&zero_revision).is_none());
    }

    #[test]
    fn writer_admission_is_job_and_byte_bounded_through_real_job_lifetime() {
        let budget = WriterBudget::new(2, 10);
        let mut first = empty_job(budget.try_reserve(6).unwrap());
        let second = empty_job(budget.try_reserve(4).unwrap());
        assert!(budget.try_reserve(1).is_none());

        // Client-side cancellation does not own the permit: only the actual
        // queued job's completion/drop can recycle it.
        let (reply, receiver) = oneshot::channel::<bool>();
        first.native_reply = Some(reply);
        drop(receiver);
        assert!(budget.try_reserve(1).is_none());
        drop(first);
        let replacement = empty_job(budget.try_reserve(6).unwrap());
        assert!(budget.try_reserve(1).is_none());
        drop(second);
        drop(replacement);
        assert!(budget.try_reserve(10).is_some());
    }

    #[test]
    fn standalone_table_is_byte_keyed_and_uses_documented_entry_layout() {
        let directory = tempfile::tempdir().unwrap();
        let database = redb::Database::create(directory.path().join("kv.redb")).unwrap();
        let key = vec![0xff, 0x80, b'k'];
        let value = Arc::new(vec![0x00, 0x7f, 0xfe]);
        let mtime_ns = 0x0123_4567_89ab_cdef;
        let modification_revision = 0xfedc_ba98_7654_3210;
        let budget = WriterBudget::new(1, 1024);
        let permit = budget.try_reserve(key.len() + value.len()).unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || writer_loop(database, receiver));
        let (reply, completion) = oneshot::channel();
        sender
            .send(WriteJob {
                mutations: vec![PersistedMutation {
                    key: key.clone(),
                    value: Some((value.clone(), mtime_ns, modification_revision)),
                }],
                operation: None,
                evicted_operations: Vec::new(),
                durable: true,
                native_reply: Some(reply),
                _permit: permit,
            })
            .unwrap();
        assert!(completion.blocking_recv().unwrap());
        drop(sender);
        worker.join().unwrap();

        let database = redb::Database::open(directory.path().join("kv.redb")).unwrap();
        let txn = database.begin_read().unwrap();
        let table_names = txn
            .list_tables()
            .unwrap()
            .map(|table| table.name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(table_names, ["kv_operations_v1", "kv_v1"]);
        let table = txn.open_table(TABLE).unwrap();
        let stored = table.get(key.as_slice()).unwrap().unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&mtime_ns.to_le_bytes());
        expected.extend_from_slice(&modification_revision.to_le_bytes());
        expected.extend_from_slice(value.as_slice());
        assert_eq!(stored.value(), expected);
    }

    #[test]
    fn replay_horizon_keeps_live_entry_operations_and_recent_failures() {
        let pinned_id = 1_u128.to_le_bytes();
        let pinned = replay(1, 7, yas_wire::core::Status::Ok);
        let mut rows = vec![(pinned_id, pinned)];
        for sequence in 2..=MAX_RECENT_OPERATION_REPLAYS as u64 + 12 {
            rows.push((
                u128::from(sequence + 1).to_le_bytes(),
                replay(sequence, 99, yas_wire::core::Status::Conflict),
            ));
        }
        let live_revisions = BTreeSet::from([7]);
        let forward = retained_operations(rows.clone(), &live_revisions);
        rows.reverse();
        let reverse = retained_operations(rows, &live_revisions);

        assert_eq!(forward.values.len(), MAX_RECENT_OPERATION_REPLAYS + 1);
        assert!(forward.values.contains_key(&pinned_id));
        assert!(!forward.values.contains_key(&3_u128.to_le_bytes()));
        assert!(
            forward
                .values
                .contains_key(&u128::from(MAX_RECENT_OPERATION_REPLAYS as u64 + 13).to_le_bytes())
        );
        assert_eq!(forward.order, reverse.order);
        assert_eq!(
            forward.values.keys().collect::<Vec<_>>(),
            reverse.values.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            forward.maximum_sequence,
            MAX_RECENT_OPERATION_REPLAYS as u64 + 12
        );
    }

    #[test]
    fn replay_horizon_has_an_exact_worst_case_stage_witness_byte_bound() {
        let maximum_entries = yas_wire::kv::Limits::HARD.max_entries as usize;
        let mut live_revisions = BTreeSet::new();
        let mut rows = Vec::with_capacity(maximum_entries + MAX_RECENT_OPERATION_REPLAYS);
        for revision in 1..=maximum_entries as u64 {
            live_revisions.insert(revision);
            let mut result = replay(revision, revision, yas_wire::core::Status::Ok);
            result.stage_witnesses = maximum_stage_witnesses();
            rows.push((u128::from(revision).to_le_bytes(), result));
        }
        for offset in 1..=MAX_RECENT_OPERATION_REPLAYS as u64 {
            let sequence = maximum_entries as u64 + offset;
            let mut result = replay(
                sequence,
                maximum_entries as u64 + 1,
                yas_wire::core::Status::Conflict,
            );
            result.stage_witnesses = maximum_stage_witnesses();
            rows.push((u128::from(sequence).to_le_bytes(), result));
        }

        let retained = retained_operations(rows, &live_revisions);
        let witnesses = retained
            .values
            .values()
            .map(|result| result.stage_witnesses.len())
            .sum::<usize>();
        assert_eq!(
            retained.values.len(),
            maximum_entries + MAX_RECENT_OPERATION_REPLAYS
        );
        assert_eq!(witnesses, MAX_RETAINED_STAGE_WITNESSES);
        assert_eq!(
            witnesses * STAGE_WITNESS_ENCODED_BYTES,
            MAX_RETAINED_STAGE_WITNESS_BYTES
        );
    }

    #[test]
    fn persisted_replay_pruning_is_deterministic_across_restart_load() {
        let directory = tempfile::tempdir().unwrap();
        let database = redb::Database::create(directory.path().join("kv.redb")).unwrap();
        let pinned_id = 10_u128.to_le_bytes();
        let stale_id = 102_u128.to_le_bytes();
        let recent_sequence = MAX_RECENT_OPERATION_REPLAYS as u64 + 9;
        let recent_id = u128::from(recent_sequence + 100).to_le_bytes();
        {
            let txn = database.begin_write().unwrap();
            {
                let mut table = txn.open_table(OPERATION_TABLE).unwrap();
                let pinned = replay(1, 7, yas_wire::core::Status::Ok);
                let encoded = encode_native_dedup(&pinned);
                table
                    .insert(pinned_id.as_slice(), encoded.as_slice())
                    .unwrap();
                for sequence in 2..=recent_sequence {
                    let id = u128::from(sequence + 100).to_le_bytes();
                    let result = replay(sequence, 8, yas_wire::core::Status::Conflict);
                    let encoded = encode_native_dedup(&result);
                    table.insert(id.as_slice(), encoded.as_slice()).unwrap();
                }
                table
                    .insert(b"malformed".as_slice(), b"row".as_slice())
                    .unwrap();
            }
            txn.commit().unwrap();
        }

        let live_revisions = BTreeSet::from([7]);
        let retained = load_operation_replays(&database, &live_revisions).unwrap();
        assert_eq!(retained.values.len(), MAX_RECENT_OPERATION_REPLAYS + 1);
        assert!(retained.values.contains_key(&pinned_id));
        assert!(!retained.values.contains_key(&stale_id));
        assert!(retained.values.contains_key(&recent_id));
        prune_persisted_operation_replays(&database, &retained.values).unwrap();

        let txn = database.begin_read().unwrap();
        let table = txn.open_table(OPERATION_TABLE).unwrap();
        assert!(table.get(pinned_id.as_slice()).unwrap().is_some());
        assert!(table.get(stale_id.as_slice()).unwrap().is_none());
        assert!(table.get(recent_id.as_slice()).unwrap().is_some());
        assert!(table.get(b"malformed".as_slice()).unwrap().is_none());
    }

    #[test]
    fn writer_commit_atomically_inserts_and_prunes_replay_then_releases_admission() {
        let directory = tempfile::tempdir().unwrap();
        let database = redb::Database::create(directory.path().join("kv.redb")).unwrap();
        let old_id = 21_u128.to_le_bytes();
        let new_id = 22_u128.to_le_bytes();
        {
            let txn = database.begin_write().unwrap();
            {
                let mut table = txn.open_table(OPERATION_TABLE).unwrap();
                let old = encode_native_dedup(&replay(1, 1, yas_wire::core::Status::Conflict));
                table.insert(old_id.as_slice(), old.as_slice()).unwrap();
            }
            txn.commit().unwrap();
        }
        let budget = WriterBudget::new(1, 1024);
        let permit = budget.try_reserve(256).unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || writer_loop(database, receiver));
        let (reply, completion) = oneshot::channel();
        let mut new_replay = replay(2, 1, yas_wire::core::Status::Conflict);
        new_replay.stage_witnesses = vec![NativeStageWitness {
            ordinal: 0,
            original_handle: 0xabcd,
            byte_len: 0x1234,
            content_hash: [0x56; 32],
        }];
        let encoded = Arc::new(encode_native_dedup(&new_replay));
        assert!(
            sender
                .send(WriteJob {
                    mutations: Vec::new(),
                    operation: Some((new_id, encoded)),
                    evicted_operations: vec![old_id],
                    durable: true,
                    native_reply: Some(reply),
                    _permit: permit,
                })
                .is_ok()
        );
        assert!(budget.try_reserve(1).is_none());
        assert!(completion.blocking_recv().unwrap());
        drop(sender);
        worker.join().unwrap();
        let recycled = budget.try_reserve(1024).unwrap();
        drop(recycled);

        let database = redb::Database::open(directory.path().join("kv.redb")).unwrap();
        let txn = database.begin_read().unwrap();
        let table = txn.open_table(OPERATION_TABLE).unwrap();
        assert!(table.get(old_id.as_slice()).unwrap().is_none());
        let stored = table.get(new_id.as_slice()).unwrap().unwrap();
        let decoded = decode_native_dedup(stored.value()).unwrap();
        assert_eq!(decoded.stage_witnesses, new_replay.stage_witnesses);
    }
}
