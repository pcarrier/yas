import { YAS_FAMILY_TRANSFER } from "./core";
import type { ConnectionStatus } from "../types";
import {
  YAS_TRANSFER_BYTE_DATA,
  YAS_TRANSFER_CLOSE,
  YAS_TRANSFER_CREDIT,
  YAS_TRANSFER_DIRECTION_RECEIVER_TO_SENDER,
  YAS_TRANSFER_DIRECTION_SENDER_TO_RECEIVER,
  YAS_TRANSFER_DELIVERY_INLINE,
  YAS_TRANSFER_DELIVERY_TRANSFER,
  YAS_TRANSFER_MESSAGE_DATA,
  YAS_TRANSFER_MESSAGE_END,
  YAS_TRANSFER_MESSAGE_START,
  YAS_TRANSFER_MAX_OPEN_MESSAGES_EXTENSION,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RESET,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  YAS_TRANSFER_VERSION,
} from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import {
  YAS_MAX_BULK_CHUNK,
  YAS_STATUS_CANCELLED,
  YAS_STATUS_INVALID,
  YAS_STATUS_OK,
  YasCursor,
  YasProtocolError,
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
} from "./wire";

export {
  YAS_TRANSFER_BYTE_DATA,
  YAS_TRANSFER_CLOSE,
  YAS_TRANSFER_CREDIT,
  YAS_TRANSFER_MESSAGE_DATA,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RESET,
  YAS_TRANSFER_VERSION,
} from "./generated";

export const YAS_TRANSFER_RECEIVER_TO_SENDER =
  YAS_TRANSFER_DIRECTION_RECEIVER_TO_SENDER;
export const YAS_TRANSFER_SENDER_TO_RECEIVER =
  YAS_TRANSFER_DIRECTION_SENDER_TO_RECEIVER;
export const YAS_MESSAGE_START = YAS_TRANSFER_MESSAGE_START;
export const YAS_MESSAGE_END = YAS_TRANSFER_MESSAGE_END;

export interface YasTransferDescriptor {
  transferId: number;
  mode: number;
  direction: number;
  flags: number;
  receiverSendCredit: bigint;
  senderSendCredit: bigint;
  maxItemBytes: bigint;
  maxChunkBytes: number;
  contentFamily: number;
  contentKind: number;
  contentVersion: number;
  extensions: readonly YasExtension[];
  maxOpenMessages: number;
  /** Tag 2: DATA payloads must carry the frame SENSITIVE diagnostic flag. */
  sensitiveContent?: boolean;
  /** Required tag 3 on server-allocated, uncommitted upload stages. */
  uploadStage?: YasTransferUploadStage;
}

export interface YasTransferUploadStage {
  stagingHandle: bigint;
  expiresServerNs: bigint;
}

export type YasInlineOrTransfer =
  | {
      byteLength: bigint;
      contentHash: Uint8Array;
      delivery: "inline";
      bytes: Uint8Array;
    }
  | {
      byteLength: bigint;
      contentHash: Uint8Array;
      delivery: "transfer";
      descriptor: YasTransferDescriptor;
    };

/** Common result body used by Surface, Selection, Desktop, and Media. */
export function decodeInlineOrTransfer(bytes: Uint8Array): YasInlineOrTransfer {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("delivery kind");
  if (cursor.take(3, "delivery reserved").some((byte) => byte !== 0))
    throw new YasProtocolError("delivery reserved bytes are nonzero");
  const byteLength = cursor.u64("delivery byte length");
  const contentHash = new Uint8Array(cursor.take(32, "delivery content hash"));
  if (kind === YAS_TRANSFER_DELIVERY_INLINE) {
    const inline = new Uint8Array(cursor.bytesU32("inline delivery"));
    cursor.end("inline delivery");
    if (BigInt(inline.length) !== byteLength)
      throw new YasProtocolError("inline delivery length does not match");
    return { byteLength, contentHash, delivery: "inline", bytes: inline };
  }
  if (kind === YAS_TRANSFER_DELIVERY_TRANSFER) {
    const descriptor = decodeTransferDescriptor(cursor);
    cursor.end("Transfer delivery");
    if (
      descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
      descriptor.maxItemBytes !== 0n
    )
      throw new YasProtocolError("delivery descriptor is not a BYTE Transfer");
    return { byteLength, contentHash, delivery: "transfer", descriptor };
  }
  throw new YasProtocolError("unknown delivery kind");
}

export function encodeInlineOrTransfer(value: YasInlineOrTransfer): Uint8Array {
  if (value.contentHash.length !== 32)
    throw new YasProtocolError("delivery content hash is not 32 bytes");
  const writer = new YasWriter()
    .u8(
      value.delivery === "inline"
        ? YAS_TRANSFER_DELIVERY_INLINE
        : YAS_TRANSFER_DELIVERY_TRANSFER,
    )
    .bytes(new Uint8Array(3))
    .u64(value.byteLength)
    .bytes(value.contentHash);
  if (value.delivery === "inline") {
    if (BigInt(value.bytes.length) !== value.byteLength)
      throw new YasProtocolError("inline delivery length does not match");
    writer.bytesU32(value.bytes);
  } else {
    if (
      value.descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
      value.descriptor.maxItemBytes !== 0n
    )
      throw new YasProtocolError("delivery descriptor is not a BYTE Transfer");
    writer.bytes(encodeTransferDescriptor(value.descriptor));
  }
  return writer.finish();
}

