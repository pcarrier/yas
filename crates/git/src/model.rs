//! Semantic values shared by the Git engine's worker modules.
//!
//! These are deliberately not a object model.  Requests are borrowed values,
//! responses own their records, and state events move typed snapshots straight
//! to subscribers.

pub type GitOid = [u8; 32];
pub const GIT_OID_NONE: GitOid = [0; 32];
pub const MAX_ENGINE_BYTES: usize = 64 * 1024 * 1024;

pub const GIT_STATUS_OK: u8 = 0;
pub const GIT_STATUS_UNKNOWN_ID: u8 = 1;
pub const GIT_STATUS_NOT_FOUND: u8 = 2;
pub const GIT_STATUS_WRONG_TYPE: u8 = 3;
pub const GIT_STATUS_PERMISSION: u8 = 4;
pub const GIT_STATUS_TOO_LARGE: u8 = 5;
pub const GIT_STATUS_BUDGET: u8 = 6;
pub const GIT_STATUS_INVALID: u8 = 7;
pub const GIT_STATUS_CANCELLED: u8 = 8;
pub const GIT_STATUS_OTHER: u8 = 9;
pub const GIT_STATUS_CONFLICT: u8 = 11;
pub const GIT_STATUS_NO_MERGE_BASE: u8 = 12;

pub const GIT_OID_FORMAT_SHA1: u8 = 0;
pub const GIT_OID_FORMAT_SHA256: u8 = 1;
pub const GIT_REPO_BARE: u8 = 1 << 0;
pub const GIT_REPO_SHALLOW: u8 = 1 << 1;
pub const GIT_REPO_SPARSE: u8 = 1 << 2;
pub const GIT_REPO_LINKED: u8 = 1 << 3;
pub const GIT_REPO_WRITABLE: u8 = 1 << 4;
pub const GIT_REPO_FETCHABLE: u8 = 1 << 5;

pub const GIT_CLOSED_BACKEND_FAILED: u8 = 3;
pub const GIT_CLOSED_RESOURCE_LIMIT: u8 = 4;

