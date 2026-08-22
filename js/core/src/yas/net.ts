/** YAS Net family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasDisconnectedError,
  YasProtocolError,
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
} from "./wire";

export {
  YAS_FAMILY_NET,
  YAS_NET_CLOSE,
  YAS_NET_DATAGRAM,
  YAS_NET_DATAGRAM_STATS,
  YAS_NET_OPEN,
  YAS_NET_VERSION,
} from "./generated";

export type YasNetAddress =
  | { kind: "tcp"; host: string; port: number }
  | { kind: "udp"; host: string; port: number }
  | {
      kind: "unix-stream" | "unix-datagram" | "unix-seqpacket";
      nameKind: number;
      name: Uint8Array;
    }
  | { kind: "windows-pipe"; requestedMode: number; name: string };

export interface YasNetTlsOptions {
  verification: number;
  sni: string;
  alpn: readonly Uint8Array[];
  extensions?: readonly YasExtension[];
}

export interface YasNetOpen {
  operationId: Uint8Array;
  address: YasNetAddress;
  deliveryPreference: number;
  dropPolicy: number;
  initialReceiveCredit: bigint;
  earlyData?: Uint8Array;
  tls?: YasNetTlsOptions;
  extensions?: readonly YasExtension[];
}

export interface YasNetEndpoint {
  flowHandle: bigint;
  mode: number;
  direction: number;
  selectedDelivery: number;
  maxDatagramPayload: number;
  serverInstanceLimit: number;
  maxMessageBytes: bigint;
  localAddress?: YasNetAddress;
  peerAddress: YasNetAddress;
  negotiatedAlpn: Uint8Array;
  descriptor?: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasNetDatagram {
  flowHandle: bigint;
  sequence: bigint;
  payload: Uint8Array;
}

export interface YasNetReceivedDatagram extends YasNetDatagram {
  /** Number of peer-assigned sequence values missing since the prior event. */
  droppedBefore: bigint;
}

export interface YasNetDatagramStats {
  flowHandle: bigint;
  revision: bigint;
  final: boolean;
  clientToPeerDelivered: bigint;
  peerToClientDelivered: bigint;
  clientOversizedDrops: bigint;
  peerOversizedDrops: bigint;
  clientCongestiveDrops: bigint;
  peerCongestiveDrops: bigint;
  transportErrors: bigint;
  extensions: readonly YasExtension[];
}

interface YasNetOpenOperation {
  payloadKey: string;
  wirePayload: Uint8Array | null;
  receiveCredit: bigint;
  pending: Promise<YasNetFlow> | null;
  flow: YasNetFlow | null;
  retainPayload: boolean;
}

export function encodeNetAddress(value: YasNetAddress): Uint8Array {
  validateAddress(value);
  const writer = new YasWriter();
  if (value.kind === "tcp" || value.kind === "udp") {
    writer
      .u8(value.kind === "tcp" ? g.YAS_NET_ADDRESS_TCP : g.YAS_NET_ADDRESS_UDP)
      .bytes(new Uint8Array(3))
      .utf8U16(value.host)
      .u16(value.port)
      .u16(0);
  } else if (value.kind === "windows-pipe") {
    writer
      .u8(g.YAS_NET_ADDRESS_WINDOWS_PIPE)
      .bytes(new Uint8Array(3))
      .u8(value.requestedMode)
      .bytes(new Uint8Array(3))
      .utf8U16(value.name);
  } else {
    const kind =
      value.kind === "unix-stream"
        ? g.YAS_NET_ADDRESS_UNIX_STREAM
        : value.kind === "unix-datagram"
          ? g.YAS_NET_ADDRESS_UNIX_DATAGRAM
          : g.YAS_NET_ADDRESS_UNIX_SEQPACKET;
    writer
      .u8(kind)
      .bytes(new Uint8Array(3))
      .u8(value.nameKind)
      .bytes(new Uint8Array(3))
      .bytesU32(value.name);
  }
  return writer.finish();
}