export function decodeTransferDescriptor(
  cursor: YasCursor,
): YasTransferDescriptor {
  const transferId = cursor.u32("transfer ID");
  const mode = cursor.u8("transfer mode");
  const direction = cursor.u8("transfer direction");
  const flags = cursor.u16("transfer flags");
  const receiverSendCredit = cursor.u64("receiver send credit");
  const senderSendCredit = cursor.u64("sender send credit");
  const maxItemBytes = cursor.u64("maximum item bytes");
  const maxChunkBytes = cursor.u32("maximum chunk bytes");
  const contentFamily = cursor.u16("content family");
  const contentKind = cursor.u16("content kind");
  const contentVersion = cursor.u16("content version");
  const extensions = decodeExtensions(
    cursor,
    new Set([
      YAS_TRANSFER_MAX_OPEN_MESSAGES_EXTENSION,
      YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
      YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
    ]),
    "Transfer descriptor extensions",
  );
  if (transferId === 0)
    throw new YasProtocolError("Transfer ID zero is invalid");
  if (mode !== YAS_TRANSFER_MODE_BYTE && mode !== YAS_TRANSFER_MODE_MESSAGE)
    throw new YasProtocolError("unknown Transfer mode");
  if (direction === 0 || direction & ~3)
    throw new YasProtocolError("invalid Transfer direction");
  if (flags !== 0)
    throw new YasProtocolError("reserved Transfer flags are nonzero");
  if (maxChunkBytes === 0 || maxChunkBytes > YAS_MAX_BULK_CHUNK)
    throw new YasProtocolError("invalid Transfer maximum chunk size");
  if (mode === YAS_TRANSFER_MODE_BYTE && maxItemBytes !== 0n)
    throw new YasProtocolError("BYTE Transfer has a nonzero item limit");
  if (mode === YAS_TRANSFER_MODE_MESSAGE && maxItemBytes === 0n)
    throw new YasProtocolError("MESSAGE Transfer has a zero item limit");
  if (
    !(direction & YAS_TRANSFER_RECEIVER_TO_SENDER) &&
    receiverSendCredit !== 0n
  )
    throw new YasProtocolError("disallowed Transfer direction has credit");
  if (!(direction & YAS_TRANSFER_SENDER_TO_RECEIVER) && senderSendCredit !== 0n)
    throw new YasProtocolError("disallowed Transfer direction has credit");
  let maxOpenMessages = 1;
  let sensitiveContent = false;
  let uploadStage: YasTransferUploadStage | undefined;
  for (const extension of extensions) {
    if (extension.tag === YAS_TRANSFER_MAX_OPEN_MESSAGES_EXTENSION) {
      if (mode === YAS_TRANSFER_MODE_BYTE)
        throw new YasProtocolError(
          "BYTE Transfer has a max-open-messages extension",
        );
      const value = new YasCursor(extension.value);
      maxOpenMessages = value.u32("maximum open messages");
      value.end("maximum open messages");
      if (maxOpenMessages === 0)
        throw new YasProtocolError("maximum open messages is zero");
    } else if (extension.tag === YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION) {
      if (!extension.required)
        throw new YasProtocolError(
          "Transfer sensitive-content extension is not required",
        );
      if (extension.value.length !== 0)
        throw new YasProtocolError(
          "Transfer sensitive-content extension has a value",
        );
      sensitiveContent = true;
    } else if (extension.tag === YAS_TRANSFER_UPLOAD_STAGE_EXTENSION) {
      uploadStage = decodeUploadStageExtension(extension);
    }
  }
  if (
    uploadStage &&
    (mode !== YAS_TRANSFER_MODE_BYTE ||
      direction !== YAS_TRANSFER_RECEIVER_TO_SENDER ||
      senderSendCredit !== 0n ||
      !sensitiveContent)
  )
    throw new YasProtocolError("invalid Transfer upload-stage descriptor");
  return {
    transferId,
    mode,
    direction,
    flags,
    receiverSendCredit,
    senderSendCredit,
    maxItemBytes,
    maxChunkBytes,
    contentFamily,
    contentKind,
    contentVersion,
    extensions,
    maxOpenMessages,
    sensitiveContent,
    uploadStage,
  };
}

export function encodeTransferDescriptor(
  descriptor: YasTransferDescriptor,
): Uint8Array {
  const maxOpen = descriptor.extensions.find(
    (extension) => extension.tag === YAS_TRANSFER_MAX_OPEN_MESSAGES_EXTENSION,
  );
  if (maxOpen) {
    if (descriptor.mode === YAS_TRANSFER_MODE_BYTE)
      throw new YasProtocolError(
        "BYTE Transfer has a max-open-messages extension",
      );
    const cursor = new YasCursor(maxOpen.value);
    const value = cursor.u32("maximum open messages");
    cursor.end("maximum open messages");
    if (value === 0 || value !== descriptor.maxOpenMessages)
      throw new YasProtocolError(
        "invalid Transfer max-open-messages extension",
      );
  } else if (descriptor.maxOpenMessages !== 1) {
    throw new YasProtocolError(
      "Transfer max-open-messages extension is missing",
    );
  }
  const sensitive = descriptor.extensions.find(
    (extension) => extension.tag === YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  );
  if (sensitive?.value.length)
    throw new YasProtocolError(
      "Transfer sensitive-content extension has a value",
    );
  if (sensitive !== undefined && !sensitive.required)
    throw new YasProtocolError(
      "Transfer sensitive-content extension is not required",
    );
  if (
    descriptor.sensitiveContent !== undefined &&
    descriptor.sensitiveContent !== (sensitive !== undefined)
  )
    throw new YasProtocolError(
      "Transfer sensitive-content property does not match extensions",
    );
  const uploadExtension = descriptor.extensions.find(
    (extension) => extension.tag === YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  );
  const uploadStage = uploadExtension
    ? decodeUploadStageExtension(uploadExtension)
    : undefined;
  if (
    descriptor.uploadStage !== undefined &&
    (uploadStage === undefined ||
      descriptor.uploadStage.stagingHandle !== uploadStage.stagingHandle ||
      descriptor.uploadStage.expiresServerNs !== uploadStage.expiresServerNs)
  )
    throw new YasProtocolError(
      "Transfer upload-stage property does not match extensions",
    );
  if (
    uploadStage &&
    (descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
      descriptor.direction !== YAS_TRANSFER_RECEIVER_TO_SENDER ||
      descriptor.senderSendCredit !== 0n ||
      sensitive === undefined)
  )
    throw new YasProtocolError("invalid Transfer upload-stage descriptor");
  return new YasWriter()
    .u32(descriptor.transferId)
    .u8(descriptor.mode)
    .u8(descriptor.direction)
    .u16(descriptor.flags)
    .u64(descriptor.receiverSendCredit)
    .u64(descriptor.senderSendCredit)
    .u64(descriptor.maxItemBytes)
    .u32(descriptor.maxChunkBytes)
    .u16(descriptor.contentFamily)
    .u16(descriptor.contentKind)
    .u16(descriptor.contentVersion)
    .bytes(encodeExtensions(descriptor.extensions))
    .finish();
}

/** Require one descriptor to belong to the named atomic upload stage. */
export function requireTransferUploadStage(
  descriptor: YasTransferDescriptor,
  stagingHandle: bigint,
  context: string,
): YasTransferUploadStage {
  const extension = descriptor.extensions.find(
    (candidate) => candidate.tag === YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  );
  if (!extension)
    throw new YasProtocolError(`${context} is missing its upload stage`);
  const stage = decodeUploadStageExtension(extension);
  if (stage.stagingHandle !== stagingHandle)
    throw new YasProtocolError(`${context} has the wrong upload stage`);
  return stage;
}