pub const GIT_STATE_REFS_TRUNCATED: u8 = 1 << 0;
pub const GIT_STATE_STATUS_TRUNCATED: u8 = 1 << 1;
pub const GIT_LOG_FIRST_PARENT: u8 = 1 << 0;
pub const GIT_LOG_TOPO: u8 = 1 << 1;
pub const GIT_LOG_FULL_MESSAGE: u8 = 1 << 2;
pub const GIT_LOG_FOLLOW: u8 = 1 << 3;
pub const GIT_LOG_PATH_OIDS: u8 = 1 << 4;
pub const GIT_COMMITS_MORE: u8 = 1 << 0;
pub const GIT_BLOB_WHOLE: u8 = 1 << 0;
pub const GIT_DIFF_RENAMES: u8 = 1 << 0;
pub const GIT_DIFF_UNTRACKED: u8 = 1 << 1;
pub const GIT_DIFF_IGNORED: u8 = 1 << 2;
pub const GIT_DIFF_IGNORE_SPACE_CHANGE: u8 = 1 << 3;
pub const GIT_DIFF_IGNORE_ALL_SPACE: u8 = 1 << 4;
pub const GIT_DIFF_RAW: u8 = 1 << 5;
pub const GIT_PATCH_RENAMES: u16 = GIT_DIFF_RENAMES as u16;
pub const GIT_PATCH_UNTRACKED: u16 = GIT_DIFF_UNTRACKED as u16;
pub const GIT_PATCH_IGNORED: u16 = GIT_DIFF_IGNORED as u16;
pub const GIT_PATCH_IGNORE_SPACE_CHANGE: u16 = GIT_DIFF_IGNORE_SPACE_CHANGE as u16;
pub const GIT_PATCH_IGNORE_ALL_SPACE: u16 = GIT_DIFF_IGNORE_ALL_SPACE as u16;
pub const GIT_PATCH_RAW: u16 = GIT_DIFF_RAW as u16;
pub const GIT_PATCH_TEXT: u16 = 1 << 6;
pub const GIT_PATCH_CHAR_SPANS: u16 = 1 << 7;
pub const GIT_PATCH_NO_SPANS: u16 = 1 << 8;
pub const GIT_PATCH_BINARY: u16 = 1 << 9;
pub const GIT_TREE_TRUNCATED: u8 = 1 << 0;
pub const GIT_DIFF_TRUNCATED: u8 = 1 << 0;
pub const GIT_DIFF_RENAME_LIMIT: u8 = 1 << 1;
pub const GIT_INDEX_TRUNCATED: u8 = 1 << 0;
pub const GIT_PATCH_STRUCTURED: u8 = 1 << 0;
pub const GIT_PATCH_TRUNCATED: u8 = 1 << 1;
pub const GIT_DISCOVER_TRUNCATED: u8 = 1 << 0;
pub const GIT_BLAME_TRUNCATED: u8 = 1 << 0;
pub const GIT_REFLOG_TRUNCATED: u8 = 1 << 0;
pub const GIT_WORKTREES_TRUNCATED: u8 = 1 << 0;
pub const GIT_DISCOVER_NESTED: u8 = 1 << 0;
pub const GIT_DISCOVER_BARE: u8 = 1 << 1;
pub const GIT_BLAME_FOLLOW_RENAMES: u8 = 1 << 0;
pub const GIT_BLAME_FOLLOW_COPIES: u8 = 1 << 1;
pub const GIT_REFLOG_OLDEST_FIRST: u8 = 1 << 0;
pub const GIT_FETCH_PRUNE: u8 = 1 << 0;
pub const GIT_FETCH_NO_TAGS: u8 = 1 << 1;
pub const GIT_FETCH_ANCHOR: u8 = 1 << 2;
pub const GIT_ENDPOINT_EMPTY: u8 = 0;
pub const GIT_ENDPOINT_COMMIT: u8 = 1;
pub const GIT_ENDPOINT_TREE: u8 = 2;
pub const GIT_ENDPOINT_INDEX: u8 = 3;
pub const GIT_ENDPOINT_WORKTREE: u8 = 4;
pub const GIT_ENDPOINT_MERGE_BASE: u8 = 5;
pub const GIT_REMOTE_DEFAULT: u8 = 1 << 0;
pub const GIT_HEAD_DETACHED: u8 = 1 << 0;
pub const GIT_HEAD_UNBORN: u8 = 1 << 1;
pub const GIT_REF_PEELED_VALID: u8 = 1 << 0;
pub const GIT_REF_SYMBOLIC: u8 = 1 << 1;
pub const GIT_OP_MERGE: u8 = 1;
pub const GIT_OP_REBASE: u8 = 2;
pub const GIT_OP_CHERRY_PICK: u8 = 3;
pub const GIT_OP_REVERT: u8 = 4;
pub const GIT_OP_BISECT: u8 = 5;
pub const GIT_STATUS_ENTRY_CONFLICTED: u8 = 1 << 0;
pub const GIT_UPSTREAM_GONE: u8 = 1 << 0;
pub const GIT_UPSTREAM_COUNTS_VALID: u8 = 1 << 1;
pub const GIT_COMMIT_LOSSY_ENCODING: u8 = 1 << 0;
pub const GIT_OTYPE_COMMIT: u8 = 1;
pub const GIT_OTYPE_TREE: u8 = 2;
pub const GIT_OTYPE_BLOB: u8 = 3;
pub const GIT_DIFF_ENTRY_BINARY: u8 = 1 << 0;
pub const GIT_DIFF_ENTRY_SUBMODULE: u8 = 1 << 1;
pub const GIT_DIFF_ENTRY_FILTERED: u8 = 1 << 2;
pub const GIT_PATCH_FILE_BINARY: u8 = 1 << 0;
pub const GIT_PATCH_FILE_FILTERED: u8 = 1 << 1;
pub const GIT_WORKTREE_MAIN: u8 = 1 << 0;
pub const GIT_WORKTREE_CURRENT: u8 = 1 << 1;
pub const GIT_WORKTREE_LOCKED: u8 = 1 << 2;
pub const GIT_WORKTREE_PRUNABLE: u8 = 1 << 3;
pub const GIT_WORKTREE_DETACHED: u8 = 1 << 4;
pub const GIT_WORKTREE_BARE: u8 = 1 << 5;
pub const GIT_FOUND_BARE: u8 = 1 << 0;
pub const GIT_FOUND_LINKED: u8 = 1 << 1;
pub const GIT_FOUND_SUBMODULE: u8 = 1 << 2;
pub const GIT_FETCH_REF_FORCED: u8 = 1 << 0;
pub const GIT_FETCH_REF_PRUNED: u8 = 1 << 1;
pub const GIT_FETCH_REF_NEW: u8 = 1 << 2;
pub const GIT_FETCH_REF_TAG_UPDATE: u8 = 1 << 3;
pub const GIT_INDEX_INTENT_TO_ADD: u8 = 1 << 0;
pub const GIT_INDEX_SKIP_WORKTREE: u8 = 1 << 1;
pub const GIT_RENAME_MAX: u8 = 100;