export function decodeNetAddress(bytes: Uint8Array): YasNetAddress {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Net address kind");
  requireZero(cursor.take(3, "Net address reserved"), "Net address");
  let value: YasNetAddress;
  if (kind === g.YAS_NET_ADDRESS_TCP || kind === g.YAS_NET_ADDRESS_UDP) {
    const host = cursor.utf8U16("Net host");
    const port = cursor.u16("Net port");
    if (cursor.u16("Net address reserved") !== 0)
      throw new YasProtocolError("Net address reserved field is nonzero");
    value = {
      kind: kind === g.YAS_NET_ADDRESS_TCP ? "tcp" : "udp",
      host,
      port,
    };
  } else if (
    kind === g.YAS_NET_ADDRESS_UNIX_STREAM ||
    kind === g.YAS_NET_ADDRESS_UNIX_DATAGRAM ||
    kind === g.YAS_NET_ADDRESS_UNIX_SEQPACKET
  ) {
    const nameKind = cursor.u8("Net Unix address kind");
    requireZero(
      cursor.take(3, "Net Unix address reserved"),
      "Net Unix address",
    );
    value = {
      kind:
        kind === g.YAS_NET_ADDRESS_UNIX_STREAM
          ? "unix-stream"
          : kind === g.YAS_NET_ADDRESS_UNIX_DATAGRAM
            ? "unix-datagram"
            : "unix-seqpacket",
      nameKind,
      name: new Uint8Array(cursor.bytesU32("Net Unix address")),
    };
  } else if (kind === g.YAS_NET_ADDRESS_WINDOWS_PIPE) {
    const requestedMode = cursor.u8("Net pipe mode");
    requireZero(cursor.take(3, "Net pipe reserved"), "Net pipe");
    value = {
      kind: "windows-pipe",
      requestedMode,
      name: cursor.utf8U16("Net pipe name"),
    };
  } else throw new YasProtocolError("unknown Net address kind");
  cursor.end("Net address");
  validateAddress(value);
  return value;
}

