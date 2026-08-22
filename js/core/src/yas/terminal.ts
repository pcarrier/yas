import * as g from "./generated";
import {
  YAS_CLASS_EVENT,
  YAS_FAMILY_TERMINAL,
  YAS_TERMINAL_CLOSE,
  YAS_TERMINAL_CLOSE_VIEW,
  YAS_TERMINAL_COMMAND_ARGV,
  YAS_TERMINAL_COMMAND_DEFAULT_SHELL,
  YAS_TERMINAL_COMMAND_SHELL_COMMAND,
  YAS_TERMINAL_CONFIGURE_VIEW,
  YAS_TERMINAL_CONFIGURE_COLS_EXTENSION,
  YAS_TERMINAL_CONFIGURE_MAX_FPS_EXTENSION,
  YAS_TERMINAL_CONFIGURE_PRESENTATION_METRICS_EXTENSION,
  YAS_TERMINAL_CONFIGURE_QUEUE_TARGET_EXTENSION,
  YAS_TERMINAL_CONFIGURE_ROWS_EXTENSION,
  YAS_TERMINAL_COPY_RANGE,
  YAS_TERMINAL_CREATE,
  YAS_TERMINAL_CUTOVER_START_THEN_SWITCH,
  YAS_TERMINAL_CUTOVER_STOP_THEN_START,
  YAS_TERMINAL_CWD,
  YAS_TERMINAL_CWD_PATH,
  YAS_TERMINAL_CWD_SERVER_DEFAULT,
  YAS_TERMINAL_CWD_TERMINAL,
  YAS_TERMINAL_DEADLINE_CLEAR,
  YAS_TERMINAL_DEADLINE_SET,
  YAS_TERMINAL_ENVIRONMENT_EMPTY,
  YAS_TERMINAL_ENVIRONMENT_REMOVE,
  YAS_TERMINAL_ENVIRONMENT_SERVER,
  YAS_TERMINAL_ENVIRONMENT_SET,
  YAS_TERMINAL_EXIT_KIND_CODE,
  YAS_TERMINAL_EXIT_KIND_OTHER,
  YAS_TERMINAL_EXIT_KIND_SIGNAL,
  YAS_TERMINAL_EXIT_REASON_UNKNOWN,
  YAS_TERMINAL_EXIT_REASON_HANGUP,
  YAS_TERMINAL_FRAME,
  YAS_TERMINAL_FRAME_ACK,
  YAS_TERMINAL_FRAME_CHUNK,
  YAS_TERMINAL_FRAME_EXPLICIT_BASE,
  YAS_TERMINAL_INPUT,
  YAS_TERMINAL_JOURNAL,
  YAS_TERMINAL_LAUNCH_REPLACE,
  YAS_TERMINAL_LAUNCH_REPLAY,
  YAS_TERMINAL_LIFECYCLE_EXITED,
  YAS_TERMINAL_LIFECYCLE_RUNNING,
  YAS_TERMINAL_MOUSE,
  YAS_TERMINAL_MOUSE_ACTION_UP,
  YAS_TERMINAL_MOUSE_BUTTON_FORWARD,
  YAS_TERMINAL_MODIFIER_ALT,
  YAS_TERMINAL_MODIFIER_CAPS_LOCK,
  YAS_TERMINAL_MODIFIER_CTRL,
  YAS_TERMINAL_MODIFIER_NUM_LOCK,
  YAS_TERMINAL_MODIFIER_SHIFT,
  YAS_TERMINAL_MODIFIER_SUPER,
  YAS_TERMINAL_OPEN_VIEW,
  YAS_TERMINAL_OUTPUT,
  YAS_TERMINAL_READ,
  YAS_TERMINAL_RESET_VIEW,
  YAS_TERMINAL_RESIZE,
  YAS_TERMINAL_RESTART,
  YAS_TERMINAL_SCROLL,
  YAS_TERMINAL_SCROLL_ABSOLUTE,
  YAS_TERMINAL_SCROLL_RELATIVE,
  YAS_TERMINAL_SEARCH,
  YAS_TERMINAL_SEARCH_CATALOG,
  YAS_TERMINAL_SET_DEADLINE,
  YAS_TERMINAL_SET_FOCUS,
  YAS_TERMINAL_SIGNAL,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_STATE_APP_HANDLE_EXTENSION,
  YAS_TERMINAL_STATE_COMMAND_DISPLAY_EXTENSION,
  YAS_TERMINAL_STATE_CWD_EXTENSION,
  YAS_TERMINAL_STATE_DEADLINE_SERVER_NS_EXTENSION,
  YAS_TERMINAL_STATE_EXIT_EXTENSION,
  YAS_TERMINAL_STATE_JOURNAL_CURSOR_EXTENSION,
  YAS_TERMINAL_STATE_RESOURCE_TAG_EXTENSION,
  YAS_TERMINAL_STATE_TITLE_EXTENSION,
  YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES,
  YAS_TERMINAL_QUERY_EXTENSION_NEXT_CURSOR,
  YAS_TERMINAL_QUERY_EXTENSION_SATISFYING_STATE_REVISION,
  YAS_TERMINAL_QUERY_EXTENSION_TOTAL_LINES,
  YAS_TERMINAL_QUERY_EXTENSION_TRANSFER,
  YAS_TERMINAL_QUERY_INLINE,
  YAS_TERMINAL_QUERY_TRANSFER,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_VERSION,
  YAS_TERMINAL_WAIT,
  YAS_TERMINAL_WATCH,
  YAS_TERMINAL_WHEEL,
  YAS_TERMINAL_WHEEL_AT,
  YAS_TERMINAL_WHEEL_SOURCE_CONTINUOUS,
  YAS_TERMINAL_WRITE,
} from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateSubscription,
  YasStateCatalogueRetention,
  detachStateRetainedValue,
  negotiatedStateLimitU32,
  estimateStateRetainedBytes,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
  type YasTransferManager,
} from "./transfer";
import {
  YasCursor,
  YAS_MAX_BULK_CHUNK,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export * from "./terminal-grid";
export {
  YAS_FAMILY_TERMINAL,
  YAS_TERMINAL_CLOSE,
  YAS_TERMINAL_CLOSE_VIEW,
  YAS_TERMINAL_CONFIGURE_VIEW,
  YAS_TERMINAL_COPY_RANGE,
  YAS_TERMINAL_CREATE,
  YAS_TERMINAL_CWD,
  YAS_TERMINAL_FRAME,
  YAS_TERMINAL_FRAME_ACK,
  YAS_TERMINAL_FRAME_CHUNK,
  YAS_TERMINAL_INPUT,
  YAS_TERMINAL_JOURNAL,
  YAS_TERMINAL_MOUSE,
  YAS_TERMINAL_OPEN_VIEW,
  YAS_TERMINAL_OUTPUT,
  YAS_TERMINAL_READ,
  YAS_TERMINAL_RESET_VIEW,
  YAS_TERMINAL_RESIZE,
  YAS_TERMINAL_RESTART,
  YAS_TERMINAL_SCROLL,
  YAS_TERMINAL_SEARCH,
  YAS_TERMINAL_SEARCH_CATALOG,
  YAS_TERMINAL_SET_DEADLINE,
  YAS_TERMINAL_SET_FOCUS,
  YAS_TERMINAL_SIGNAL,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_VERSION,
  YAS_TERMINAL_WAIT,
  YAS_TERMINAL_WATCH,
  YAS_TERMINAL_WHEEL,
  YAS_TERMINAL_WRITE,
} from "./generated";

const terminalStateTags = new Set([
  YAS_TERMINAL_STATE_TITLE_EXTENSION,
  YAS_TERMINAL_STATE_CWD_EXTENSION,
  YAS_TERMINAL_STATE_COMMAND_DISPLAY_EXTENSION,
  YAS_TERMINAL_STATE_EXIT_EXTENSION,
  YAS_TERMINAL_STATE_DEADLINE_SERVER_NS_EXTENSION,
  YAS_TERMINAL_STATE_APP_HANDLE_EXTENSION,
  YAS_TERMINAL_STATE_JOURNAL_CURSOR_EXTENSION,
  YAS_TERMINAL_STATE_RESOURCE_TAG_EXTENSION,
]);
const queryTags: ReadonlySet<number> = new Set<number>([
  YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES,
  YAS_TERMINAL_QUERY_EXTENSION_TRANSFER,
  YAS_TERMINAL_QUERY_EXTENSION_NEXT_CURSOR,
  YAS_TERMINAL_QUERY_EXTENSION_TOTAL_LINES,
  YAS_TERMINAL_QUERY_EXTENSION_SATISFYING_STATE_REVISION,
]);

export type YasTerminalCommand =
  | { kind: typeof YAS_TERMINAL_COMMAND_DEFAULT_SHELL }
  | { kind: typeof YAS_TERMINAL_COMMAND_ARGV; argv: readonly Uint8Array[] }
  | { kind: typeof YAS_TERMINAL_COMMAND_SHELL_COMMAND; command: string };

export type YasTerminalCwd =
  | { kind: typeof YAS_TERMINAL_CWD_SERVER_DEFAULT }
  | { kind: typeof YAS_TERMINAL_CWD_PATH; path: Uint8Array }
  | { kind: typeof YAS_TERMINAL_CWD_TERMINAL; terminalHandle: bigint };

export interface YasTerminalEnvironmentEntry {
  key: Uint8Array;
  kind:
    | typeof YAS_TERMINAL_ENVIRONMENT_SET
    | typeof YAS_TERMINAL_ENVIRONMENT_REMOVE;
  value?: Uint8Array;
}

export interface YasTerminalLaunch {
  command: YasTerminalCommand;
  cwd: YasTerminalCwd;
  environmentBase:
    | typeof YAS_TERMINAL_ENVIRONMENT_SERVER
    | typeof YAS_TERMINAL_ENVIRONMENT_EMPTY;
  environment?: readonly YasTerminalEnvironmentEntry[];
  extensions?: readonly YasExtension[];
}

export interface YasTerminalRecord {
  handle: bigint;
  lifecycle:
    | typeof YAS_TERMINAL_LIFECYCLE_RUNNING
    | typeof YAS_TERMINAL_LIFECYCLE_EXITED;
  rows: number;
  cols: number;
  generation: number;
  usedRows: number;
  title?: string;
  cwd?: Uint8Array;
  commandDisplay?: string;
  exit?: YasTerminalExitRecord;
  deadlineServerNs?: bigint;
  appHandle?: bigint;
  journalCursor?: bigint;
  resourceTag?: string;
  extensions: readonly YasExtension[];
}

export type YasTerminalExitRecord =
  | { kind: typeof YAS_TERMINAL_EXIT_KIND_CODE; code: number; detail: string }
  | {
      kind: typeof YAS_TERMINAL_EXIT_KIND_SIGNAL;
      reason: number;
      nativeSignal: number;
      detail: string;
    }
  | { kind: typeof YAS_TERMINAL_EXIT_KIND_OTHER; detail: string };

export interface YasTerminalSnapshot {
  revision: bigint;
  terminals: readonly YasTerminalRecord[];
}

export interface YasTerminalCreate {
  rows: number;
  cols: number;
  operationId: Uint8Array;
  launch: YasTerminalLaunch;
  initialView?: YasTerminalInitialViewRequest;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalInitialViewRequest {
  rows: number;
  cols: number;
  maxFps: number;
  codecVersions: readonly number[];
  extensions?: readonly YasExtension[];
}

export interface YasTerminalCreateResult {
  terminalHandle: bigint;
  stateRevision: bigint;
  generation: number;
  initialView?: YasTerminalView;
  extensions: readonly YasExtension[];
}

interface YasTerminalDecodedCreateResult {
  terminalHandle: bigint;
  stateRevision: bigint;
  generation: number;
  initialViewResult?: YasTerminalOpenViewResult;
  extensions: readonly YasExtension[];
}

export interface YasTerminalOpenView {
  terminalHandle: bigint;
  rows: number;
  cols: number;
  maxFps: number;
  codecVersions: readonly number[];
  extensions?: readonly YasExtension[];
}

export interface YasTerminalOpenViewResult {
  viewId: number;
  codecVersion: number;
  maxInflightFrames: number;
  maxEncodedFrame: number;
  maxDecodedFrame: number;
  firstSequence: number;
  extensions: readonly YasExtension[];
}

export interface YasTerminalViewFeedback {
  viewId: number;
  presentedSequence: number;
  decoderQueueDepth: number;
  availableFrameSlots: number;
}

export interface YasTerminalPresentationMetrics {
  viewportWidthPx: number;
  viewportHeightPx: number;
  cellWidth16_16: number;
  cellHeight16_16: number;
  deviceScale16_16: number;
}

export interface YasTerminalViewConfiguration {
  rows?: number;
  cols?: number;
  maxFps?: number;
  presentationMetrics?: YasTerminalPresentationMetrics;
  queueTarget?: number;
}

export interface YasTerminalFrameEvent {
  viewId: number;
  sequence: number;
  flags: number;
  explicitBase?: number;
  gridPayload: Uint8Array;
}

export interface YasTerminalFrameChunkEvent {
  viewId: number;
  sequence: number;
  chunkIndex: number;
  chunkCount: number;
  logicalFrameLength: number;
  chunk: Uint8Array;
}

export interface YasTerminalQueryCursor {
  kind: number;
  a: bigint;
  b: number;
}

export type YasTerminalQueryNextCursor =
  | { kind: "read"; cursor: YasTerminalQueryCursor }
  | { kind: "search"; cursor: YasTerminalQueryCursor }
  | { kind: "journal"; index: bigint }
  | { kind: "output"; cursor: YasTerminalQueryCursor };

export interface YasTerminalRead {
  terminalHandle: bigint;
  generation: number;
  cursor: YasTerminalQueryCursor;
  representation: number;
  flags: number;
  maxBytes: number;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalSearch {
  terminalHandle: bigint;
  generation: number;
  flags: number;
  startCursor: YasTerminalQueryCursor;
  maxResults: number;
  query: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalCatalogSearch {
  maxResults: number;
  query: string;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalCatalogSearchEntry {
  terminalHandle: bigint;
  generation: number;
  score: number;
  primarySource: number;
  matchedSources: number;
  /** Lines above the live viewport. Zero means no scrollback jump. */
  scrollOffset: bigint;
  context: string;
}

export interface YasTerminalCatalogSearchResult {
  flags: number;
  entries: readonly YasTerminalCatalogSearchEntry[];
  extensions: readonly YasExtension[];
}

export interface YasTerminalCwdQuery {
  terminalHandle: bigint;
  generation: number;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalJournal {
  terminalHandle: bigint;
  generation: number;
  flags: number;
  limit: number;
  fromIndex: bigint;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalOutput {
  terminalHandle: bigint;
  generation: number;
  cursor: YasTerminalQueryCursor;
  flags: number;
  maxBytes: number;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalWait {
  terminalHandle: bigint;
  generation: number;
  waitKind: number;
  flags: number;
  cursorA: bigint;
  cursorB: number;
  maxBytes: number;
  timeoutNs: bigint;
  needle: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasTerminalCopyRange {
  terminalHandle: bigint;
  generation: number;
  representation: number;
  startRow: bigint;
  startCol: number;
  endRow: bigint;
  endCol: number;
  maxBytes: number;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export type YasTerminalQueryDelivery =
  | { kind: "inline"; bytes: Uint8Array }
  | { kind: "transfer"; descriptor: YasTransferDescriptor };

export interface YasTerminalQueryBody {
  representation: number;
  contentKind: number;
  encoding: number;
  flags: number;
  delivery: YasTerminalQueryDelivery;
  nextCursor?: YasTerminalQueryNextCursor;
  totalLines?: bigint;
  satisfyingStateRevision?: bigint;
  extensions: readonly YasExtension[];
}

export interface YasTerminalQueryResult {
  representation: number;
  contentKind: number;
  encoding: number;
  flags: number;
  extensions: readonly YasExtension[];
  nextCursor?: YasTerminalQueryNextCursor;
  totalLines?: bigint;
  satisfyingStateRevision?: bigint;
  bytes(): Promise<Uint8Array>;
  content(): Promise<YasTerminalQueryContent>;
}

export interface YasTerminalSearchMatch {
  startRow: bigint;
  startCol: number;
  endRow: bigint;
  endCol: number;
  preview: string;
}

export interface YasTerminalJournalRecord {
  index: bigint;
  generation: number;
  flags: number;
  exitCode: number;
  startSequence: bigint;
  endSequence: bigint;
  startedUnixMs: bigint;
  endedUnixMs: bigint;
  command: string;
}

export interface YasTerminalJournalResult {
  oldestIndex: bigint;
  nextIndex: bigint;
  records: readonly YasTerminalJournalRecord[];
}

export interface YasTerminalOutputResult {
  generation: number;
  flags: number;
  startSequence: bigint;
  startCol: number;
  nextSequence: bigint;
  nextCol: number;
  text: Uint8Array;
}

export interface YasTerminalStyledOverflow {
  cellOffset: number;
  text: string;
}

export interface YasTerminalStyledHyperlink {
  startCol: number;
  cellCount: number;
  uri: string;
}

export interface YasTerminalStyledLine {
  row: bigint;
  startCol: number;
  cells: readonly Uint8Array[];
  overflow: readonly YasTerminalStyledOverflow[];
  hyperlinks: readonly YasTerminalStyledHyperlink[];
}

export interface YasTerminalTextAndStyled {
  plain: string;
  styled: readonly YasTerminalStyledLine[];
}

export type YasTerminalQueryContent =
  | { kind: "text"; value: string }
  | { kind: "path"; value: Uint8Array }
  | { kind: "styled-lines"; value: readonly YasTerminalStyledLine[] }
  | { kind: "search-results"; value: readonly YasTerminalSearchMatch[] }
  | { kind: "journal"; value: YasTerminalJournalResult }
  | { kind: "output"; value: YasTerminalOutputResult }
  | { kind: "text-and-styled"; value: YasTerminalTextAndStyled };

export function encodeTerminalQueryCursor(
  value: YasTerminalQueryCursor,
): Uint8Array {
  return new YasWriter().u8(value.kind).u64(value.a).u32(value.b).finish();
}

export function decodeTerminalQueryCursor(
  bytes: Uint8Array,
): YasTerminalQueryCursor {
  const cursor = new YasCursor(bytes);
  const value = decodeQueryCursorFrom(cursor);
  cursor.end("Terminal query cursor");
  return value;
}

export function encodeTerminalRead(value: YasTerminalRead): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  validateQueryRepresentation(value.representation);
  if (
    value.cursor.kind > g.YAS_TERMINAL_READ_CURSOR_TAIL ||
    value.flags !== g.YAS_TERMINAL_READ_FLAGS ||
    value.maxBytes === 0
  )
    throw new YasProtocolError("invalid Terminal READ cursor or flags");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u8(value.cursor.kind)
    .u8(value.representation)
    .u16(value.flags)
    .u64(value.cursor.a)
    .u32(value.cursor.b)
    .u32(value.maxBytes)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalRead(bytes: Uint8Array): YasTerminalRead {
  const cursor = new YasCursor(bytes);
  const value: YasTerminalRead = {
    terminalHandle: cursor.u64("Terminal handle"),
    generation: cursor.u32("Terminal generation"),
    cursor: {
      kind: cursor.u8("Terminal READ cursor kind"),
      a: 0n,
      b: 0,
    },
    representation: cursor.u8("Terminal query representation"),
    flags: cursor.u16("Terminal READ flags"),
    maxBytes: 0,
    initialReceiveCredit: 0n,
    extensions: [],
  };
  value.cursor.a = cursor.u64("Terminal READ cursor a");
  value.cursor.b = cursor.u32("Terminal READ cursor b");
  value.maxBytes = cursor.u32("Terminal READ byte limit");
  value.initialReceiveCredit = cursor.u64("Terminal READ receive credit");
  value.extensions = decodeTerminalRequestExtensions(cursor, "READ");
  cursor.end("Terminal READ");
  encodeTerminalRead(value);
  return value;
}

export function encodeTerminalSearch(value: YasTerminalSearch): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  if (
    value.flags & ~g.YAS_TERMINAL_SEARCH_FLAGS ||
    value.startCursor.kind !== g.YAS_TERMINAL_SEARCH_CURSOR_POSITION ||
    value.maxResults === 0 ||
    value.query.length === 0 ||
    value.query.length > g.YAS_TERMINAL_MAX_INPUT_BYTES
  )
    throw new YasProtocolError("invalid Terminal SEARCH bounds or query");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u16(value.flags)
    .u16(0)
    .bytes(encodeTerminalQueryCursor(value.startCursor))
    .u32(value.maxResults)
    .bytesU32(value.query)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalSearch(bytes: Uint8Array): YasTerminalSearch {
  const cursor = new YasCursor(bytes);
  const terminalHandle = cursor.u64("Terminal handle");
  const generation = cursor.u32("Terminal generation");
  const flags = cursor.u16("Terminal SEARCH flags");
  requireTerminalZero(cursor.take(2, "Terminal SEARCH reserved"), "SEARCH");
  const value: YasTerminalSearch = {
    terminalHandle,
    generation,
    flags,
    startCursor: decodeQueryCursorFrom(cursor),
    maxResults: cursor.u32("Terminal SEARCH result limit"),
    query: new Uint8Array(cursor.bytesU32("Terminal SEARCH query")),
    initialReceiveCredit: cursor.u64("Terminal SEARCH receive credit"),
    extensions: decodeTerminalRequestExtensions(cursor, "SEARCH"),
  };
  cursor.end("Terminal SEARCH");
  encodeTerminalSearch(value);
  return value;
}

export function encodeTerminalCatalogSearch(
  value: YasTerminalCatalogSearch,
): Uint8Array {
  if (
    !Number.isInteger(value.maxResults) ||
    value.maxResults <= 0 ||
    value.maxResults > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
    new TextEncoder().encode(value.query).length >
      g.YAS_TERMINAL_MAX_CATALOG_SEARCH_QUERY_BYTES
  )
    throw new YasProtocolError("invalid Terminal catalogue search bounds");
  return new YasWriter()
    .u32(value.maxResults)
    .utf8U32(value.query)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalCatalogSearch(
  bytes: Uint8Array,
): YasTerminalCatalogSearch {
  const cursor = new YasCursor(bytes);
  const value: YasTerminalCatalogSearch = {
    maxResults: cursor.u32("Terminal catalogue search result limit"),
    query: cursor.utf8U32("Terminal catalogue search query"),
    extensions: decodeTerminalRequestExtensions(cursor, "SEARCH_CATALOG"),
  };
  cursor.end("Terminal SEARCH_CATALOG");
  encodeTerminalCatalogSearch(value);
  return value;
}

export function encodeTerminalCatalogSearchResult(
  value: YasTerminalCatalogSearchResult,
): Uint8Array {
  if (
    value.flags & ~g.YAS_TERMINAL_CATALOG_SEARCH_RESULT_FLAGS ||
    value.entries.length > g.YAS_TERMINAL_MAX_QUERY_RECORDS
  )
    throw new YasProtocolError("invalid Terminal catalogue search result");
  const writer = new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u32(value.entries.length);
  for (const entry of value.entries) {
    validateTerminalCatalogSearchEntry(entry);
    writer
      .u64(entry.terminalHandle)
      .u32(entry.generation)
      .u32(entry.score)
      .u8(entry.primarySource)
      .u8(entry.matchedSources)
      .u16(0)
      .u64(entry.scrollOffset)
      .utf8U32(entry.context);
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeTerminalCatalogSearchResult(
  bytes: Uint8Array,
): YasTerminalCatalogSearchResult {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Terminal catalogue search flags");
  if (cursor.u16("Terminal catalogue search reserved") !== 0)
    throw new YasProtocolError(
      "Terminal catalogue search Result reserved field is nonzero",
    );
  const count = cursor.u32("Terminal catalogue search result count");
  if (
    count > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
    count > Math.floor(cursor.remaining / 32)
  )
    throw new YasProtocolError(
      "invalid Terminal catalogue search result count",
    );
  const entries: YasTerminalCatalogSearchEntry[] = [];
  for (let index = 0; index < count; index++) {
    const entry: YasTerminalCatalogSearchEntry = {
      terminalHandle: cursor.u64("Terminal catalogue search handle"),
      generation: cursor.u32("Terminal catalogue search generation"),
      score: cursor.u32("Terminal catalogue search score"),
      primarySource: cursor.u8("Terminal catalogue search primary source"),
      matchedSources: cursor.u8("Terminal catalogue search matched sources"),
      scrollOffset: 0n,
      context: "",
    };
    if (cursor.u16("Terminal catalogue search entry reserved") !== 0)
      throw new YasProtocolError(
        "Terminal catalogue search entry reserved field is nonzero",
      );
    entry.scrollOffset = cursor.u64("Terminal catalogue search scroll offset");
    entry.context = cursor.utf8U32("Terminal catalogue search context");
    validateTerminalCatalogSearchEntry(entry);
    entries.push(entry);
  }
  const value: YasTerminalCatalogSearchResult = {
    flags,
    entries,
    extensions: decodeTerminalRequestExtensions(
      cursor,
      "SEARCH_CATALOG Result",
    ),
  };
  cursor.end("Terminal SEARCH_CATALOG Result");
  if (value.flags & ~g.YAS_TERMINAL_CATALOG_SEARCH_RESULT_FLAGS)
    throw new YasProtocolError(
      "invalid Terminal catalogue search result flags",
    );
  return value;
}

export function encodeTerminalCwdQuery(value: YasTerminalCwdQuery): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u32(0)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalCwdQuery(bytes: Uint8Array): YasTerminalCwdQuery {
  const cursor = new YasCursor(bytes);
  const value: YasTerminalCwdQuery = {
    terminalHandle: cursor.u64("Terminal handle"),
    generation: cursor.u32("Terminal generation"),
    initialReceiveCredit: 0n,
    extensions: [],
  };
  if (cursor.u32("Terminal CWD reserved") !== 0)
    throw new YasProtocolError("Terminal CWD reserved field is nonzero");
  value.initialReceiveCredit = cursor.u64("Terminal CWD receive credit");
  value.extensions = decodeTerminalRequestExtensions(cursor, "CWD");
  cursor.end("Terminal CWD");
  encodeTerminalCwdQuery(value);
  return value;
}

export function encodeTerminalJournal(value: YasTerminalJournal): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  if (value.flags & ~g.YAS_TERMINAL_JOURNAL_REQUEST_FLAGS || value.limit === 0)
    throw new YasProtocolError("invalid Terminal JOURNAL bounds");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u16(value.flags)
    .u16(value.limit)
    .u64(value.fromIndex)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalJournal(bytes: Uint8Array): YasTerminalJournal {
  const cursor = new YasCursor(bytes);
  const value: YasTerminalJournal = {
    terminalHandle: cursor.u64("Terminal handle"),
    generation: cursor.u32("Terminal generation"),
    flags: cursor.u16("Terminal JOURNAL flags"),
    limit: cursor.u16("Terminal JOURNAL limit"),
    fromIndex: cursor.u64("Terminal JOURNAL index"),
    initialReceiveCredit: cursor.u64("Terminal JOURNAL receive credit"),
    extensions: decodeTerminalRequestExtensions(cursor, "JOURNAL"),
  };
  cursor.end("Terminal JOURNAL");
  encodeTerminalJournal(value);
  return value;
}

export function encodeTerminalOutput(value: YasTerminalOutput): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  if (
    value.cursor.kind > g.YAS_TERMINAL_OUTPUT_CURSOR_PROBE ||
    value.flags !== g.YAS_TERMINAL_OUTPUT_REQUEST_FLAGS ||
    (value.cursor.kind === g.YAS_TERMINAL_OUTPUT_CURSOR_LATEST_COMMAND &&
      value.cursor.a !== 0n) ||
    value.maxBytes === 0
  )
    throw new YasProtocolError("invalid Terminal OUTPUT cursor or flags");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u8(value.cursor.kind)
    .u8(value.flags)
    .u16(0)
    .u64(value.cursor.a)
    .u32(value.cursor.b)
    .u32(value.maxBytes)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalOutput(bytes: Uint8Array): YasTerminalOutput {
  const cursor = new YasCursor(bytes);
  const terminalHandle = cursor.u64("Terminal handle");
  const generation = cursor.u32("Terminal generation");
  const kind = cursor.u8("Terminal OUTPUT cursor kind");
  const flags = cursor.u8("Terminal OUTPUT flags");
  requireTerminalZero(cursor.take(2, "Terminal OUTPUT reserved"), "OUTPUT");
  const value: YasTerminalOutput = {
    terminalHandle,
    generation,
    cursor: {
      kind,
      a: cursor.u64("Terminal OUTPUT cursor a"),
      b: cursor.u32("Terminal OUTPUT cursor b"),
    },
    flags,
    maxBytes: cursor.u32("Terminal OUTPUT byte limit"),
    initialReceiveCredit: cursor.u64("Terminal OUTPUT receive credit"),
    extensions: decodeTerminalRequestExtensions(cursor, "OUTPUT"),
  };
  cursor.end("Terminal OUTPUT");
  encodeTerminalOutput(value);
  return value;
}

export function encodeTerminalWait(value: YasTerminalWait): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  if (
    value.waitKind > g.YAS_TERMINAL_WAIT_LATEST_COMMAND ||
    value.flags !== g.YAS_TERMINAL_WAIT_FLAGS ||
    value.maxBytes === 0 ||
    value.timeoutNs === 0n ||
    value.needle.length > g.YAS_TERMINAL_MAX_INPUT_BYTES ||
    (value.waitKind === g.YAS_TERMINAL_WAIT_OUTPUT &&
      value.needle.length === 0) ||
    (value.waitKind === g.YAS_TERMINAL_WAIT_COMMAND &&
      (value.cursorB !== 0 || value.needle.length !== 0)) ||
    (value.waitKind === g.YAS_TERMINAL_WAIT_LATEST_COMMAND &&
      (value.cursorA !== 0n ||
        value.cursorB !== 0 ||
        value.needle.length !== 0))
  )
    throw new YasProtocolError("invalid Terminal WAIT bounds");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u8(value.waitKind)
    .u8(value.flags)
    .u16(0)
    .u64(value.cursorA)
    .u32(value.cursorB)
    .u32(value.maxBytes)
    .u64(value.timeoutNs)
    .bytesU32(value.needle)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalWait(bytes: Uint8Array): YasTerminalWait {
  const cursor = new YasCursor(bytes);
  const terminalHandle = cursor.u64("Terminal handle");
  const generation = cursor.u32("Terminal generation");
  const waitKind = cursor.u8("Terminal WAIT kind");
  const flags = cursor.u8("Terminal WAIT flags");
  requireTerminalZero(cursor.take(2, "Terminal WAIT reserved"), "WAIT");
  const value: YasTerminalWait = {
    terminalHandle,
    generation,
    waitKind,
    flags,
    cursorA: cursor.u64("Terminal WAIT cursor a"),
    cursorB: cursor.u32("Terminal WAIT cursor b"),
    maxBytes: cursor.u32("Terminal WAIT byte limit"),
    timeoutNs: cursor.u64("Terminal WAIT timeout"),
    needle: new Uint8Array(cursor.bytesU32("Terminal WAIT needle")),
    initialReceiveCredit: cursor.u64("Terminal WAIT receive credit"),
    extensions: decodeTerminalRequestExtensions(cursor, "WAIT"),
  };
  cursor.end("Terminal WAIT");
  encodeTerminalWait(value);
  return value;
}

export function encodeTerminalCopyRange(
  value: YasTerminalCopyRange,
): Uint8Array {
  validateQueryIdentity(value.terminalHandle, value.generation);
  validateQueryRepresentation(value.representation);
  if (
    value.maxBytes === 0 ||
    (value.startRow === value.endRow && value.startCol > value.endCol)
  )
    throw new YasProtocolError("invalid Terminal COPY_RANGE bounds");
  return new YasWriter()
    .u64(value.terminalHandle)
    .u32(value.generation)
    .u8(value.representation)
    .bytes(new Uint8Array(3))
    .i64(value.startRow)
    .u32(value.startCol)
    .i64(value.endRow)
    .u32(value.endCol)
    .u32(value.maxBytes)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalCopyRange(
  bytes: Uint8Array,
): YasTerminalCopyRange {
  const cursor = new YasCursor(bytes);
  const terminalHandle = cursor.u64("Terminal handle");
  const generation = cursor.u32("Terminal generation");
  const representation = cursor.u8("Terminal query representation");
  requireTerminalZero(
    cursor.take(3, "Terminal COPY_RANGE reserved"),
    "COPY_RANGE",
  );
  const value: YasTerminalCopyRange = {
    terminalHandle,
    generation,
    representation,
    startRow: cursor.i64("Terminal COPY_RANGE start row"),
    startCol: cursor.u32("Terminal COPY_RANGE start column"),
    endRow: cursor.i64("Terminal COPY_RANGE end row"),
    endCol: cursor.u32("Terminal COPY_RANGE end column"),
    maxBytes: cursor.u32("Terminal COPY_RANGE byte limit"),
    initialReceiveCredit: cursor.u64("Terminal COPY_RANGE receive credit"),
    extensions: decodeTerminalRequestExtensions(cursor, "COPY_RANGE"),
  };
  cursor.end("Terminal COPY_RANGE");
  encodeTerminalCopyRange(value);
  return value;
}

export function encodeTerminalLaunch(launch: YasTerminalLaunch): Uint8Array {
  const writer = new YasWriter()
    .u8(launch.command.kind)
    .u8(launch.cwd.kind)
    .u8(launch.environmentBase)
    .u8(0);
  if (launch.command.kind === YAS_TERMINAL_COMMAND_ARGV) {
    if (launch.command.argv.length === 0 || launch.command.argv.length > 0xffff)
      throw new YasProtocolError("Terminal ARGV count is invalid");
    writer.u16(launch.command.argv.length);
    for (const argument of launch.command.argv) writer.bytesU32(argument);
  } else if (launch.command.kind === YAS_TERMINAL_COMMAND_SHELL_COMMAND) {
    writer.bytesU32(new TextEncoder().encode(launch.command.command));
  }
  if (launch.cwd.kind === YAS_TERMINAL_CWD_PATH) {
    writer.bytesU32(launch.cwd.path);
  } else if (launch.cwd.kind === YAS_TERMINAL_CWD_TERMINAL) {
    if (launch.cwd.terminalHandle === 0n)
      throw new YasProtocolError("Terminal cwd source handle is zero");
    writer.u64(launch.cwd.terminalHandle);
  }
  const entries = [...(launch.environment ?? [])];
  if (entries.length > 0xffff)
    throw new YasProtocolError("Terminal environment has too many entries");
  writer.u16(entries.length);
  let previous: Uint8Array | undefined;
  for (const entry of entries) {
    if (
      entry.key.length === 0 ||
      entry.key.includes(0) ||
      entry.key.includes(61)
    )
      throw new YasProtocolError("invalid Terminal environment key");
    if (previous && compareBytes(previous, entry.key) >= 0)
      throw new YasProtocolError("Terminal environment keys are not ordered");
    previous = entry.key;
    const value = entry.value ?? new Uint8Array(0);
    if (entry.kind === YAS_TERMINAL_ENVIRONMENT_REMOVE && value.length !== 0)
      throw new YasProtocolError(
        "removed Terminal environment key has a value",
      );
    writer.bytesU16(entry.key).u8(entry.kind).bytesU32(value);
  }
  return writer.bytes(encodeExtensions(launch.extensions)).finish();
}

export function decodeTerminalRecord(body: Uint8Array): YasTerminalRecord {
  const cursor = new YasCursor(body);
  const handle = cursor.u64("terminal handle");
  const lifecycle = cursor.u8("terminal lifecycle");
  if (cursor.u8("terminal reserved") !== 0)
    throw new YasProtocolError("Terminal record reserved byte is nonzero");
  const rows = cursor.u16("terminal rows");
  const cols = cursor.u16("terminal cols");
  const generation = cursor.u32("terminal generation");
  const usedRows = cursor.u32("terminal used rows");
  const extensions = decodeExtensions(
    cursor,
    terminalStateTags,
    "Terminal state extensions",
  );
  cursor.end("Terminal state record");
  if (
    handle === 0n ||
    generation === 0 ||
    lifecycle < YAS_TERMINAL_LIFECYCLE_RUNNING ||
    lifecycle > YAS_TERMINAL_LIFECYCLE_EXITED
  )
    throw new YasProtocolError("invalid Terminal state identity");
  return applyTerminalExtensions(
    {
      handle,
      lifecycle: lifecycle as YasTerminalRecord["lifecycle"],
      rows,
      cols,
      generation,
      usedRows,
      extensions,
    },
    extensions,
  );
}

function applyTerminalExtensions(
  record: YasTerminalRecord,
  extensions: readonly YasExtension[],
): YasTerminalRecord {
  const next = { ...record, extensions };
  for (const extension of extensions) {
    const cursor = new YasCursor(extension.value);
    if (extension.tag === YAS_TERMINAL_STATE_TITLE_EXTENSION)
      next.title = cursor.utf8(cursor.remaining, "title");
    else if (extension.tag === YAS_TERMINAL_STATE_CWD_EXTENSION)
      next.cwd = new Uint8Array(cursor.take(cursor.remaining));
    else if (extension.tag === YAS_TERMINAL_STATE_COMMAND_DISPLAY_EXTENSION)
      next.commandDisplay = cursor.utf8(cursor.remaining, "command display");
    else if (extension.tag === YAS_TERMINAL_STATE_EXIT_EXTENSION)
      next.exit = decodeTerminalExitRecord(cursor.take(cursor.remaining));
    else if (extension.tag === YAS_TERMINAL_STATE_DEADLINE_SERVER_NS_EXTENSION)
      next.deadlineServerNs = cursor.u64("deadline server time");
    else if (extension.tag === YAS_TERMINAL_STATE_APP_HANDLE_EXTENSION)
      next.appHandle = cursor.u64("app handle");
    else if (extension.tag === YAS_TERMINAL_STATE_JOURNAL_CURSOR_EXTENSION)
      next.journalCursor = cursor.u64("journal cursor");
    else if (extension.tag === YAS_TERMINAL_STATE_RESOURCE_TAG_EXTENSION)
      next.resourceTag = cursor.utf8(cursor.remaining, "resource tag");
    else continue;
    cursor.end("Terminal state extension");
  }
  return next;
}

export function decodeTerminalExitRecord(
  body: Uint8Array,
): YasTerminalExitRecord {
  const cursor = new YasCursor(body);
  const kind = cursor.u8("Terminal exit kind");
  const reason = cursor.u8("Terminal exit reason");
  if (cursor.u16("Terminal exit reserved") !== 0)
    throw new YasProtocolError("Terminal exit reserved field is nonzero");
  const value = cursor.i32("Terminal exit code or signal");
  const detail = cursor.utf8U32("Terminal exit detail");
  cursor.end("Terminal exit record");
  if (kind === YAS_TERMINAL_EXIT_KIND_CODE) {
    if (reason !== YAS_TERMINAL_EXIT_REASON_UNKNOWN)
      throw new YasProtocolError("Terminal code exit has a signal reason");
    return { kind, code: value, detail };
  }
  if (kind === YAS_TERMINAL_EXIT_KIND_SIGNAL)
    if (reason <= YAS_TERMINAL_EXIT_REASON_HANGUP)
      return { kind, reason, nativeSignal: value, detail };
  if (
    kind === YAS_TERMINAL_EXIT_KIND_OTHER &&
    reason === YAS_TERMINAL_EXIT_REASON_UNKNOWN &&
    value === 0 &&
    detail.length !== 0
  )
    return { kind, detail };
  throw new YasProtocolError("invalid Terminal exit record");
}

export function decodeTerminalFrame(
  payload: Uint8Array,
): YasTerminalFrameEvent {
  const cursor = new YasCursor(payload);
  const viewId = cursor.u32("Terminal frame view ID");
  const sequence = cursor.u32("Terminal frame sequence");
  const flags = cursor.u16("Terminal frame flags");
  const explicitBase =
    flags & YAS_TERMINAL_FRAME_EXPLICIT_BASE
      ? cursor.u32("Terminal frame base")
      : undefined;
  if (viewId === 0)
    throw new YasProtocolError("Terminal frame view ID is zero");
  return {
    viewId,
    sequence,
    flags,
    explicitBase,
    gridPayload: new Uint8Array(cursor.take(cursor.remaining)),
  };
}

export function decodeTerminalFrameChunk(
  payload: Uint8Array,
): YasTerminalFrameChunkEvent {
  const cursor = new YasCursor(payload);
  const result = {
    viewId: cursor.u32("Terminal chunk view ID"),
    sequence: cursor.u32("Terminal chunk sequence"),
    chunkIndex: cursor.u16("Terminal chunk index"),
    chunkCount: cursor.u16("Terminal chunk count"),
    logicalFrameLength: cursor.u32("Terminal logical frame length"),
    chunk: new Uint8Array(cursor.take(cursor.remaining)),
  };
  if (
    result.viewId === 0 ||
    result.chunkCount === 0 ||
    result.chunkIndex >= result.chunkCount ||
    result.logicalFrameLength === 0 ||
    result.chunk.length === 0
  )
    throw new YasProtocolError("invalid Terminal frame chunk");
  return result;
}

export function encodeTerminalFeedback(
  feedback: YasTerminalViewFeedback,
): Uint8Array {
  if (feedback.viewId === 0)
    throw new YasProtocolError("Terminal feedback view ID is zero");
  return new YasWriter()
    .u32(feedback.viewId)
    .u32(feedback.presentedSequence)
    .u8(feedback.decoderQueueDepth)
    .u8(feedback.availableFrameSlots)
    .finish();
}

export class YasTerminalCatalog {
  private current = new Map<bigint, YasTerminalRecord>();
  private staging: Map<bigint, YasTerminalRecord> | null = null;
  private retention: YasStateCatalogueRetention<bigint>;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasTerminalSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private _revision = 0n;
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_TERMINAL) {
        this.cancelPendingWatch(
          new YasProtocolError("Terminal catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasTerminalSnapshot {
    return {
      revision: this._revision,
      terminals: [...this.current.values()],
    };
  }

  subscribe(listener: (snapshot: YasTerminalSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Terminal catalogue is disposed");
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      this.listeners.delete(listener);
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasTerminalSnapshot> {
    if (this.disposed) throw new Error("Terminal catalogue is disposed");
    if (this._revision !== 0n && this.subscription?.active)
      return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasTerminalSnapshot>((resolve, reject) => {
      rejectPending = (error) => {
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        reject(error);
      };
      this.pendingFirstSnapshots.add(rejectPending);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        resolve(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } finally {
      remove?.();
      this.pendingFirstSnapshots.delete(rejectPending);
    }
  }

  watch(options: YasWatchOptions = {}): Promise<void> {
    if (this.disposed)
      return Promise.reject(new Error("Terminal catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.subscription = null;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_WATCH,
      YAS_TERMINAL_UNWATCH,
      YAS_TERMINAL_STATE,
      YAS_TERMINAL_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Terminal catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    let pending!: Promise<void>;
    pending = Promise.race([watched, cancelled]).finally(() => {
      if (this.pendingWatch !== pending) return;
      this.pendingWatch = null;
      if (this.pendingWatchCancel === cancel) this.pendingWatchCancel = null;
    });
    this.pendingWatch = pending;
    this.pendingWatchCancel = cancel;
    return pending;
  }

  async unwatch(): Promise<void> {
    this.cancelPendingWatch(
      new YasProtocolError("Terminal catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Terminal catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current.clear();
    this.staging = null;
    this.stagingRetention = null;
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      // RESET announces the snapshot that follows it; it is not a moment when
      // this session has no terminals. The server sends one before every
      // republication of the catalogue (`run_terminal_watch`), so publishing
      // an empty catalogue here told the UI that every terminal had gone away
      // and then come back — which unmounted and rebuilt every terminal pane,
      // canvas and view included, on each command that changed a cwd or a
      // title. The records stay until SNAPSHOT_END swaps in the new ones,
      // exactly as they do for a delta.
      //
      // A local reset — an invalidated family, an unwatch — still empties the
      // catalogue through `clearState`, because there the terminals really are
      // no longer being tracked.
      this.discardStaging();
    } else if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.discardStaging();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
    } else if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Terminal snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
    } else if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Terminal snapshot end without begin");
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
      this._revision = batch.toRevision;
      this.emit();
    } else if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.retention.clone();
      let next: Map<bigint, YasTerminalRecord>;
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
      this._revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<bigint, YasTerminalRecord>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      if (action.kind === YAS_STATE_ADD || action.kind === YAS_STATE_REPLACE) {
        const record = detachStateRetainedValue(
          decodeTerminalRecord(action.body),
        );
        const exists = target.has(record.handle);
        if ((action.kind === YAS_STATE_ADD) === exists)
          throw new YasProtocolError(
            "Terminal ADD/REPLACE precondition failed",
          );
        if (action.kind === YAS_STATE_ADD && target.size >= this.catalogLimit())
          throw new YasProtocolError(
            "Terminal catalogue exceeds its negotiated terminal limit",
          );
        retention.upsert(record.handle, estimateStateRetainedBytes(record));
        target.set(record.handle, record);
      } else if (action.kind === YAS_STATE_PATCH) {
        const cursor = new YasCursor(action.body);
        const handle = cursor.u64("patched terminal handle");
        const extensions = decodeExtensions(
          cursor,
          terminalStateTags,
          "Terminal patch extensions",
        );
        cursor.end("Terminal PATCH");
        const previous = target.get(handle);
        if (!previous)
          throw new YasProtocolError("Terminal PATCH names an unknown handle");
        const next = detachStateRetainedValue(
          applyTerminalExtensions(
            previous,
            mergeExtensions(previous.extensions, extensions),
          ),
        );
        retention.upsert(handle, estimateStateRetainedBytes(next));
        target.set(handle, next);
      } else if (action.kind === YAS_STATE_REMOVE) {
        const cursor = new YasCursor(action.body);
        const handle = cursor.u64("removed terminal handle");
        cursor.end("Terminal REMOVE");
        if (!target.has(handle))
          throw new YasProtocolError("Terminal REMOVE names an unknown handle");
        retention.remove(handle);
        target.delete(handle);
      }
    }
  }

  private emit(): void {
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block sibling delivery or wire cleanup.
      }
    }
  }

  private validateCatalog(
    records: ReadonlyMap<bigint, YasTerminalRecord>,
  ): void {
    if (records.size > this.catalogLimit())
      throw new YasProtocolError(
        "Terminal catalogue exceeds its negotiated terminal limit",
      );
  }

  private catalogLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_VERSION,
      g.YAS_TERMINAL_LIMIT_MAX_TERMINALS_PER_SESSION,
      g.YAS_TERMINAL_MAX_TERMINALS_PER_SESSION,
    );
  }

  private resetLocal(): void {
    if (this.disposed) return;
    this.subscription = null;
    this.clearState();
  }

  private cancelPendingWatch(error: unknown): void {
    this.watchEpoch++;
    const cancel = this.pendingWatchCancel;
    this.pendingWatch = null;
    this.pendingWatchCancel = null;
    cancel?.(error);
    for (const reject of [...this.pendingFirstSnapshots]) reject(error);
    this.pendingFirstSnapshots.clear();
  }

  private clearState(): void {
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
    this.stagingRetention = null;
    this.current = new Map();
    this._revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }
}

interface ChunkAssembly {
  count: number;
  length: number;
  next: number;
  chunks: Uint8Array[];
  received: number;
}

export class YasTerminalView {
  private chunks = new Map<number, ChunkAssembly>();
  private listeners = new Set<(frame: YasTerminalFrameEvent) => void>();
  private pendingFrames: YasTerminalFrameEvent[] = [];
  private closed = false;
  private retainedChunkBytes = 0;
  private leaseReleased = false;
  private nextSequence: number;
  private highestPresented: number;

  constructor(
    private readonly client: YasTerminalClient,
    readonly result: YasTerminalOpenViewResult,
    private readonly lease: YasReceiveBudgetLease,
  ) {
    this.nextSequence = result.firstSequence;
    this.highestPresented = (result.firstSequence - 1) >>> 0;
  }

  subscribe(listener: (frame: YasTerminalFrameEvent) => void): () => void {
    this.listeners.add(listener);
    if (this.pendingFrames.length !== 0) {
      const pending = this.pendingFrames;
      this.pendingFrames = [];
      for (const frame of pending) listener(frame);
    }
    return () => this.listeners.delete(listener);
  }

  feedback(
    presentedSequence: number,
    decoderQueueDepth: number,
    availableFrameSlots: number,
  ): YasTerminalViewFeedback {
    return {
      viewId: this.result.viewId,
      presentedSequence,
      decoderQueueDepth,
      availableFrameSlots,
    };
  }

  acknowledge(feedback: YasTerminalViewFeedback): void {
    this.recordFeedback(feedback);
    this.client.connection.sendEvent(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_FRAME_ACK,
      encodeTerminalFeedback(feedback),
    );
  }

  recordFeedback(feedback: YasTerminalViewFeedback): void {
    if (feedback.viewId !== this.result.viewId)
      throw new YasProtocolError("Terminal feedback names another view");
    const advance = serialDistance(
      this.highestPresented,
      feedback.presentedSequence,
    );
    const received = serialDistance(
      this.highestPresented,
      (this.nextSequence - 1) >>> 0,
    );
    if (advance >= 0x8000_0000 || advance > received)
      throw new YasProtocolError(
        "Terminal feedback acknowledges an unseen frame",
      );
    this.highestPresented = feedback.presentedSequence;
  }

  async configure(
    configuration: YasTerminalViewConfiguration,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    const reserved = new Set<number>([
      YAS_TERMINAL_CONFIGURE_ROWS_EXTENSION,
      YAS_TERMINAL_CONFIGURE_COLS_EXTENSION,
      YAS_TERMINAL_CONFIGURE_MAX_FPS_EXTENSION,
      YAS_TERMINAL_CONFIGURE_PRESENTATION_METRICS_EXTENSION,
      YAS_TERMINAL_CONFIGURE_QUEUE_TARGET_EXTENSION,
    ]);
    if (extensions.some((extension) => reserved.has(extension.tag)))
      throw new YasProtocolError(
        "typed Terminal view configuration extension is duplicated",
      );
    const configured = [...extensions];
    const u16 = (tag: number, value: number | undefined, name: string) => {
      if (value === undefined) return;
      if (!Number.isInteger(value) || value <= 0 || value > 0xffff)
        throw new YasProtocolError(`invalid Terminal configured ${name}`);
      configured.push({ tag, value: new YasWriter().u16(value).finish() });
    };
    u16(YAS_TERMINAL_CONFIGURE_ROWS_EXTENSION, configuration.rows, "rows");
    u16(YAS_TERMINAL_CONFIGURE_COLS_EXTENSION, configuration.cols, "columns");
    u16(YAS_TERMINAL_CONFIGURE_MAX_FPS_EXTENSION, configuration.maxFps, "FPS");
    if (configuration.presentationMetrics) {
      const metrics = configuration.presentationMetrics;
      for (const value of [
        metrics.viewportWidthPx,
        metrics.viewportHeightPx,
        metrics.cellWidth16_16,
        metrics.cellHeight16_16,
        metrics.deviceScale16_16,
      ])
        if (!Number.isInteger(value) || value <= 0 || value > 0xffff_ffff)
          throw new YasProtocolError("invalid Terminal presentation metrics");
      configured.push({
        tag: YAS_TERMINAL_CONFIGURE_PRESENTATION_METRICS_EXTENSION,
        value: new YasWriter()
          .u32(metrics.viewportWidthPx)
          .u32(metrics.viewportHeightPx)
          .u32(metrics.cellWidth16_16)
          .u32(metrics.cellHeight16_16)
          .u32(metrics.deviceScale16_16)
          .finish(),
      });
    }
    if (configuration.queueTarget !== undefined) {
      if (
        !Number.isInteger(configuration.queueTarget) ||
        configuration.queueTarget <= 0 ||
        configuration.queueTarget > 0xff
      )
        throw new YasProtocolError("invalid Terminal queue target");
      configured.push({
        tag: YAS_TERMINAL_CONFIGURE_QUEUE_TARGET_EXTENSION,
        value: new YasWriter().u8(configuration.queueTarget).finish(),
      });
    }
    configured.sort((left, right) => left.tag - right.tag);
    await this.client.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_CONFIGURE_VIEW,
      new YasWriter()
        .u32(this.result.viewId)
        .bytes(encodeExtensions(configured))
        .finish(),
    );
  }

  async reset(): Promise<void> {
    await this.client.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_RESET_VIEW,
      new YasWriter().u32(this.result.viewId).finish(),
    );
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.retainedChunkBytes = 0;
    this.pendingFrames = [];
    try {
      await this.client.connection.request(
        YAS_FAMILY_TERMINAL,
        YAS_TERMINAL_CLOSE_VIEW,
        new YasWriter().u32(this.result.viewId).finish(),
      );
    } finally {
      this.closeLocal();
    }
  }

  closeLocal(): void {
    this.closed = true;
    this.chunks.clear();
    this.pendingFrames = [];
    this.retainedChunkBytes = 0;
    this.listeners.clear();
    this.client.removeView(this.result.viewId);
    if (!this.leaseReleased) {
      this.leaseReleased = true;
      this.lease.release();
    }
  }

  acceptFrame(frame: YasTerminalFrameEvent): void {
    if (this.closed) return;
    if (this.chunks.has(frame.sequence))
      throw new YasProtocolError("Terminal frame duplicates a chunk assembly");
    this.validateNextSequence(frame.sequence);
    const logicalLength =
      2 + (frame.explicitBase === undefined ? 0 : 4) + frame.gridPayload.length;
    if (logicalLength > this.result.maxEncodedFrame)
      throw new YasProtocolError(
        "Terminal frame exceeds negotiated view limit",
      );
    this.nextSequence = (this.nextSequence + 1) >>> 0;
    if (this.listeners.size === 0) {
      if (this.pendingFrames.length >= this.result.maxInflightFrames)
        throw new YasProtocolError("too many queued Terminal frames");
      this.pendingFrames.push(frame);
    } else {
      for (const listener of this.listeners) listener(frame);
    }
  }

  acceptChunk(chunk: YasTerminalFrameChunkEvent): void {
    if (this.closed) return;
    this.validateNextSequence(chunk.sequence);
    if (chunk.logicalFrameLength > this.result.maxEncodedFrame)
      throw new YasProtocolError("Terminal logical frame exceeds view limit");
    if (
      chunk.logicalFrameLength < 2 ||
      chunk.chunkCount > chunk.logicalFrameLength ||
      chunk.chunk.length > YAS_MAX_BULK_CHUNK
    )
      throw new YasProtocolError("Terminal frame chunk geometry is invalid");
    let assembly = this.chunks.get(chunk.sequence);
    if (!assembly) {
      if (chunk.chunkIndex !== 0)
        throw new YasProtocolError("Terminal frame chunk starts out of order");
      if (this.chunks.size >= this.result.maxInflightFrames)
        throw new YasProtocolError(
          "too many in-flight Terminal frame assemblies",
        );
      assembly = {
        count: chunk.chunkCount,
        length: chunk.logicalFrameLength,
        next: 0,
        chunks: [],
        received: 0,
      };
      this.chunks.set(chunk.sequence, assembly);
    }
    if (
      assembly.count !== chunk.chunkCount ||
      assembly.length !== chunk.logicalFrameLength ||
      assembly.next !== chunk.chunkIndex
    )
      throw new YasProtocolError("inconsistent Terminal frame chunks");
    assembly.next++;
    assembly.received += chunk.chunk.length;
    this.retainedChunkBytes += chunk.chunk.length;
    const chunksRemaining = assembly.count - assembly.next;
    if (
      assembly.received > assembly.length ||
      assembly.received + chunksRemaining > assembly.length ||
      BigInt(this.retainedChunkBytes) > this.lease.bytes
    )
      throw new YasProtocolError("Terminal frame chunks exceed logical length");
    assembly.chunks.push(chunk.chunk);
    if (assembly.next !== assembly.count) return;
    this.chunks.delete(chunk.sequence);
    this.retainedChunkBytes -= assembly.received;
    if (assembly.received !== assembly.length)
      throw new YasProtocolError("Terminal frame chunks have the wrong length");
    const body = concat(assembly.chunks, assembly.length);
    const cursor = new YasCursor(body);
    const flags = cursor.u16("Terminal logical frame flags");
    const explicitBase =
      flags & YAS_TERMINAL_FRAME_EXPLICIT_BASE
        ? cursor.u32("Terminal frame base")
        : undefined;
    this.acceptFrame({
      viewId: this.result.viewId,
      sequence: chunk.sequence,
      flags,
      explicitBase,
      gridPayload: new Uint8Array(cursor.take(cursor.remaining)),
    });
  }

  private validateNextSequence(sequence: number): void {
    if (sequence !== this.nextSequence)
      throw new YasProtocolError("Terminal frame sequence is not consecutive");
    const inflight = serialDistance(this.highestPresented, sequence);
    if (inflight === 0 || inflight > this.result.maxInflightFrames)
      throw new YasProtocolError(
        "Terminal sender exceeded its in-flight frame window",
      );
  }
}

export class YasTerminalClient {
  readonly catalog: YasTerminalCatalog;
  readonly transfers;
  private views = new Map<number, YasTerminalView>();
  private removeEvents: (() => void)[];
  private viewGeneration = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasTerminalCatalog(connection);
    this.transfers = transfersFor(connection);
    this.removeEvents = [
      connection.onEvent(
        YAS_FAMILY_TERMINAL,
        YAS_TERMINAL_FRAME,
        ({ payload }) => {
          const frame = decodeTerminalFrame(payload);
          this.views.get(frame.viewId)?.acceptFrame(frame);
        },
      ),
      connection.onEvent(
        YAS_FAMILY_TERMINAL,
        YAS_TERMINAL_FRAME_CHUNK,
        ({ payload }) => {
          const chunk = decodeTerminalFrameChunk(payload);
          this.views.get(chunk.viewId)?.acceptChunk(chunk);
        },
      ),
      connection.onInvalidation(({ family }) => {
        if (family !== undefined && family !== YAS_FAMILY_TERMINAL) return;
        this.viewGeneration++;
        for (const view of [...this.views.values()]) {
          this.closeViewRemote(view.result.viewId);
          view.closeLocal();
        }
      }),
    ];
  }

  list(options: YasWatchOptions = {}): Promise<YasTerminalSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  async create(request: YasTerminalCreate): Promise<YasTerminalCreateResult> {
    if (this.disposed)
      throw new YasProtocolError("Terminal client is disposed");
    requireId(request.operationId, "Terminal CREATE operation ID");
    const generation = this.viewGeneration;
    let initialLease = request.initialView
      ? this.connection.receiveBudget.reserveExact(
          BigInt(this.connection.options.receiveMaxDecoded!),
        )
      : null;
    try {
      const outcome = await this.connection.requestDecoded<
        | { result: YasTerminalCreateResult }
        | {
            result: YasTerminalDecodedCreateResult;
            viewResult?: YasTerminalOpenViewResult;
            closeRemote: boolean;
            error: unknown;
          }
      >(
        YAS_FAMILY_TERMINAL,
        YAS_TERMINAL_CREATE,
        encodeTerminalCreate(request),
        (body) => {
          const decoded = decodeCreateResult(body);
          if (!!decoded.initialViewResult !== !!request.initialView)
            throw new YasProtocolError(
              "Terminal CREATE initial-view Result does not match the Request",
            );
          if (this.disposed || generation !== this.viewGeneration)
            return {
              result: decoded,
              viewResult: decoded.initialViewResult,
              closeRemote: decoded.initialViewResult !== undefined,
              error: new YasProtocolError(
                "Terminal CREATE completed after client disposal or family invalidation",
              ),
            };
          if (!decoded.initialViewResult)
            return {
              result: {
                terminalHandle: decoded.terminalHandle,
                stateRevision: decoded.stateRevision,
                generation: decoded.generation,
                extensions: decoded.extensions,
              },
            };
          const viewResult = decoded.initialViewResult;
          const existing = this.views.get(viewResult.viewId);
          if (existing)
            throw new YasProtocolError(
              "Terminal CREATE initial-view ID was reused",
            );
          try {
            if (
              !request.initialView!.codecVersions.includes(
                viewResult.codecVersion,
              )
            )
              throw new YasProtocolError(
                "server selected an unoffered Terminal initial-view codec",
              );
            if (
              this.disposed ||
              generation !== this.viewGeneration ||
              !initialLease
            )
              throw new YasProtocolError(
                "Terminal initial view completed after client disposal or family invalidation",
              );
            const maximum =
              BigInt(
                Math.max(
                  viewResult.maxEncodedFrame,
                  viewResult.maxDecodedFrame,
                ),
              ) * BigInt(viewResult.maxInflightFrames);
            initialLease.resizeExact(maximum);
            const view = new YasTerminalView(this, viewResult, initialLease);
            this.views.set(viewResult.viewId, view);
            initialLease = null;
            return {
              result: {
                terminalHandle: decoded.terminalHandle,
                stateRevision: decoded.stateRevision,
                generation: decoded.generation,
                initialView: view,
                extensions: decoded.extensions,
              },
            };
          } catch (error) {
            return {
              result: decoded,
              viewResult,
              closeRemote: !existing,
              error,
            };
          }
        },
        true,
      );
      if ("error" in outcome) {
        if (outcome.closeRemote && outcome.viewResult)
          this.closeViewRemote(outcome.viewResult.viewId);
        this.closeTerminalRemote(outcome.result.terminalHandle);
        throw outcome.error;
      }
      if (this.disposed || generation !== this.viewGeneration) {
        if (outcome.result.initialView) {
          this.closeViewRemote(outcome.result.initialView.result.viewId);
          outcome.result.initialView.closeLocal();
        }
        this.closeTerminalRemote(outcome.result.terminalHandle);
        throw new YasProtocolError(
          "Terminal CREATE completed after client disposal or family invalidation",
        );
      }
      return outcome.result;
    } finally {
      initialLease?.release();
    }
  }

  async restart(
    terminalHandle: bigint,
    operationId: Uint8Array,
    options:
      | {
          launchMode: typeof YAS_TERMINAL_LAUNCH_REPLAY;
          cutoverMode?: number;
          extensions?: readonly YasExtension[];
        }
      | {
          launchMode: typeof YAS_TERMINAL_LAUNCH_REPLACE;
          launch: YasTerminalLaunch;
          cutoverMode?: number;
          extensions?: readonly YasExtension[];
        },
  ): Promise<{ stateRevision: bigint; generation: number }> {
    requireId(operationId, "Terminal RESTART operation ID");
    const cutover = options.cutoverMode ?? YAS_TERMINAL_CUTOVER_STOP_THEN_START;
    if (
      cutover !== YAS_TERMINAL_CUTOVER_STOP_THEN_START &&
      cutover !== YAS_TERMINAL_CUTOVER_START_THEN_SWITCH
    )
      throw new YasProtocolError("unknown Terminal restart cutover mode");
    const writer = new YasWriter()
      .u64(terminalHandle)
      .bytes(operationId)
      .u8(options.launchMode)
      .u8(cutover)
      .u16(0);
    if (options.launchMode === YAS_TERMINAL_LAUNCH_REPLACE)
      writer.bytesU32(encodeTerminalLaunch(options.launch));
    writer.bytes(encodeExtensions(options.extensions));
    return this.connection.requestDecoded(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_RESTART,
      writer.finish(),
      (body) => {
        const cursor = new YasCursor(body);
        const stateRevision = cursor.u64("Terminal restart revision");
        const generation = cursor.u32("Terminal restart generation");
        if (cursor.u32("Terminal restart reserved") !== 0)
          throw new YasProtocolError(
            "Terminal RESTART reserved field is nonzero",
          );
        cursor.end("Terminal RESTART Result");
        return { stateRevision, generation };
      },
    );
  }

  async signal(
    terminalHandle: bigint,
    operationId: Uint8Array,
    signal: number,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    requireId(operationId, "Terminal SIGNAL operation ID");
    await this.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_SIGNAL,
      new YasWriter()
        .u64(terminalHandle)
        .bytes(operationId)
        .u16(signal)
        .u16(0)
        .bytes(encodeExtensions(extensions))
        .finish(),
    );
  }

  async close(terminalHandle: bigint, operationId: Uint8Array): Promise<void> {
    requireId(operationId, "Terminal CLOSE operation ID");
    await this.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_CLOSE,
      new YasWriter().u64(terminalHandle).bytes(operationId).finish(),
    );
  }

  async setDeadline(
    terminalHandle: bigint,
    operationId: Uint8Array,
    durationNs: bigint | null,
  ): Promise<void> {
    requireId(operationId, "Terminal deadline operation ID");
    await this.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_SET_DEADLINE,
      new YasWriter()
        .u64(terminalHandle)
        .bytes(operationId)
        .u8(
          durationNs === null
            ? YAS_TERMINAL_DEADLINE_CLEAR
            : YAS_TERMINAL_DEADLINE_SET,
        )
        .bytes(new Uint8Array(7))
        .u64(durationNs ?? 0n)
        .finish(),
    );
  }

  async resize(
    terminalHandle: bigint,
    rows: number,
    cols: number,
  ): Promise<bigint> {
    return this.revisionRequest(
      YAS_TERMINAL_RESIZE,
      new YasWriter().u64(terminalHandle).u16(rows).u16(cols).finish(),
    );
  }

  async setFocus(viewId: number, focused: boolean): Promise<void> {
    await this.connection.request(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_SET_FOCUS,
      new YasWriter()
        .u32(viewId)
        .u8(focused ? 1 : 0)
        .bytes(new Uint8Array(3))
        .finish(),
    );
  }

  async scroll(
    viewId: number,
    amount: bigint,
    mode:
      typeof YAS_TERMINAL_SCROLL_ABSOLUTE | typeof YAS_TERMINAL_SCROLL_RELATIVE,
  ): Promise<bigint> {
    return this.connection.requestDecoded(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_SCROLL,
      new YasWriter()
        .u32(viewId)
        .u8(mode)
        .bytes(new Uint8Array(7))
        .i64(amount)
        .finish(),
      (body) => {
        const cursor = new YasCursor(body);
        const result = cursor.i64("Terminal applied scroll offset");
        cursor.end("Terminal SCROLL Result");
        return result;
      },
    );
  }

  async openView(request: YasTerminalOpenView): Promise<YasTerminalView> {
    if (this.disposed)
      throw new YasProtocolError("Terminal client is disposed");
    if (
      request.codecVersions.length === 0 ||
      request.codecVersions.length > 0xff
    )
      throw new YasProtocolError("Terminal view codec count is invalid");
    const generation = this.viewGeneration;
    const outcome = await this.connection.requestDecoded<
      | { view: YasTerminalView }
      | { result: YasTerminalOpenViewResult; error: unknown }
    >(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_OPEN_VIEW,
      new YasWriter()
        .u64(request.terminalHandle)
        .u16(request.rows)
        .u16(request.cols)
        .u16(request.maxFps)
        .u8(request.codecVersions.length)
        .u8(0)
        .bytes(
          request.codecVersions
            .reduce((writer, codec) => writer.u16(codec), new YasWriter())
            .finish(),
        )
        .bytes(encodeExtensions(request.extensions))
        .finish(),
      (body) => {
        const result = decodeOpenViewResult(body);
        let lease: YasReceiveBudgetLease | null = null;
        try {
          if (!request.codecVersions.includes(result.codecVersion))
            throw new YasProtocolError(
              "server selected an unoffered Terminal codec",
            );
          if (this.disposed || generation !== this.viewGeneration)
            throw new YasProtocolError(
              "Terminal OPEN_VIEW completed after client disposal or family invalidation",
            );
          const maximum =
            BigInt(Math.max(result.maxEncodedFrame, result.maxDecodedFrame)) *
            BigInt(result.maxInflightFrames);
          lease = this.connection.receiveBudget.reserve(maximum, maximum);
          if (this.views.has(result.viewId))
            throw new YasProtocolError("Terminal view ID was reused");
          const view = new YasTerminalView(this, result, lease);
          this.views.set(result.viewId, view);
          return { view };
        } catch (error) {
          lease?.release();
          return { result, error };
        }
      },
    );
    if ("error" in outcome) {
      // OPEN_VIEW has already allocated the peer resource. Any local
      // admission failure must close it, including receive-budget pressure.
      this.closeViewRemote(outcome.result.viewId);
      throw outcome.error;
    }
    if (
      this.disposed ||
      generation !== this.viewGeneration ||
      this.views.get(outcome.view.result.viewId) !== outcome.view
    ) {
      this.closeViewRemote(outcome.view.result.viewId);
      outcome.view.closeLocal();
      throw new YasProtocolError(
        "Terminal OPEN_VIEW was invalidated before completion",
      );
    }
    return outcome.view;
  }

  write(terminalHandle: bigint, data: Uint8Array): void {
    if (data.length === 0 || data.length > 16 * 1024)
      throw new YasProtocolError("Terminal WRITE data length is invalid");
    this.connection.sendEvent(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_WRITE,
      new YasWriter().u64(terminalHandle).bytes(data).finish(),
      true,
    );
  }

  input(feedback: YasTerminalViewFeedback, data: Uint8Array): void {
    if (data.length === 0 || data.length > 16 * 1024)
      throw new YasProtocolError("Terminal INPUT data length is invalid");
    this.requireView(feedback).recordFeedback(feedback);
    this.connection.sendEvent(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_INPUT,
      new YasWriter()
        .bytes(encodeTerminalFeedback(feedback))
        .bytes(data)
        .finish(),
      true,
    );
  }

  mouse(
    feedback: YasTerminalViewFeedback,
    event: {
      clientMonotonicNs: bigint;
      action: number;
      button: number;
      modifiers: number;
      column: number;
      row: number;
    },
  ): void {
    if (
      event.action < 0 ||
      event.action > YAS_TERMINAL_MOUSE_ACTION_UP ||
      event.button < 0 ||
      event.button > YAS_TERMINAL_MOUSE_BUTTON_FORWARD ||
      event.modifiers &
        ~(
          YAS_TERMINAL_MODIFIER_SHIFT |
          YAS_TERMINAL_MODIFIER_CTRL |
          YAS_TERMINAL_MODIFIER_ALT |
          YAS_TERMINAL_MODIFIER_SUPER |
          YAS_TERMINAL_MODIFIER_CAPS_LOCK |
          YAS_TERMINAL_MODIFIER_NUM_LOCK
        )
    )
      throw new YasProtocolError("invalid Terminal mouse event");
    this.requireView(feedback).recordFeedback(feedback);
    this.connection.sendEvent(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_MOUSE,
      new YasWriter()
        .bytes(encodeTerminalFeedback(feedback))
        .u64(event.clientMonotonicNs)
        .u8(event.action)
        .u8(event.button)
        .u16(event.modifiers)
        .i32(event.column)
        .i32(event.row)
        .finish(),
    );
  }

  /**
   * One wheel turn.
   *
   * The cell is sent when the server advertises WHEEL_AT, and dropped when it
   * does not: the older WHEEL carries no position, so its reports land on the
   * origin cell — which is the wrong pane in anything that splits its window.
   */
  wheel(
    feedback: YasTerminalViewFeedback,
    event: {
      clientMonotonicNs: bigint;
      source: number;
      dx: bigint;
      dy: bigint;
      column: number;
      row: number;
    },
  ): void {
    if (event.source < 0 || event.source > YAS_TERMINAL_WHEEL_SOURCE_CONTINUOUS)
      throw new YasProtocolError("invalid Terminal wheel source");
    this.requireView(feedback).recordFeedback(feedback);
    const positioned = this.connection.operationAdvertised(
      YAS_FAMILY_TERMINAL,
      YAS_CLASS_EVENT,
      YAS_TERMINAL_WHEEL_AT,
    );
    const writer = new YasWriter()
      .bytes(encodeTerminalFeedback(feedback))
      .u64(event.clientMonotonicNs)
      .u8(event.source)
      .bytes(new Uint8Array(3))
      .i64(event.dx)
      .i64(event.dy);
    if (positioned) writer.i32(event.column).i32(event.row);
    this.connection.sendEvent(
      YAS_FAMILY_TERMINAL,
      positioned ? YAS_TERMINAL_WHEEL_AT : YAS_TERMINAL_WHEEL,
      writer.finish(),
    );
  }

  read(value: YasTerminalRead | Uint8Array): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalRead(value) : value;
    return this.query(
      YAS_TERMINAL_READ,
      value instanceof Uint8Array ? value : encodeTerminalRead(value),
      decoded.initialReceiveCredit,
    );
  }
  search(
    value: YasTerminalSearch | Uint8Array,
  ): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalSearch(value) : value;
    return this.query(
      YAS_TERMINAL_SEARCH,
      value instanceof Uint8Array ? value : encodeTerminalSearch(value),
      decoded.initialReceiveCredit,
    );
  }
  searchCatalog(
    value: YasTerminalCatalogSearch | Uint8Array,
  ): Promise<YasTerminalCatalogSearchResult> {
    return this.connection.requestDecoded(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_SEARCH_CATALOG,
      value instanceof Uint8Array ? value : encodeTerminalCatalogSearch(value),
      decodeTerminalCatalogSearchResult,
    );
  }
  cwd(
    value: YasTerminalCwdQuery | Uint8Array,
  ): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalCwdQuery(value) : value;
    return this.query(
      YAS_TERMINAL_CWD,
      value instanceof Uint8Array ? value : encodeTerminalCwdQuery(value),
      decoded.initialReceiveCredit,
    );
  }
  journal(
    value: YasTerminalJournal | Uint8Array,
  ): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalJournal(value) : value;
    return this.query(
      YAS_TERMINAL_JOURNAL,
      value instanceof Uint8Array ? value : encodeTerminalJournal(value),
      decoded.initialReceiveCredit,
    );
  }
  output(
    value: YasTerminalOutput | Uint8Array,
  ): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalOutput(value) : value;
    return this.query(
      YAS_TERMINAL_OUTPUT,
      value instanceof Uint8Array ? value : encodeTerminalOutput(value),
      decoded.initialReceiveCredit,
    );
  }
  wait(value: YasTerminalWait | Uint8Array): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalWait(value) : value;
    return this.query(
      YAS_TERMINAL_WAIT,
      value instanceof Uint8Array ? value : encodeTerminalWait(value),
      decoded.initialReceiveCredit,
    );
  }
  copyRange(
    value: YasTerminalCopyRange | Uint8Array,
  ): Promise<YasTerminalQueryResult> {
    const decoded =
      value instanceof Uint8Array ? decodeTerminalCopyRange(value) : value;
    return this.query(
      YAS_TERMINAL_COPY_RANGE,
      value instanceof Uint8Array ? value : encodeTerminalCopyRange(value),
      decoded.initialReceiveCredit,
    );
  }

  removeView(viewId: number): void {
    this.views.delete(viewId);
  }

  private requireView(feedback: YasTerminalViewFeedback): YasTerminalView {
    const view = this.views.get(feedback.viewId);
    if (!view)
      throw new YasProtocolError("Terminal feedback names an unknown view");
    return view;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.viewGeneration++;
    for (const remove of this.removeEvents) remove();
    this.removeEvents = [];
    for (const view of [...this.views.values()]) {
      this.closeViewRemote(view.result.viewId);
      view.closeLocal();
    }
    this.catalog.dispose();
  }

  private closeViewRemote(viewId: number): void {
    try {
      void this.connection
        .request(
          YAS_FAMILY_TERMINAL,
          YAS_TERMINAL_CLOSE_VIEW,
          new YasWriter().u32(viewId).finish(),
        )
        .catch(() => undefined);
    } catch {
      // Family invalidation may make the cleanup request unavailable.
    }
  }

  private closeTerminalRemote(terminalHandle: bigint): void {
    try {
      void this.connection
        .request(
          YAS_FAMILY_TERMINAL,
          YAS_TERMINAL_CLOSE,
          new YasWriter()
            .u64(terminalHandle)
            .bytes(terminalOperationId())
            .finish(),
        )
        .catch(() => undefined);
    } catch {
      // Family invalidation may make the cleanup request unavailable.
    }
  }

  private async revisionRequest(
    kind: number,
    payload: Uint8Array,
  ): Promise<bigint> {
    return this.connection.requestDecoded(
      YAS_FAMILY_TERMINAL,
      kind,
      payload,
      (body) => {
        const cursor = new YasCursor(body);
        const revision = cursor.u64("Terminal state revision");
        cursor.end("Terminal revision Result");
        return revision;
      },
    );
  }

  private async query(
    kind: number,
    payload: Uint8Array,
    initialReceiveCredit: bigint,
  ): Promise<YasTerminalQueryResult> {
    const lease =
      initialReceiveCredit === 0n
        ? undefined
        : this.transfers.reserveReceiveCredit(
            initialReceiveCredit,
            initialReceiveCredit,
          );
    let consumed = false;
    try {
      return await this.connection.requestDecoded(
        YAS_FAMILY_TERMINAL,
        kind,
        payload,
        (body) => {
          const result = decodeTerminalQueryResult(body, this.transfers, lease);
          consumed = true;
          return result;
        },
      );
    } catch (error) {
      if (!consumed) lease?.release();
      throw error;
    }
  }
}

export function encodeTerminalCreate(request: YasTerminalCreate): Uint8Array {
  requireId(request.operationId, "Terminal CREATE operation ID");
  const extensions = [...(request.extensions ?? [])];
  if (
    extensions.some(
      (extension) =>
        extension.tag === g.YAS_TERMINAL_CREATE_INITIAL_VIEW_EXTENSION,
    )
  )
    throw new YasProtocolError(
      "typed Terminal CREATE initial view is duplicated by a raw extension",
    );
  if (request.initialView)
    extensions.push({
      tag: g.YAS_TERMINAL_CREATE_INITIAL_VIEW_EXTENSION,
      value: encodeTerminalInitialViewRequest(request.initialView),
    });
  extensions.sort((left, right) => left.tag - right.tag);
  return new YasWriter()
    .u16(request.rows)
    .u16(request.cols)
    .u32(0)
    .bytes(request.operationId)
    .bytesU32(encodeTerminalLaunch(request.launch))
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function encodeTerminalInitialViewRequest(
  value: YasTerminalInitialViewRequest,
): Uint8Array {
  if (
    value.rows === 0 ||
    value.cols === 0 ||
    value.maxFps === 0 ||
    value.codecVersions.length === 0 ||
    value.codecVersions.length > 0xff
  )
    throw new YasProtocolError("invalid Terminal initial-view parameters");
  let previous = 0;
  const codecs = new YasWriter();
  for (const codec of value.codecVersions) {
    if (codec === 0 || codec <= previous)
      throw new YasProtocolError(
        "Terminal initial-view codecs are not strictly ordered",
      );
    previous = codec;
    codecs.u16(codec);
  }
  return new YasWriter()
    .u16(value.rows)
    .u16(value.cols)
    .u16(value.maxFps)
    .u8(value.codecVersions.length)
    .u8(0)
    .bytes(codecs.finish())
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeTerminalInitialViewRequest(
  bytes: Uint8Array,
): YasTerminalInitialViewRequest {
  const cursor = new YasCursor(bytes);
  const value: YasTerminalInitialViewRequest = {
    rows: cursor.u16("Terminal initial-view rows"),
    cols: cursor.u16("Terminal initial-view columns"),
    maxFps: cursor.u16("Terminal initial-view maximum FPS"),
    codecVersions: [],
  };
  const count = cursor.u8("Terminal initial-view codec count");
  if (cursor.u8("Terminal initial-view reserved") !== 0)
    throw new YasProtocolError(
      "Terminal initial-view reserved byte is nonzero",
    );
  const codecVersions: number[] = [];
  for (let index = 0; index < count; index++)
    codecVersions.push(cursor.u16("Terminal initial-view codec"));
  value.codecVersions = codecVersions;
  value.extensions = decodeExtensions(
    cursor,
    undefined,
    "Terminal initial-view extensions",
  );
  cursor.end("Terminal initial-view request");
  encodeTerminalInitialViewRequest(value);
  return value;
}

export function decodeTerminalQueryResult(
  body: Uint8Array,
  transfers?: YasTransferManager,
  lease?: YasReceiveBudgetLease,
): YasTerminalQueryResult {
  const decoded = decodeTerminalQueryBody(body);
  let delivery: Promise<Uint8Array>;
  if (decoded.delivery.kind === "inline") {
    lease?.release();
    const bytes = new Uint8Array(decoded.delivery.bytes);
    delivery = Promise.resolve(bytes);
  } else {
    if (!transfers || !lease)
      throw new YasProtocolError(
        "Terminal query Transfer requires its proposed receive-credit lease",
      );
    const descriptor = decoded.delivery.descriptor;
    const transfer = transfers.acceptServerDescriptor(descriptor, lease);
    delivery = transfer.collect().then((bytes) => {
      validateTerminalQueryContent(decoded.contentKind, bytes);
      return new Uint8Array(bytes);
    });
  }
  return {
    representation: decoded.representation,
    contentKind: decoded.contentKind,
    encoding: decoded.encoding,
    flags: decoded.flags,
    extensions: decoded.extensions,
    nextCursor: decoded.nextCursor,
    totalLines: decoded.totalLines,
    satisfyingStateRevision: decoded.satisfyingStateRevision,
    bytes: async () => new Uint8Array(await delivery),
    content: async () =>
      decodeTerminalQueryContent(decoded.contentKind, await delivery),
  };
}

export function encodeTerminalQueryBody(
  value: YasTerminalQueryBody,
): Uint8Array {
  validateTerminalQueryBody(value);
  const extensions = [...value.extensions];
  if (
    extensions.some(
      (extension) =>
        queryTags.has(extension.tag) || extension.required === true,
    )
  )
    throw new YasProtocolError(
      "Terminal query has a duplicate or unknown required extension",
    );
  extensions.push({
    tag:
      value.delivery.kind === "inline"
        ? YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES
        : YAS_TERMINAL_QUERY_EXTENSION_TRANSFER,
    required: true,
    value:
      value.delivery.kind === "inline"
        ? value.delivery.bytes
        : encodeTransferDescriptor(value.delivery.descriptor),
  });
  if (value.nextCursor)
    extensions.push({
      tag: YAS_TERMINAL_QUERY_EXTENSION_NEXT_CURSOR,
      value: encodeQueryNextCursor(value.contentKind, value.nextCursor),
    });
  if (value.totalLines !== undefined)
    extensions.push({
      tag: YAS_TERMINAL_QUERY_EXTENSION_TOTAL_LINES,
      value: new YasWriter().u64(value.totalLines).finish(),
    });
  if (value.satisfyingStateRevision !== undefined)
    extensions.push({
      tag: YAS_TERMINAL_QUERY_EXTENSION_SATISFYING_STATE_REVISION,
      value: new YasWriter().u64(value.satisfyingStateRevision).finish(),
    });
  extensions.sort((left, right) => left.tag - right.tag);
  return new YasWriter()
    .u8(value.representation)
    .u8(value.contentKind)
    .u8(value.encoding)
    .u8(0)
    .u16(value.flags)
    .u16(0)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeTerminalQueryBody(
  body: Uint8Array,
): YasTerminalQueryBody {
  const cursor = new YasCursor(body);
  const representation = cursor.u8("Terminal query representation");
  const contentKind = cursor.u8("Terminal query content kind");
  const encoding = cursor.u8("Terminal query encoding");
  if (cursor.u8("Terminal query reserved byte") !== 0)
    throw new YasProtocolError("Terminal query reserved byte is nonzero");
  const flags = cursor.u16("Terminal query flags");
  if (cursor.u16("Terminal query reserved field") !== 0)
    throw new YasProtocolError("Terminal query reserved field is nonzero");
  const extensions = decodeExtensions(
    cursor,
    queryTags,
    "Terminal query extensions",
  );
  cursor.end("Terminal query Result");
  const wire = [...extensions];
  const expectedTag =
    representation === YAS_TERMINAL_QUERY_INLINE
      ? YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES
      : representation === YAS_TERMINAL_QUERY_TRANSFER
        ? YAS_TERMINAL_QUERY_EXTENSION_TRANSFER
        : -1;
  if (expectedTag < 0)
    throw new YasProtocolError("unknown Terminal query representation");
  const deliveryExtension = takeTerminalExtension(wire, expectedTag);
  if (!deliveryExtension || deliveryExtension.required !== true)
    throw new YasProtocolError("missing required Terminal query delivery");
  const otherTag =
    expectedTag === YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES
      ? YAS_TERMINAL_QUERY_EXTENSION_TRANSFER
      : YAS_TERMINAL_QUERY_EXTENSION_INLINE_BYTES;
  if (wire.some((extension) => extension.tag === otherTag))
    throw new YasProtocolError("multiple Terminal query deliveries");
  let delivery: YasTerminalQueryDelivery;
  if (representation === YAS_TERMINAL_QUERY_INLINE) {
    delivery = {
      kind: "inline",
      bytes: new Uint8Array(deliveryExtension.value),
    };
  } else {
    const descriptorCursor = new YasCursor(deliveryExtension.value);
    delivery = {
      kind: "transfer",
      descriptor: decodeTransferDescriptor(descriptorCursor),
    };
    descriptorCursor.end("Terminal query Transfer descriptor");
  }
  const next = takeTerminalExtension(
    wire,
    YAS_TERMINAL_QUERY_EXTENSION_NEXT_CURSOR,
  );
  const total = takeTerminalExtension(
    wire,
    YAS_TERMINAL_QUERY_EXTENSION_TOTAL_LINES,
  );
  const satisfying = takeTerminalExtension(
    wire,
    YAS_TERMINAL_QUERY_EXTENSION_SATISFYING_STATE_REVISION,
  );
  const value: YasTerminalQueryBody = {
    representation,
    contentKind,
    encoding,
    flags,
    delivery,
    nextCursor: next
      ? decodeQueryNextCursor(contentKind, next.value)
      : undefined,
    totalLines: total
      ? decodeU64Extension(total.value, "Terminal query total lines")
      : undefined,
    satisfyingStateRevision: satisfying
      ? decodeU64Extension(
          satisfying.value,
          "Terminal query satisfying revision",
        )
      : undefined,
    extensions: wire,
  };
  validateTerminalQueryBody(value);
  return value;
}

export function encodeTerminalSearchResults(
  values: readonly YasTerminalSearchMatch[],
): Uint8Array {
  if (values.length > g.YAS_TERMINAL_MAX_QUERY_RECORDS)
    throw new YasProtocolError("Terminal search result count is too large");
  const writer = new YasWriter().u32(values.length);
  for (const value of values) {
    if (
      terminalPositionAfter(
        value.startRow,
        value.startCol,
        value.endRow,
        value.endCol,
      )
    )
      throw new YasProtocolError("invalid Terminal search range");
    writer
      .u64(value.startRow)
      .u32(value.startCol)
      .u64(value.endRow)
      .u32(value.endCol)
      .utf8U32(value.preview);
  }
  return writer.finish();
}

export function decodeTerminalSearchResults(
  bytes: Uint8Array,
): readonly YasTerminalSearchMatch[] {
  const cursor = new YasCursor(bytes);
  const count = cursor.u32("Terminal search result count");
  if (
    count > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
    count > Math.floor(cursor.remaining / 28)
  )
    throw new YasProtocolError("invalid Terminal search result count");
  const values: YasTerminalSearchMatch[] = [];
  for (let index = 0; index < count; index++) {
    const value = {
      startRow: cursor.u64("Terminal search start row"),
      startCol: cursor.u32("Terminal search start column"),
      endRow: cursor.u64("Terminal search end row"),
      endCol: cursor.u32("Terminal search end column"),
      preview: cursor.utf8U32("Terminal search preview"),
    };
    if (
      terminalPositionAfter(
        value.startRow,
        value.startCol,
        value.endRow,
        value.endCol,
      )
    )
      throw new YasProtocolError("invalid Terminal search range");
    values.push(value);
  }
  cursor.end("Terminal search results");
  return values;
}

const terminalJournalFlags =
  g.YAS_TERMINAL_JOURNAL_RUNNING |
  g.YAS_TERMINAL_JOURNAL_HAS_EXIT |
  g.YAS_TERMINAL_JOURNAL_NO_COMMAND |
  g.YAS_TERMINAL_JOURNAL_INCOMPLETE |
  g.YAS_TERMINAL_JOURNAL_EVICTED |
  g.YAS_TERMINAL_JOURNAL_PTY_EXITED;

export function encodeTerminalJournalResult(
  value: YasTerminalJournalResult,
): Uint8Array {
  if (
    value.oldestIndex > value.nextIndex ||
    value.records.length > g.YAS_TERMINAL_MAX_QUERY_RECORDS
  )
    throw new YasProtocolError("invalid Terminal journal bounds");
  const writer = new YasWriter()
    .u64(value.oldestIndex)
    .u64(value.nextIndex)
    .u32(value.records.length);
  let previous: bigint | undefined;
  for (const record of value.records) {
    validateTerminalJournalRecord(record);
    if (
      (previous !== undefined && previous >= record.index) ||
      record.index < value.oldestIndex ||
      record.index >= value.nextIndex
    )
      throw new YasProtocolError("invalid Terminal journal record order");
    previous = record.index;
    writer
      .u64(record.index)
      .u32(record.generation)
      .u16(record.flags)
      .u16(0)
      .i32(record.exitCode)
      .u64(record.startSequence)
      .u64(record.endSequence)
      .u64(record.startedUnixMs)
      .u64(record.endedUnixMs)
      .utf8U32(record.command);
  }
  return writer.finish();
}

export function decodeTerminalJournalResult(
  bytes: Uint8Array,
): YasTerminalJournalResult {
  const cursor = new YasCursor(bytes);
  const oldestIndex = cursor.u64("Terminal oldest journal index");
  const nextIndex = cursor.u64("Terminal next journal index");
  const count = cursor.u32("Terminal journal record count");
  if (
    count > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
    count > Math.floor(cursor.remaining / 56)
  )
    throw new YasProtocolError("invalid Terminal journal record count");
  const records: YasTerminalJournalRecord[] = [];
  for (let index = 0; index < count; index++) {
    const recordIndex = cursor.u64("Terminal journal index");
    const generation = cursor.u32("Terminal journal generation");
    const flags = cursor.u16("Terminal journal flags");
    if (cursor.u16("Terminal journal reserved") !== 0)
      throw new YasProtocolError("Terminal journal reserved field is nonzero");
    records.push({
      index: recordIndex,
      generation,
      flags,
      exitCode: cursor.i32("Terminal journal exit code"),
      startSequence: cursor.u64("Terminal journal start sequence"),
      endSequence: cursor.u64("Terminal journal end sequence"),
      startedUnixMs: cursor.u64("Terminal journal start time"),
      endedUnixMs: cursor.u64("Terminal journal end time"),
      command: cursor.utf8U32("Terminal journal command"),
    });
  }
  cursor.end("Terminal journal Result");
  const value = { oldestIndex, nextIndex, records };
  encodeTerminalJournalResult(value);
  return value;
}

const terminalOutputFlags =
  g.YAS_TERMINAL_OUTPUT_TRUNCATED |
  g.YAS_TERMINAL_OUTPUT_EVICTED |
  g.YAS_TERMINAL_OUTPUT_ALT_SCREEN |
  g.YAS_TERMINAL_OUTPUT_MATCHED;

export function encodeTerminalOutputResult(
  value: YasTerminalOutputResult,
): Uint8Array {
  if (
    value.generation === 0 ||
    value.flags & ~terminalOutputFlags ||
    terminalPositionAfter(
      value.startSequence,
      value.startCol,
      value.nextSequence,
      value.nextCol,
    )
  )
    throw new YasProtocolError("invalid Terminal output Result");
  return new YasWriter()
    .u32(value.generation)
    .u16(value.flags)
    .u16(0)
    .u64(value.startSequence)
    .u32(value.startCol)
    .u64(value.nextSequence)
    .u32(value.nextCol)
    .bytesU32(value.text)
    .finish();
}

export function decodeTerminalOutputResult(
  bytes: Uint8Array,
): YasTerminalOutputResult {
  const cursor = new YasCursor(bytes);
  const generation = cursor.u32("Terminal output generation");
  const flags = cursor.u16("Terminal output flags");
  if (cursor.u16("Terminal output reserved") !== 0)
    throw new YasProtocolError("Terminal output reserved field is nonzero");
  const value = {
    generation,
    flags,
    startSequence: cursor.u64("Terminal output start sequence"),
    startCol: cursor.u32("Terminal output start column"),
    nextSequence: cursor.u64("Terminal output next sequence"),
    nextCol: cursor.u32("Terminal output next column"),
    text: new Uint8Array(cursor.bytesU32("Terminal output text")),
  };
  cursor.end("Terminal output Result");
  encodeTerminalOutputResult(value);
  return value;
}

export function encodeTerminalStyledLines(
  lines: readonly YasTerminalStyledLine[],
): Uint8Array {
  if (lines.length > g.YAS_TERMINAL_MAX_QUERY_RECORDS)
    throw new YasProtocolError("Terminal styled line count is too large");
  const writer = new YasWriter().u32(lines.length);
  let previousRow: bigint | undefined;
  for (const line of lines) {
    if (previousRow !== undefined && previousRow >= line.row)
      throw new YasProtocolError("Terminal styled lines are not ordered");
    previousRow = line.row;
    validateTerminalStyledLine(line);
    writer.i64(line.row).u32(line.startCol).u32(line.cells.length);
    for (const cell of line.cells) writer.bytes(cell);
    writer.u32(line.overflow.length);
    for (const overflow of line.overflow)
      writer.u32(overflow.cellOffset).utf8U32(overflow.text);
    writer.u32(line.hyperlinks.length);
    for (const hyperlink of line.hyperlinks)
      writer
        .u32(hyperlink.startCol)
        .u32(hyperlink.cellCount)
        .utf8U16(hyperlink.uri);
  }
  return writer.finish();
}

export function decodeTerminalStyledLines(
  bytes: Uint8Array,
): readonly YasTerminalStyledLine[] {
  const cursor = new YasCursor(bytes);
  const count = cursor.u32("Terminal styled line count");
  if (
    count > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
    count > Math.floor(cursor.remaining / 24)
  )
    throw new YasProtocolError("invalid Terminal styled line count");
  const lines: YasTerminalStyledLine[] = [];
  for (let index = 0; index < count; index++) {
    const row = cursor.i64("Terminal styled row");
    const startCol = cursor.u32("Terminal styled start column");
    const cellCount = cursor.u32("Terminal styled cell count");
    const cellBytes = cursor.take(
      cellCount * g.YAS_TERMINAL_CELL_BYTES,
      "Terminal styled cells",
    );
    const cells: Uint8Array[] = [];
    for (
      let offset = 0;
      offset < cellBytes.length;
      offset += g.YAS_TERMINAL_CELL_BYTES
    )
      cells.push(
        new Uint8Array(
          cellBytes.subarray(offset, offset + g.YAS_TERMINAL_CELL_BYTES),
        ),
      );
    const overflowCount = cursor.u32("Terminal styled overflow count");
    if (
      overflowCount > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
      overflowCount > Math.floor(cursor.remaining / 8)
    )
      throw new YasProtocolError("invalid Terminal styled overflow count");
    const overflow: YasTerminalStyledOverflow[] = [];
    for (let item = 0; item < overflowCount; item++)
      overflow.push({
        cellOffset: cursor.u32("Terminal styled overflow offset"),
        text: cursor.utf8U32("Terminal styled overflow text"),
      });
    const hyperlinkCount = cursor.u32("Terminal styled hyperlink count");
    if (
      hyperlinkCount > g.YAS_TERMINAL_MAX_QUERY_RECORDS ||
      hyperlinkCount > Math.floor(cursor.remaining / 10)
    )
      throw new YasProtocolError("invalid Terminal styled hyperlink count");
    const hyperlinks: YasTerminalStyledHyperlink[] = [];
    for (let item = 0; item < hyperlinkCount; item++)
      hyperlinks.push({
        startCol: cursor.u32("Terminal styled hyperlink start"),
        cellCount: cursor.u32("Terminal styled hyperlink cells"),
        uri: cursor.utf8U16("Terminal styled hyperlink URI"),
      });
    const line = { row, startCol, cells, overflow, hyperlinks };
    validateTerminalStyledLine(line);
    lines.push(line);
  }
  cursor.end("Terminal styled lines");
  encodeTerminalStyledLines(lines);
  return lines;
}

export function encodeTerminalTextAndStyled(
  value: YasTerminalTextAndStyled,
): Uint8Array {
  return new YasWriter()
    .utf8U32(value.plain)
    .bytesU32(encodeTerminalStyledLines(value.styled))
    .finish();
}

export function decodeTerminalTextAndStyled(
  bytes: Uint8Array,
): YasTerminalTextAndStyled {
  const cursor = new YasCursor(bytes);
  const value = {
    plain: cursor.utf8U32("Terminal plain text"),
    styled: decodeTerminalStyledLines(cursor.bytesU32("Terminal styled lines")),
  };
  cursor.end("Terminal text-and-styled Result");
  return value;
}

export function decodeTerminalQueryContent(
  contentKind: number,
  bytes: Uint8Array,
): YasTerminalQueryContent {
  if (contentKind === g.YAS_TERMINAL_CONTENT_TEXT)
    return { kind: "text", value: terminalUtf8(bytes, "Terminal query text") };
  if (contentKind === g.YAS_TERMINAL_CONTENT_PATH)
    return { kind: "path", value: new Uint8Array(bytes) };
  if (contentKind === g.YAS_TERMINAL_CONTENT_STYLED_LINES)
    return { kind: "styled-lines", value: decodeTerminalStyledLines(bytes) };
  if (contentKind === g.YAS_TERMINAL_CONTENT_SEARCH_RESULTS)
    return {
      kind: "search-results",
      value: decodeTerminalSearchResults(bytes),
    };
  if (contentKind === g.YAS_TERMINAL_CONTENT_JOURNAL)
    return { kind: "journal", value: decodeTerminalJournalResult(bytes) };
  if (contentKind === g.YAS_TERMINAL_CONTENT_OUTPUT)
    return { kind: "output", value: decodeTerminalOutputResult(bytes) };
  if (contentKind === g.YAS_TERMINAL_CONTENT_TEXT_AND_STYLED)
    return {
      kind: "text-and-styled",
      value: decodeTerminalTextAndStyled(bytes),
    };
  throw new YasProtocolError("unknown Terminal query content kind");
}

function validateTerminalQueryBody(value: YasTerminalQueryBody): void {
  const expectedEncoding =
    value.contentKind === g.YAS_TERMINAL_CONTENT_TEXT
      ? g.YAS_TERMINAL_QUERY_ENCODING_UTF8
      : value.contentKind === g.YAS_TERMINAL_CONTENT_PATH
        ? g.YAS_TERMINAL_QUERY_ENCODING_BYTES
        : value.contentKind >= g.YAS_TERMINAL_CONTENT_STYLED_LINES &&
            value.contentKind <= g.YAS_TERMINAL_CONTENT_TEXT_AND_STYLED
          ? g.YAS_TERMINAL_QUERY_ENCODING_TERMINAL_RECORDS
          : -1;
  if (
    expectedEncoding < 0 ||
    value.encoding !== expectedEncoding ||
    value.flags & ~g.YAS_TERMINAL_QUERY_TRUNCATED
  )
    throw new YasProtocolError("invalid Terminal query encoding or flags");
  const expectedRepresentation =
    value.delivery.kind === "inline"
      ? YAS_TERMINAL_QUERY_INLINE
      : YAS_TERMINAL_QUERY_TRANSFER;
  if (value.representation !== expectedRepresentation)
    throw new YasProtocolError(
      "Terminal query delivery does not match representation",
    );
  if (value.nextCursor)
    encodeQueryNextCursor(value.contentKind, value.nextCursor);
  if (
    value.contentKind === g.YAS_TERMINAL_CONTENT_PATH &&
    (value.nextCursor !== undefined || value.totalLines !== undefined)
  )
    throw new YasProtocolError("Terminal PATH query has line metadata");
  if (value.satisfyingStateRevision === 0n)
    throw new YasProtocolError("Terminal query satisfying revision is zero");
  if (
    value.extensions.some(
      (extension) =>
        queryTags.has(extension.tag) || extension.required === true,
    )
  )
    throw new YasProtocolError("invalid Terminal query extension");
  if (value.delivery.kind === "inline") {
    if (value.delivery.bytes.length > g.YAS_TERMINAL_MAX_INLINE_QUERY_BYTES)
      throw new YasProtocolError("inline Terminal query exceeds its limit");
    validateTerminalQueryContent(value.contentKind, value.delivery.bytes);
  } else validateTerminalQueryTransfer(value.delivery.descriptor);
}

function validateTerminalQueryContent(
  contentKind: number,
  bytes: Uint8Array,
): void {
  decodeTerminalQueryContent(contentKind, bytes);
}

function validateTerminalQueryTransfer(
  descriptor: YasTransferDescriptor,
): void {
  // Re-encode to run the common descriptor invariants as well.
  encodeTransferDescriptor(descriptor);
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.contentFamily !== YAS_FAMILY_TERMINAL ||
    descriptor.contentKind !== g.YAS_TERMINAL_QUERY_CONTENT_KIND ||
    descriptor.contentVersion !== YAS_TERMINAL_VERSION ||
    descriptor.sensitiveContent !== true
  )
    throw new YasProtocolError(
      "Terminal query returned the wrong Transfer type",
    );
}

function encodeQueryNextCursor(
  contentKind: number,
  value: YasTerminalQueryNextCursor,
): Uint8Array {
  if (
    contentKind === g.YAS_TERMINAL_CONTENT_TEXT ||
    contentKind === g.YAS_TERMINAL_CONTENT_STYLED_LINES ||
    contentKind === g.YAS_TERMINAL_CONTENT_TEXT_AND_STYLED
  ) {
    if (
      value.kind !== "read" ||
      value.cursor.kind > g.YAS_TERMINAL_READ_CURSOR_TAIL
    )
      throw new YasProtocolError("invalid Terminal READ next cursor");
    return encodeTerminalQueryCursor(value.cursor);
  }
  if (contentKind === g.YAS_TERMINAL_CONTENT_SEARCH_RESULTS) {
    if (
      value.kind !== "search" ||
      value.cursor.kind !== g.YAS_TERMINAL_SEARCH_CURSOR_POSITION
    )
      throw new YasProtocolError("invalid Terminal SEARCH next cursor");
    return encodeTerminalQueryCursor(value.cursor);
  }
  if (contentKind === g.YAS_TERMINAL_CONTENT_JOURNAL) {
    if (value.kind !== "journal")
      throw new YasProtocolError("invalid Terminal JOURNAL next cursor");
    return new YasWriter().u64(value.index).finish();
  }
  if (contentKind === g.YAS_TERMINAL_CONTENT_OUTPUT) {
    if (
      value.kind !== "output" ||
      value.cursor.kind !== g.YAS_TERMINAL_OUTPUT_CURSOR_SEQUENCE
    )
      throw new YasProtocolError("invalid Terminal OUTPUT next cursor");
    return encodeTerminalQueryCursor(value.cursor);
  }
  throw new YasProtocolError("Terminal content kind has no next cursor");
}

function decodeQueryNextCursor(
  contentKind: number,
  bytes: Uint8Array,
): YasTerminalQueryNextCursor {
  if (
    contentKind === g.YAS_TERMINAL_CONTENT_TEXT ||
    contentKind === g.YAS_TERMINAL_CONTENT_STYLED_LINES ||
    contentKind === g.YAS_TERMINAL_CONTENT_TEXT_AND_STYLED
  ) {
    const cursor = decodeTerminalQueryCursor(bytes);
    if (cursor.kind > g.YAS_TERMINAL_READ_CURSOR_TAIL)
      throw new YasProtocolError("invalid Terminal READ next cursor");
    return { kind: "read", cursor };
  }
  if (contentKind === g.YAS_TERMINAL_CONTENT_SEARCH_RESULTS) {
    const cursor = decodeTerminalQueryCursor(bytes);
    if (cursor.kind !== g.YAS_TERMINAL_SEARCH_CURSOR_POSITION)
      throw new YasProtocolError("invalid Terminal SEARCH next cursor");
    return { kind: "search", cursor };
  }
  if (contentKind === g.YAS_TERMINAL_CONTENT_JOURNAL)
    return {
      kind: "journal",
      index: decodeU64Extension(bytes, "Terminal JOURNAL next cursor"),
    };
  if (contentKind === g.YAS_TERMINAL_CONTENT_OUTPUT) {
    const cursor = decodeTerminalQueryCursor(bytes);
    if (cursor.kind !== g.YAS_TERMINAL_OUTPUT_CURSOR_SEQUENCE)
      throw new YasProtocolError("invalid Terminal OUTPUT next cursor");
    return { kind: "output", cursor };
  }
  throw new YasProtocolError("Terminal content kind has no next cursor");
}

function validateTerminalStyledLine(line: YasTerminalStyledLine): void {
  const lineEnd = line.startCol + line.cells.length;
  if (lineEnd > 0xffff_ffff)
    throw new YasProtocolError("Terminal styled line overflows columns");
  for (const cell of line.cells)
    if (cell.length !== g.YAS_TERMINAL_CELL_BYTES)
      throw new YasProtocolError("Terminal styled cell has the wrong length");
  let previousOffset = -1;
  for (const overflow of line.overflow) {
    if (
      overflow.cellOffset >= line.cells.length ||
      overflow.cellOffset <= previousOffset
    )
      throw new YasProtocolError("invalid Terminal styled overflow order");
    previousOffset = overflow.cellOffset;
  }
  let hyperlinkEnd = line.startCol;
  for (const hyperlink of line.hyperlinks) {
    const end = hyperlink.startCol + hyperlink.cellCount;
    if (
      hyperlink.cellCount === 0 ||
      hyperlink.startCol < hyperlinkEnd ||
      end > lineEnd ||
      new TextEncoder().encode(hyperlink.uri).length >
        g.YAS_TERMINAL_MAX_HYPERLINK_URI_BYTES
    )
      throw new YasProtocolError("invalid Terminal styled hyperlink");
    hyperlinkEnd = end;
  }
}

function validateTerminalJournalRecord(value: YasTerminalJournalRecord): void {
  if (
    value.generation === 0 ||
    value.flags & ~terminalJournalFlags ||
    value.startSequence > value.endSequence ||
    (value.endedUnixMs !== 0n && value.startedUnixMs > value.endedUnixMs) ||
    Boolean(value.flags & g.YAS_TERMINAL_JOURNAL_NO_COMMAND) !==
      (value.command.length === 0)
  )
    throw new YasProtocolError("invalid Terminal journal record");
}

function terminalPositionAfter(
  leftRow: bigint,
  leftCol: number,
  rightRow: bigint,
  rightCol: number,
): boolean {
  return leftRow > rightRow || (leftRow === rightRow && leftCol > rightCol);
}

function takeTerminalExtension(
  extensions: YasExtension[],
  tag: number,
): YasExtension | undefined {
  const index = extensions.findIndex((extension) => extension.tag === tag);
  return index < 0 ? undefined : extensions.splice(index, 1)[0];
}

function terminalUtf8(bytes: Uint8Array, name: string): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw new YasProtocolError(`invalid ${name}`);
  }
}

function decodeQueryCursorFrom(cursor: YasCursor): YasTerminalQueryCursor {
  return {
    kind: cursor.u8("Terminal query cursor kind"),
    a: cursor.u64("Terminal query cursor a"),
    b: cursor.u32("Terminal query cursor b"),
  };
}

function validateQueryIdentity(
  terminalHandle: bigint,
  generation: number,
): void {
  if (terminalHandle === 0n || generation === 0)
    throw new YasProtocolError("invalid Terminal query identity");
}

function validateTerminalCatalogSearchEntry(
  value: YasTerminalCatalogSearchEntry,
): void {
  validateQueryIdentity(value.terminalHandle, value.generation);
  const primaryBit = 1 << value.primarySource;
  if (
    !Number.isInteger(value.score) ||
    value.score < 0 ||
    value.score > 0xffff_ffff ||
    value.primarySource < g.YAS_TERMINAL_CATALOG_SEARCH_SOURCE_TITLE ||
    value.primarySource > g.YAS_TERMINAL_CATALOG_SEARCH_SOURCE_SCROLLBACK ||
    value.matchedSources === 0 ||
    value.matchedSources & ~g.YAS_TERMINAL_CATALOG_SEARCH_MATCH_MASK ||
    (value.matchedSources & primaryBit) === 0 ||
    new TextEncoder().encode(value.context).length >
      g.YAS_TERMINAL_MAX_CATALOG_SEARCH_CONTEXT_BYTES
  )
    throw new YasProtocolError("invalid Terminal catalogue search entry");
}

function validateQueryRepresentation(representation: number): void {
  if (
    !Number.isInteger(representation) ||
    representation < g.YAS_TERMINAL_QUERY_REPRESENTATION_PLAIN ||
    representation > g.YAS_TERMINAL_QUERY_REPRESENTATION_BOTH
  )
    throw new YasProtocolError("invalid Terminal query representation");
}

function decodeTerminalRequestExtensions(
  cursor: YasCursor,
  operation: string,
): YasExtension[] {
  return decodeExtensions(
    cursor,
    new Set(),
    `Terminal ${operation} extensions`,
  );
}

function requireTerminalZero(bytes: Uint8Array, operation: string): void {
  if (bytes.some((byte) => byte !== 0))
    throw new YasProtocolError(
      `Terminal ${operation} reserved bytes are nonzero`,
    );
}

function decodeU64Extension(value: Uint8Array, name: string): bigint {
  const cursor = new YasCursor(value);
  const result = cursor.u64(name);
  cursor.end(name);
  return result;
}

function decodeCreateResult(body: Uint8Array): YasTerminalDecodedCreateResult {
  const cursor = new YasCursor(body);
  const terminalHandle = cursor.u64("created terminal handle");
  const stateRevision = cursor.u64("created terminal revision");
  const generation = cursor.u32("created terminal generation");
  if (cursor.u32("Terminal CREATE reserved") !== 0)
    throw new YasProtocolError("Terminal CREATE Result reserved is nonzero");
  const extensions = [
    ...decodeExtensions(
      cursor,
      new Set([g.YAS_TERMINAL_CREATE_RESULT_INITIAL_VIEW_EXTENSION]),
      "Terminal CREATE Result extensions",
    ),
  ];
  cursor.end("Terminal CREATE Result");
  if (terminalHandle === 0n || stateRevision === 0n || generation === 0)
    throw new YasProtocolError("invalid Terminal CREATE Result identity");
  const initialView = takeTerminalExtension(
    extensions,
    g.YAS_TERMINAL_CREATE_RESULT_INITIAL_VIEW_EXTENSION,
  );
  return {
    terminalHandle,
    stateRevision,
    generation,
    initialViewResult: initialView
      ? decodeOpenViewResult(initialView.value)
      : undefined,
    extensions,
  };
}

function decodeOpenViewResult(body: Uint8Array): YasTerminalOpenViewResult {
  const cursor = new YasCursor(body);
  const result = {
    viewId: cursor.u32("Terminal view ID"),
    codecVersion: cursor.u16("Terminal grid codec"),
    maxInflightFrames: cursor.u8("Terminal maximum inflight frames"),
    reserved: cursor.u8("Terminal view reserved"),
    maxEncodedFrame: cursor.u32("Terminal maximum encoded frame"),
    maxDecodedFrame: cursor.u32("Terminal maximum decoded frame"),
    firstSequence: cursor.u32("Terminal first frame sequence"),
    extensions: decodeExtensions(
      cursor,
      undefined,
      "Terminal view Result extensions",
    ),
  };
  cursor.end("Terminal OPEN_VIEW Result");
  if (
    result.viewId === 0 ||
    result.codecVersion === 0 ||
    result.reserved !== 0 ||
    result.maxInflightFrames === 0 ||
    result.maxEncodedFrame === 0 ||
    result.maxDecodedFrame === 0
  )
    throw new YasProtocolError("invalid Terminal OPEN_VIEW Result");
  const { reserved: _, ...publicResult } = result;
  return publicResult;
}

function requireId(value: Uint8Array, name: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${name} must contain a nonzero 16-byte value`);
}

function terminalOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto.getRandomValues(value);
  return value;
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let i = 0; i < length; i++) {
    const difference = left[i]! - right[i]!;
    if (difference !== 0) return difference;
  }
  return left.length - right.length;
}

function mergeExtensions(
  previous: readonly YasExtension[],
  patch: readonly YasExtension[],
): YasExtension[] {
  const merged = new Map(
    previous.map((extension) => [extension.tag, extension]),
  );
  for (const extension of patch) merged.set(extension.tag, extension);
  return [...merged.values()].sort((left, right) => left.tag - right.tag);
}

function concat(parts: readonly Uint8Array[], length: number): Uint8Array {
  const result = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    result.set(part, offset);
    offset += part.length;
  }
  return result;
}

function serialDistance(from: number, to: number): number {
  return (to - from) >>> 0;
}
