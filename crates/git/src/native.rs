//! Owned, typed Git engine API used by the YAS v1 server.
//!
//! Native callers exchange semantic values directly with the engine.

use std::path::{Path, PathBuf};

use crate::model as engine;

use crate::{Cancel, RepoHandle, StateHandle, StateOptions};

pub type Oid = [u8; 32];
pub type RepoPath = Vec<Vec<u8>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFormat {
    Sha1,
    Sha256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    NotFound,
    WrongType,
    Permission,
    ResourceExhausted,
    Invalid,
    Cancelled,
    Conflict,
    Other,
}

impl Status {
    fn from_engine(value: u8) -> Self {
        match value {
            engine::GIT_STATUS_OK => Self::Ok,
            engine::GIT_STATUS_NOT_FOUND | engine::GIT_STATUS_UNKNOWN_ID => Self::NotFound,
            engine::GIT_STATUS_WRONG_TYPE => Self::WrongType,
            engine::GIT_STATUS_PERMISSION => Self::Permission,
            engine::GIT_STATUS_TOO_LARGE | engine::GIT_STATUS_BUDGET => Self::ResourceExhausted,
            engine::GIT_STATUS_INVALID => Self::Invalid,
            engine::GIT_STATUS_CANCELLED => Self::Cancelled,
            engine::GIT_STATUS_CONFLICT | engine::GIT_STATUS_NO_MERGE_BASE => Self::Conflict,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub status: Status,
    pub detail: String,
}

impl Failure {
    fn from_status(status: u8) -> Self {
        Self {
            status: Status::from_engine(status),
            detail: engine::git_status_text(status).to_owned(),
        }
    }

    fn malformed(operation: &'static str) -> Self {
        Self {
            status: Status::Other,
            detail: format!("Git engine returned a malformed {operation} result"),
        }
    }
}

fn result_status(status: u8) -> Result<(), Failure> {
    if status == engine::GIT_STATUS_OK {
        Ok(())
    } else {
        Err(Failure::from_status(status))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryInfo {
    pub object_format: ObjectFormat,
    pub bare: bool,
    pub shallow: bool,
    pub sparse: bool,
    pub linked: bool,
    pub writable: bool,
    pub fetchable: bool,
    pub worktree_path: Option<PathBuf>,
    pub git_dir: PathBuf,
}

fn repository_info(handle: &RepoHandle, info: crate::RepoInfo) -> Result<RepositoryInfo, Failure> {
    let object_format = match info.oid_format {
        engine::GIT_OID_FORMAT_SHA1 => ObjectFormat::Sha1,
        engine::GIT_OID_FORMAT_SHA256 => ObjectFormat::Sha256,
        _ => return Err(Failure::malformed("repository")),
    };
    let repo = handle.local();
    Ok(RepositoryInfo {
        object_format,
        bare: info.flags & engine::GIT_REPO_BARE != 0,
        shallow: info.flags & engine::GIT_REPO_SHALLOW != 0,
        sparse: info.flags & engine::GIT_REPO_SPARSE != 0,
        linked: info.flags & engine::GIT_REPO_LINKED != 0,
        writable: info.flags & engine::GIT_REPO_WRITABLE != 0,
        fetchable: info.flags & engine::GIT_REPO_FETCHABLE != 0,
        worktree_path: repo.workdir().map(crate::canonical),
        git_dir: crate::canonical(repo.git_dir()),
    })
}

pub fn open_path(path: &Path) -> Result<(RepoHandle, RepositoryInfo), Failure> {
    let (handle, info) = crate::open_path(path).map_err(|(status, detail)| Failure {
        status: Status::from_engine(status),
        detail,
    })?;
    let info = repository_info(&handle, info)?;
    Ok((handle, info))
}

pub fn open_submodule_path(
    parent: &RepoHandle,
    path: &Path,
) -> Result<(RepoHandle, RepositoryInfo), Failure> {
    let (handle, info) =
        crate::reads::open_submodule_path(parent, path).map_err(|(status, detail)| Failure {
            status: Status::from_engine(status),
            detail,
        })?;
    let info = repository_info(&handle, info)?;
    Ok((handle, info))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosedReason {
    ClientRequest,
    RepositoryGone,
    PermissionLost,
    ResourceLimit,
    BackendFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateEvent {
    Snapshot {
        state_id: u32,
        records: Vec<StateRecord>,
    },
    Closed(ClosedReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateRecord {
    Head {
        detached: bool,
        unborn: bool,
        object: Oid,
        symbolic_target: Vec<u8>,
    },
    Ref {
        peeled_valid: bool,
        symbolic: bool,
        object: Oid,
        peeled: Oid,
        name: Vec<u8>,
        symbolic_target: Vec<u8>,
    },
    Operation {
        kind: u8,
        head: Oid,
        detail: String,
    },
    Status {
        index_status: u8,
        worktree_status: u8,
        conflicted: bool,
        object: Oid,
        old_path: Option<RepoPath>,
        path: RepoPath,
    },
    Upstream {
        gone: bool,
        counts_valid: bool,
        ahead: u32,
        behind: u32,
        name: Vec<u8>,
        upstream: Vec<u8>,
    },
    Stash {
        index: u16,
        object: Oid,
        created_unix_seconds: i64,
        timezone_minutes: i16,
        message: Vec<u8>,
    },
    Remote {
        default: bool,
        name: Vec<u8>,
        fetch_url: Vec<u8>,
        push_url: Vec<u8>,
    },
    WorktreeGeneration {
        count: u32,
        digest: u64,
    },
}

pub type StateSink = Box<dyn FnMut(StateEvent) -> bool + Send>;

impl RepoHandle {
    pub fn start_native_state(&self, options: StateOptions, sink: StateSink) -> StateHandle {
        self.start_state(options, sink)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryPage {
    pub records: Vec<DiscoveryRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryRecord {
    Repository {
        bare: bool,
        linked: bool,
        submodule: bool,
        object_format: ObjectFormat,
        worktree_path: Option<PathBuf>,
        git_dir: PathBuf,
    },
    Cursor(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlamePage {
    pub records: Vec<BlameRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlameRecord {
    Range {
        flags: u8,
        commit: Oid,
        start_line: u32,
        line_count: u32,
        original_start_line: u32,
        original_path: Option<RepoPath>,
    },
    Cursor(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogPage {
    pub records: Vec<ReflogRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflogRecord {
    Entry {
        flags: u8,
        old_object: Oid,
        new_object: Oid,
        committed_unix_seconds: i64,
        timezone_minutes: i16,
        message: Vec<u8>,
    },
    Cursor(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreesPage {
    pub records: Vec<WorktreeRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeRecord {
    Worktree {
        bare: bool,
        main: bool,
        current: bool,
        locked: bool,
        prunable: bool,
        detached: bool,
        head: Oid,
        path: Option<PathBuf>,
        branch: Vec<u8>,
        lock_reason: String,
    },
    Cursor(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchResult {
    pub refs: Vec<FetchRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchRef {
    pub forced: bool,
    pub pruned: bool,
    pub new_ref: bool,
    pub tag_update: bool,
    pub status: Status,
    pub old_object: Oid,
    pub new_object: Oid,
    pub name: Vec<u8>,
    pub detail: String,
}

pub fn discover_path(
    flags: u8,
    max_depth: u8,
    path: &Path,
    after: Option<&Path>,
    cancel: &Cancel,
) -> Result<DiscoveryPage, Failure> {
    let after = after.map(yas_fssync::escape_path).unwrap_or_default();
    let response = crate::reads::discover_path(1, flags, max_depth, path, &after, cancel);
    result_status(response.status)?;
    let mut records = Vec::new();
    for record in response.value {
        records.push(match record {
            engine::OwnedGitDiscoverRecord::Repo {
                flags,
                workdir,
                gitdir,
            } => {
                let worktree_path = if workdir.is_empty() {
                    None
                } else {
                    Some(
                        decode_platform_path(&workdir)
                            .ok_or_else(|| Failure::malformed("DISCOVER worktree path"))?,
                    )
                };
                let git_dir = decode_platform_path(&gitdir)
                    .ok_or_else(|| Failure::malformed("DISCOVER git directory"))?;
                let probe = worktree_path.as_deref().unwrap_or(&git_dir);
                let (_, info) = open_path(probe)?;
                DiscoveryRecord::Repository {
                    bare: flags & engine::GIT_FOUND_BARE != 0,
                    linked: flags & engine::GIT_FOUND_LINKED != 0,
                    submodule: flags & engine::GIT_FOUND_SUBMODULE != 0,
                    object_format: info.object_format,
                    worktree_path,
                    git_dir,
                }
            }
            engine::OwnedGitDiscoverRecord::Cursor { after, .. } => DiscoveryRecord::Cursor(
                decode_platform_path(&after)
                    .ok_or_else(|| Failure::malformed("DISCOVER cursor"))?,
            ),
        });
    }
    Ok(DiscoveryPage { records })
}

impl RepoHandle {
    pub fn native_blame(
        &self,
        object: Oid,
        path: &[Vec<u8>],
        start_line: u32,
        line_count: u32,
        flags: u8,
        cancel: &Cancel,
    ) -> Result<BlamePage, Failure> {
        let path = encode_path(path)?;
        let response = self.blame(
            &engine::GitBlameRequest {
                nonce: 1,
                repo_id: 0,
                flags,
                oid: object,
                start_line,
                line_count,
                path: &path,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitBlameRecord::Range {
                    flags,
                    commit,
                    start_line,
                    line_count,
                    orig_start,
                    orig_path,
                } => BlameRecord::Range {
                    flags,
                    commit,
                    start_line,
                    line_count,
                    original_start_line: orig_start,
                    original_path: decode_optional_path(&orig_path)
                        .ok_or_else(|| Failure::malformed("BLAME path"))?,
                },
                engine::OwnedGitBlameRecord::Cursor { pos, .. } => BlameRecord::Cursor(pos),
            });
        }
        Ok(BlamePage { records })
    }

    pub fn native_reflog(
        &self,
        name: &str,
        flags: u8,
        limit: u16,
        after_position: u64,
        cancel: &Cancel,
    ) -> Result<ReflogPage, Failure> {
        let response = self.reflog(
            &engine::GitReflogRequest {
                nonce: 1,
                repo_id: 0,
                flags,
                limit,
                ref_name: name,
                after_pos: after_position,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitReflogRecord::Entry {
                    flags,
                    old,
                    new,
                    time,
                    tz,
                    msg,
                } => ReflogRecord::Entry {
                    flags,
                    old_object: old,
                    new_object: new,
                    committed_unix_seconds: time,
                    timezone_minutes: tz,
                    message: msg.into_bytes(),
                },
                engine::OwnedGitReflogRecord::Cursor { pos, .. } => ReflogRecord::Cursor(pos),
            });
        }
        Ok(ReflogPage { records })
    }

    pub fn native_worktrees(
        &self,
        after_position: u64,
        cancel: &Cancel,
    ) -> Result<WorktreesPage, Failure> {
        let response = self.worktrees(
            &engine::GitWorktreesRequest {
                nonce: 1,
                repo_id: 0,
                flags: 0,
                after_pos: after_position,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitWorktreeRecord::Tree {
                    flags,
                    oid,
                    path,
                    branch,
                    lock_reason,
                } => WorktreeRecord::Worktree {
                    bare: flags & engine::GIT_WORKTREE_BARE != 0,
                    main: flags & engine::GIT_WORKTREE_MAIN != 0,
                    current: flags & engine::GIT_WORKTREE_CURRENT != 0,
                    locked: flags & engine::GIT_WORKTREE_LOCKED != 0,
                    prunable: flags & engine::GIT_WORKTREE_PRUNABLE != 0,
                    detached: flags & engine::GIT_WORKTREE_DETACHED != 0,
                    head: oid,
                    path: if path.is_empty() {
                        None
                    } else {
                        Some(
                            decode_platform_path(&path)
                                .ok_or_else(|| Failure::malformed("WORKTREES path"))?,
                        )
                    },
                    branch: branch.into_bytes(),
                    lock_reason,
                },
                engine::OwnedGitWorktreeRecord::Cursor { pos, .. } => WorktreeRecord::Cursor(pos),
            });
        }
        Ok(WorktreesPage { records })
    }

    pub fn native_fetch(
        &self,
        remote: &str,
        refspecs: &[String],
        flags: u8,
        timeout_ms: u32,
        cancel: &Cancel,
    ) -> Result<FetchResult, Failure> {
        let refspecs = refspecs.iter().map(String::as_str).collect::<Vec<_>>();
        let response = self.fetch(
            &engine::GitFetchRequest {
                nonce: 1,
                repo_id: 0,
                flags,
                timeout_ms,
                remote,
                refspecs,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut refs = Vec::new();
        for record in response.value {
            let engine::OwnedGitFetchRecord::Ref {
                flags,
                status,
                old,
                new,
                name,
                detail,
            } = record;
            refs.push(FetchRef {
                forced: flags & engine::GIT_FETCH_REF_FORCED != 0,
                pruned: flags & engine::GIT_FETCH_REF_PRUNED != 0,
                new_ref: flags & engine::GIT_FETCH_REF_NEW != 0,
                tag_update: flags & engine::GIT_FETCH_REF_TAG_UPDATE != 0,
                status: Status::from_engine(status),
                old_object: old,
                new_object: new,
                name: name.into_bytes(),
                detail,
            });
        }
        Ok(FetchResult { refs })
    }
}

fn decode_platform_path(value: &str) -> Option<PathBuf> {
    let bytes = crate::decode_path_bytes(value)?;
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(platform_os_string(bytes)))
}

#[cfg(unix)]
fn platform_os_string(bytes: Vec<u8>) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes)
}

#[cfg(windows)]
fn platform_os_string(bytes: Vec<u8>) -> std::ffi::OsString {
    String::from_utf8_lossy(&bytes).into_owned().into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Empty,
    Commit(Oid),
    Tree(Oid),
    Index,
    Worktree,
    MergeBase(Oid),
}

fn engine_endpoint(endpoint: Endpoint) -> engine::GitEndpoint {
    let (kind, oid) = match endpoint {
        Endpoint::Empty => (engine::GIT_ENDPOINT_EMPTY, [0; 32]),
        Endpoint::Commit(oid) => (engine::GIT_ENDPOINT_COMMIT, oid),
        Endpoint::Tree(oid) => (engine::GIT_ENDPOINT_TREE, oid),
        Endpoint::Index => (engine::GIT_ENDPOINT_INDEX, [0; 32]),
        Endpoint::Worktree => (engine::GIT_ENDPOINT_WORKTREE, [0; 32]),
        Endpoint::MergeBase(oid) => (engine::GIT_ENDPOINT_MERGE_BASE, oid),
    };
    engine::GitEndpoint { kind, oid }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRequest {
    pub flags: u8,
    pub rename_threshold: u8,
    pub old: Endpoint,
    pub new: Endpoint,
    pub path: RepoPath,
    pub after: RepoPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPage {
    pub records: Vec<DiffRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRecord {
    Entry {
        status: u8,
        similarity_percent: u8,
        binary: bool,
        submodule: bool,
        filtered: bool,
        old_mode: u32,
        new_mode: u32,
        old_object: Oid,
        new_object: Oid,
        old_path: Option<RepoPath>,
        new_path: Option<RepoPath>,
    },
    Base(Oid),
    Cursor(RepoPath),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchRequest {
    pub flags: u16,
    pub context_lines: u8,
    pub rename_threshold: u8,
    pub old: Endpoint,
    pub new: Endpoint,
    pub path: RepoPath,
    pub max_bytes: u32,
    pub after: RepoPath,
    pub after_position: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchResult {
    Text(Vec<u8>),
    Structured(Vec<PatchRecord>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchRecord {
    File {
        status: u8,
        similarity_percent: u8,
        binary: bool,
        filtered: bool,
        old_path: Option<RepoPath>,
        new_path: Option<RepoPath>,
    },
    Row {
        old_line: u32,
        new_line: u32,
        old_text: Vec<u8>,
        new_text: Vec<u8>,
        old_spans: Vec<(u32, u32)>,
        new_spans: Vec<(u32, u32)>,
    },
    Gap {
        old_line: u32,
        new_line: u32,
    },
    Base(Oid),
    Cursor {
        after: RepoPath,
        position: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexPage {
    pub records: Vec<IndexRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexRecord {
    Entry {
        stage: u8,
        intent_to_add: bool,
        skip_worktree: bool,
        mode: u32,
        size: u64,
        modified_unix_ns: u64,
        object: Oid,
        path: RepoPath,
    },
    Cursor(RepoPath),
}

impl RepoHandle {
    pub fn native_diff(&self, request: &DiffRequest, cancel: &Cancel) -> Result<DiffPage, Failure> {
        let path = encode_path(&request.path)?;
        let after = encode_path(&request.after)?;
        let response = self.diff(
            &engine::GitDiffRequest {
                nonce: 1,
                repo_id: 0,
                flags: request.flags,
                rename: request.rename_threshold,
                old: engine_endpoint(request.old),
                new: engine_endpoint(request.new),
                path: &path,
                after: &after,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitDiffRecord::Entry {
                    st,
                    similarity,
                    dflags,
                    old_mode,
                    new_mode,
                    old_oid,
                    new_oid,
                    old_path,
                    new_path,
                } => DiffRecord::Entry {
                    status: st,
                    similarity_percent: similarity,
                    binary: dflags & engine::GIT_DIFF_ENTRY_BINARY != 0,
                    submodule: dflags & engine::GIT_DIFF_ENTRY_SUBMODULE != 0,
                    filtered: dflags & engine::GIT_DIFF_ENTRY_FILTERED != 0,
                    old_mode,
                    new_mode,
                    old_object: old_oid,
                    new_object: new_oid,
                    old_path: decode_optional_path(&old_path)
                        .ok_or_else(|| Failure::malformed("DIFF old path"))?,
                    new_path: decode_optional_path(&new_path)
                        .ok_or_else(|| Failure::malformed("DIFF new path"))?,
                },
                engine::OwnedGitDiffRecord::Base { oid } => DiffRecord::Base(oid),
                engine::OwnedGitDiffRecord::Cursor { after, .. } => DiffRecord::Cursor(
                    decode_path(&after).ok_or_else(|| Failure::malformed("DIFF cursor"))?,
                ),
            });
        }
        Ok(DiffPage { records })
    }

    pub fn native_patch(
        &self,
        request: &PatchRequest,
        cancel: &Cancel,
    ) -> Result<PatchResult, Failure> {
        let path = encode_path(&request.path)?;
        let after = encode_path(&request.after)?;
        let response = self.patch(
            &engine::GitPatchRequest {
                nonce: 1,
                repo_id: 0,
                flags: request.flags,
                context: request.context_lines,
                rename: request.rename_threshold,
                old: engine_endpoint(request.old),
                new: engine_endpoint(request.new),
                path: &path,
                max_len: request.max_bytes,
                after: &after,
                after_pos: request.after_position,
            },
            cancel,
        );
        result_status(response.status)?;
        let encoded = match response.payload {
            engine::PatchPayload::Text(bytes) => return Ok(PatchResult::Text(bytes)),
            engine::PatchPayload::Structured(records) => records,
        };
        let mut records = Vec::new();
        for record in encoded {
            records.push(match record {
                engine::OwnedGitPatchRecord::File {
                    st,
                    similarity,
                    flags,
                    old_path,
                    new_path,
                } => PatchRecord::File {
                    status: st,
                    similarity_percent: similarity,
                    binary: flags & engine::GIT_PATCH_FILE_BINARY != 0,
                    filtered: flags & engine::GIT_PATCH_FILE_FILTERED != 0,
                    old_path: decode_optional_path(&old_path)
                        .ok_or_else(|| Failure::malformed("PATCH old path"))?,
                    new_path: decode_optional_path(&new_path)
                        .ok_or_else(|| Failure::malformed("PATCH new path"))?,
                },
                engine::OwnedGitPatchRecord::Row {
                    old_line,
                    new_line,
                    old_text,
                    new_text,
                    old_spans,
                    new_spans,
                } => PatchRecord::Row {
                    old_line,
                    new_line,
                    old_text,
                    new_text,
                    old_spans,
                    new_spans,
                },
                engine::OwnedGitPatchRecord::Gap { old_line, new_line } => {
                    PatchRecord::Gap { old_line, new_line }
                }
                engine::OwnedGitPatchRecord::Base { oid } => PatchRecord::Base(oid),
                engine::OwnedGitPatchRecord::Cursor { after, pos } => PatchRecord::Cursor {
                    after: decode_path(&after).ok_or_else(|| Failure::malformed("PATCH cursor"))?,
                    position: pos,
                },
            });
        }
        Ok(PatchResult::Structured(records))
    }

    pub fn native_index(
        &self,
        path: &[Vec<u8>],
        after: &[Vec<u8>],
        cancel: &Cancel,
    ) -> Result<IndexPage, Failure> {
        let path = encode_path(path)?;
        let after = encode_path(after)?;
        let response = self.index(
            &engine::GitIndexRequest {
                nonce: 1,
                repo_id: 0,
                flags: 0,
                path: &path,
                after: &after,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitIndexRecord::Entry {
                    stage,
                    iflags,
                    mode,
                    size,
                    mtime_ns,
                    oid,
                    path,
                } => IndexRecord::Entry {
                    stage,
                    intent_to_add: iflags & engine::GIT_INDEX_INTENT_TO_ADD != 0,
                    skip_worktree: iflags & engine::GIT_INDEX_SKIP_WORKTREE != 0,
                    mode,
                    size,
                    modified_unix_ns: mtime_ns,
                    object: oid,
                    path: decode_path(&path).ok_or_else(|| Failure::malformed("INDEX path"))?,
                },
                engine::OwnedGitIndexRecord::Cursor { after, .. } => IndexRecord::Cursor(
                    decode_path(&after).ok_or_else(|| Failure::malformed("INDEX cursor"))?,
                ),
            });
        }
        Ok(IndexPage { records })
    }
}

pub(crate) fn state_records(encoded: Vec<engine::OwnedGitStateRecord>) -> Option<Vec<StateRecord>> {
    let mut records = Vec::new();
    for record in encoded {
        records.push(match record {
            engine::OwnedGitStateRecord::Head { flags, oid, name } => StateRecord::Head {
                detached: flags & engine::GIT_HEAD_DETACHED != 0,
                unborn: flags & engine::GIT_HEAD_UNBORN != 0,
                object: oid,
                symbolic_target: name.into_bytes(),
            },
            engine::OwnedGitStateRecord::Ref {
                flags,
                oid,
                peeled,
                name,
                target,
            } => StateRecord::Ref {
                peeled_valid: flags & engine::GIT_REF_PEELED_VALID != 0,
                symbolic: flags & engine::GIT_REF_SYMBOLIC != 0,
                object: oid,
                peeled,
                name: name.into_bytes(),
                symbolic_target: target.into_bytes(),
            },
            engine::OwnedGitStateRecord::Op { op, oid, detail } => StateRecord::Operation {
                kind: op,
                head: oid,
                detail,
            },
            engine::OwnedGitStateRecord::Status {
                staged,
                unstaged,
                flags,
                oid,
                old_path,
                path,
            } => StateRecord::Status {
                index_status: staged,
                worktree_status: unstaged,
                conflicted: flags & engine::GIT_STATUS_ENTRY_CONFLICTED != 0,
                object: oid,
                old_path: decode_optional_path(&old_path)?,
                path: decode_path(&path)?,
            },
            engine::OwnedGitStateRecord::Upstream {
                flags,
                ahead,
                behind,
                name,
                upstream,
            } => StateRecord::Upstream {
                gone: flags & engine::GIT_UPSTREAM_GONE != 0,
                counts_valid: flags & engine::GIT_UPSTREAM_COUNTS_VALID != 0,
                ahead,
                behind,
                name: name.into_bytes(),
                upstream: upstream.into_bytes(),
            },
            engine::OwnedGitStateRecord::Stash {
                index,
                oid,
                time,
                tz,
                msg,
            } => StateRecord::Stash {
                index,
                object: oid,
                created_unix_seconds: time,
                timezone_minutes: tz,
                message: msg.into_bytes(),
            },
            engine::OwnedGitStateRecord::Remote {
                flags,
                name,
                fetch_url,
                push_url,
            } => StateRecord::Remote {
                default: flags & engine::GIT_REMOTE_DEFAULT != 0,
                name: name.into_bytes(),
                fetch_url: fetch_url.into_bytes(),
                push_url: push_url.into_bytes(),
            },
            engine::OwnedGitStateRecord::WorktreeGen { count, digest } => {
                StateRecord::WorktreeGeneration { count, digest }
            }
        });
    }
    Some(records)
}

fn decode_path(value: &str) -> Option<RepoPath> {
    let value = crate::decode_path_bytes(value)?;
    if value.is_empty() {
        return Some(Vec::new());
    }
    value
        .split(|byte| *byte == b'/')
        .map(|component| {
            (!component.is_empty() && !component.contains(&0)).then(|| component.to_vec())
        })
        .collect()
}

fn decode_optional_path(value: &str) -> Option<Option<RepoPath>> {
    if value.is_empty() {
        Some(None)
    } else {
        decode_path(value).map(Some)
    }
}

fn encode_path(path: &[Vec<u8>]) -> Result<String, Failure> {
    let mut encoded = String::new();
    for (index, component) in path.iter().enumerate() {
        if component.is_empty() || component.contains(&b'/') || component.contains(&0) {
            return Err(Failure {
                status: Status::Invalid,
                detail: "invalid Git path component".to_owned(),
            });
        }
        if index != 0 {
            encoded.push('/');
        }
        encoded.push_str(&crate::escape_bstr(component));
    }
    Ok(encoded)
}

pub fn fetch_available() -> bool {
    crate::reads::fetch_available()
}

// Query and fetch operations follow below.  They intentionally own every
// returned string/path/record so no parser lifetime or encoded packet escapes
// the engine crate.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Blob,
    Tree,
    Commit,
}

fn tree_kind(value: u8) -> Result<TreeKind, Failure> {
    match value {
        engine::GIT_OTYPE_BLOB => Ok(TreeKind::Blob),
        engine::GIT_OTYPE_TREE => Ok(TreeKind::Tree),
        engine::GIT_OTYPE_COMMIT => Ok(TreeKind::Commit),
        _ => Err(Failure::malformed("tree object kind")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResult {
    pub tips: Vec<Oid>,
    pub hides: Vec<Oid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRequest {
    pub flags: u8,
    pub limit: u16,
    pub path: RepoPath,
    pub tips: Vec<Oid>,
    pub hides: Vec<Oid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPage {
    pub more: bool,
    pub frontier: Vec<Oid>,
    pub records: Vec<LogRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogRecord {
    Commit {
        lossy_encoding: bool,
        object: Oid,
        tree: Oid,
        parents: Vec<Oid>,
        authored_unix_seconds: i64,
        author_timezone_minutes: i16,
        committed_unix_seconds: i64,
        committer_timezone_minutes: i16,
        author_name: Vec<u8>,
        author_email: Vec<u8>,
        committer_name: Vec<u8>,
        committer_email: Vec<u8>,
        message: Vec<u8>,
    },
    PathAt {
        kind: TreeKind,
        mode: u32,
        object: Oid,
        path: RepoPath,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreePage {
    pub records: Vec<TreeRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRecord {
    Entry {
        kind: TreeKind,
        mode: u32,
        object: Oid,
        name: Vec<u8>,
    },
    Cursor {
        after: RepoPath,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobResult {
    pub byte_len: u64,
    pub bytes: Vec<u8>,
}

impl RepoHandle {
    pub fn native_resolve(&self, spec: &str, cancel: &Cancel) -> Result<ResolveResult, Failure> {
        let response = self.resolve(1, spec, cancel);
        result_status(response.status)?;
        Ok(ResolveResult {
            tips: response.tips,
            hides: response.hides,
        })
    }

    pub fn native_merge_base(&self, objects: &[Oid], cancel: &Cancel) -> Result<Vec<Oid>, Failure> {
        let response = self.base(1, objects, cancel);
        result_status(response.status)?;
        Ok(response.value)
    }

    pub fn native_log(&self, request: &LogRequest, cancel: &Cancel) -> Result<LogPage, Failure> {
        let path = encode_path(&request.path)?;
        let response = self.log(
            &engine::GitLogRequest {
                nonce: 1,
                repo_id: 0,
                flags: request.flags,
                limit: request.limit,
                path: &path,
                tips: request.tips.clone(),
                hides: request.hides.clone(),
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.records {
            records.push(match record {
                engine::OwnedGitCommitRecord::Commit {
                    flags,
                    oid,
                    tree,
                    parents,
                    author_time,
                    author_tz,
                    committer_time,
                    committer_tz,
                    author_name,
                    author_email,
                    committer_name,
                    committer_email,
                    message,
                } => LogRecord::Commit {
                    lossy_encoding: flags & engine::GIT_COMMIT_LOSSY_ENCODING != 0,
                    object: oid,
                    tree,
                    parents,
                    authored_unix_seconds: author_time,
                    author_timezone_minutes: author_tz,
                    committed_unix_seconds: committer_time,
                    committer_timezone_minutes: committer_tz,
                    author_name: author_name.into_bytes(),
                    author_email: author_email.into_bytes(),
                    committer_name: committer_name.into_bytes(),
                    committer_email: committer_email.into_bytes(),
                    message: message.into_bytes(),
                },
                engine::OwnedGitCommitRecord::PathAt {
                    otype,
                    mode,
                    oid,
                    path,
                } => LogRecord::PathAt {
                    kind: tree_kind(otype)?,
                    mode,
                    object: oid,
                    path: decode_path(&path).ok_or_else(|| Failure::malformed("LOG path"))?,
                },
            });
        }
        Ok(LogPage {
            more: response.flags & engine::GIT_COMMITS_MORE != 0,
            frontier: response.frontier,
            records,
        })
    }

    pub fn native_tree(
        &self,
        tree: Oid,
        path: &[Vec<u8>],
        after: &[Vec<u8>],
        cancel: &Cancel,
    ) -> Result<TreePage, Failure> {
        let path = encode_path(path)?;
        let after = encode_path(after)?;
        let response = self.tree(
            &engine::GitTreeRequest {
                nonce: 1,
                repo_id: 0,
                flags: 0,
                oid: tree,
                path: &path,
                after: &after,
            },
            cancel,
        );
        result_status(response.status)?;
        let mut records = Vec::new();
        for record in response.value {
            records.push(match record {
                engine::OwnedGitTreeRecord::Entry {
                    otype,
                    mode,
                    oid,
                    name,
                } => TreeRecord::Entry {
                    kind: tree_kind(otype)?,
                    mode,
                    object: oid,
                    name: crate::decode_path_bytes(&name)
                        .filter(|name| {
                            !name.is_empty() && !name.contains(&b'/') && !name.contains(&0)
                        })
                        .ok_or_else(|| Failure::malformed("TREE name"))?,
                },
                engine::OwnedGitTreeRecord::Cursor { after, .. } => TreeRecord::Cursor {
                    after: decode_path(&after).ok_or_else(|| Failure::malformed("TREE cursor"))?,
                },
            });
        }
        Ok(TreePage { records })
    }

    pub fn native_blob(
        &self,
        object: Oid,
        path: Option<&[Vec<u8>]>,
        offset: u64,
        max_bytes: u32,
        flags: u8,
    ) -> Result<BlobResult, Failure> {
        let path = path.map(encode_path).transpose()?.unwrap_or_default();
        let response = self.blob(&engine::GitBlobRequest {
            nonce: 1,
            repo_id: 0,
            flags,
            oid: object,
            path: &path,
            offset,
            max_len: max_bytes,
        });
        result_status(response.status)?;
        Ok(BlobResult {
            byte_len: response.size,
            bytes: response.data,
        })
    }
}