function decodeUploadStageExtension(
  extension: YasExtension,
): YasTransferUploadStage {
  if (!extension.required || extension.value.length !== 16)
    throw new YasProtocolError("invalid Transfer upload-stage extension");
  const cursor = new YasCursor(extension.value);
  const stagingHandle = cursor.u64("Transfer upload staging handle");
  const expiresServerNs = cursor.u64("Transfer upload stage expiry");
  cursor.end("Transfer upload-stage extension");
  if (stagingHandle === 0n || expiresServerNs === 0n)
    throw new YasProtocolError("invalid Transfer upload-stage extension");
  // The allocation-time lifetime bound is checked by the descriptor issuer;
  // a receiver may observe an already-expired stage after network delay.
  return { stagingHandle, expiresServerNs };
}

export interface YasTransferByteData {
  transferId: number;
  offset: bigint;
  data: Uint8Array;
}

export interface YasTransferMessageData {
  transferId: number;
  sequence: bigint;
  fragmentOffset: bigint;
  flags: number;
  data: Uint8Array;
}

export interface YasTransferCredit {
  transferId: number;
  cumulativeLimit: bigint;
}

export interface YasTransferClose {
  transferId: number;
  finalDataBytes: bigint;
  status: number;
  detail: Uint8Array;
}

export interface YasTransferReset {
  transferId: number;
  status: number;
  detail: Uint8Array;
}

export function encodeTransferByteData(value: YasTransferByteData): Uint8Array {
  requireTransferId(value.transferId);
  if (value.data.length === 0 || value.data.length > YAS_MAX_BULK_CHUNK)
    throw new YasProtocolError("invalid Transfer BYTE_DATA length");
  return new YasWriter()
    .u32(value.transferId)
    .u64(value.offset)
    .bytes(value.data)
    .finish();
}

export function decodeTransferByteData(bytes: Uint8Array): YasTransferByteData {
  const cursor = new YasCursor(bytes);
  const value = {
    transferId: cursor.u32("transfer ID"),
    offset: cursor.u64("BYTE_DATA offset"),
    data: new Uint8Array(cursor.take(cursor.remaining)),
  };
  encodeTransferByteData(value);
  return value;
}

export function encodeTransferMessageData(
  value: YasTransferMessageData,
): Uint8Array {
  requireTransferId(value.transferId);
  if (
    value.flags & ~(YAS_MESSAGE_START | YAS_MESSAGE_END) ||
    value.data.length === 0 ||
    value.data.length > YAS_MAX_BULK_CHUNK
  )
    throw new YasProtocolError("invalid Transfer MESSAGE_DATA metadata");
  return new YasWriter()
    .u32(value.transferId)
    .u64(value.sequence)
    .u64(value.fragmentOffset)
    .u8(value.flags)
    .bytes(new Uint8Array(3))
    .bytes(value.data)
    .finish();
}

export function decodeTransferMessageData(
  bytes: Uint8Array,
): YasTransferMessageData {
  const cursor = new YasCursor(bytes);
  const value = {
    transferId: cursor.u32("transfer ID"),
    sequence: cursor.u64("MESSAGE_DATA sequence"),
    fragmentOffset: cursor.u64("MESSAGE_DATA fragment offset"),
    flags: cursor.u8("MESSAGE_DATA flags"),
    data: new Uint8Array(),
  };
  if (cursor.take(3, "MESSAGE_DATA reserved").some((byte) => byte !== 0))
    throw new YasProtocolError("MESSAGE_DATA reserved bytes are nonzero");
  value.data = new Uint8Array(cursor.take(cursor.remaining));
  encodeTransferMessageData(value);
  return value;
}

export function encodeTransferCredit(value: YasTransferCredit): Uint8Array {
  requireTransferId(value.transferId);
  return new YasWriter()
    .u32(value.transferId)
    .u64(value.cumulativeLimit)
    .finish();
}

export function decodeTransferCredit(bytes: Uint8Array): YasTransferCredit {
  const cursor = new YasCursor(bytes);
  const value = {
    transferId: cursor.u32("transfer ID"),
    cumulativeLimit: cursor.u64("cumulative credit limit"),
  };
  cursor.end("Transfer CREDIT");
  encodeTransferCredit(value);
  return value;
}

export function encodeTransferClose(value: YasTransferClose): Uint8Array {
  requireTransferId(value.transferId);
  return new YasWriter()
    .u32(value.transferId)
    .u64(value.finalDataBytes)
    .u16(value.status)
    .u16(0)
    .bytesU32(value.detail)
    .finish();
}

export function decodeTransferClose(bytes: Uint8Array): YasTransferClose {
  const cursor = new YasCursor(bytes);
  const value = {
    transferId: cursor.u32("transfer ID"),
    finalDataBytes: cursor.u64("final data bytes"),
    status: cursor.u16("Transfer CLOSE status"),
    detail: new Uint8Array(),
  };
  if (cursor.u16("Transfer CLOSE reserved") !== 0)
    throw new YasProtocolError("Transfer CLOSE reserved field is nonzero");
  value.detail = new Uint8Array(cursor.bytesU32("Transfer CLOSE detail"));
  cursor.end("Transfer CLOSE");
  encodeTransferClose(value);
  return value;
}

export function encodeTransferReset(value: YasTransferReset): Uint8Array {
  requireTransferId(value.transferId);
  return new YasWriter()
    .u32(value.transferId)
    .u16(value.status)
    .u16(0)
    .bytesU32(value.detail)
    .finish();
}

export function decodeTransferReset(bytes: Uint8Array): YasTransferReset {
  const cursor = new YasCursor(bytes);
  const value = {
    transferId: cursor.u32("transfer ID"),
    status: cursor.u16("Transfer RESET status"),
    detail: new Uint8Array(),
  };
  if (cursor.u16("Transfer RESET reserved") !== 0)
    throw new YasProtocolError("Transfer RESET reserved field is nonzero");
  value.detail = new Uint8Array(cursor.bytesU32("Transfer RESET detail"));
  cursor.end("Transfer RESET");
  encodeTransferReset(value);
  return value;
}

function requireTransferId(value: number): void {
  if (!Number.isInteger(value) || value <= 0 || value > 0xffff_ffff)
    throw new YasProtocolError("Transfer ID is invalid");
}

interface ReadWaiter {
  resolve: (value: Uint8Array | null) => void;
  reject: (error: unknown) => void;
}

interface WriteWaiter {
  resolve: () => void;
  reject: (error: unknown) => void;
}

interface OpenMessage {
  sequence: bigint;
  bytes: Uint8Array[];
  length: number;
}

interface YasTransferReceiveLease {
  readonly bytes: bigint;
  release(): void;
}

const noReceiveLease: YasTransferReceiveLease = {
  bytes: 0n,
  release() {},
};

