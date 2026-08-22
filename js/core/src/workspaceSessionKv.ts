/** Native KV contract used only by durable browser workspace-session state. */

export type WorkspaceSessionHash = Uint8Array;

export interface WorkspaceSessionKvEntry {
  /** Exact 32-byte BLAKE3 content hash returned by native YAS KV. */
  hash: WorkspaceSessionHash;
  size: number;
  mtimeNs: bigint;
  value: Uint8Array | null;
}

export interface WorkspaceSessionKvMirror {
  readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
  snapshotDone: boolean;
}

export interface WorkspaceSessionKvPutOptions {
  ifHash?: WorkspaceSessionHash;
  create?: boolean;
  durable?: boolean;
}

export interface WorkspaceSessionKvDeleteOptions {
  ifHash?: WorkspaceSessionHash;
  durable?: boolean;
}

export interface WorkspaceSessionKvWatchOptions {
  inlineMax?: number;
  onUpdate?: (mirror: WorkspaceSessionKvMirror) => void;
  onClosed?: (error: Error) => void;
}

export interface WorkspaceSessionKvWatch {
  /** Opaque native namespace handle. It is never projected to a number. */
  readonly namespaceHandle: bigint;
  readonly mirror: WorkspaceSessionKvMirror;
  close(): void;
}

export interface WorkspaceSessionKv {
  kvPut(
    key: string,
    value: Uint8Array,
    options?: WorkspaceSessionKvPutOptions,
  ): Promise<{ hash: WorkspaceSessionHash; mtimeNs: bigint }>;
  kvDelete(
    key: string,
    options?: WorkspaceSessionKvDeleteOptions,
  ): Promise<void>;
  kvFetch(
    key: string,
  ): Promise<{ hash: WorkspaceSessionHash; value: Uint8Array } | null>;
  watchKv(
    prefix: string,
    options?: WorkspaceSessionKvWatchOptions,
  ): Promise<WorkspaceSessionKvWatch>;
}

export interface WorkspaceSessionOwnedKv extends WorkspaceSessionKv {
  dispose(): void;
}

export class WorkspaceSessionKvConflictError extends Error {
  readonly hash: WorkspaceSessionHash;

  constructor(hash: WorkspaceSessionHash) {
    super("workspace session KV mutation conflicted");
    this.name = "WorkspaceSessionKvConflictError";
    this.hash = copyWorkspaceSessionHash(hash);
  }
}

export function copyWorkspaceSessionHash(
  hash: WorkspaceSessionHash,
): WorkspaceSessionHash {
  if (hash.length !== 32)
    throw new Error("workspace session KV hash is not 32 bytes");
  return new Uint8Array(hash);
}

export function workspaceSessionHashesEqual(
  left: WorkspaceSessionHash | null | undefined,
  right: WorkspaceSessionHash | null | undefined,
): boolean {
  if (left === right) return true;
  if (!left || !right || left.length !== 32 || right.length !== 32)
    return false;
  let difference = 0;
  for (let index = 0; index < 32; index++)
    difference |= left[index]! ^ right[index]!;
  return difference === 0;
}