export function encodeNetTlsOptions(value: YasNetTlsOptions): Uint8Array {
  validateTls(value);
  const writer = new YasWriter()
    .u8(value.verification)
    .bytes(new Uint8Array(3))
    .utf8U16(value.sni)
    .u16(value.alpn.length)
    .u16(0);
  for (const protocol of value.alpn) writer.bytesU16(protocol);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeNetTlsOptions(bytes: Uint8Array): YasNetTlsOptions {
  const cursor = new YasCursor(bytes);
  const verification = cursor.u8("Net TLS verification");
  requireZero(cursor.take(3, "Net TLS reserved"), "Net TLS");
  const sni = cursor.utf8U16("Net TLS SNI");
  const count = cursor.u16("Net TLS ALPN count");
  if (
    cursor.u16("Net TLS reserved") !== 0 ||
    count > g.YAS_NET_MAX_ALPN_PROTOCOLS ||
    count > Math.floor(cursor.remaining / 2)
  )
    throw new YasProtocolError("invalid Net TLS ALPN count or reserved field");
  const alpn: Uint8Array[] = [];
  for (let index = 0; index < count; index++)
    alpn.push(new Uint8Array(cursor.bytesU16("Net TLS ALPN protocol")));
  const value = {
    verification,
    sni,
    alpn,
    extensions: decodeExtensions(cursor, new Set(), "Net TLS extensions"),
  };
  cursor.end("Net TLS options");
  validateTls(value);
  return value;
}

export function encodeNetOpen(value: YasNetOpen): Uint8Array {
  validateOpen(value);
  return new YasWriter()
    .bytes(value.operationId)
    .bytesU32(encodeNetAddress(value.address))
    .u8(value.deliveryPreference)
    .u8(value.dropPolicy)
    .u16(0)
    .u64(value.initialReceiveCredit)
    .bytesU32(value.earlyData ?? new Uint8Array(0))
    .bytesU32(value.tls ? encodeNetTlsOptions(value.tls) : new Uint8Array(0))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeNetOpen(bytes: Uint8Array): YasNetOpen {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Net operation ID"));
  const address = decodeNetAddress(cursor.bytesU32("Net address"));
  const deliveryPreference = cursor.u8("Net delivery preference");
  const dropPolicy = cursor.u8("Net drop policy");
  if (cursor.u16("Net OPEN reserved") !== 0)
    throw new YasProtocolError("Net OPEN reserved field is nonzero");
  const initialReceiveCredit = cursor.u64("Net initial receive credit");
  const earlyData = new Uint8Array(cursor.bytesU32("Net early data"));
  const tlsBytes = cursor.bytesU32("Net TLS options");
  const value = {
    operationId,
    address,
    deliveryPreference,
    dropPolicy,
    initialReceiveCredit,
    earlyData,
    tls: tlsBytes.length ? decodeNetTlsOptions(tlsBytes) : undefined,
    extensions: decodeExtensions(cursor, new Set(), "Net OPEN extensions"),
  };
  cursor.end("Net OPEN");
  validateOpen(value);
  return value;
}

export function encodeNetClose(
  flowHandle: bigint,
  operationId: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(flowHandle, "Net flow handle");
  requireOperationId(operationId);
  return new YasWriter()
    .u64(flowHandle)
    .bytes(operationId)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeNetEndpoint(bytes: Uint8Array): YasNetEndpoint {
  const cursor = new YasCursor(bytes);
  const flowHandle = cursor.u64("Net flow handle");
  const mode = cursor.u8("Net flow mode");
  const direction = cursor.u8("Net flow direction");
  const selectedDelivery = cursor.u8("Net selected delivery");
  if (cursor.u8("Net endpoint reserved") !== 0)
    throw new YasProtocolError("Net endpoint reserved byte is nonzero");
  const maxDatagramPayload = cursor.u32("Net maximum datagram payload");
  const serverInstanceLimit = cursor.u32("Net server instance limit");
  const maxMessageBytes = cursor.u64("Net maximum message bytes");
  const localBytes = cursor.bytesU32("Net local address");
  const peerAddress = decodeNetAddress(cursor.bytesU32("Net peer address"));
  const negotiatedAlpn = new Uint8Array(cursor.bytesU16("Net negotiated ALPN"));
  const descriptorBytes = cursor.bytesU32("Net Transfer descriptor");
  const value = {
    flowHandle,
    mode,
    direction,
    selectedDelivery,
    maxDatagramPayload,
    serverInstanceLimit,
    maxMessageBytes,
    localAddress: localBytes.length ? decodeNetAddress(localBytes) : undefined,
    peerAddress,
    negotiatedAlpn,
    descriptor: descriptorBytes.length
      ? decodeDescriptor(descriptorBytes)
      : undefined,
    extensions: decodeExtensions(cursor, new Set(), "Net endpoint extensions"),
  };
  cursor.end("Net endpoint");
  validateEndpoint(value);
  return value;
}

export function encodeNetEndpoint(value: YasNetEndpoint): Uint8Array {
  validateEndpoint(value);
  return new YasWriter()
    .u64(value.flowHandle)
    .u8(value.mode)
    .u8(value.direction)
    .u8(value.selectedDelivery)
    .u8(0)
    .u32(value.maxDatagramPayload)
    .u32(value.serverInstanceLimit)
    .u64(value.maxMessageBytes)
    .bytesU32(
      value.localAddress
        ? encodeNetAddress(value.localAddress)
        : new Uint8Array(0),
    )
    .bytesU32(encodeNetAddress(value.peerAddress))
    .bytesU16(value.negotiatedAlpn)
    .bytesU32(
      value.descriptor
        ? encodeTransferDescriptor(value.descriptor)
        : new Uint8Array(0),
    )
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeNetDatagram(bytes: Uint8Array): YasNetDatagram {
  const cursor = new YasCursor(bytes);
  const value = {
    flowHandle: cursor.u64("Net flow handle"),
    sequence: cursor.u64("Net datagram sequence"),
    payload: new Uint8Array(
      cursor.take(cursor.remaining, "Net datagram payload"),
    ),
  };
  cursor.end("Net DATAGRAM");
  validateDatagram(value);
  return value;
}

export function encodeNetDatagram(value: YasNetDatagram): Uint8Array {
  validateDatagram(value);
  return new YasWriter()
    .u64(value.flowHandle)
    .u64(value.sequence)
    .bytes(value.payload)
    .finish();
}

export function decodeNetDatagramStats(bytes: Uint8Array): YasNetDatagramStats {
  const cursor = new YasCursor(bytes);
  const flowHandle = cursor.u64("Net flow handle");
  const revision = cursor.u64("Net stats revision");
  const flags = cursor.u16("Net stats flags");
  if (
    flags & ~g.YAS_NET_DATAGRAM_STATS_FINAL ||
    cursor.u16("Net stats reserved") !== 0
  )
    throw new YasProtocolError("invalid Net datagram-stats flags");
  const value = {
    flowHandle,
    revision,
    final: Boolean(flags & g.YAS_NET_DATAGRAM_STATS_FINAL),
    clientToPeerDelivered: cursor.u64("Net client delivered"),
    peerToClientDelivered: cursor.u64("Net peer delivered"),
    clientOversizedDrops: cursor.u64("Net client oversized drops"),
    peerOversizedDrops: cursor.u64("Net peer oversized drops"),
    clientCongestiveDrops: cursor.u64("Net client congestive drops"),
    peerCongestiveDrops: cursor.u64("Net peer congestive drops"),
    transportErrors: cursor.u64("Net transport errors"),
    extensions: decodeExtensions(cursor, new Set(), "Net stats extensions"),
  };
  cursor.end("Net DATAGRAM_STATS");
  validateStats(value);
  return value;
}

export function encodeNetDatagramStats(value: YasNetDatagramStats): Uint8Array {
  validateStats(value);
  return new YasWriter()
    .u64(value.flowHandle)
    .u64(value.revision)
    .u16(value.final ? g.YAS_NET_DATAGRAM_STATS_FINAL : 0)
    .u16(0)
    .u64(value.clientToPeerDelivered)
    .u64(value.peerToClientDelivered)
    .u64(value.clientOversizedDrops)
    .u64(value.peerOversizedDrops)
    .u64(value.clientCongestiveDrops)
    .u64(value.peerCongestiveDrops)
    .u64(value.transportErrors)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export class YasNetFlow {
  private nextSendSequence = 0n;
  private lastReceiveSequence: bigint | undefined;
  private lastStatsRevision = 0n;
  private finalStats = false;
  private closed = false;
  private datagramListeners = new Set<
    (value: YasNetReceivedDatagram) => void
  >();
  private statsListeners = new Set<(value: YasNetDatagramStats) => void>();

  constructor(
    private readonly client: YasNetClient,
    readonly endpoint: YasNetEndpoint,
    readonly transfer?: YasTransfer,
  ) {}

  onDatagram(listener: (value: YasNetReceivedDatagram) => void): () => void {
    this.datagramListeners.add(listener);
    return () => this.datagramListeners.delete(listener);
  }

  onStats(listener: (value: YasNetDatagramStats) => void): () => void {
    this.statsListeners.add(listener);
    return () => this.statsListeners.delete(listener);
  }

  sendDatagram(payload: Uint8Array): bigint {
    if (this.closed) throw new YasProtocolError("Net flow is closed");
    if (this.endpoint.mode !== g.YAS_NET_MODE_DATAGRAM)
      throw new YasProtocolError("Net flow is not datagram mode");
    if (!(this.endpoint.direction & g.YAS_NET_DIRECTION_CLIENT_TO_PEER))
      throw new YasProtocolError("Net flow forbids client-to-peer datagrams");
    if (payload.length > this.endpoint.maxDatagramPayload)
      throw new YasProtocolError("Net datagram exceeds endpoint maximum");
    const sequence = this.nextSendSequence++;
    const encoded = encodeNetDatagram({
      flowHandle: this.endpoint.flowHandle,
      sequence,
      payload,
    });
    if (this.endpoint.selectedDelivery === g.YAS_NET_DELIVERY_NATIVE_DATAGRAM) {
      if (
        !this.client.connection.sendDatagramEvent(
          g.YAS_FAMILY_NET,
          g.YAS_NET_DATAGRAM,
          encoded,
        )
      )
        throw new YasProtocolError(
          "Net selected a native datagram path that is unavailable",
        );
    } else {
      this.client.connection.sendEvent(
        g.YAS_FAMILY_NET,
        g.YAS_NET_DATAGRAM,
        encoded,
      );
    }
    return sequence;
  }

  async close(
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    await this.client.closeFlow(
      this.endpoint.flowHandle,
      operationId,
      extensions,
    );
  }

  receiveDatagram(value: YasNetDatagram, transportDatagram = false): void {
    if (this.closed) return;
    if (!(this.endpoint.direction & g.YAS_NET_DIRECTION_PEER_TO_CLIENT))
      throw new YasProtocolError(
        "Net flow received a forbidden datagram direction",
      );
    const native =
      this.endpoint.selectedDelivery === g.YAS_NET_DELIVERY_NATIVE_DATAGRAM;
    if (native !== transportDatagram)
      throw new YasProtocolError("Net datagram used the wrong delivery path");
    if (value.payload.length > this.endpoint.maxDatagramPayload)
      throw new YasProtocolError("Net peer exceeded endpoint datagram maximum");
    let droppedBefore = 0n;
    if (this.lastReceiveSequence === undefined) {
      droppedBefore = value.sequence;
      this.lastReceiveSequence = value.sequence;
    } else if (value.sequence > this.lastReceiveSequence) {
      droppedBefore = value.sequence - this.lastReceiveSequence - 1n;
      this.lastReceiveSequence = value.sequence;
    }
    const received = { ...value, droppedBefore };
    for (const listener of this.datagramListeners) {
      try {
        listener(received);
      } catch {
        // One observer cannot fail Event dispatch for its siblings.
      }
    }
  }

  receiveStats(value: YasNetDatagramStats): void {
    if (this.closed && !value.final) return;
    if (this.finalStats || value.revision <= this.lastStatsRevision)
      throw new YasProtocolError(
        "Net datagram-stats revision did not increase",
      );
    this.lastStatsRevision = value.revision;
    this.finalStats = value.final;
    for (const listener of this.statsListeners) {
      try {
        listener(value);
      } catch {
        // One observer cannot fail Event dispatch for its siblings.
      }
    }
    if (value.final) this.client.releaseFlow(this.endpoint.flowHandle, this);
  }

  invalidate(): void {
    this.closed = true;
    this.datagramListeners.clear();
    this.statsListeners.clear();
  }
}

export class YasNetClient {
  private readonly transfers;
  private readonly flows = new Map<bigint, YasNetFlow>();
  private readonly flowOperationKeys = new WeakMap<YasNetFlow, string>();
  private readonly openOperations = new Map<string, YasNetOpenOperation>();
  private readonly pendingOpenOperations = new Map<
    string,
    YasNetOpenOperation
  >();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private removeListeners: (() => void)[];
  private epoch = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    this.transfers = transfersFor(connection);
    this.removeListeners = [
      connection.onEvent(
        g.YAS_FAMILY_NET,
        g.YAS_NET_DATAGRAM,
        ({ payload, datagram }) => {
          const value = decodeNetDatagram(payload);
          this.flows.get(value.flowHandle)?.receiveDatagram(value, datagram);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_NET,
        g.YAS_NET_DATAGRAM_STATS,
        ({ payload }) => {
          const value = decodeNetDatagramStats(payload);
          this.flows.get(value.flowHandle)?.receiveStats(value);
        },
      ),
      connection.onInvalidation(({ family }) => {
        if (family === undefined || family === g.YAS_FAMILY_NET)
          this.invalidate();
      }),
    ];
  }

  open(
    value: Omit<YasNetOpen, "initialReceiveCredit">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasNetFlow> {
    this.assertOpen();
    const datagram = isDatagramAddress(value.address);
    const canonicalPayload = encodeNetOpen({
      ...value,
      initialReceiveCredit: datagram ? 0n : initialReceiveCredit,
    });
    const operationKey = byteKey(value.operationId);
    const payloadKey = byteKey(canonicalPayload);
    let operation =
      this.openOperations.get(operationKey) ??
      this.pendingOpenOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Net OPEN operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (operation.flow) {
        if (
          this.flows.get(operation.flow.endpoint.flowHandle) === operation.flow
        )
          return Promise.resolve(operation.flow);
        operation.flow = null;
      }
    } else {
      this.ensureOpenReplaySlot(operationKey);
      operation = {
        payloadKey,
        wirePayload: null,
        receiveCredit: 0n,
        pending: null,
        flow: null,
        retainPayload: false,
      };
      this.pendingOpenOperations.set(operationKey, operation);
    }
    if (operation.retainPayload) this.ensureOpenReplaySlot(operationKey);
    const epoch = this.epoch;
    let running: Promise<YasNetFlow>;
    try {
      running = operation.wirePayload
        ? this.performOpenPayload(
            operation.wirePayload,
            operation.receiveCredit,
            operationKey,
            operation,
            epoch,
            true,
          )
        : this.performFreshOpen(
            value,
            initialReceiveCredit,
            operationKey,
            operation,
            epoch,
          );
    } catch (error) {
      if (!operation.flow && !operation.retainPayload)
        this.pendingOpenOperations.delete(operationKey);
      throw error;
    }
    let pending!: Promise<YasNetFlow>;
    pending = this.runOwned(running)
      .then((flow) => {
        if (this.disposed || epoch !== this.epoch)
          throw new YasDisconnectedError(
            "Net OPEN completed after client disposal or family invalidation",
          );
        return flow;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          !operation.flow &&
          !operation.retainPayload &&
          this.pendingOpenOperations.get(operationKey) === operation
        )
          this.pendingOpenOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  private performFreshOpen(
    value: Omit<YasNetOpen, "initialReceiveCredit">,
    initialReceiveCredit: bigint,
    operationKey: string,
    operation: YasNetOpenOperation,
    epoch: number,
  ): Promise<YasNetFlow> {
    const datagram = isDatagramAddress(value.address);
    let lease: YasReceiveBudgetLease | undefined;
    if (!datagram)
      lease = this.transfers.reserveReceiveCredit(initialReceiveCredit, 1n);
    operation.receiveCredit = lease?.bytes ?? 0n;
    operation.wirePayload = encodeNetOpen({
      ...value,
      initialReceiveCredit: operation.receiveCredit,
    });
    return this.requestOpen(
      operation.wirePayload,
      lease,
      operationKey,
      operation,
      epoch,
    );
  }

  private performOpenPayload(
    payload: Uint8Array,
    receiveCredit: bigint,
    operationKey: string,
    operation: YasNetOpenOperation,
    epoch: number,
    exact: boolean,
  ): Promise<YasNetFlow> {
    const lease =
      receiveCredit === 0n
        ? undefined
        : this.transfers.reserveReceiveCredit(
            receiveCredit,
            exact ? receiveCredit : 1n,
          );
    return this.requestOpen(payload, lease, operationKey, operation, epoch);
  }

  private async requestOpen(
    payload: Uint8Array,
    lease: YasReceiveBudgetLease | undefined,
    operationKey: string,
    operation: YasNetOpenOperation,
    epoch: number,
  ): Promise<YasNetFlow> {
    let leaseOwned = lease !== undefined;
    const install = (endpoint: YasNetEndpoint): YasNetFlow => {
      if (this.disposed || epoch !== this.epoch) {
        void this.closeFlowIfUnowned(endpoint.flowHandle).catch(
          () => undefined,
        );
        throw new YasDisconnectedError(
          "Net OPEN completed after client disposal or family invalidation",
        );
      }
      // Never close a reused handle: doing so could destroy the pre-existing
      // flow the client still owns.
      if (this.flows.has(endpoint.flowHandle))
        throw new YasProtocolError("Net flow handle was reused");
      if (operation.retainPayload && !operation.flow) {
        void this.closeFlowIfUnowned(endpoint.flowHandle).catch(
          () => undefined,
        );
        throw new YasProtocolError(
          "Net OPEN replayed a retired flow instead of STALE",
        );
      }
      let transfer: YasTransfer | undefined;
      try {
        if (endpoint.descriptor) {
          if (!lease)
            throw new YasProtocolError("Net returned an unbudgeted Transfer");
          if (endpoint.direction & g.YAS_NET_DIRECTION_PEER_TO_CLIENT) {
            transfer = this.transfers.acceptServerDescriptor(
              endpoint.descriptor,
              lease,
            );
            leaseOwned = false;
          } else {
            lease.release();
            leaseOwned = false;
            transfer = this.transfers.acceptServerUploadDescriptor(
              endpoint.descriptor,
            );
          }
        } else if (lease) {
          lease.release();
          leaseOwned = false;
        }
        const flow = new YasNetFlow(this, endpoint, transfer);
        this.flows.set(endpoint.flowHandle, flow);
        this.flowOperationKeys.set(flow, operationKey);
        operation.flow = flow;
        operation.retainPayload = true;
        if (!this.retainOpenOperation(operationKey, operation)) {
          this.flows.delete(endpoint.flowHandle);
          operation.flow = null;
          try {
            transfer?.reset();
          } catch {
            // Failed admission still retires newly accepted authority.
          }
          void this.closeFlowIfUnowned(endpoint.flowHandle).catch(
            () => undefined,
          );
          throw new YasProtocolError("Net OPEN replay ledger overflowed");
        }
        if (transfer)
          void transfer.closed.then(
            () => this.releaseFlow(endpoint.flowHandle, flow),
            () => this.releaseFlow(endpoint.flowHandle, flow),
          );
        return flow;
      } catch (error) {
        try {
          transfer?.reset();
        } catch {
          // A failed install may race family teardown.
        }
        throw error;
      }
    };
    try {
      const result = await this.connection.requestDecoded(
        g.YAS_FAMILY_NET,
        g.YAS_NET_OPEN,
        payload,
        (body) => install(decodeNetEndpoint(body)),
      );
      return result instanceof YasNetFlow
        ? result
        : install(result as YasNetEndpoint);
    } catch (error) {
      if (leaseOwned) lease?.release();
      throw error;
    }
  }

  async closeFlow(
    flowHandle: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    const owned = this.flows.get(flowHandle);
    if (owned) {
      this.tombstoneFlow(owned);
      this.flows.delete(flowHandle);
    }
    try {
      await this.requestCloseFlow(flowHandle, operationId, extensions);
    } finally {
      if (owned) {
        if (this.flows.get(flowHandle) === owned) this.flows.delete(flowHandle);
        try {
          owned.transfer?.reset();
        } catch {
          // The flow still retires coherently when remote CLOSE fails.
        }
        owned.invalidate();
        this.tombstoneFlow(owned);
      }
    }
  }

  releaseFlow(flowHandle: bigint, flow: YasNetFlow): void {
    if (this.flows.get(flowHandle) !== flow) return;
    this.flows.delete(flowHandle);
    this.tombstoneFlow(flow);
    flow.invalidate();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    const error = new YasDisconnectedError("YAS Net client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    for (const remove of this.removeListeners) remove();
    this.removeListeners = [];
    for (const flow of this.flows.values()) {
      this.tombstoneFlow(flow);
      void this.requestCloseFlow(flow.endpoint.flowHandle).catch(
        () => undefined,
      );
      try {
        flow.transfer?.reset();
      } catch {
        // Transfer teardown is best-effort after transport loss.
      }
      flow.invalidate();
    }
    this.flows.clear();
    this.retirePendingOpenOperations();
    this.openOperations.clear();
    this.pendingOpenOperations.clear();
  }

  private invalidate(): void {
    this.epoch++;
    const error = new YasDisconnectedError("YAS Net client was invalidated");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    for (const flow of this.flows.values()) {
      this.tombstoneFlow(flow);
      try {
        flow.transfer?.reset();
      } catch {
        // The Transfer registry may already have been invalidated.
      }
      flow.invalidate();
    }
    this.flows.clear();
    this.retirePendingOpenOperations();
  }

  private tombstoneFlow(flow: YasNetFlow): void {
    const operationKey = this.flowOperationKeys.get(flow);
    if (!operationKey) return;
    const operation = this.openOperations.get(operationKey);
    if (operation?.flow !== flow) return;
    operation.flow = null;
    operation.retainPayload = true;
  }

  private retirePendingOpenOperations(): void {
    for (const operation of this.openOperations.values()) {
      if (!operation.pending) continue;
      operation.pending = null;
      operation.flow = null;
      operation.retainPayload = true;
    }
    for (const [operationKey, operation] of this.pendingOpenOperations) {
      operation.pending = null;
      operation.flow = null;
      operation.retainPayload = true;
      this.retainOpenOperation(operationKey, operation);
    }
    this.pendingOpenOperations.clear();
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

  private requestCloseFlow(
    flowHandle: bigint,
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<Uint8Array> {
    return this.connection.request(
      g.YAS_FAMILY_NET,
      g.YAS_NET_CLOSE,
      encodeNetClose(flowHandle, operationId, extensions),
    );
  }

  private closeFlowIfUnowned(flowHandle: bigint): Promise<void> {
    if (this.flows.has(flowHandle)) return Promise.resolve();
    return this.requestCloseFlow(flowHandle).then(() => undefined);
  }

  private ensureOpenReplaySlot(operationKey: string): void {
    let pinned = 0;
    for (const [key, operation] of this.openOperations) {
      if (key === operationKey) continue;
      if (operation.pending || operation.flow) pinned++;
    }
    for (const key of this.pendingOpenOperations.keys())
      if (key !== operationKey) pinned++;
    if (pinned + 1 > this.openReplayLimit())
      throw new YasResultError(
        g.YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "Net OPEN replay ledger is full",
      );
  }

  private retainOpenOperation(
    operationKey: string,
    operation: YasNetOpenOperation,
  ): boolean {
    if (this.openOperations.get(operationKey) === operation) return true;
    const limit = this.openReplayLimit();
    let needed = this.openOperations.size - limit + 1;
    for (const [key, operation] of this.openOperations) {
      if (needed <= 0) break;
      if (!operation.pending && !operation.flow && operation.retainPayload) {
        this.openOperations.delete(key);
        needed--;
      }
    }
    if (needed > 0) return false;
    this.pendingOpenOperations.delete(operationKey);
    this.openOperations.set(operationKey, operation);
    return true;
  }

  private openReplayLimit(): number {
    const extension = this.connection
      .family(g.YAS_FAMILY_NET, g.YAS_NET_VERSION)
      .limits.find(
        (candidate) => candidate.tag === g.YAS_NET_LIMIT_MAX_MUTATION_REPLAYS,
      );
    if (!extension)
      throw new YasProtocolError(
        "required Net mutation replay limit is absent",
      );
    const cursor = new YasCursor(extension.value);
    const value = cursor.u32("Net mutation replay limit");
    cursor.end("Net mutation replay limit");
    if (value === 0 || value > g.YAS_NET_MAX_MUTATION_REPLAYS)
      throw new YasProtocolError("invalid Net mutation replay limit");
    return value;
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Net client is disposed");
  }
}

function validateAddress(value: YasNetAddress): void {
  if (value.kind === "tcp" || value.kind === "udp") {
    const length = utf8Length(value.host);
    if (
      length === 0 ||
      length > g.YAS_NET_MAX_HOST_BYTES ||
      value.host.includes("\0") ||
      value.port === 0
    )
      throw new YasProtocolError("invalid Net host or port");
  } else if (value.kind === "windows-pipe") {
    const length = utf8Length(value.name);
    if (
      value.requestedMode > g.YAS_NET_PIPE_MODE_MESSAGE ||
      length === 0 ||
      length > g.YAS_NET_MAX_PIPE_NAME_BYTES ||
      value.name.includes("\0")
    )
      throw new YasProtocolError("invalid Net Windows pipe address");
  } else if (
    (value.nameKind !== g.YAS_NET_UNIX_FILESYSTEM &&
      value.nameKind !== g.YAS_NET_UNIX_ABSTRACT) ||
    value.name.length === 0 ||
    value.name.length > g.YAS_NET_MAX_LOCAL_ADDRESS_BYTES ||
    (value.nameKind === g.YAS_NET_UNIX_FILESYSTEM && value.name.includes(0))
  )
    throw new YasProtocolError("invalid Net Unix address");
}

function validateTls(value: YasNetTlsOptions): void {
  if (
    value.verification > g.YAS_NET_TLS_VERIFY_INSECURE ||
    utf8Length(value.sni) > g.YAS_NET_MAX_HOST_BYTES ||
    value.sni.includes("\0") ||
    value.alpn.length > g.YAS_NET_MAX_ALPN_PROTOCOLS
  )
    throw new YasProtocolError("invalid Net TLS options");
  const protocols = new Set<string>();
  for (const protocol of value.alpn) {
    const key = byteKey(protocol);
    if (
      protocol.length === 0 ||
      protocol.length > g.YAS_NET_MAX_ALPN_BYTES ||
      protocols.has(key)
    )
      throw new YasProtocolError("invalid Net TLS ALPN protocol");
    protocols.add(key);
  }
}

function validateOpen(value: YasNetOpen): void {
  requireOperationId(value.operationId);
  validateAddress(value.address);
  const earlyData = value.earlyData ?? new Uint8Array(0);
  if (earlyData.length > g.YAS_NET_MAX_EARLY_DATA_BYTES)
    throw new YasProtocolError("Net early data exceeds its limit");
  if (isDatagramAddress(value.address)) {
    if (
      value.deliveryPreference ===
        g.YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE ||
      value.deliveryPreference > g.YAS_NET_DELIVERY_REQUIRE_RELIABLE_TUNNEL ||
      value.dropPolicy === g.YAS_NET_DROP_NOT_APPLICABLE ||
      value.dropPolicy > g.YAS_NET_DROP_LATEST ||
      value.initialReceiveCredit !== 0n ||
      earlyData.length !== 0 ||
      value.tls !== undefined
    )
      throw new YasProtocolError("invalid Net datagram OPEN options");
  } else if (
    value.deliveryPreference !== g.YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE ||
    value.dropPolicy !== g.YAS_NET_DROP_NOT_APPLICABLE ||
    value.initialReceiveCredit === 0n
  )
    throw new YasProtocolError("invalid Net reliable OPEN options");
  if (value.tls) {
    if (value.address.kind !== "tcp")
      throw new YasProtocolError("Net TLS is only valid for TCP");
    validateTls(value.tls);
  }
}

function validateEndpoint(value: YasNetEndpoint): void {
  requireHandle(value.flowHandle, "Net flow handle");
  validateAddress(value.peerAddress);
  if (value.localAddress) validateAddress(value.localAddress);
  if (
    value.mode > g.YAS_NET_MODE_DATAGRAM ||
    value.direction === 0 ||
    value.direction & ~g.YAS_NET_DIRECTION_DUPLEX ||
    value.selectedDelivery > g.YAS_NET_DELIVERY_RELIABLE_TUNNEL ||
    value.negotiatedAlpn.length > g.YAS_NET_MAX_ALPN_BYTES
  )
    throw new YasProtocolError("invalid Net endpoint enums");
  if (value.mode === g.YAS_NET_MODE_DATAGRAM) {
    if (
      !isDatagramAddress(value.peerAddress) ||
      value.descriptor !== undefined ||
      value.selectedDelivery === g.YAS_NET_DELIVERY_NOT_APPLICABLE ||
      value.maxDatagramPayload === 0 ||
      value.maxDatagramPayload > g.YAS_NET_MAX_DATAGRAM_PAYLOAD ||
      value.serverInstanceLimit !== 0 ||
      value.maxMessageBytes !== 0n ||
      value.negotiatedAlpn.length !== 0
    )
      throw new YasProtocolError("invalid Net datagram endpoint");
  } else {
    if (
      isDatagramAddress(value.peerAddress) ||
      value.selectedDelivery !== g.YAS_NET_DELIVERY_NOT_APPLICABLE ||
      value.maxDatagramPayload !== 0 ||
      !value.descriptor
    )
      throw new YasProtocolError("invalid Net reliable endpoint");
    validateFlowDescriptor(value.descriptor, value.mode, value.direction);
    if (
      (value.mode === g.YAS_NET_MODE_BYTE && value.maxMessageBytes !== 0n) ||
      (value.mode === g.YAS_NET_MODE_MESSAGE &&
        (value.maxMessageBytes === 0n ||
          value.descriptor.maxItemBytes !== value.maxMessageBytes)) ||
      (value.peerAddress.kind !== "tcp" && value.negotiatedAlpn.length !== 0)
    )
      throw new YasProtocolError("invalid Net reliable endpoint metadata");
  }
}

function validateFlowDescriptor(
  value: YasTransferDescriptor,
  mode: number,
  direction: number,
): void {
  const transferMode =
    mode === g.YAS_NET_MODE_BYTE
      ? YAS_TRANSFER_MODE_BYTE
      : YAS_TRANSFER_MODE_MESSAGE;
  if (
    value.mode !== transferMode ||
    value.direction !== direction ||
    value.contentFamily !== g.YAS_FAMILY_NET ||
    value.contentKind !== g.YAS_NET_FLOW_CONTENT_KIND ||
    value.contentVersion !== g.YAS_NET_VERSION ||
    !value.sensitiveContent ||
    value.maxChunkBytes > g.YAS_NET_MAX_BUFFERED_PER_FLOW
  )
    throw new YasProtocolError("invalid Net flow Transfer descriptor");
}

function validateDatagram(value: YasNetDatagram): void {
  requireHandle(value.flowHandle, "Net flow handle");
  if (value.payload.length > g.YAS_NET_MAX_DATAGRAM_PAYLOAD)
    throw new YasProtocolError("Net datagram exceeds the hard maximum");
}

function validateStats(value: YasNetDatagramStats): void {
  requireHandle(value.flowHandle, "Net flow handle");
  if (value.revision === 0n)
    throw new YasProtocolError("Net datagram-stats revision is zero");
}

function decodeDescriptor(bytes: Uint8Array): YasTransferDescriptor {
  const cursor = new YasCursor(bytes);
  const value = decodeTransferDescriptor(cursor);
  cursor.end("Net Transfer descriptor");
  return value;
}

function isDatagramAddress(value: YasNetAddress): boolean {
  return value.kind === "udp" || value.kind === "unix-datagram";
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireOperationId(value: Uint8Array): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError("Net operation ID is invalid");
}

function requireZero(bytes: Uint8Array, context: string): void {
  if (bytes.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

function byteKey(value: Uint8Array): string {
  let output = "";
  for (const byte of value) output += String.fromCharCode(byte);
  return output;
}

function randomOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  crypto.getRandomValues(value);
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}