export class YasTransfer {
  private incomingOffset = 0n;
  private outgoingOffset = 0n;
  private incomingGranted: bigint;
  private outgoingLimit: bigint;
  private incomingClosed = false;
  private outgoingClosed = false;
  private resetError: Error | null = null;
  private queue: Uint8Array[] = [];
  private readWaiters: ReadWaiter[] = [];
  private writeWaiters: WriteWaiter[] = [];
  private outgoingCreditListeners = new Set<(available: bigint) => void>();
  private openMessages = new Map<bigint, OpenMessage>();
  private completedMessages = new Map<bigint, Uint8Array>();
  private nextMessageStart = 0n;
  private nextMessageReceive = 0n;
  private nextMessageSend = 0n;
  private messageSendChain: Promise<void> = Promise.resolve();
  private terminalObserved = false;
  private terminalListeners = new Set<() => void>();
  private resetObserved = false;
  private resetListeners = new Set<() => void>();
  private leaseReleased = false;
  private closedResolve!: () => void;
  readonly closed: Promise<void>;

  constructor(
    private readonly manager: YasTransferManager,
    readonly descriptor: YasTransferDescriptor,
    private readonly receiveLease: YasTransferReceiveLease,
    private readonly localIsDescriptorSender = false,
  ) {
    this.incomingGranted = localIsDescriptorSender
      ? descriptor.receiverSendCredit
      : descriptor.senderSendCredit;
    this.outgoingLimit = localIsDescriptorSender
      ? descriptor.senderSendCredit
      : descriptor.receiverSendCredit;
    this.outgoingClosed = !this.canSend;
    this.incomingClosed = !this.canReceive;
    this.closed = new Promise((resolve) => {
      this.closedResolve = resolve;
    });
  }

  get id(): number {
    return this.descriptor.transferId;
  }

  private get canSend(): boolean {
    const direction = this.localIsDescriptorSender
      ? YAS_TRANSFER_SENDER_TO_RECEIVER
      : YAS_TRANSFER_RECEIVER_TO_SENDER;
    return (this.descriptor.direction & direction) !== 0;
  }

  private get canReceive(): boolean {
    const direction = this.localIsDescriptorSender
      ? YAS_TRANSFER_RECEIVER_TO_SENDER
      : YAS_TRANSFER_SENDER_TO_RECEIVER;
    return (this.descriptor.direction & direction) !== 0;
  }

  get bufferedReceiveBytes(): bigint {
    return this.incomingOffset;
  }

  get bufferedAmount(): number {
    const outstanding =
      this.outgoingOffset >= this.outgoingLimit
        ? this.outgoingOffset - this.outgoingLimit
        : 0n;
    return outstanding > BigInt(Number.MAX_SAFE_INTEGER)
      ? Number.MAX_SAFE_INTEGER
      : Number(outstanding);
  }

  get outgoingCreditOutstanding(): bigint {
    return this.outgoingLimit > this.outgoingOffset
      ? this.outgoingLimit - this.outgoingOffset
      : 0n;
  }

  /**
   * Observe peer credit grants. This is primarily for synchronous browser
   * adapters whose public API reports immediately-available send credit; the
   * Transfer writer itself continues to provide the authoritative async
   * backpressure.
   */
  subscribeOutgoingCredit(listener: (available: bigint) => void): () => void {
    this.outgoingCreditListeners.add(listener);
    return () => this.outgoingCreditListeners.delete(listener);
  }

  /** Observe RESET separately from a normal directional CLOSE. */
  subscribeReset(listener: () => void): () => void {
    if (this.resetObserved) {
      try {
        listener();
      } catch {
        // A lifecycle observer cannot block Transfer cleanup.
      }
      return () => undefined;
    }
    this.resetListeners.add(listener);
    return () => this.resetListeners.delete(listener);
  }

  /** Observe the first directional CLOSE or RESET on this Transfer ID. */
  subscribeTerminal(listener: () => void): () => void {
    if (this.terminalObserved) {
      try {
        listener();
      } catch {
        // A lifecycle observer cannot block Transfer cleanup.
      }
      return () => undefined;
    }
    this.terminalListeners.add(listener);
    return () => this.terminalListeners.delete(listener);
  }

  /** Bound for synchronous transport adapters which must queue before write(). */
  get outboundQueueHighWaterMark(): number {
    return this.manager.outboundQueueHighWaterMark(this.descriptor);
  }

  activateReceiveCredit(): void {
    if (!this.canReceive || this.receiveLease.bytes <= this.incomingGranted)
      return;
    this.incomingGranted = this.receiveLease.bytes;
    this.manager.send(
      YAS_TRANSFER_CREDIT,
      new YasWriter().u32(this.id).u64(this.incomingGranted).finish(),
    );
  }

