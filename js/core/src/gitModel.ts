import type { SessionId } from "./types";

export const GIT_STATUS_OK = 0;
export const GIT_STATUS_UNKNOWN_ID = 1;
export const GIT_STATUS_NOT_FOUND = 2;
export const GIT_STATUS_WRONG_TYPE = 3;
export const GIT_STATUS_PERMISSION = 4;
export const GIT_STATUS_TOO_LARGE = 5;
export const GIT_STATUS_BUDGET = 6;
export const GIT_STATUS_INVALID = 7;
export const GIT_STATUS_CANCELLED = 8;
export const GIT_STATUS_OTHER = 9;
export const GIT_STATUS_CONFLICT = 11;
export const GIT_STATUS_NO_MERGE_BASE = 12;

export const GIT_OID_FORMAT_SHA1 = 0;
export const GIT_OID_FORMAT_SHA256 = 1;

export const GIT_CLOSED_CLIENT_REQUEST = 0;
export const GIT_CLOSED_REPO_GONE = 1;
export const GIT_CLOSED_PERMISSION_LOST = 2;
export const GIT_CLOSED_BACKEND_FAILED = 3;
export const GIT_CLOSED_RESOURCE_LIMIT = 4;
export const GIT_CLOSED_CONNECTION_LOST = -1;

export const GIT_LOG_FIRST_PARENT = 1 << 0;
export const GIT_LOG_TOPO = 1 << 1;
export const GIT_LOG_FULL_MESSAGE = 1 << 2;
export const GIT_LOG_FOLLOW = 1 << 3;
export const GIT_LOG_PATH_OIDS = 1 << 4;
export const GIT_COMMITS_MORE = 1 << 0;

export const GIT_DIFF_RENAMES = 1 << 0;
export const GIT_DIFF_UNTRACKED = 1 << 1;
export const GIT_DIFF_IGNORED = 1 << 2;
export const GIT_DIFF_IGNORE_SPACE_CHANGE = 1 << 3;
export const GIT_DIFF_IGNORE_ALL_SPACE = 1 << 4;
export const GIT_DIFF_RAW = 1 << 5;
export const GIT_PATCH_TEXT = 1 << 6;
export const GIT_PATCH_STRUCTURED = 1 << 0;
export const GIT_PATCH_TRUNCATED = 1 << 1;

export const GIT_ENDPOINT_EMPTY = 0;
export const GIT_ENDPOINT_COMMIT = 1;
export const GIT_ENDPOINT_TREE = 2;
export const GIT_ENDPOINT_INDEX = 3;
export const GIT_ENDPOINT_WORKTREE = 4;
export const GIT_ENDPOINT_MERGE_BASE = 5;

export const GIT_HEAD_DETACHED = 1 << 0;
export const GIT_HEAD_UNBORN = 1 << 1;
export const GIT_REF_PEELED_VALID = 1 << 0;
export const GIT_REF_SYMBOLIC = 1 << 1;
export const GIT_OP_MERGE = 1;
export const GIT_OP_REBASE = 2;
export const GIT_OP_CHERRY_PICK = 3;
export const GIT_OP_REVERT = 4;
export const GIT_OP_BISECT = 5;
export const GIT_STATUS_ENTRY_CONFLICTED = 1 << 0;
export const GIT_UPSTREAM_GONE = 1 << 0;
export const GIT_UPSTREAM_COUNTS_VALID = 1 << 1;

export const GIT_WORKTREE_MAIN = 1 << 0;
export const GIT_WORKTREE_CURRENT = 1 << 1;
export const GIT_WORKTREE_LOCKED = 1 << 2;
export const GIT_WORKTREE_PRUNABLE = 1 << 3;
export const GIT_WORKTREE_DETACHED = 1 << 4;
export const GIT_WORKTREE_BARE = 1 << 5;

/** Exact 32-byte native object identifier, zero-padded for SHA-1 repos. */
export type GitOid = Uint8Array;
export const GIT_OID_NONE: GitOid = new Uint8Array(32);

export function gitOidEqual(a: GitOid, b: GitOid): boolean {
  if (a.length !== 32 || b.length !== 32) return false;
  for (let index = 0; index < 32; index++)
    if (a[index] !== b[index]) return false;
  return true;
}

export function gitOidIsZero(oid: GitOid): boolean {
  return gitOidEqual(oid, GIT_OID_NONE);
}

