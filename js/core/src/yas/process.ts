/** YAS Process family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateCatalogueRetention,
  YasStateSubscription,
  detachStateRetainedValue,
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FAMILY_PROCESS,
  YAS_PROCESS_ATTACH,
  YAS_PROCESS_CONTROL,
  YAS_PROCESS_SPAWN,
  YAS_PROCESS_STATE,
  YAS_PROCESS_STATE_ACK,
  YAS_PROCESS_UNWATCH,
  YAS_PROCESS_VERSION,
  YAS_PROCESS_WAIT,
  YAS_PROCESS_WATCH,
} from "./generated";

export type YasProcessCwd =
  | { kind: "server-default" }
  | { kind: "path"; path: Uint8Array }
  | { kind: "terminal"; terminalHandle: bigint }
  | { kind: "fs"; rootHandle: bigint; components: readonly Uint8Array[] };

export interface YasProcessEnvironmentEntry {
  key: Uint8Array;
  value: Uint8Array;
}

export interface YasProcessSpawn {
  operationId: Uint8Array;
  flags: number;
  environmentKind: number;
  cwd: YasProcessCwd;
  argv: readonly Uint8Array[];
  environment: readonly YasProcessEnvironmentEntry[];
  stdoutReceiveCredit: bigint;
  stderrReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasProcessAttach {
  processHandle: bigint;
  flags: number;
  stdoutReceiveCredit: bigint;
  stderrReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasProcessControl {
  processHandle: bigint;
  operationId: Uint8Array;
  action: number;
  value: number;
  extensions?: readonly YasExtension[];
}

export interface YasProcessExit {
  kind: number;
  reason: number;
  code: number;
  exitedServerNs: bigint;
  detail: Uint8Array;
}

export interface YasProcessStreamBundle {
  processHandle: bigint;
  stdoutLifetimeOffset: bigint;
  stderrLifetimeOffset: bigint;
  stdin?: YasTransferDescriptor;
  stdout: YasTransferDescriptor;
  stderr?: YasTransferDescriptor;
  mergedStderr: boolean;
  extensions: readonly YasExtension[];
}

export interface YasProcessRecord {
  processHandle: bigint;
  lifecycle: number;
  streamState: number;
  flags: number;
  nativePid: bigint;
  ownerSession: Uint8Array;
  argv0: Uint8Array;
  stdinReceived: bigint;
  stdoutProduced: bigint;
  stderrProduced: bigint;
  retentionDeadlineServerNs: bigint;
  exit?: YasProcessExit;
  extensions: readonly YasExtension[];
}

export interface YasProcessSnapshot {
  revision: bigint;
  processes: readonly YasProcessRecord[];
}

export interface YasProcessStreams {
  processHandle: bigint;
  stdoutLifetimeOffset: bigint;
  stderrLifetimeOffset: bigint;
  stdin?: YasTransfer;
  stdout: YasTransfer;
  stderr?: YasTransfer;
  mergedStderr: boolean;
  extensions: readonly YasExtension[];
}

interface YasProcessSpawnOperation {
  payloadKey: string;
  wirePayload: Uint8Array | null;
  stdoutReceiveCredit: bigint;
  stderrReceiveCredit: bigint;
  pending: Promise<YasProcessStreams> | null;
  streams: YasProcessStreams | null;
  identity: YasProcessSpawnIdentity | null;
  settled: boolean;
}

interface YasProcessSpawnIdentity {
  processHandle: bigint;
  transferIds: readonly number[];
}

export interface YasProcessLimits {
  maxArgc: number;
  maxArgBytes: number;
  maxEnvc: number;
  maxEnvBytes: number;
  maxProcessesPerSession: number;
  maxProcesses: number;
  maxPendingSpawns: number;
  maxStreamBufferBytes: bigint;
  maxDetachedRetentionNs: bigint;
  maxMutationReplays: number;
}

export function encodeProcessCwd(value: YasProcessCwd): Uint8Array {
  validateCwd(value);
  const writer = new YasWriter();
  if (value.kind === "server-default")
    writer.u8(g.YAS_PROCESS_CWD_SERVER_DEFAULT).bytes(new Uint8Array(3));
  else if (value.kind === "path")
    writer
      .u8(g.YAS_PROCESS_CWD_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(value.path);
  else if (value.kind === "terminal")
    writer
      .u8(g.YAS_PROCESS_CWD_TERMINAL)
      .bytes(new Uint8Array(3))
      .u64(value.terminalHandle);
  else {
    writer
      .u8(g.YAS_PROCESS_CWD_FS)
      .bytes(new Uint8Array(3))
      .u64(value.rootHandle)
      .u16(value.components.length);
    for (const component of value.components) writer.bytesU16(component);
  }
  return writer.finish();
}

export function decodeProcessCwd(bytes: Uint8Array): YasProcessCwd {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Process cwd kind");
  requireZero(cursor.take(3, "Process cwd reserved"), "Process cwd");
  let value: YasProcessCwd;
  if (kind === g.YAS_PROCESS_CWD_SERVER_DEFAULT)
    value = { kind: "server-default" };
  else if (kind === g.YAS_PROCESS_CWD_PATH)
    value = {
      kind: "path",
      path: new Uint8Array(cursor.bytesU32("Process cwd path")),
    };
  else if (kind === g.YAS_PROCESS_CWD_TERMINAL)
    value = {
      kind: "terminal",
      terminalHandle: cursor.u64("Process cwd terminal"),
    };
  else if (kind === g.YAS_PROCESS_CWD_FS) {
    const rootHandle = cursor.u64("Process cwd FS root");
    const count = cursor.u16("Process cwd component count");
    if (
      count > g.YAS_PROCESS_MAX_PATH_COMPONENTS ||
      count > Math.floor(cursor.remaining / 2)
    )
      throw new YasProtocolError("invalid Process cwd component count");
    const components: Uint8Array[] = [];
    for (let index = 0; index < count; index++)
      components.push(new Uint8Array(cursor.bytesU16("Process cwd component")));
    value = { kind: "fs", rootHandle, components };
  } else throw new YasProtocolError("unknown Process cwd kind");
  cursor.end("Process cwd");
  validateCwd(value);
  return value;
}

export function encodeProcessSpawn(value: YasProcessSpawn): Uint8Array {
  validateSpawn(value);
  const writer = new YasWriter()
    .bytes(value.operationId)
    .u16(value.flags)
    .u8(value.environmentKind)
    .u8(0)
    .bytesU32(encodeProcessCwd(value.cwd))
    .u16(value.argv.length);
  for (const arg of value.argv) writer.bytesU32(arg);
  writer.u16(value.environment.length);
  for (const entry of value.environment)
    writer.bytesU16(entry.key).bytesU32(entry.value);
  return writer
    .u64(value.stdoutReceiveCredit)
    .u64(value.stderrReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeProcessSpawn(bytes: Uint8Array): YasProcessSpawn {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Process operation ID"));
  const flags = cursor.u16("Process spawn flags");
  const environmentKind = cursor.u8("Process environment kind");
  if (cursor.u8("Process SPAWN reserved") !== 0)
    throw new YasProtocolError("Process SPAWN reserved byte is nonzero");
  const cwd = decodeProcessCwd(cursor.bytesU32("Process cwd"));
  const argc = cursor.u16("Process argument count");
  if (
    argc === 0 ||
    argc > g.YAS_PROCESS_MAX_ARGC ||
    argc > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Process argument count");
  const argv: Uint8Array[] = [];
  for (let index = 0; index < argc; index++)
    argv.push(new Uint8Array(cursor.bytesU32("Process argument")));
  const envc = cursor.u16("Process environment count");
  if (envc > g.YAS_PROCESS_MAX_ENVC || envc > Math.floor(cursor.remaining / 6))
    throw new YasProtocolError("invalid Process environment count");
  const environment: YasProcessEnvironmentEntry[] = [];
  for (let index = 0; index < envc; index++)
    environment.push({
      key: new Uint8Array(cursor.bytesU16("Process environment key")),
      value: new Uint8Array(cursor.bytesU32("Process environment value")),
    });
  const value = {
    operationId,
    flags,
    environmentKind,
    cwd,
    argv,
    environment,
    stdoutReceiveCredit: cursor.u64("Process stdout receive credit"),
    stderrReceiveCredit: cursor.u64("Process stderr receive credit"),
    extensions: decodeExtensions(
      cursor,
      new Set([
        g.YAS_PROCESS_SPAWN_SURFACE_APP_EXTENSION,
        g.YAS_PROCESS_SPAWN_RESOURCE_TAG_EXTENSION,
      ]),
      "Process SPAWN extensions",
    ),
  };
  cursor.end("Process SPAWN");
  validateSpawn(value);
  return value;
}

export function encodeProcessAttach(value: YasProcessAttach): Uint8Array {
  validateAttach(value);
  return new YasWriter()
    .u64(value.processHandle)
    .u16(value.flags)
    .u16(0)
    .u64(value.stdoutReceiveCredit)
    .u64(value.stderrReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeProcessAttach(bytes: Uint8Array): YasProcessAttach {
  const cursor = new YasCursor(bytes);
  const processHandle = cursor.u64("Process handle");
  const flags = cursor.u16("Process attach flags");
  if (cursor.u16("Process ATTACH reserved") !== 0)
    throw new YasProtocolError("Process ATTACH reserved field is nonzero");
  const value = {
    processHandle,
    flags,
    stdoutReceiveCredit: cursor.u64("Process stdout receive credit"),
    stderrReceiveCredit: cursor.u64("Process stderr receive credit"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Process ATTACH extensions",
    ),
  };
  cursor.end("Process ATTACH");
  validateAttach(value);
  return value;
}

export function encodeProcessControl(value: YasProcessControl): Uint8Array {
  validateControl(value);
  return new YasWriter()
    .u64(value.processHandle)
    .bytes(value.operationId)
    .u8(value.action)
    .u8(0)
    .u16(value.value)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeProcessControl(bytes: Uint8Array): YasProcessControl {
  const cursor = new YasCursor(bytes);
  const processHandle = cursor.u64("Process handle");
  const operationId = new Uint8Array(cursor.take(16, "Process operation ID"));
  const action = cursor.u8("Process control action");
  if (cursor.u8("Process CONTROL reserved") !== 0)
    throw new YasProtocolError("Process CONTROL reserved byte is nonzero");
  const value = {
    processHandle,
    operationId,
    action,
    value: cursor.u16("Process control value"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Process CONTROL extensions",
    ),
  };
  cursor.end("Process CONTROL");
  validateControl(value);
  return value;
}

export function encodeProcessWait(
  processHandle: bigint,
  timeoutNs: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(processHandle, "Process handle");
  return new YasWriter()
    .u64(processHandle)
    .u64(timeoutNs)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeProcessExit(bytes: Uint8Array): YasProcessExit {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Process exit kind");
  const reason = cursor.u8("Process exit reason");
  if (cursor.u16("Process exit reserved") !== 0)
    throw new YasProtocolError("Process exit reserved field is nonzero");
  const value = {
    kind,
    reason,
    code: cursor.i32("Process exit code"),
    exitedServerNs: cursor.u64("Process exit time"),
    detail: new Uint8Array(cursor.bytesU32("Process exit detail")),
  };
  cursor.end("Process exit record");
  validateExit(value);
  return value;
}

export function encodeProcessExit(value: YasProcessExit): Uint8Array {
  validateExit(value);
  return new YasWriter()
    .u8(value.kind)
    .u8(value.reason)
    .u16(0)
    .i32(value.code)
    .u64(value.exitedServerNs)
    .bytesU32(value.detail)
    .finish();
}

export function decodeProcessStreamBundle(
  bytes: Uint8Array,
): YasProcessStreamBundle {
  const cursor = new YasCursor(bytes);
  const processHandle = cursor.u64("Process handle");
  const flags = cursor.u16("Process stream bundle flags");
  if (
    flags & ~g.YAS_PROCESS_BUNDLE_FLAGS ||
    cursor.u16("Process stream bundle reserved") !== 0 ||
    !(flags & g.YAS_PROCESS_BUNDLE_STDOUT)
  )
    throw new YasProtocolError("invalid Process stream bundle flags");
  const stdoutLifetimeOffset = cursor.u64("Process stdout lifetime offset");
  const stderrLifetimeOffset = cursor.u64("Process stderr lifetime offset");
  const stdin =
    flags & g.YAS_PROCESS_BUNDLE_STDIN
      ? decodeDescriptorBytes(cursor.bytesU32("Process stdin descriptor"))
      : undefined;
  const stdout = decodeDescriptorBytes(
    cursor.bytesU32("Process stdout descriptor"),
  );
  const stderr =
    flags & g.YAS_PROCESS_BUNDLE_STDERR
      ? decodeDescriptorBytes(cursor.bytesU32("Process stderr descriptor"))
      : undefined;
  const value = {
    processHandle,
    stdoutLifetimeOffset,
    stderrLifetimeOffset,
    stdin,
    stdout,
    stderr,
    mergedStderr: Boolean(flags & g.YAS_PROCESS_BUNDLE_MERGED_STDERR),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Process stream bundle extensions",
    ),
  };
  cursor.end("Process stream bundle");
  validateStreamBundle(value);
  return value;
}

export function encodeProcessStreamBundle(
  value: YasProcessStreamBundle,
): Uint8Array {
  validateStreamBundle(value);
  let flags = g.YAS_PROCESS_BUNDLE_STDOUT;
  if (value.stdin) flags |= g.YAS_PROCESS_BUNDLE_STDIN;
  if (value.stderr) flags |= g.YAS_PROCESS_BUNDLE_STDERR;
  if (value.mergedStderr) flags |= g.YAS_PROCESS_BUNDLE_MERGED_STDERR;
  const writer = new YasWriter()
    .u64(value.processHandle)
    .u16(flags)
    .u16(0)
    .u64(value.stdoutLifetimeOffset)
    .u64(value.stderrLifetimeOffset);
  if (value.stdin) writer.bytesU32(encodeTransferDescriptor(value.stdin));
  writer.bytesU32(encodeTransferDescriptor(value.stdout));
  if (value.stderr) writer.bytesU32(encodeTransferDescriptor(value.stderr));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeProcessRecord(bytes: Uint8Array): YasProcessRecord {
  const cursor = new YasCursor(bytes);
  const processHandle = cursor.u64("Process handle");
  const lifecycle = cursor.u8("Process lifecycle");
  const streamState = cursor.u8("Process stream state");
  const flags = cursor.u16("Process flags");
  const nativePid = cursor.u64("Process native PID");
  const ownerSession = new Uint8Array(cursor.take(16, "Process owner session"));
  const argv0 = new Uint8Array(cursor.bytesU32("Process argv0"));
  const stdinReceived = cursor.u64("Process stdin received");
  const stdoutProduced = cursor.u64("Process stdout produced");
  const stderrProduced = cursor.u64("Process stderr produced");
  const retentionDeadlineServerNs = cursor.u64("Process retention deadline");
  const exitPresent = cursor.u8("Process exit presence");
  requireZero(cursor.take(7, "Process exit reserved"), "Process record");
  if (exitPresent > 1)
    throw new YasProtocolError("invalid Process exit presence");
  const exit = exitPresent
    ? decodeProcessExit(cursor.bytesU32("Process exit record"))
    : undefined;
  const value = {
    processHandle,
    lifecycle,
    streamState,
    flags,
    nativePid,
    ownerSession,
    argv0,
    stdinReceived,
    stdoutProduced,
    stderrProduced,
    retentionDeadlineServerNs,
    exit,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Process record extensions",
    ),
  };
  cursor.end("Process record");
  validateProcessRecord(value);
  return value;
}

export function encodeProcessRecord(value: YasProcessRecord): Uint8Array {
  validateProcessRecord(value);
  const writer = new YasWriter()
    .u64(value.processHandle)
    .u8(value.lifecycle)
    .u8(value.streamState)
    .u16(value.flags)
    .u64(value.nativePid)
    .bytes(value.ownerSession)
    .bytesU32(value.argv0)
    .u64(value.stdinReceived)
    .u64(value.stdoutProduced)
    .u64(value.stderrProduced)
    .u64(value.retentionDeadlineServerNs)
    .u8(value.exit ? 1 : 0)
    .bytes(new Uint8Array(7));
  if (value.exit) writer.bytesU32(encodeProcessExit(value.exit));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function processLimitsFromExtensions(
  extensions: readonly YasExtension[],
): YasProcessLimits {
  const tags = new Set<number>([
    g.YAS_PROCESS_LIMIT_MAX_ARGC,
    g.YAS_PROCESS_LIMIT_MAX_ARG_BYTES,
    g.YAS_PROCESS_LIMIT_MAX_ENVC,
    g.YAS_PROCESS_LIMIT_MAX_ENV_BYTES,
    g.YAS_PROCESS_LIMIT_MAX_PROCESSES_PER_SESSION,
    g.YAS_PROCESS_LIMIT_MAX_PROCESSES,
    g.YAS_PROCESS_LIMIT_MAX_PENDING_SPAWNS,
    g.YAS_PROCESS_LIMIT_MAX_STREAM_BUFFER_BYTES,
    g.YAS_PROCESS_LIMIT_MAX_DETACHED_RETENTION_NS,
    g.YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS,
  ]);
  for (const extension of extensions)
    if (extension.required && !tags.has(extension.tag))
      throw new YasProtocolError("unknown required Process limit extension");
  const value = {
    maxArgc: extensionU32(extensions, g.YAS_PROCESS_LIMIT_MAX_ARGC),
    maxArgBytes: extensionU32(extensions, g.YAS_PROCESS_LIMIT_MAX_ARG_BYTES),
    maxEnvc: extensionU32(extensions, g.YAS_PROCESS_LIMIT_MAX_ENVC),
    maxEnvBytes: extensionU32(extensions, g.YAS_PROCESS_LIMIT_MAX_ENV_BYTES),
    maxProcessesPerSession: extensionU32(
      extensions,
      g.YAS_PROCESS_LIMIT_MAX_PROCESSES_PER_SESSION,
    ),
    maxProcesses: extensionU32(extensions, g.YAS_PROCESS_LIMIT_MAX_PROCESSES),
    maxPendingSpawns: extensionU32(
      extensions,
      g.YAS_PROCESS_LIMIT_MAX_PENDING_SPAWNS,
    ),
    maxStreamBufferBytes: extensionU64(
      extensions,
      g.YAS_PROCESS_LIMIT_MAX_STREAM_BUFFER_BYTES,
    ),
    maxDetachedRetentionNs: extensionU64(
      extensions,
      g.YAS_PROCESS_LIMIT_MAX_DETACHED_RETENTION_NS,
    ),
    maxMutationReplays: extensionU32(
      extensions,
      g.YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS,
    ),
  };
  validateLimits(value);
  return value;
}

export function processLimitsExtensions(
  value: YasProcessLimits,
): YasExtension[] {
  validateLimits(value);
  return [
    extension32(g.YAS_PROCESS_LIMIT_MAX_ARGC, value.maxArgc),
    extension32(g.YAS_PROCESS_LIMIT_MAX_ARG_BYTES, value.maxArgBytes),
    extension32(g.YAS_PROCESS_LIMIT_MAX_ENVC, value.maxEnvc),
    extension32(g.YAS_PROCESS_LIMIT_MAX_ENV_BYTES, value.maxEnvBytes),
    extension32(
      g.YAS_PROCESS_LIMIT_MAX_PROCESSES_PER_SESSION,
      value.maxProcessesPerSession,
    ),
    extension32(g.YAS_PROCESS_LIMIT_MAX_PROCESSES, value.maxProcesses),
    extension32(g.YAS_PROCESS_LIMIT_MAX_PENDING_SPAWNS, value.maxPendingSpawns),
    extension64(
      g.YAS_PROCESS_LIMIT_MAX_STREAM_BUFFER_BYTES,
      value.maxStreamBufferBytes,
    ),
    extension64(
      g.YAS_PROCESS_LIMIT_MAX_DETACHED_RETENTION_NS,
      value.maxDetachedRetentionNs,
    ),
    extension32(
      g.YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS,
      value.maxMutationReplays,
    ),
  ];
}

export class YasProcessCatalog {
  private current = new Map<bigint, YasProcessRecord>();
  private staging: Map<bigint, YasProcessRecord> | null = null;
  private retention: YasStateCatalogueRetention<bigint>;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasProcessSnapshot) => void>();
  private readonly snapshotRejectors = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private epoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_PROCESS) {
        this.epoch++;
        const error = new YasProtocolError("Process catalogue was invalidated");
        this.pendingWatchCancel?.(error);
        this.cancelSnapshots(error);
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasProcessSnapshot {
    return { revision: this.revision, processes: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasProcessSnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      // One observer cannot block catalogue delivery or cleanup.
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasProcessSnapshot> {
    this.assertOpen();
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectSnapshot: ((error: unknown) => void) | undefined;
    const result = new Promise<YasProcessSnapshot>((resolve, reject) => {
      let settled = false;
      const finish = (snapshot?: YasProcessSnapshot, error?: unknown) => {
        if (settled) return;
        settled = true;
        remove?.();
        if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
        if (error !== undefined) reject(error);
        else resolve(snapshot!);
      };
      rejectSnapshot = (error) => finish(undefined, error);
      this.snapshotRejectors.add(rejectSnapshot);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        finish(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } catch (error) {
      remove?.();
      if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
      throw error;
    }
  }

  async watch(options: YasWatchOptions = {}): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.pendingWatch) return this.pendingWatch;
    this.resetLocal();
    const epoch = this.epoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_PROCESS,
      g.YAS_PROCESS_WATCH,
      g.YAS_PROCESS_UNWATCH,
      g.YAS_PROCESS_STATE,
      g.YAS_PROCESS_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.epoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.epoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Process catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    const cancelled = new Promise<never>((_, reject) => {
      this.pendingWatchCancel = reject;
    });
    const pending = Promise.race([watched, cancelled]);
    this.pendingWatch = pending;
    try {
      await pending;
    } finally {
      if (this.pendingWatch === pending) this.pendingWatch = null;
      if (this.pendingWatchCancel) this.pendingWatchCancel = null;
    }
  }

  async unwatch(): Promise<void> {
    this.assertOpen();
    this.epoch++;
    this.pendingWatchCancel?.(
      new YasProtocolError("Process catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.removeInvalidation();
    const subscription = this.subscription;
    this.subscription = null;
    const error = new YasProtocolError("Process catalogue is disposed");
    this.pendingWatchCancel?.(error);
    this.cancelSnapshots(error);
    this.clearState();
    this.listeners.clear();
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.clearState();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.discardStaging();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Process snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Process snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = this.staging;
      this.retention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.retention.clone();
      let next: Map<bigint, YasProcessRecord>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, retention, batch.records);
        this.validateCatalog(next);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = next;
      this.retention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<bigint, YasProcessRecord>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      if (action.kind === YAS_STATE_ADD || action.kind === YAS_STATE_REPLACE) {
        const value = detachStateRetainedValue(
          decodeProcessRecord(action.body),
        );
        const exists = target.has(value.processHandle);
        if ((action.kind === YAS_STATE_ADD) === exists)
          throw new YasProtocolError("Process ADD/REPLACE precondition failed");
        if (action.kind === YAS_STATE_ADD && target.size >= this.catalogLimit())
          throw new YasProtocolError(
            "Process catalogue exceeds its negotiated process limit",
          );
        retention.upsert(
          value.processHandle,
          Math.max(
            encodeProcessRecord(value).length,
            estimateStateRetainedBytes(value),
          ),
        );
        target.set(value.processHandle, value);
      } else if (action.kind === YAS_STATE_REMOVE) {
        const cursor = new YasCursor(action.body);
        const handle = cursor.u64("removed Process handle");
        cursor.end("Process REMOVE");
        requireHandle(handle, "removed Process handle");
        if (!target.has(handle))
          throw new YasProtocolError("Process REMOVE names an unknown process");
        retention.remove(handle);
        target.delete(handle);
      } else
        throw new YasProtocolError("unsupported Process state record kind");
    }
  }

  private validateCatalog(
    records: ReadonlyMap<bigint, YasProcessRecord>,
  ): void {
    if (records.size > this.catalogLimit())
      throw new YasProtocolError(
        "Process catalogue exceeds its negotiated process limit",
      );
  }

  private catalogLimit(): number {
    return processLimitsFromExtensions(
      this.connection.family(g.YAS_FAMILY_PROCESS, g.YAS_PROCESS_VERSION)
        .limits,
    ).maxProcesses;
  }

  private resetLocal(): void {
    this.subscription = null;
    this.clearState();
  }

  private clearState(): void {
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current = new Map();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
    this.stagingRetention = null;
    this.revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }

  private emit(): void {
    if (this.disposed) return;
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block catalogue delivery or cleanup.
      }
    }
  }

  private cancelSnapshots(error: unknown): void {
    for (const reject of [...this.snapshotRejectors]) reject(error);
    this.snapshotRejectors.clear();
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Process catalogue is disposed");
  }
}

export class YasProcessClient {
  readonly catalog: YasProcessCatalog;
  private readonly transfers;
  private readonly activeStreams = new Set<YasProcessStreams>();
  private readonly spawnOperations = new Map<
    string,
    YasProcessSpawnOperation
  >();
  private readonly pendingSpawnOperations = new Map<
    string,
    YasProcessSpawnOperation
  >();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private epoch = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasProcessCatalog(connection);
    this.transfers = transfersFor(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_PROCESS) {
        this.epoch++;
        this.retireSpawnOperations();
        const error = new YasProtocolError(
          "YAS Process client was invalidated",
        );
        for (const cancel of [...this.pendingCancels]) cancel(error);
        this.pendingCancels.clear();
        this.resetStreams();
      }
    });
  }

  list(options: YasWatchOptions = {}): Promise<YasProcessSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.retireSpawnOperations();
    const error = new YasProtocolError("YAS Process client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.removeInvalidation();
    this.resetStreams();
    this.spawnOperations.clear();
    this.pendingSpawnOperations.clear();
    this.catalog.dispose();
  }

  spawn(
    value: Omit<YasProcessSpawn, "stdoutReceiveCredit" | "stderrReceiveCredit">,
    stdoutCredit = 1024n * 1024n,
    stderrCredit = 1024n * 1024n,
  ): Promise<YasProcessStreams> {
    this.assertOpen();
    const merged = Boolean(value.flags & g.YAS_PROCESS_SPAWN_MERGE_STDERR);
    const requestedStderrCredit = merged ? 0n : stderrCredit;
    const payloadKey = byteKey(
      encodeProcessSpawn({
        ...value,
        stdoutReceiveCredit: stdoutCredit,
        stderrReceiveCredit: requestedStderrCredit,
      }),
    );
    const operationKey = byteKey(value.operationId);
    let operation =
      this.spawnOperations.get(operationKey) ??
      this.pendingSpawnOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Process SPAWN operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (operation.streams) return Promise.resolve(operation.streams);
      if (!this.canReserveSpawnOperation(operationKey))
        return Promise.reject(
          new YasProtocolError(
            "Process SPAWN replay ledger has no evictable settlement",
          ),
        );
      return this.startSpawnOperation(
        operationKey,
        operation,
        value,
        stdoutCredit,
        requestedStderrCredit,
        false,
      );
    } else {
      if (!this.canReserveSpawnOperation(operationKey))
        return Promise.reject(
          new YasProtocolError(
            "Process SPAWN replay ledger has no evictable settlement",
          ),
        );
      operation = {
        payloadKey,
        wirePayload: null,
        stdoutReceiveCredit: 0n,
        stderrReceiveCredit: 0n,
        pending: null,
        streams: null,
        identity: null,
        settled: false,
      };
      this.pendingSpawnOperations.set(operationKey, operation);
    }
    return this.startSpawnOperation(
      operationKey,
      operation,
      value,
      stdoutCredit,
      requestedStderrCredit,
      true,
    );
  }

  private startSpawnOperation(
    operationKey: string,
    operation: YasProcessSpawnOperation,
    value: Omit<YasProcessSpawn, "stdoutReceiveCredit" | "stderrReceiveCredit">,
    stdoutCredit: bigint,
    stderrCredit: bigint,
    fresh: boolean,
  ): Promise<YasProcessStreams> {
    const epoch = this.epoch;
    const running = fresh
      ? this.performFreshSpawn(
          operation,
          value,
          stdoutCredit,
          stderrCredit,
          epoch,
        )
      : this.performRetiredSpawn(
          operation,
          operation.wirePayload!,
          operation.stdoutReceiveCredit,
          operation.stderrReceiveCredit,
        );
    let pending!: Promise<YasProcessStreams>;
    pending = this.runOwned(running)
      .then((streams) => {
        if (!fresh) {
          this.discardUnexpectedStreams(streams, operation.identity);
          throw new YasProtocolError(
            "retired Process SPAWN unexpectedly returned OK",
          );
        }
        if (this.disposed || epoch !== this.epoch) {
          this.discardUnexpectedStreams(streams, operation.identity);
          throw new YasProtocolError("Process SPAWN completed after disposal");
        }
        if (!this.retainSpawnOperation(operationKey, operation)) {
          this.discardUnexpectedStreams(streams, operation.identity);
          throw new YasProtocolError(
            "Process SPAWN replay ledger has no evictable settlement",
          );
        }
        operation.streams = streams;
        operation.identity = processSpawnIdentity(streams);
        operation.settled = true;
        this.trackStreams(streams, operationKey);
        return streams;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          fresh &&
          this.pendingSpawnOperations.get(operationKey) === operation
        )
          this.pendingSpawnOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  private performFreshSpawn(
    operation: YasProcessSpawnOperation,
    value: Omit<YasProcessSpawn, "stdoutReceiveCredit" | "stderrReceiveCredit">,
    stdoutCredit: bigint,
    stderrCredit: bigint,
    epoch: number,
  ): Promise<YasProcessStreams> {
    return this.withStreamLeases(
      stdoutCredit,
      stderrCredit,
      (stdoutLease, stderrLease) => {
        operation.stdoutReceiveCredit = stdoutLease.bytes;
        operation.stderrReceiveCredit = stderrLease?.bytes ?? 0n;
        operation.wirePayload = encodeProcessSpawn({
          ...value,
          stdoutReceiveCredit: operation.stdoutReceiveCredit,
          stderrReceiveCredit: operation.stderrReceiveCredit,
        });
        return this.connection.requestDecoded<YasProcessStreams>(
          g.YAS_FAMILY_PROCESS,
          g.YAS_PROCESS_SPAWN,
          operation.wirePayload,
          (body) => {
            const bundle = decodeProcessStreamBundle(body);
            if (this.disposed || epoch !== this.epoch) {
              this.discardUnexpectedBundle(
                bundle,
                stdoutLease,
                stderrLease,
                operation.identity,
              );
              throw new YasProtocolError(
                "Process SPAWN completed after family invalidation",
              );
            }
            if (this.bundleHasOwnedAuthority(bundle, operation.identity))
              throw new YasProtocolError(
                "Process SPAWN returned an owned stream authority",
              );
            return this.acceptBundle(bundle, stdoutLease, stderrLease);
          },
        );
      },
    );
  }

  private performRetiredSpawn(
    operation: YasProcessSpawnOperation,
    payload: Uint8Array,
    stdoutCredit: bigint,
    stderrCredit: bigint,
  ): Promise<YasProcessStreams> {
    return this.withStreamLeases(
      stdoutCredit,
      stderrCredit,
      (stdoutLease, stderrLease) =>
        this.connection.requestDecoded<YasProcessStreams>(
          g.YAS_FAMILY_PROCESS,
          g.YAS_PROCESS_SPAWN,
          payload,
          (body) => {
            const bundle = decodeProcessStreamBundle(body);
            this.discardUnexpectedBundle(
              bundle,
              stdoutLease,
              stderrLease,
              operation.identity,
            );
            throw new YasProtocolError(
              "retired Process SPAWN unexpectedly returned OK",
            );
          },
        ),
      true,
    );
  }

  attach(
    processHandle: bigint,
    options: {
      stdin?: boolean;
      stdoutCredit?: bigint;
      stderrCredit?: bigint;
      extensions?: readonly YasExtension[];
    } = {},
  ): Promise<YasProcessStreams> {
    this.assertOpen();
    const epoch = this.epoch;
    return this.runOwned(this.performAttach(processHandle, options, epoch));
  }

  private async performAttach(
    processHandle: bigint,
    options: {
      stdin?: boolean;
      stdoutCredit?: bigint;
      stderrCredit?: bigint;
      extensions?: readonly YasExtension[];
    },
    epoch: number,
  ): Promise<YasProcessStreams> {
    const stdoutCredit = options.stdoutCredit ?? 1024n * 1024n;
    const stderrCredit = options.stderrCredit ?? 1024n * 1024n;
    const streams = await this.withStreamLeases(
      stdoutCredit,
      stderrCredit,
      (stdoutLease, stderrLease) =>
        this.connection.requestDecoded(
          g.YAS_FAMILY_PROCESS,
          g.YAS_PROCESS_ATTACH,
          encodeProcessAttach({
            processHandle,
            flags: options.stdin ? g.YAS_PROCESS_ATTACH_STDIN : 0,
            stdoutReceiveCredit: stdoutLease.bytes,
            stderrReceiveCredit: stderrLease?.bytes ?? 0n,
            extensions: options.extensions,
          }),
          (body) =>
            this.acceptBundle(
              decodeProcessStreamBundle(body),
              stdoutLease,
              stderrLease,
            ),
        ),
    );
    if (this.disposed || epoch !== this.epoch) {
      this.resetStreamBundle(streams);
      throw new YasProtocolError("Process ATTACH completed after disposal");
    }
    this.trackStreams(streams);
    return streams;
  }

  async control(value: YasProcessControl): Promise<bigint> {
    this.assertOpen();
    const body = await this.connection.request(
      g.YAS_FAMILY_PROCESS,
      g.YAS_PROCESS_CONTROL,
      encodeProcessControl(value),
    );
    const cursor = new YasCursor(body);
    const revision = cursor.u64("Process state revision");
    cursor.end("Process CONTROL Result");
    requireRevision(revision, "Process state revision");
    return revision;
  }

  wait(
    processHandle: bigint,
    timeoutNs = 0n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasProcessExit> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_PROCESS,
      g.YAS_PROCESS_WAIT,
      encodeProcessWait(processHandle, timeoutNs, extensions),
      decodeProcessExit,
    );
  }

  private async withStreamLeases(
    stdoutCredit: bigint,
    stderrCredit: bigint,
    run: (
      stdout: YasReceiveBudgetLease,
      stderr: YasReceiveBudgetLease | undefined,
    ) => Promise<YasProcessStreams>,
    exact = false,
  ): Promise<YasProcessStreams> {
    const stdout = this.transfers.reserveReceiveCredit(
      stdoutCredit,
      exact ? stdoutCredit : 1n,
    );
    let stderr: YasReceiveBudgetLease | undefined;
    try {
      if (stderrCredit !== 0n)
        stderr = this.transfers.reserveReceiveCredit(
          stderrCredit,
          exact ? stderrCredit : 1n,
        );
      return await run(stdout, stderr);
    } catch (error) {
      stdout.release();
      stderr?.release();
      throw error;
    }
  }

  private acceptBundle(
    bundle: YasProcessStreamBundle,
    stdoutLease: YasReceiveBudgetLease,
    stderrLease: YasReceiveBudgetLease | undefined,
  ): YasProcessStreams {
    const accepted: YasTransfer[] = [];
    let stdout: YasTransfer | undefined;
    let stderr: YasTransfer | undefined;
    let stdin: YasTransfer | undefined;
    try {
      if (bundle.stdin) {
        stdin = this.transfers.acceptServerUploadDescriptor(bundle.stdin);
        accepted.push(stdin);
      }
      stdout = this.transfers.acceptServerDescriptor(
        bundle.stdout,
        stdoutLease,
      );
      accepted.push(stdout);
      if (bundle.stderr) {
        if (!stderrLease)
          throw new YasProtocolError(
            "Process returned an unbudgeted stderr stream",
          );
        stderr = this.transfers.acceptServerDescriptor(
          bundle.stderr,
          stderrLease,
        );
        accepted.push(stderr);
      } else stderrLease?.release();
      return {
        processHandle: bundle.processHandle,
        stdoutLifetimeOffset: bundle.stdoutLifetimeOffset,
        stderrLifetimeOffset: bundle.stderrLifetimeOffset,
        stdin,
        stdout,
        stderr,
        mergedStderr: bundle.mergedStderr,
        extensions: bundle.extensions,
      };
    } catch (error) {
      for (const transfer of accepted) transfer.reset();
      if (!stdout) stdoutLease.release();
      if (!stderr) stderrLease?.release();
      throw error;
    }
  }

  private replayLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      g.YAS_FAMILY_PROCESS,
      g.YAS_PROCESS_VERSION,
      g.YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS,
      g.YAS_PROCESS_MAX_MUTATION_REPLAYS,
    );
  }

  private canReserveSpawnOperation(operationKey: string): boolean {
    const limit = this.replayLimit();
    let pinned = 0;
    for (const [key, operation] of this.spawnOperations) {
      if (key === operationKey) continue;
      if (operation.pending || operation.streams) pinned++;
    }
    for (const key of this.pendingSpawnOperations.keys())
      if (key !== operationKey) pinned++;
    return pinned + 1 <= limit;
  }

  private retainSpawnOperation(
    operationKey: string,
    operation: YasProcessSpawnOperation,
  ): boolean {
    if (this.spawnOperations.get(operationKey) === operation) return true;
    const limit = this.replayLimit();
    const needed = this.spawnOperations.size - limit + 1;
    if (needed > 0) {
      const evictable = this.evictableSpawnOperations();
      if (evictable.length < needed) return false;
      for (const key of evictable.slice(0, needed))
        this.spawnOperations.delete(key);
    }
    this.pendingSpawnOperations.delete(operationKey);
    this.spawnOperations.set(operationKey, operation);
    return true;
  }

  private evictableSpawnOperations(): string[] {
    const result: string[] = [];
    for (const [operationKey, operation] of this.spawnOperations)
      if (operation.settled && !operation.pending && !operation.streams)
        result.push(operationKey);
    return result;
  }

  private bundleHasOwnedAuthority(
    bundle: YasProcessStreamBundle,
    identity: YasProcessSpawnIdentity | null,
  ): boolean {
    const transferIds = processBundleTransferIds(bundle);
    if (
      identity &&
      (identity.processHandle === bundle.processHandle ||
        transferIds.some((id) => identity.transferIds.includes(id)))
    )
      return true;
    for (const operation of this.spawnOperations.values()) {
      const owned = operation.identity;
      if (
        owned &&
        (owned.processHandle === bundle.processHandle ||
          transferIds.some((id) => owned.transferIds.includes(id)))
      )
        return true;
    }
    for (const streams of this.activeStreams) {
      if (streams.processHandle === bundle.processHandle) return true;
      const ownedIds = processStreamsTransferIds(streams);
      if (transferIds.some((id) => ownedIds.includes(id))) return true;
    }
    return transferIds.some((id) => this.transfers.get(id) !== undefined);
  }

  private discardUnexpectedBundle(
    bundle: YasProcessStreamBundle,
    stdoutLease: YasReceiveBudgetLease,
    stderrLease: YasReceiveBudgetLease | undefined,
    identity: YasProcessSpawnIdentity | null,
  ): void {
    if (this.bundleHasOwnedAuthority(bundle, identity)) return;
    try {
      const streams = this.acceptBundle(bundle, stdoutLease, stderrLease);
      this.discardUnexpectedStreams(streams, identity);
    } catch (error) {
      if (!this.processHandleIsOwned(bundle.processHandle, identity))
        void this.killOrphanedProcess(bundle.processHandle).catch(
          () => undefined,
        );
      throw error;
    }
  }

  private discardUnexpectedStreams(
    streams: YasProcessStreams,
    identity: YasProcessSpawnIdentity | null,
  ): void {
    for (const transfer of [streams.stdin, streams.stdout, streams.stderr]) {
      if (
        !transfer ||
        this.transfers.get(transfer.descriptor.transferId) !== transfer
      )
        continue;
      try {
        transfer.reset();
      } catch {
        // A concurrent invalidation may already have retired the Transfer.
      }
    }
    if (!this.processHandleIsOwned(streams.processHandle, identity))
      void this.killOrphanedProcess(streams.processHandle).catch(
        () => undefined,
      );
  }

  private processHandleIsOwned(
    processHandle: bigint,
    identity: YasProcessSpawnIdentity | null,
  ): boolean {
    return (
      identity?.processHandle === processHandle ||
      [...this.spawnOperations.values()].some(
        (operation) => operation.identity?.processHandle === processHandle,
      ) ||
      [...this.activeStreams].some(
        (owned) => owned.processHandle === processHandle,
      )
    );
  }

  private trackStreams(
    streams: YasProcessStreams,
    operationKey?: string,
  ): void {
    this.activeStreams.add(streams);
    const transfers = [streams.stdin, streams.stdout, streams.stderr].filter(
      (transfer): transfer is YasTransfer => transfer !== undefined,
    );
    if (operationKey) {
      let retired = false;
      const removeTerminalListeners: (() => void)[] = [];
      const retire = () => {
        if (retired) return;
        retired = true;
        for (const remove of removeTerminalListeners) remove();
        this.tombstoneSpawnOperation(operationKey, streams);
      };
      for (const transfer of transfers) {
        const remove = transfer.subscribeTerminal(retire);
        removeTerminalListeners.push(remove);
        if (retired) remove();
      }
    }
    void Promise.all(
      transfers.map((transfer) =>
        transfer.closed.then(
          () => undefined,
          () => undefined,
        ),
      ),
    ).then(() => this.activeStreams.delete(streams));
  }

  private tombstoneSpawnOperation(
    operationKey: string,
    streams: YasProcessStreams,
  ): void {
    const operation = this.spawnOperations.get(operationKey);
    if (operation?.streams !== streams) return;
    operation.streams = null;
    operation.settled = true;
  }

  private retireSpawnOperations(): void {
    for (const operation of this.spawnOperations.values()) {
      operation.pending = null;
      operation.streams = null;
      operation.settled = true;
    }
    for (const [operationKey, operation] of this.pendingSpawnOperations) {
      operation.pending = null;
      operation.streams = null;
      operation.settled = true;
      if (operation.wirePayload)
        this.retainSpawnOperation(operationKey, operation);
    }
    this.pendingSpawnOperations.clear();
  }

  private resetStreamBundle(streams: YasProcessStreams): void {
    for (const transfer of [streams.stdin, streams.stdout, streams.stderr]) {
      try {
        transfer?.reset();
      } catch {
        // The shared Transfer registry may already be invalidated.
      }
    }
    this.activeStreams.delete(streams);
  }

  private resetStreams(): void {
    for (const streams of [...this.activeStreams])
      this.resetStreamBundle(streams);
  }

  private runOwned<T>(operation: Promise<T>): Promise<T> {
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    this.pendingCancels.add(cancel);
    return Promise.race([operation, cancelled]).finally(() => {
      this.pendingCancels.delete(cancel);
    });
  }

  private killOrphanedProcess(processHandle: bigint): Promise<Uint8Array> {
    return this.connection.request(
      g.YAS_FAMILY_PROCESS,
      g.YAS_PROCESS_CONTROL,
      encodeProcessControl({
        processHandle,
        operationId: processOperationId(),
        action: g.YAS_PROCESS_CONTROL_KILL,
        value: 0,
      }),
    );
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Process client is disposed");
  }
}

let processOperationIdCounter = 1n;
function processOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto?.getRandomValues(value);
  if (value.every((byte) => byte === 0))
    new DataView(value.buffer).setBigUint64(
      8,
      processOperationIdCounter++,
      true,
    );
  return value;
}

function byteKey(value: Uint8Array): string {
  let output = "";
  for (const byte of value) output += String.fromCharCode(byte);
  return output;
}

function processBundleTransferIds(
  bundle: YasProcessStreamBundle,
): readonly number[] {
  return [bundle.stdin, bundle.stdout, bundle.stderr]
    .filter(
      (descriptor): descriptor is YasTransferDescriptor =>
        descriptor !== undefined,
    )
    .map((descriptor) => descriptor.transferId);
}

function processStreamsTransferIds(
  streams: YasProcessStreams,
): readonly number[] {
  return [streams.stdin, streams.stdout, streams.stderr]
    .filter((transfer): transfer is YasTransfer => transfer !== undefined)
    .map((transfer) => transfer.descriptor.transferId);
}

function processSpawnIdentity(
  streams: YasProcessStreams,
): YasProcessSpawnIdentity {
  return {
    processHandle: streams.processHandle,
    transferIds: processStreamsTransferIds(streams),
  };
}

function validateCwd(value: YasProcessCwd): void {
  if (value.kind === "path") validateNativePath(value.path);
  else if (value.kind === "terminal")
    requireHandle(value.terminalHandle, "Process cwd terminal handle");
  else if (value.kind === "fs") {
    requireHandle(value.rootHandle, "Process cwd FS root handle");
    if (value.components.length > g.YAS_PROCESS_MAX_PATH_COMPONENTS)
      throw new YasProtocolError("too many Process cwd path components");
    let total = 0;
    for (const component of value.components) {
      validateComponent(component);
      total += component.length;
    }
    if (total > g.YAS_PROCESS_MAX_CWD_BYTES)
      throw new YasProtocolError("Process cwd exceeds its byte limit");
  }
}

function validateSpawn(value: YasProcessSpawn): void {
  requireOperationId(value.operationId, "Process SPAWN");
  if (
    value.flags & ~g.YAS_PROCESS_SPAWN_FLAGS ||
    (value.environmentKind !== g.YAS_PROCESS_ENV_EMPTY &&
      value.environmentKind !== g.YAS_PROCESS_ENV_SESSION)
  )
    throw new YasProtocolError(
      "invalid Process spawn flags or environment kind",
    );
  validateCwd(value.cwd);
  validateArgv(value.argv);
  validateEnvironment(value.environment);
  if (value.stdoutReceiveCredit === 0n)
    throw new YasProtocolError("zero Process stdout receive credit");
  const merged = Boolean(value.flags & g.YAS_PROCESS_SPAWN_MERGE_STDERR);
  if (merged !== (value.stderrReceiveCredit === 0n))
    throw new YasProtocolError("invalid Process stderr receive credit");
  validateSpawnExtensions(value.extensions ?? []);
}

function validateAttach(value: YasProcessAttach): void {
  requireHandle(value.processHandle, "Process handle");
  if (
    value.flags & ~g.YAS_PROCESS_ATTACH_STDIN ||
    value.stdoutReceiveCredit === 0n
  )
    throw new YasProtocolError("invalid Process ATTACH flags or credit");
}

function validateControl(value: YasProcessControl): void {
  requireHandle(value.processHandle, "Process handle");
  requireOperationId(value.operationId, "Process CONTROL");
  if (value.action > g.YAS_PROCESS_CONTROL_DETACH)
    throw new YasProtocolError("invalid Process control action");
  if (value.action === g.YAS_PROCESS_CONTROL_SIGNAL) {
    if (
      value.value < g.YAS_PROCESS_SIGNAL_INTERRUPT ||
      value.value > g.YAS_PROCESS_SIGNAL_HANGUP
    )
      throw new YasProtocolError("invalid Process signal");
  } else if (value.value !== 0)
    throw new YasProtocolError("non-signal Process control has a value");
}

function validateExit(value: YasProcessExit): void {
  if (
    value.kind > g.YAS_PROCESS_EXIT_KIND_OTHER ||
    value.reason > g.YAS_PROCESS_EXIT_REASON_SERVER_SHUTDOWN ||
    value.exitedServerNs === 0n ||
    value.detail.length > 4096
  )
    throw new YasProtocolError("invalid Process exit record");
  const valid =
    (value.kind === g.YAS_PROCESS_EXIT_KIND_CODE &&
      value.reason === g.YAS_PROCESS_EXIT_REASON_UNKNOWN) ||
    (value.kind === g.YAS_PROCESS_EXIT_KIND_SIGNAL &&
      value.reason >= g.YAS_PROCESS_EXIT_REASON_INTERRUPT &&
      value.reason <= g.YAS_PROCESS_EXIT_REASON_HANGUP) ||
    (value.kind === g.YAS_PROCESS_EXIT_KIND_KILLED &&
      value.reason >= g.YAS_PROCESS_EXIT_REASON_CLIENT &&
      value.code === 0) ||
    (value.kind === g.YAS_PROCESS_EXIT_KIND_OTHER &&
      value.reason === g.YAS_PROCESS_EXIT_REASON_UNKNOWN &&
      value.code === 0 &&
      value.detail.length !== 0);
  if (!valid)
    throw new YasProtocolError("invalid Process exit field combination");
}

function validateStreamBundle(value: YasProcessStreamBundle): void {
  requireHandle(value.processHandle, "Process handle");
  if (value.mergedStderr !== (value.stderr === undefined))
    throw new YasProtocolError("invalid Process stderr bundle shape");
  if (value.stdin)
    validateStreamDescriptor(
      value.stdin,
      g.YAS_PROCESS_STREAM_STDIN_CONTENT_KIND,
      YAS_TRANSFER_RECEIVER_TO_SENDER,
    );
  validateStreamDescriptor(
    value.stdout,
    g.YAS_PROCESS_STREAM_STDOUT_CONTENT_KIND,
    YAS_TRANSFER_SENDER_TO_RECEIVER,
  );
  if (value.stderr)
    validateStreamDescriptor(
      value.stderr,
      g.YAS_PROCESS_STREAM_STDERR_CONTENT_KIND,
      YAS_TRANSFER_SENDER_TO_RECEIVER,
    );
  const ids = [
    value.stdin?.transferId,
    value.stdout.transferId,
    value.stderr?.transferId,
  ].filter((id): id is number => id !== undefined);
  if (new Set(ids).size !== ids.length)
    throw new YasProtocolError("reused Process stream Transfer ID");
}

function validateStreamDescriptor(
  value: YasTransferDescriptor,
  contentKind: number,
  direction: number,
): void {
  const sensitive = value.extensions.some(
    (extension) =>
      extension.tag === g.YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION &&
      extension.required &&
      extension.value.length === 0,
  );
  if (
    value.mode !== YAS_TRANSFER_MODE_BYTE ||
    value.direction !== direction ||
    value.contentFamily !== g.YAS_FAMILY_PROCESS ||
    value.contentKind !== contentKind ||
    value.contentVersion !== g.YAS_PROCESS_VERSION ||
    !sensitive
  )
    throw new YasProtocolError("invalid Process stream Transfer descriptor");
}

function validateProcessRecord(value: YasProcessRecord): void {
  requireHandle(value.processHandle, "Process handle");
  const running = value.lifecycle === g.YAS_PROCESS_LIFECYCLE_RUNNING;
  const exited = value.lifecycle === g.YAS_PROCESS_LIFECYCLE_EXITED;
  if (
    (!running && !exited) ||
    value.streamState & ~g.YAS_PROCESS_STREAM_STATE_FLAGS ||
    value.flags & ~g.YAS_PROCESS_SPAWN_FLAGS ||
    value.nativePid === 0n ||
    value.ownerSession.length !== 16 ||
    value.ownerSession.every((byte) => byte === 0)
  )
    throw new YasProtocolError("invalid Process record identity or flags");
  validateArg(value.argv0);
  if (
    exited !== (value.exit !== undefined) ||
    (exited && value.streamState !== 0)
  )
    throw new YasProtocolError("invalid Process lifecycle or stream state");
  if (
    !(value.flags & g.YAS_PROCESS_SPAWN_DETACHABLE) &&
    value.retentionDeadlineServerNs !== 0n
  )
    throw new YasProtocolError("ordinary Process has a retention deadline");
  if (value.exit) validateExit(value.exit);
}

function validateArgv(argv: readonly Uint8Array[]): void {
  if (argv.length === 0 || argv.length > g.YAS_PROCESS_MAX_ARGC)
    throw new YasProtocolError("invalid Process argument count");
  let total = 0;
  for (const arg of argv) {
    validateArg(arg);
    total += arg.length;
  }
  if (total > g.YAS_PROCESS_MAX_ARG_BYTES)
    throw new YasProtocolError("Process arguments exceed their byte limit");
}

function validateArg(value: Uint8Array): void {
  if (
    value.length === 0 ||
    value.length > g.YAS_PROCESS_MAX_ARG_LEN ||
    value.includes(0)
  )
    throw new YasProtocolError("invalid Process argument");
}

function validateEnvironment(env: readonly YasProcessEnvironmentEntry[]): void {
  if (env.length > g.YAS_PROCESS_MAX_ENVC)
    throw new YasProtocolError("too many Process environment entries");
  let total = 0;
  let previous: Uint8Array | undefined;
  for (const entry of env) {
    if (
      entry.key.length === 0 ||
      entry.key.length > g.YAS_PROCESS_MAX_ENV_KEY_BYTES ||
      entry.value.length > g.YAS_PROCESS_MAX_ENV_VALUE_BYTES ||
      entry.key.includes(0) ||
      entry.key.includes(0x3d) ||
      entry.value.includes(0) ||
      (previous !== undefined && compareBytes(previous, entry.key) >= 0)
    )
      throw new YasProtocolError("invalid Process environment entry");
    previous = entry.key;
    total += entry.key.length + entry.value.length;
  }
  if (total > g.YAS_PROCESS_MAX_ENV_BYTES)
    throw new YasProtocolError("Process environment exceeds its byte limit");
}

function validateNativePath(path: Uint8Array): void {
  if (
    path.length === 0 ||
    path.length > g.YAS_PROCESS_MAX_CWD_BYTES ||
    path.includes(0)
  )
    throw new YasProtocolError("invalid Process native cwd path");
}

function validateComponent(value: Uint8Array): void {
  const dot = value.length === 1 && value[0] === 0x2e;
  const dotDot = value.length === 2 && value[0] === 0x2e && value[1] === 0x2e;
  if (
    value.length === 0 ||
    dot ||
    dotDot ||
    value.includes(0) ||
    value.includes(0x2f) ||
    value.includes(0x5c)
  )
    throw new YasProtocolError("invalid Process FS cwd component");
}

function validateSpawnExtensions(extensions: readonly YasExtension[]): void {
  for (const extension of extensions) {
    if (extension.tag === g.YAS_PROCESS_SPAWN_SURFACE_APP_EXTENSION) {
      const cursor = new YasCursor(extension.value);
      requireHandle(
        cursor.u64("Process surface application handle"),
        "Process surface application handle",
      );
      cursor.end("Process surface application extension");
    } else if (extension.tag === g.YAS_PROCESS_SPAWN_RESOURCE_TAG_EXTENSION) {
      if (extension.value.length > 4096)
        throw new YasProtocolError("Process resource tag exceeds its limit");
    } else if (extension.required)
      throw new YasProtocolError("unknown required Process SPAWN extension");
  }
}

function validateLimits(value: YasProcessLimits): void {
  const valid =
    within(value.maxArgc, g.YAS_PROCESS_MAX_ARGC) &&
    within(value.maxArgBytes, g.YAS_PROCESS_MAX_ARG_BYTES) &&
    within(value.maxEnvc, g.YAS_PROCESS_MAX_ENVC) &&
    within(value.maxEnvBytes, g.YAS_PROCESS_MAX_ENV_BYTES) &&
    within(
      value.maxProcessesPerSession,
      g.YAS_PROCESS_MAX_PROCESSES_PER_SESSION,
    ) &&
    within(value.maxProcesses, g.YAS_PROCESS_MAX_PROCESSES) &&
    within(value.maxPendingSpawns, g.YAS_PROCESS_MAX_PENDING_SPAWNS) &&
    value.maxStreamBufferBytes > 0n &&
    value.maxStreamBufferBytes <=
      BigInt(g.YAS_PROCESS_MAX_STREAM_BUFFER_BYTES) &&
    value.maxDetachedRetentionNs > 0n &&
    value.maxDetachedRetentionNs <=
      BigInt(g.YAS_PROCESS_MAX_DETACHED_RETENTION_NS) &&
    within(value.maxMutationReplays, g.YAS_PROCESS_MAX_MUTATION_REPLAYS);
  if (!valid) throw new YasProtocolError("invalid Process family limit");
}

function decodeDescriptorBytes(bytes: Uint8Array): YasTransferDescriptor {
  const cursor = new YasCursor(bytes);
  const value = decodeTransferDescriptor(cursor);
  cursor.end("Process Transfer descriptor");
  return value;
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const count = Math.min(left.length, right.length);
  for (let index = 0; index < count; index++) {
    const difference = left[index]! - right[index]!;
    if (difference !== 0) return difference;
  }
  return left.length - right.length;
}

function requireOperationId(value: Uint8Array, context: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${context} operation ID is invalid`);
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireRevision(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireZero(bytes: Uint8Array, context: string): void {
  if (bytes.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function within(value: number, maximum: number): boolean {
  return Number.isInteger(value) && value > 0 && value <= maximum;
}

function extension32(tag: number, value: number): YasExtension {
  return { tag, required: false, value: new YasWriter().u32(value).finish() };
}

function extension64(tag: number, value: bigint): YasExtension {
  return { tag, required: false, value: new YasWriter().u64(value).finish() };
}

function extensionU32(
  extensions: readonly YasExtension[],
  tag: number,
): number {
  const value = extensions.find((extension) => extension.tag === tag);
  if (!value) throw new YasProtocolError("missing Process family limit");
  const cursor = new YasCursor(value.value);
  const result = cursor.u32("Process family limit");
  cursor.end("Process family limit");
  return result;
}

function extensionU64(
  extensions: readonly YasExtension[],
  tag: number,
): bigint {
  const value = extensions.find((extension) => extension.tag === tag);
  if (!value) throw new YasProtocolError("missing Process family limit");
  const cursor = new YasCursor(value.value);
  const result = cursor.u64("Process family limit");
  cursor.end("Process family limit");
  return result;
}