  async write(data: Uint8Array): Promise<void> {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_BYTE)
      throw new YasProtocolError("write() requires a BYTE Transfer");
    if (!this.canSend)
      throw new YasProtocolError(
        "Transfer does not permit client-to-server data",
      );
    if (this.outgoingClosed)
      throw new YasProtocolError("Transfer send direction is closed");
    let offset = 0;
    while (offset < data.length) {
      await this.waitForCredit();
      this.throwIfReset();
      const available = this.outgoingLimit - this.outgoingOffset;
      const length = Math.min(
        data.length - offset,
        this.descriptor.maxChunkBytes,
        this.manager.maxOutboundChunkBytes(YAS_TRANSFER_MODE_BYTE),
        Number(
          available > BigInt(Number.MAX_SAFE_INTEGER)
            ? BigInt(Number.MAX_SAFE_INTEGER)
            : available,
        ),
      );
      if (length <= 0) continue;
      const chunk = data.subarray(offset, offset + length);
      this.manager.send(
        YAS_TRANSFER_BYTE_DATA,
        new YasWriter()
          .u32(this.id)
          .u64(this.outgoingOffset)
          .bytes(chunk)
          .finish(),
        this.descriptor.sensitiveContent,
      );
      this.outgoingOffset += BigInt(length);
      offset += length;
    }
  }

  sendMessage(message: Uint8Array): Promise<void> {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE)
      return Promise.reject(
        new YasProtocolError("sendMessage() requires a MESSAGE Transfer"),
      );
    if (message.length === 0)
      return Promise.reject(
        new YasProtocolError("empty Transfer messages are forbidden"),
      );
    if (BigInt(message.length) > this.descriptor.maxItemBytes)
      return Promise.reject(
        new YasProtocolError("Transfer message exceeds max_item_bytes"),
      );
    const copy = new Uint8Array(message);
    const sending = this.messageSendChain.then(() =>
      this.sendMessageSerial(copy),
    );
    // A failed message must not poison later callers; they will independently
    // observe the reset/closed lifecycle when their turn starts.
    this.messageSendChain = sending.catch(() => undefined);
    return sending;
  }

  private async sendMessageSerial(message: Uint8Array): Promise<void> {
    const sequence = this.nextMessageSend++;
    let offset = 0;
    do {
      if (message.length !== 0) await this.waitForCredit();
      this.throwIfReset();
      const available = this.outgoingLimit - this.outgoingOffset;
      const length =
        message.length === 0
          ? 0
          : Math.min(
              message.length - offset,
              this.descriptor.maxChunkBytes,
              this.manager.maxOutboundChunkBytes(YAS_TRANSFER_MODE_MESSAGE),
              Number(
                available > BigInt(Number.MAX_SAFE_INTEGER)
                  ? BigInt(Number.MAX_SAFE_INTEGER)
                  : available,
              ),
            );
      if (message.length !== 0 && length <= 0) continue;
      const last = offset + length === message.length;
      const flags =
        (offset === 0 ? YAS_MESSAGE_START : 0) | (last ? YAS_MESSAGE_END : 0);
      this.manager.send(
        YAS_TRANSFER_MESSAGE_DATA,
        new YasWriter()
          .u32(this.id)
          .u64(sequence)
          .u64(BigInt(offset))
          .u8(flags)
          .bytes(new Uint8Array(3))
          .bytes(message.subarray(offset, offset + length))
          .finish(),
        this.descriptor.sensitiveContent,
      );
      this.outgoingOffset += BigInt(length);
      offset += length;
      if (last) break;
    } while (true);
  }

  read(): Promise<Uint8Array | null> {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_BYTE)
      return Promise.reject(
        new YasProtocolError("read() requires a BYTE Transfer"),
      );
    return this.readItem();
  }

  readMessage(): Promise<Uint8Array | null> {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE)
      return Promise.reject(
        new YasProtocolError("readMessage() requires a MESSAGE Transfer"),
      );
    return this.readItem();
  }

  async collect(expectedLength?: bigint): Promise<Uint8Array> {
    if (
      expectedLength !== undefined &&
      expectedLength > BigInt(Number.MAX_SAFE_INTEGER)
    )
      throw new YasProtocolError("Transfer item is too large for this client");
    const chunks: Uint8Array[] = [];
    let length = 0;
    while (true) {
      const chunk = await this.read();
      if (chunk === null) break;
      chunks.push(chunk);
      length += chunk.length;
      if (expectedLength !== undefined && BigInt(length) > expectedLength)
        throw this.localReset(
          YAS_STATUS_INVALID,
          "Transfer exceeded expected length",
        );
    }
    if (expectedLength !== undefined && BigInt(length) !== expectedLength)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "Transfer ended at the wrong length",
      );
    const output = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      output.set(chunk, offset);
      offset += chunk.length;
    }
    return output;
  }

  closeWrite(
    status: number = YAS_STATUS_OK,
    detail: Uint8Array = new Uint8Array(0),
  ): void {
    if (this.outgoingClosed) return;
    this.outgoingClosed = true;
    this.observeTerminal();
    const closed = new YasProtocolError("Transfer send direction is closed");
    for (const waiter of this.writeWaiters.splice(0)) waiter.reject(closed);
    this.manager.send(
      YAS_TRANSFER_CLOSE,
      new YasWriter()
        .u32(this.id)
        .u64(this.outgoingOffset)
        .u16(status)
        .u16(0)
        .bytesU32(detail)
        .finish(),
      this.descriptor.sensitiveContent,
    );
    this.maybeFinish();
  }

  reset(
    status: number = YAS_STATUS_CANCELLED,
    detail: Uint8Array = new Uint8Array(0),
  ): void {
    if (this.resetError || this.resetObserved) return;
    this.observeTerminal();
    this.observeReset();
    this.manager.send(
      YAS_TRANSFER_RESET,
      new YasWriter().u32(this.id).u16(status).u16(0).bytesU32(detail).finish(),
      this.descriptor.sensitiveContent,
    );
    this.manager.abortUploadStage(this, status, detail);
  }

  onByteData(offset: bigint, data: Uint8Array): void {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_BYTE)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "BYTE_DATA sent to MESSAGE Transfer",
      );
    if (!this.canReceive)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "data sent in a disallowed Transfer direction",
      );
    if (this.incomingClosed)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "data sent after Transfer CLOSE",
      );
    if (data.length === 0 || data.length > this.descriptor.maxChunkBytes)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "invalid Transfer DATA chunk length",
      );
    if (offset !== this.incomingOffset)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "non-contiguous Transfer BYTE_DATA offset",
      );
    if (offset + BigInt(data.length) > this.incomingGranted)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "Transfer sender exceeded receive credit",
      );
    this.incomingOffset += BigInt(data.length);
    this.enqueue(new Uint8Array(data));
  }

  onMessageData(
    sequence: bigint,
    fragmentOffset: bigint,
    flags: number,
    data: Uint8Array,
  ): void {
    if (this.descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "MESSAGE_DATA sent to BYTE Transfer",
      );
    if (!this.canReceive || this.incomingClosed)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "invalid MESSAGE_DATA direction or lifecycle",
      );
    if (
      flags & ~3 ||
      data.length === 0 ||
      data.length > this.descriptor.maxChunkBytes
    )
      throw this.localReset(
        YAS_STATUS_INVALID,
        "invalid MESSAGE_DATA flags or length",
      );
    if (this.incomingOffset + BigInt(data.length) > this.incomingGranted)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "Transfer sender exceeded receive credit",
      );
    let message = this.openMessages.get(sequence);
    if (flags & YAS_MESSAGE_START) {
      if (
        message ||
        sequence !== this.nextMessageStart ||
        fragmentOffset !== 0n
      )
        throw this.localReset(YAS_STATUS_INVALID, "invalid MESSAGE_DATA start");
      if (this.openMessages.size >= this.descriptor.maxOpenMessages)
        throw this.localReset(
          YAS_STATUS_INVALID,
          "too many open Transfer messages",
        );
      message = { sequence, bytes: [], length: 0 };
      this.openMessages.set(sequence, message);
      this.nextMessageStart++;
    }
    if (!message || fragmentOffset !== BigInt(message.length))
      throw this.localReset(
        YAS_STATUS_INVALID,
        "non-contiguous MESSAGE_DATA fragment",
      );
    if (BigInt(message.length + data.length) > this.descriptor.maxItemBytes)
      throw this.localReset(
        YAS_STATUS_INVALID,
        "Transfer message exceeds max_item_bytes",
      );
    message.bytes.push(new Uint8Array(data));
    message.length += data.length;
    this.incomingOffset += BigInt(data.length);
    if (flags & YAS_MESSAGE_END) {
      const output = new Uint8Array(message.length);
      let offset = 0;
      for (const chunk of message.bytes) {
        output.set(chunk, offset);
        offset += chunk.length;
      }
      this.openMessages.delete(sequence);
      this.completedMessages.set(sequence, output);
      while (this.completedMessages.has(this.nextMessageReceive)) {
        this.enqueue(this.completedMessages.get(this.nextMessageReceive)!);
        this.completedMessages.delete(this.nextMessageReceive);
        this.nextMessageReceive++;
      }
    }
  }

  onCredit(cumulativeLimit: bigint): void {
    if (cumulativeLimit < this.outgoingLimit)
      throw this.localReset(YAS_STATUS_INVALID, "Transfer credit decreased");
    this.manager.validateOutboundCredit(
      this,
      this.outgoingOffset,
      this.outgoingLimit,
      cumulativeLimit,
    );
    this.outgoingLimit = cumulativeLimit;
    for (const waiter of this.writeWaiters.splice(0)) waiter.resolve();
    const available = this.outgoingCreditOutstanding;
    for (const listener of this.outgoingCreditListeners) listener(available);
  }

  onClose(finalDataBytes: bigint, status: number, detail: Uint8Array): void {
    if (this.incomingClosed)
      throw this.localReset(YAS_STATUS_INVALID, "duplicate Transfer CLOSE");
    if (
      finalDataBytes !== this.incomingOffset ||
      this.openMessages.size !== 0 ||
      this.completedMessages.size !== 0
    )
      throw this.localReset(
        YAS_STATUS_INVALID,
        "Transfer CLOSE has invalid final byte count",
      );
    this.incomingClosed = true;
    this.observeTerminal();
    if (status !== YAS_STATUS_OK)
      this.resetError = new YasResultError(status, detail);
    this.flushReaders();
    this.maybeFinish();
  }

  onReset(status: number, detail: Uint8Array): void {
    this.observeTerminal();
    this.observeReset();
    this.abort(new YasResultError(status, detail));
  }

  private observeReset(): void {
    if (this.resetObserved) return;
    this.resetObserved = true;
    for (const listener of [...this.resetListeners]) {
      try {
        listener();
      } catch {
        // A lifecycle observer cannot block Transfer cleanup.
      }
    }
    this.resetListeners.clear();
  }

  private observeTerminal(): void {
    if (this.terminalObserved) return;
    this.terminalObserved = true;
    for (const listener of [...this.terminalListeners]) {
      try {
        listener();
      } catch {
        // A lifecycle observer cannot block Transfer cleanup.
      }
    }
    this.terminalListeners.clear();
  }

  private readItem(): Promise<Uint8Array | null> {
    if (this.queue.length !== 0) {
      const item = this.queue.shift()!;
      this.consumed(item.length);
      this.maybeFinish();
      return Promise.resolve(item);
    }
    if (this.resetError) return Promise.reject(this.resetError);
    if (this.incomingClosed) return Promise.resolve(null);
    return new Promise((resolve, reject) =>
      this.readWaiters.push({ resolve, reject }),
    );
  }

  private enqueue(item: Uint8Array): void {
    const waiter = this.readWaiters.shift();
    if (waiter) {
      this.consumed(item.length);
      waiter.resolve(item);
      this.maybeFinish();
    } else {
      if (this.queue.length >= 1024)
        throw this.localReset(
          YAS_STATUS_INVALID,
          "too many buffered Transfer items",
        );
      this.queue.push(item);
    }
  }

  private flushReaders(): void {
    while (this.readWaiters.length !== 0 && this.queue.length !== 0) {
      const waiter = this.readWaiters.shift()!;
      const item = this.queue.shift()!;
      this.consumed(item.length);
      waiter.resolve(item);
      this.maybeFinish();
    }
    if (this.queue.length !== 0) return;
    for (const waiter of this.readWaiters.splice(0)) {
      if (this.resetError) waiter.reject(this.resetError);
      else waiter.resolve(null);
    }
  }

  private consumed(length: number): void {
    if (this.incomingClosed || length === 0) return;
    this.incomingGranted += BigInt(length);
    this.manager.send(
      YAS_TRANSFER_CREDIT,
      new YasWriter().u32(this.id).u64(this.incomingGranted).finish(),
    );
  }

  private waitForCredit(): Promise<void> {
    this.throwIfReset();
    if (this.outgoingOffset < this.outgoingLimit) return Promise.resolve();
    return new Promise((resolve, reject) =>
      this.writeWaiters.push({ resolve, reject }),
    );
  }

  private throwIfReset(): void {
    if (this.resetError) throw this.resetError;
  }

  private localReset(status: number, message: string): Error {
    const detail = new TextEncoder().encode(message);
    this.reset(status, detail);
    return new YasProtocolError(message);
  }

  private abort(error: Error): void {
    if (!this.resetError) this.resetError = error;
    this.incomingClosed = true;
    this.outgoingClosed = true;
    this.queue = [];
    this.openMessages.clear();
    this.completedMessages.clear();
    this.outgoingCreditListeners.clear();
    for (const waiter of this.readWaiters.splice(0))
      waiter.reject(this.resetError);
    for (const waiter of this.writeWaiters.splice(0))
      waiter.reject(this.resetError);
    this.finish();
  }

  private maybeFinish(): void {
    if (this.incomingClosed && this.outgoingClosed && this.queue.length === 0)
      this.finish();
  }

  private finish(): void {
    if (this.leaseReleased) return;
    this.leaseReleased = true;
    this.receiveLease.release();
    this.manager.remove(this.id);
    this.closedResolve();
  }
}