pub fn git_status_text(status: u8) -> &'static str {
    match status {
        GIT_STATUS_OK => "ok",
        GIT_STATUS_UNKNOWN_ID => "unknown repository",
        GIT_STATUS_NOT_FOUND => "not found",
        GIT_STATUS_WRONG_TYPE => "wrong object type",
        GIT_STATUS_PERMISSION => "permission denied",
        GIT_STATUS_TOO_LARGE => "too large",
        GIT_STATUS_BUDGET => "budget exhausted",
        GIT_STATUS_INVALID => "invalid request",
        GIT_STATUS_CANCELLED => "cancelled",
        GIT_STATUS_OTHER => "backend error",
        GIT_STATUS_CONFLICT => "conflict",
        GIT_STATUS_NO_MERGE_BASE => "no merge base",
        _ => "unknown status",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitEndpoint {
    pub kind: u8,
    pub oid: GitOid,
}

macro_rules! request {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct $name<'a> { $(pub $field: $ty,)* }
    };
}

request!(GitLogRequest { nonce: u16, repo_id: u16, flags: u8, limit: u16, path: &'a str, tips: Vec<GitOid>, hides: Vec<GitOid> });
request!(GitTreeRequest { nonce: u16, repo_id: u16, flags: u8, oid: GitOid, path: &'a str, after: &'a str });
request!(GitBlobRequest { nonce: u16, repo_id: u16, flags: u8, oid: GitOid, path: &'a str, offset: u64, max_len: u32 });
request!(GitDiffRequest { nonce: u16, repo_id: u16, flags: u8, rename: u8, old: GitEndpoint, new: GitEndpoint, path: &'a str, after: &'a str });
request!(GitPatchRequest { nonce: u16, repo_id: u16, flags: u16, context: u8, rename: u8, old: GitEndpoint, new: GitEndpoint, path: &'a str, max_len: u32, after: &'a str, after_pos: u64 });
request!(GitIndexRequest { nonce: u16, repo_id: u16, flags: u8, path: &'a str, after: &'a str });
request!(GitBlameRequest { nonce: u16, repo_id: u16, flags: u8, oid: GitOid, start_line: u32, line_count: u32, path: &'a str });
request!(GitReflogRequest { nonce: u16, repo_id: u16, flags: u8, limit: u16, ref_name: &'a str, after_pos: u64 });
request!(GitFetchRequest { nonce: u16, repo_id: u16, flags: u8, timeout_ms: u32, remote: &'a str, refspecs: Vec<&'a str> });

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitWorktreesRequest {
    pub nonce: u16,
    pub repo_id: u16,
    pub flags: u8,
    pub after_pos: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitStateRecord<'a> {
    Head {
        flags: u8,
        oid: GitOid,
        name: &'a str,
    },
    Ref {
        flags: u8,
        oid: GitOid,
        peeled: GitOid,
        name: &'a str,
        target: &'a str,
    },
    Op {
        op: u8,
        oid: GitOid,
        detail: &'a str,
    },
    Status {
        staged: u8,
        unstaged: u8,
        flags: u8,
        oid: GitOid,
        old_path: &'a str,
        path: &'a str,
    },
    Upstream {
        flags: u8,
        ahead: u32,
        behind: u32,
        name: &'a str,
        upstream: &'a str,
    },
    Stash {
        index: u16,
        oid: GitOid,
        time: i64,
        tz: i16,
        msg: &'a str,
    },
    Remote {
        flags: u8,
        name: &'a str,
        fetch_url: &'a str,
        push_url: &'a str,
    },
    WorktreeGen {
        count: u32,
        digest: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitStateRecord {
    Head {
        flags: u8,
        oid: GitOid,
        name: String,
    },
    Ref {
        flags: u8,
        oid: GitOid,
        peeled: GitOid,
        name: String,
        target: String,
    },
    Op {
        op: u8,
        oid: GitOid,
        detail: String,
    },
    Status {
        staged: u8,
        unstaged: u8,
        flags: u8,
        oid: GitOid,
        old_path: String,
        path: String,
    },
    Upstream {
        flags: u8,
        ahead: u32,
        behind: u32,
        name: String,
        upstream: String,
    },
    Stash {
        index: u16,
        oid: GitOid,
        time: i64,
        tz: i16,
        msg: String,
    },
    Remote {
        flags: u8,
        name: String,
        fetch_url: String,
        push_url: String,
    },
    WorktreeGen {
        count: u32,
        digest: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitCommitRecord<'a> {
    Commit {
        flags: u8,
        oid: GitOid,
        tree: GitOid,
        parents: Vec<GitOid>,
        author_time: i64,
        author_tz: i16,
        committer_time: i64,
        committer_tz: i16,
        author_name: &'a str,
        author_email: &'a str,
        committer_name: &'a str,
        committer_email: &'a str,
        message: &'a str,
    },
    PathAt {
        otype: u8,
        mode: u32,
        oid: GitOid,
        path: &'a str,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitCommitRecord {
    Commit {
        flags: u8,
        oid: GitOid,
        tree: GitOid,
        parents: Vec<GitOid>,
        author_time: i64,
        author_tz: i16,
        committer_time: i64,
        committer_tz: i16,
        author_name: String,
        author_email: String,
        committer_name: String,
        committer_email: String,
        message: String,
    },
    PathAt {
        otype: u8,
        mode: u32,
        oid: GitOid,
        path: String,
    },
}

// Kept explicit so the engine accepts borrowed construction values but
// stores owned records in responses and snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitTreeRecord<'a> {
    Entry {
        otype: u8,
        mode: u32,
        oid: GitOid,
        name: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitTreeRecord {
    Entry {
        otype: u8,
        mode: u32,
        oid: GitOid,
        name: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitDiffRecord<'a> {
    Entry {
        st: u8,
        similarity: u8,
        dflags: u8,
        old_mode: u32,
        new_mode: u32,
        old_oid: GitOid,
        new_oid: GitOid,
        old_path: &'a str,
        new_path: &'a str,
    },
    Base {
        oid: GitOid,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitDiffRecord {
    Entry {
        st: u8,
        similarity: u8,
        dflags: u8,
        old_mode: u32,
        new_mode: u32,
        old_oid: GitOid,
        new_oid: GitOid,
        old_path: String,
        new_path: String,
    },
    Base {
        oid: GitOid,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitPatchRecord<'a> {
    File {
        st: u8,
        similarity: u8,
        flags: u8,
        old_path: &'a str,
        new_path: &'a str,
    },
    Row {
        old_line: u32,
        new_line: u32,
        old_text: &'a [u8],
        new_text: &'a [u8],
        old_spans: Vec<(u32, u32)>,
        new_spans: Vec<(u32, u32)>,
    },
    Gap {
        old_line: u32,
        new_line: u32,
    },
    Base {
        oid: GitOid,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitPatchRecord {
    File {
        st: u8,
        similarity: u8,
        flags: u8,
        old_path: String,
        new_path: String,
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
    Base {
        oid: GitOid,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitIndexRecord<'a> {
    Entry {
        stage: u8,
        iflags: u8,
        mode: u32,
        size: u64,
        mtime_ns: u64,
        oid: GitOid,
        path: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitIndexRecord {
    Entry {
        stage: u8,
        iflags: u8,
        mode: u32,
        size: u64,
        mtime_ns: u64,
        oid: GitOid,
        path: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitDiscoverRecord<'a> {
    Repo {
        flags: u8,
        workdir: &'a str,
        gitdir: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitDiscoverRecord {
    Repo {
        flags: u8,
        workdir: String,
        gitdir: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitBlameRecord<'a> {
    Range {
        flags: u8,
        commit: GitOid,
        start_line: u32,
        line_count: u32,
        orig_start: u32,
        orig_path: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitBlameRecord {
    Range {
        flags: u8,
        commit: GitOid,
        start_line: u32,
        line_count: u32,
        orig_start: u32,
        orig_path: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitReflogRecord<'a> {
    Entry {
        flags: u8,
        old: GitOid,
        new: GitOid,
        time: i64,
        tz: i16,
        msg: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitReflogRecord {
    Entry {
        flags: u8,
        old: GitOid,
        new: GitOid,
        time: i64,
        tz: i16,
        msg: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitFetchRecord<'a> {
    Ref {
        flags: u8,
        status: u8,
        old: GitOid,
        new: GitOid,
        name: &'a str,
        detail: &'a str,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitFetchRecord {
    Ref {
        flags: u8,
        status: u8,
        old: GitOid,
        new: GitOid,
        name: String,
        detail: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitWorktreeRecord<'a> {
    Tree {
        flags: u8,
        oid: GitOid,
        path: &'a str,
        branch: &'a str,
        lock_reason: &'a str,
    },
    Cursor {
        after: &'a str,
        pos: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedGitWorktreeRecord {
    Tree {
        flags: u8,
        oid: GitOid,
        path: String,
        branch: String,
        lock_reason: String,
    },
    Cursor {
        after: String,
        pos: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response<T> {
    pub status: u8,
    pub flags: u8,
    pub value: T,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitsResponse {
    pub status: u8,
    pub flags: u8,
    pub frontier: Vec<GitOid>,
    pub records: Vec<OwnedGitCommitRecord>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobResponse {
    pub status: u8,
    pub size: u64,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResponse {
    pub status: u8,
    pub tips: Vec<GitOid>,
    pub hides: Vec<GitOid>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchResponse {
    pub status: u8,
    pub flags: u8,
    pub payload: PatchPayload,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchPayload {
    Text(Vec<u8>),
    Structured(Vec<OwnedGitPatchRecord>),
}

macro_rules! push_owned {
    ($fn:ident, $borrowed:ident, $owned:ident, { $($variant:ident { $($field:ident),* $(,)? } => { $($build:tt)* }),* $(,)? }) => {
        pub fn $fn(out: &mut Vec<$owned>, value: &$borrowed<'_>) {
            out.push(match value { $($borrowed::$variant { $($field,)* } => $owned::$variant { $($build)* },)* });
        }
    };
}

push_owned!(push_git_state_record, GitStateRecord, OwnedGitStateRecord, {
    Head { flags, oid, name } => { flags: *flags, oid: *oid, name: (*name).to_owned() },
    Ref { flags, oid, peeled, name, target } => { flags: *flags, oid: *oid, peeled: *peeled, name: (*name).to_owned(), target: (*target).to_owned() },
    Op { op, oid, detail } => { op: *op, oid: *oid, detail: (*detail).to_owned() },
    Status { staged, unstaged, flags, oid, old_path, path } => { staged: *staged, unstaged: *unstaged, flags: *flags, oid: *oid, old_path: (*old_path).to_owned(), path: (*path).to_owned() },
    Upstream { flags, ahead, behind, name, upstream } => { flags: *flags, ahead: *ahead, behind: *behind, name: (*name).to_owned(), upstream: (*upstream).to_owned() },
    Stash { index, oid, time, tz, msg } => { index: *index, oid: *oid, time: *time, tz: *tz, msg: (*msg).to_owned() },
    Remote { flags, name, fetch_url, push_url } => { flags: *flags, name: (*name).to_owned(), fetch_url: (*fetch_url).to_owned(), push_url: (*push_url).to_owned() },
    WorktreeGen { count, digest } => { count: *count, digest: *digest }
});

pub fn push_git_commit_record(out: &mut Vec<OwnedGitCommitRecord>, value: &GitCommitRecord<'_>) {
    out.push(match value {
        GitCommitRecord::Commit {
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
        } => OwnedGitCommitRecord::Commit {
            flags: *flags,
            oid: *oid,
            tree: *tree,
            parents: parents.clone(),
            author_time: *author_time,
            author_tz: *author_tz,
            committer_time: *committer_time,
            committer_tz: *committer_tz,
            author_name: (*author_name).to_owned(),
            author_email: (*author_email).to_owned(),
            committer_name: (*committer_name).to_owned(),
            committer_email: (*committer_email).to_owned(),
            message: (*message).to_owned(),
        },
        GitCommitRecord::PathAt {
            otype,
            mode,
            oid,
            path,
        } => OwnedGitCommitRecord::PathAt {
            otype: *otype,
            mode: *mode,
            oid: *oid,
            path: (*path).to_owned(),
        },
    });
}

macro_rules! simple_push {
    ($fn:ident, $borrowed:ident, $owned:ident, $body:expr) => {
        pub fn $fn(out: &mut Vec<$owned>, value: &$borrowed<'_>) {
            out.push(($body)(value));
        }
    };
}
simple_push!(
    push_git_tree_record,
    GitTreeRecord,
    OwnedGitTreeRecord,
    |v: &GitTreeRecord<'_>| match v {
        GitTreeRecord::Entry {
            otype,
            mode,
            oid,
            name,
        } => OwnedGitTreeRecord::Entry {
            otype: *otype,
            mode: *mode,
            oid: *oid,
            name: (*name).to_owned()
        },
        GitTreeRecord::Cursor { after, pos } => OwnedGitTreeRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_diff_record,
    GitDiffRecord,
    OwnedGitDiffRecord,
    |v: &GitDiffRecord<'_>| match v {
        GitDiffRecord::Entry {
            st,
            similarity,
            dflags,
            old_mode,
            new_mode,
            old_oid,
            new_oid,
            old_path,
            new_path,
        } => OwnedGitDiffRecord::Entry {
            st: *st,
            similarity: *similarity,
            dflags: *dflags,
            old_mode: *old_mode,
            new_mode: *new_mode,
            old_oid: *old_oid,
            new_oid: *new_oid,
            old_path: (*old_path).to_owned(),
            new_path: (*new_path).to_owned()
        },
        GitDiffRecord::Base { oid } => OwnedGitDiffRecord::Base { oid: *oid },
        GitDiffRecord::Cursor { after, pos } => OwnedGitDiffRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_patch_record,
    GitPatchRecord,
    OwnedGitPatchRecord,
    |v: &GitPatchRecord<'_>| match v {
        GitPatchRecord::File {
            st,
            similarity,
            flags,
            old_path,
            new_path,
        } => OwnedGitPatchRecord::File {
            st: *st,
            similarity: *similarity,
            flags: *flags,
            old_path: (*old_path).to_owned(),
            new_path: (*new_path).to_owned()
        },
        GitPatchRecord::Row {
            old_line,
            new_line,
            old_text,
            new_text,
            old_spans,
            new_spans,
        } => OwnedGitPatchRecord::Row {
            old_line: *old_line,
            new_line: *new_line,
            old_text: (*old_text).to_vec(),
            new_text: (*new_text).to_vec(),
            old_spans: old_spans.clone(),
            new_spans: new_spans.clone()
        },
        GitPatchRecord::Gap { old_line, new_line } => OwnedGitPatchRecord::Gap {
            old_line: *old_line,
            new_line: *new_line
        },
        GitPatchRecord::Base { oid } => OwnedGitPatchRecord::Base { oid: *oid },
        GitPatchRecord::Cursor { after, pos } => OwnedGitPatchRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_index_record,
    GitIndexRecord,
    OwnedGitIndexRecord,
    |v: &GitIndexRecord<'_>| match v {
        GitIndexRecord::Entry {
            stage,
            iflags,
            mode,
            size,
            mtime_ns,
            oid,
            path,
        } => OwnedGitIndexRecord::Entry {
            stage: *stage,
            iflags: *iflags,
            mode: *mode,
            size: *size,
            mtime_ns: *mtime_ns,
            oid: *oid,
            path: (*path).to_owned()
        },
        GitIndexRecord::Cursor { after, pos } => OwnedGitIndexRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_discover_record,
    GitDiscoverRecord,
    OwnedGitDiscoverRecord,
    |v: &GitDiscoverRecord<'_>| match v {
        GitDiscoverRecord::Repo {
            flags,
            workdir,
            gitdir,
        } => OwnedGitDiscoverRecord::Repo {
            flags: *flags,
            workdir: (*workdir).to_owned(),
            gitdir: (*gitdir).to_owned()
        },
        GitDiscoverRecord::Cursor { after, pos } => OwnedGitDiscoverRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_blame_record,
    GitBlameRecord,
    OwnedGitBlameRecord,
    |v: &GitBlameRecord<'_>| match v {
        GitBlameRecord::Range {
            flags,
            commit,
            start_line,
            line_count,
            orig_start,
            orig_path,
        } => OwnedGitBlameRecord::Range {
            flags: *flags,
            commit: *commit,
            start_line: *start_line,
            line_count: *line_count,
            orig_start: *orig_start,
            orig_path: (*orig_path).to_owned()
        },
        GitBlameRecord::Cursor { after, pos } => OwnedGitBlameRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_reflog_record,
    GitReflogRecord,
    OwnedGitReflogRecord,
    |v: &GitReflogRecord<'_>| match v {
        GitReflogRecord::Entry {
            flags,
            old,
            new,
            time,
            tz,
            msg,
        } => OwnedGitReflogRecord::Entry {
            flags: *flags,
            old: *old,
            new: *new,
            time: *time,
            tz: *tz,
            msg: (*msg).to_owned()
        },
        GitReflogRecord::Cursor { after, pos } => OwnedGitReflogRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);
simple_push!(
    push_git_fetch_record,
    GitFetchRecord,
    OwnedGitFetchRecord,
    |v: &GitFetchRecord<'_>| match v {
        GitFetchRecord::Ref {
            flags,
            status,
            old,
            new,
            name,
            detail,
        } => OwnedGitFetchRecord::Ref {
            flags: *flags,
            status: *status,
            old: *old,
            new: *new,
            name: (*name).to_owned(),
            detail: (*detail).to_owned()
        },
    }
);
simple_push!(
    push_git_worktree_record,
    GitWorktreeRecord,
    OwnedGitWorktreeRecord,
    |v: &GitWorktreeRecord<'_>| match v {
        GitWorktreeRecord::Tree {
            flags,
            oid,
            path,
            branch,
            lock_reason,
        } => OwnedGitWorktreeRecord::Tree {
            flags: *flags,
            oid: *oid,
            path: (*path).to_owned(),
            branch: (*branch).to_owned(),
            lock_reason: (*lock_reason).to_owned()
        },
        GitWorktreeRecord::Cursor { after, pos } => OwnedGitWorktreeRecord::Cursor {
            after: (*after).to_owned(),
            pos: *pos
        },
    }
);

pub fn commits_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    frontier: &[GitOid],
    records: &[OwnedGitCommitRecord],
) -> CommitsResponse {
    CommitsResponse {
        status,
        flags,
        frontier: frontier.to_vec(),
        records: records.to_vec(),
    }
}
pub fn tree_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitTreeRecord],
) -> Response<Vec<OwnedGitTreeRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn blob_response(_nonce: u16, status: u8, size: u64, data: &[u8]) -> BlobResponse {
    BlobResponse {
        status,
        size,
        data: data.to_vec(),
    }
}
pub fn diff_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitDiffRecord],
) -> Response<Vec<OwnedGitDiffRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn patch_text_response(_nonce: u16, status: u8, flags: u8, data: &[u8]) -> PatchResponse {
    PatchResponse {
        status,
        flags,
        payload: PatchPayload::Text(data.to_vec()),
    }
}
pub fn patch_records_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitPatchRecord],
) -> PatchResponse {
    PatchResponse {
        status,
        flags,
        payload: PatchPayload::Structured(records.to_vec()),
    }
}
pub fn index_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitIndexRecord],
) -> Response<Vec<OwnedGitIndexRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn base_response(_nonce: u16, status: u8, bases: &[GitOid]) -> Response<Vec<GitOid>> {
    Response {
        status,
        flags: 0,
        value: bases.to_vec(),
    }
}
pub fn resolve_response(
    _nonce: u16,
    status: u8,
    tips: &[GitOid],
    hides: &[GitOid],
) -> ResolveResponse {
    ResolveResponse {
        status,
        tips: tips.to_vec(),
        hides: hides.to_vec(),
    }
}
pub fn discover_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitDiscoverRecord],
) -> Response<Vec<OwnedGitDiscoverRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn blame_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitBlameRecord],
) -> Response<Vec<OwnedGitBlameRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn reflog_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitReflogRecord],
) -> Response<Vec<OwnedGitReflogRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn fetch_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitFetchRecord],
) -> Response<Vec<OwnedGitFetchRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}
pub fn worktrees_response(
    _nonce: u16,
    status: u8,
    flags: u8,
    records: &[OwnedGitWorktreeRecord],
) -> Response<Vec<OwnedGitWorktreeRecord>> {
    Response {
        status,
        flags,
        value: records.to_vec(),
    }
}

pub fn patch_records_size(records: &[OwnedGitPatchRecord]) -> usize {
    records
        .iter()
        .map(|record| match record {
            OwnedGitPatchRecord::File {
                old_path, new_path, ..
            } => 16 + old_path.len() + new_path.len(),
            OwnedGitPatchRecord::Row {
                old_text,
                new_text,
                old_spans,
                new_spans,
                ..
            } => 24 + old_text.len() + new_text.len() + 8 * (old_spans.len() + new_spans.len()),
            OwnedGitPatchRecord::Gap { .. } => 12,
            OwnedGitPatchRecord::Base { .. } => 36,
            OwnedGitPatchRecord::Cursor { after, .. } => 16 + after.len(),
        })
        .sum()
}

pub fn commit_records_size(records: &[OwnedGitCommitRecord]) -> usize {
    records
        .iter()
        .map(|record| match record {
            OwnedGitCommitRecord::Commit {
                parents,
                author_name,
                author_email,
                committer_name,
                committer_email,
                message,
                ..
            } => {
                96 + 32 * parents.len()
                    + author_name.len()
                    + author_email.len()
                    + committer_name.len()
                    + committer_email.len()
                    + message.len()
            }
            OwnedGitCommitRecord::PathAt { path, .. } => 44 + path.len(),
        })
        .sum()
}