export function gitOidHex(
  oid: GitOid,
  oidFormat: number = GIT_OID_FORMAT_SHA1,
): string {
  const width = oidFormat === GIT_OID_FORMAT_SHA1 ? 20 : 32;
  return [...oid.subarray(0, width)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

export function gitOidFromHex(hex: string): GitOid | null {
  if (hex.length !== 40 && hex.length !== 64) return null;
  if (!/^[0-9a-fA-F]+$/.test(hex)) return null;
  const oid = new Uint8Array(32);
  for (let index = 0; index < hex.length / 2; index++)
    oid[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16);
  return oid;
}

export interface GitEndpoint {
  kind: number;
  oid: GitOid;
}

export interface GitLogRequest {
  flags: number;
  limit: number;
  path: string;
  tips: GitOid[];
  hides: GitOid[];
}

export type GitCursorRecord = { kind: "cursor"; after: string; pos: bigint };

export type GitPatchRecord =
  | {
      kind: "file";
      st: number;
      similarity: number;
      flags: number;
      oldPath: string;
      newPath: string;
    }
  | {
      kind: "row";
      oldLine: number;
      newLine: number;
      oldText: Uint8Array;
      newText: Uint8Array;
      oldSpans: Array<[number, number]>;
      newSpans: Array<[number, number]>;
    }
  | { kind: "gap"; oldLine: number; newLine: number }
  | { kind: "base"; oid: GitOid }
  | GitCursorRecord;

export type GitWorktreeRecord =
  | {
      kind: "tree";
      flags: number;
      oid: GitOid;
      path: string;
      branch: string;
      lockReason: string;
    }
  | GitCursorRecord;

export interface GitFoundRepo {
  workdir: string;
  gitdir: string;
  bare: boolean;
  linked: boolean;
  submodule: boolean;
}

export interface GitDiscoverOptions {
  depth?: number;
  nested?: boolean;
  bare?: boolean;
  onPage?: (repos: GitFoundRepo[]) => void;
  maxPages?: number;
  signal?: AbortSignal;
}

export interface GitOpenOptions {
  watch?: boolean;
  status?: boolean;
  untracked?: boolean;
  ignored?: boolean;
  tracking?: boolean;
  remotes?: boolean;
  refPrefixes?: string[];
  refsLatencyMs?: number;
  statusLatencyMs?: number;
  onClosed?: (reason: number) => void;
  fromSessionId?: SessionId;
}

export interface GitRequestOptions {
  signal?: AbortSignal;
}

export interface GitLogWatchOptions {
  flags?: number;
  limit?: number;
}

export function gitStatusText(status: number): string {
  switch (status) {
    case GIT_STATUS_UNKNOWN_ID:
      return "unknown repo";
    case GIT_STATUS_NOT_FOUND:
      return "not found";
    case GIT_STATUS_WRONG_TYPE:
      return "wrong object type";
    case GIT_STATUS_PERMISSION:
      return "permission denied";
    case GIT_STATUS_TOO_LARGE:
      return "too large";
    case GIT_STATUS_BUDGET:
      return "budget exhausted";
    case GIT_STATUS_INVALID:
      return "invalid request";
    case GIT_STATUS_CANCELLED:
      return "cancelled";
    case GIT_STATUS_OTHER:
      return "backend error";
    case GIT_STATUS_CONFLICT:
      return "conflict";
    case GIT_STATUS_NO_MERGE_BASE:
      return "no merge base";
    default:
      return `unknown status ${status}`;
  }
}

export class GitStatusError extends Error {
  constructor(
    readonly op: string,
    readonly status: number,
    readonly detail = "",
  ) {
    super(
      `${op} failed: ${gitStatusText(status)}${detail ? `: ${detail}` : ""}`,
    );
    this.name = "GitStatusError";
  }

  get cancelled(): boolean {
    return this.status === GIT_STATUS_CANCELLED;
  }
}

export interface GitHead {
  flags: number;
  oid: GitOid;
  name: string;
}

export interface GitRefState {
  flags: number;
  oid: GitOid;
  peeled: GitOid;
  target: string;
}

export interface GitRemoteState {
  flags: number;
  fetchUrl: string;
  pushUrl: string;
}

export interface GitUpstreamState {
  flags: number;
  ahead: number;
  behind: number;
  upstream: string;
}

export interface GitStatusEntry {
  staged: number;
  unstaged: number;
  flags: number;
  oid: GitOid;
  oldPath: string;
  path: string;
}

export interface GitStashEntry {
  index: number;
  oid: GitOid;
  time: bigint;
  tz: number;
  message: string;
}

export interface GitOpState {
  op: number;
  oid: GitOid;
  detail: string;
}

/** Protocol-independent repository state presented to product consumers. */
export class GitStateMirror {
  head: GitHead | null = null;
  refs = new Map<string, GitRefState>();
  op: GitOpState | null = null;
  status: GitStatusEntry[] = [];
  upstreams = new Map<string, GitUpstreamState>();
  stashes: GitStashEntry[] = [];
  remotes = new Map<string, GitRemoteState>();
  worktreeGen: { count: number; digest: bigint } = { count: 0, digest: 0n };
  flags = 0;
}