export class YasTransferManager {
  private readonly transfers = new Map<number, YasTransfer>();
  private nextClientTransferId = 1;
  private readonly removeListeners: (() => void)[];
  private removeInvalidationListener: (() => void) | null = null;
  private readonly onStatus = (status: ConnectionStatus) => {
    if (
      status === "connected" ||
      status === "connecting" ||
      status === "authenticating"
    )
      return;
    const detail = new TextEncoder().encode("outer YAS session disconnected");
    for (const transfer of [...this.transfers.values()])
      transfer.onReset(YAS_STATUS_CANCELLED, detail);
  };

  constructor(readonly connection: YasConnection) {
    this.removeListeners = [
      YAS_TRANSFER_BYTE_DATA,
      YAS_TRANSFER_MESSAGE_DATA,
      YAS_TRANSFER_CREDIT,
      YAS_TRANSFER_CLOSE,
      YAS_TRANSFER_RESET,
    ].map((kind) =>
      connection.onEvent(YAS_FAMILY_TRANSFER, kind, ({ payload, sensitive }) =>
        this.handle(kind, payload, sensitive),
      ),
    );
    connection.transport.addEventListener("statuschange", this.onStatus);
    this.removeInvalidationListener = connection.onInvalidation(
      ({ family }) => {
        if (family !== undefined && family !== YAS_FAMILY_TRANSFER) return;
        const detail = new TextEncoder().encode(
          family === undefined
            ? "outer YAS session invalidated"
            : "YAS Transfer family invalidated",
        );
        for (const transfer of [...this.transfers.values()])
          transfer.onReset(YAS_STATUS_CANCELLED, detail);
      },
    );
  }

  reserveReceiveCredit(preferred: bigint, minimum = 1n): YasReceiveBudgetLease {
    return this.connection.receiveBudget.reserve(preferred, minimum);
  }

  acceptServerDescriptor(
    descriptor: YasTransferDescriptor,
    lease: YasReceiveBudgetLease,
  ): YasTransfer {
    if ((descriptor.transferId & 1) !== 0)
      throw new YasProtocolError("server allocated an odd Transfer ID");
    if (descriptor.senderSendCredit > lease.bytes) {
      lease.release();
      throw new YasProtocolError(
        "Transfer descriptor exceeds proposed receive credit",
      );
    }
    if (!this.outboundCreditPermitted(descriptor.receiverSendCredit)) {
      lease.release();
      throw new YasProtocolError(
        "Transfer descriptor exceeds peer aggregate receive limit",
      );
    }
    if (this.transfers.has(descriptor.transferId)) {
      lease.release();
      throw new YasProtocolError("Transfer ID was reused");
    }
    const transfer = new YasTransfer(this, descriptor, lease);
    this.transfers.set(descriptor.transferId, transfer);
    transfer.activateReceiveCredit();
    return transfer;
  }

  /**
   * Accept a server-allocated upload stream without reserving browser receive
   * memory. Receiver-to-sender-only descriptors can never deliver DATA to the
   * browser, so a receive-budget lease would account for bytes that cannot
   * exist.
   */
  acceptServerUploadDescriptor(descriptor: YasTransferDescriptor): YasTransfer {
    if (descriptor.direction !== YAS_TRANSFER_RECEIVER_TO_SENDER)
      throw new YasProtocolError(
        "upload Transfer descriptor permits a server-to-client direction",
      );
    if (descriptor.senderSendCredit !== 0n)
      throw new YasProtocolError(
        "upload Transfer descriptor has server send credit",
      );
    if ((descriptor.transferId & 1) !== 0)
      throw new YasProtocolError("server allocated an odd Transfer ID");
    if (!this.outboundCreditPermitted(descriptor.receiverSendCredit))
      throw new YasProtocolError(
        "Transfer descriptor exceeds peer aggregate receive limit",
      );
    if (this.transfers.has(descriptor.transferId))
      throw new YasProtocolError("Transfer ID was reused");
    const transfer = new YasTransfer(this, descriptor, noReceiveLease);
    this.transfers.set(descriptor.transferId, transfer);
    return transfer;
  }

  /**
   * Allocate and register an odd-ID descriptor carried by a client Result or
   * Event. Registration precedes the domain frame, preventing an immediately
   * following peer CREDIT/DATA frame from racing the Transfer dispatcher.
   */
  createClientDescriptor(
    descriptor: Omit<
      YasTransferDescriptor,
      "transferId" | "flags" | "maxOpenMessages" | "sensitiveContent"
    > & {
      flags?: 0;
      maxOpenMessages?: number;
      sensitiveContent?: boolean;
    },
    receiveLease: YasReceiveBudgetLease | undefined = undefined,
  ): { descriptor: YasTransferDescriptor; transfer: YasTransfer } {
    const transferId = this.allocateClientTransferId();
    const value: YasTransferDescriptor = {
      ...descriptor,
      transferId,
      flags: descriptor.flags ?? 0,
      maxOpenMessages: descriptor.maxOpenMessages ?? 1,
    };
    encodeTransferDescriptor(value);
    const receives = (value.direction & YAS_TRANSFER_RECEIVER_TO_SENDER) !== 0;
    if (receives) {
      if (!receiveLease)
        throw new YasProtocolError(
          "client Transfer receive direction has no receive-budget lease",
        );
      if (value.receiverSendCredit > receiveLease.bytes) {
        receiveLease.release();
        throw new YasProtocolError(
          "client Transfer descriptor exceeds local receive credit",
        );
      }
    } else if (receiveLease) {
      receiveLease.release();
      throw new YasProtocolError(
        "client upload-only Transfer has an unnecessary receive lease",
      );
    }
    if (!this.outboundCreditPermitted(value.senderSendCredit)) {
      receiveLease?.release();
      throw new YasProtocolError(
        "client Transfer descriptor exceeds peer aggregate receive limit",
      );
    }
    const transfer = new YasTransfer(
      this,
      value,
      receiveLease ?? noReceiveLease,
      true,
    );
    this.transfers.set(transferId, transfer);
    return { descriptor: value, transfer };
  }

  get(id: number): YasTransfer | undefined {
    return this.transfers.get(id);
  }

  send(kind: number, payload: Uint8Array, sensitive?: boolean): void {
    this.connection.sendEvent(YAS_FAMILY_TRANSFER, kind, payload, sensitive);
  }

  maxOutboundChunkBytes(mode: number): number {
    const receiveMaxFrame = this.connection.hello?.receiveMaxFrame ?? 0;
    // Event header plus BYTE_DATA or MESSAGE_DATA fixed payload fields.
    const overhead = mode === YAS_TRANSFER_MODE_BYTE ? 17 : 29;
    const available = receiveMaxFrame - overhead;
    if (available <= 0)
      throw new YasProtocolError(
        "peer receive frame limit cannot carry Transfer data",
      );
    return Math.min(available, YAS_MAX_BULK_CHUNK);
  }

  outboundQueueHighWaterMark(descriptor: YasTransferDescriptor): number {
    const peerLimit =
      this.connection.hello?.receiveMaxBuffered ??
      BigInt(descriptor.maxChunkBytes);
    const localQueueCap = BigInt(descriptor.maxChunkBytes) * 4n;
    const limit = peerLimit < localQueueCap ? peerLimit : localQueueCap;
    return Number(
      limit > BigInt(Number.MAX_SAFE_INTEGER)
        ? BigInt(Number.MAX_SAFE_INTEGER)
        : limit,
    );
  }

  private aggregateOutboundCredit(except?: YasTransfer): bigint {
    let total = 0n;
    for (const transfer of this.transfers.values())
      if (transfer !== except) total += transfer.outgoingCreditOutstanding;
    return total;
  }

  private allocateClientTransferId(): number {
    const first = this.nextClientTransferId;
    do {
      const candidate = this.nextClientTransferId;
      this.nextClientTransferId += 2;
      if (this.nextClientTransferId > 0xffff_ffff)
        this.nextClientTransferId = 1;
      if (!this.transfers.has(candidate)) return candidate;
    } while (this.nextClientTransferId !== first);
    throw new YasProtocolError("client Transfer ID space is exhausted");
  }

  private outboundCreditPermitted(additional: bigint): boolean {
    const peerLimit = this.connection.hello?.receiveMaxBuffered;
    if (peerLimit === undefined) return true;
    const current = this.aggregateOutboundCredit();
    // A reduced SESSION_UPDATE does not revoke already-issued aggregate
    // credit, but it forbids growing it until outstanding credit drains.
    const permitted = current > peerLimit ? current : peerLimit;
    return current + additional <= permitted;
  }

  validateOutboundCredit(
    transfer: YasTransfer,
    sent: bigint,
    previousLimit: bigint,
    nextLimit: bigint,
  ): void {
    const peerLimit = this.connection.hello?.receiveMaxBuffered;
    if (peerLimit === undefined) return;
    const others = this.aggregateOutboundCredit(transfer);
    const previousOutstanding =
      previousLimit > sent ? previousLimit - sent : 0n;
    const nextOutstanding = nextLimit > sent ? nextLimit - sent : 0n;
    // A reduced SESSION_UPDATE limit does not revoke already granted credit,
    // but the peer may not grow the window again until it drains under the cap.
    const current = others + previousOutstanding;
    const next = others + nextOutstanding;
    const permitted = current > peerLimit ? current : peerLimit;
    if (next > permitted)
      throw new YasProtocolError(
        "Transfer CREDIT exceeds peer aggregate receive limit",
      );
  }

  remove(id: number): void {
    this.transfers.delete(id);
  }

  /** RESET discards one entire staged upload, including sibling streams. */
  abortUploadStage(
    source: YasTransfer,
    status: number,
    detail: Uint8Array,
  ): void {
    const stagingHandle = source.descriptor.uploadStage?.stagingHandle;
    if (stagingHandle === undefined) {
      source.onReset(status, detail);
      return;
    }
    const contentFamily = source.descriptor.contentFamily;
    for (const transfer of [...this.transfers.values()]) {
      if (
        transfer.descriptor.contentFamily === contentFamily &&
        transfer.descriptor.uploadStage?.stagingHandle === stagingHandle
      )
        transfer.onReset(status, detail);
    }
  }

  close(): void {
    for (const remove of this.removeListeners) remove();
    this.connection.transport.removeEventListener(
      "statuschange",
      this.onStatus,
    );
    this.removeInvalidationListener?.();
    this.removeInvalidationListener = null;
    for (const transfer of [...this.transfers.values()]) transfer.reset();
  }

  private handle(kind: number, payload: Uint8Array, sensitive: boolean): void {
    const cursor = new YasCursor(payload);
    const transferId = cursor.u32("transfer ID");
    if (transferId === 0)
      throw new YasProtocolError("Transfer ID zero is invalid");
    const transfer = this.transfers.get(transferId);
    if (!transfer) {
      // A late frame for a completed ID is resource-local. Tell the peer to
      // stop without turning one stale stream into a session failure.
      if (kind !== YAS_TRANSFER_RESET) {
        this.send(
          YAS_TRANSFER_RESET,
          new YasWriter()
            .u32(transferId)
            .u16(YAS_STATUS_INVALID)
            .u16(0)
            .u32(0)
            .finish(),
        );
      }
      return;
    }
    try {
      if (
        (kind === YAS_TRANSFER_BYTE_DATA ||
          kind === YAS_TRANSFER_MESSAGE_DATA ||
          kind === YAS_TRANSFER_CLOSE ||
          kind === YAS_TRANSFER_RESET) &&
        transfer.descriptor.sensitiveContent &&
        !sensitive
      )
        throw new YasProtocolError(
          "sensitive Transfer content omitted the SENSITIVE flag",
        );
      if (kind === YAS_TRANSFER_BYTE_DATA) {
        const offset = cursor.u64("BYTE_DATA offset");
        const data = cursor.take(cursor.remaining);
        transfer.onByteData(offset, data);
      } else if (kind === YAS_TRANSFER_MESSAGE_DATA) {
        const sequence = cursor.u64("MESSAGE_DATA sequence");
        const fragmentOffset = cursor.u64("MESSAGE_DATA fragment offset");
        const flags = cursor.u8("MESSAGE_DATA flags");
        if (
          cursor.take(3, "MESSAGE_DATA reserved").some((value) => value !== 0)
        )
          throw new YasProtocolError("MESSAGE_DATA reserved bytes are nonzero");
        transfer.onMessageData(
          sequence,
          fragmentOffset,
          flags,
          cursor.take(cursor.remaining),
        );
      } else if (kind === YAS_TRANSFER_CREDIT) {
        transfer.onCredit(cursor.u64("cumulative credit limit"));
        cursor.end("Transfer CREDIT");
      } else if (kind === YAS_TRANSFER_CLOSE) {
        const finalDataBytes = cursor.u64("final data bytes");
        const status = cursor.u16("Transfer CLOSE status");
        if (cursor.u16("Transfer CLOSE reserved") !== 0)
          throw new YasProtocolError(
            "Transfer CLOSE reserved field is nonzero",
          );
        const detail = new Uint8Array(cursor.bytesU32("Transfer CLOSE detail"));
        cursor.end("Transfer CLOSE");
        transfer.onClose(finalDataBytes, status, detail);
      } else {
        const status = cursor.u16("Transfer RESET status");
        if (cursor.u16("Transfer RESET reserved") !== 0)
          throw new YasProtocolError(
            "Transfer RESET reserved field is nonzero",
          );
        const detail = new Uint8Array(cursor.bytesU32("Transfer RESET detail"));
        cursor.end("Transfer RESET");
        this.abortUploadStage(transfer, status, detail);
      }
    } catch (error) {
      if (this.transfers.has(transferId))
        transfer.reset(
          YAS_STATUS_INVALID,
          new TextEncoder().encode(
            error instanceof Error ? error.message : String(error),
          ),
        );
    }
  }
}

const managers = new WeakMap<YasConnection, YasTransferManager>();

/** Relay and Font on one session must share one Transfer dispatcher and budget. */
export function transfersFor(connection: YasConnection): YasTransferManager {
  let manager = managers.get(connection);
  if (!manager) {
    manager = new YasTransferManager(connection);
    managers.set(connection, manager);
  }
  return manager;
}
