/** Browser product facade over the typed YAS Channel family.
 *
 * Resource identities stay as server-issued bigint handles. This module only
 * adapts asynchronous MESSAGE Transfer delivery to the UI callback shape.
 */

import {
  YAS_CHANNEL_MAX_LISTENERS_PER_SESSION,
  YAS_CHANNEL_MAX_NAME_BYTES,
  YAS_FAMILY_CHANNEL,
  YAS_STATUS_CANCELLED,
  YAS_STATUS_INTERNAL,
  YAS_STATUS_INVALID,
  YAS_STATUS_OK,
  YAS_STATUS_UNAVAILABLE,
} from "./generated";
import {
  type YasChannelConnection,
  type YasChannelListenerRecord,
  type YasChannelSnapshot,
  YasChannelClient,
} from "./channel";
import type { YasConnection } from "./session";
import type { YasWatchOptions } from "./state";
import type { YasTransfer } from "./transfer";
import { YasProtocolError, YasResultError } from "./wire";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: false });

export interface YasNativeChannelOpenOptions {
  metadata?: Uint8Array;
  /** Optional exact catalogue identity. When supplied, a listener replacement
   * between observation and CONNECT is rejected rather than retargeted. */
  expectedListener?: Pick<
    YasChannelListenerRecord,
    "listenerHandle" | "generation"
  >;
  onData?(payload: Uint8Array): void;
  onCredit?(available: bigint): void;
  /** `status` is the native YAS status that ended the MESSAGE Transfer. */
  onClosed?(status: number, detail: string): void;
}

export interface YasNativeChannelHandle {
  readonly channelHandle: bigint;
  readonly peerChannelHandle: bigint;
  readonly name: string;
  readonly peerSession: Uint8Array;
  readonly listenerMetadata: Uint8Array;
  readonly connectorMetadata: Uint8Array;
  readonly availableCredit: bigint;
  /** Queue one complete message when bounded local and peer credit permit it. */
  send(payload: Uint8Array | string): boolean;
  /** Half-close normally, or reset when a non-OK status is supplied. */
  close(status?: number): void;
}

export interface YasNativeChannelNamesWatch {
  readonly present: ReadonlySet<string>;
  stop(): void;
}

interface NativeWatch {
  readonly names: ReadonlySet<string>;
  readonly present: Set<string>;
  readonly onNames: (present: ReadonlySet<string>) => void;
  remove: () => void;
  stopped: boolean;
}

interface NativeChannel {
  readonly handle: bigint;
  readonly transfer: YasTransfer;
  readonly options: YasNativeChannelOpenOptions;
  pendingBytes: bigint;
  localClosed: boolean;
  finished: boolean;
  removeCredit: () => void;
}

interface NativeChannelClientLike {
  readonly catalogue: {
    firstSnapshot(
      options?: YasWatchOptions,
      signal?: AbortSignal,
    ): Promise<YasChannelSnapshot>;
    subscribe(listener: (snapshot: YasChannelSnapshot) => void): () => void;
  };
  connect(
    listener: Pick<YasChannelListenerRecord, "listenerHandle" | "generation">,
    options?: { metadata?: Uint8Array; initialReceiveCredit?: bigint },
  ): Promise<YasChannelConnection>;
  dispose?(): void;
}

export class YasNativeChannelFacade {
  private readonly client: NativeChannelClientLike;
  private readonly watches = new Set<NativeWatch>();
  private readonly channels = new Map<bigint, NativeChannel>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private readonly ownsClient: boolean;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    client?: NativeChannelClientLike,
  ) {
    this.ownsClient = client === undefined;
    this.client = client ?? new YasChannelClient(connection);
    this.removeInvalidation = connection.onInvalidation(({ family, error }) => {
      if (family !== undefined && family !== YAS_FAMILY_CHANNEL) return;
      this.invalidate(error);
    });
  }

  connectChannel(
    name: string,
    options: YasNativeChannelOpenOptions = {},
  ): Promise<YasNativeChannelHandle> {
    this.assertOpen();
    validateName(name);
    return this.runOwned((signal) =>
      this.performConnectChannel(name, options, signal),
    );
  }

  private async performConnectChannel(
    name: string,
    options: YasNativeChannelOpenOptions,
    signal: AbortSignal,
  ): Promise<YasNativeChannelHandle> {
    const snapshot = await this.client.catalogue.firstSnapshot({}, signal);
    this.assertOpen();
    const current = snapshot.listeners.find(
      (listener) => listener.name === name,
    );
    if (!current) throw new YasProtocolError(`Channel ${name} has no listener`);
    if (
      options.expectedListener &&
      (options.expectedListener.listenerHandle !== current.listenerHandle ||
        options.expectedListener.generation !== current.generation)
    )
      throw new YasProtocolError(`Channel ${name} listener generation changed`);

    let endpoint: YasChannelConnection;
    try {
      endpoint = await this.client.connect(current, {
        metadata: options.metadata
          ? new Uint8Array(options.metadata)
          : new Uint8Array(),
      });
    } catch (error) {
      throw connectError(name, error);
    }
    if (this.disposed) {
      endpoint.transfer.reset(
        YAS_STATUS_CANCELLED,
        encoder.encode("Channel facade closed during CONNECT"),
      );
      throw new YasProtocolError("native Channel facade is closed");
    }

    const active: NativeChannel = {
      handle: endpoint.channelHandle,
      transfer: endpoint.transfer,
      options,
      pendingBytes: 0n,
      localClosed: false,
      finished: false,
      removeCredit: () => undefined,
    };
    if (this.channels.has(active.handle)) {
      endpoint.transfer.reset(
        YAS_STATUS_INVALID,
        encoder.encode("duplicate local Channel handle"),
      );
      throw new YasProtocolError("server reused a live Channel handle");
    }
    active.removeCredit = endpoint.transfer.subscribeOutgoingCredit(() => {
      if (active.finished || active.localClosed) return;
      try {
        active.options.onCredit?.(this.available(active));
      } catch {
        // An observer cannot affect Transfer sequencing.
      }
    });
    this.channels.set(active.handle, active);
    void this.pump(active);

    const facade = this;
    return {
      channelHandle: endpoint.channelHandle,
      peerChannelHandle: endpoint.peerChannelHandle,
      name,
      peerSession: new Uint8Array(endpoint.peerSession),
      listenerMetadata: new Uint8Array(endpoint.listenerMetadata),
      connectorMetadata: new Uint8Array(endpoint.connectorMetadata),
      get availableCredit() {
        return facade.available(active);
      },
      send(payload: Uint8Array | string): boolean {
        if (active.finished || active.localClosed || facade.disposed)
          return false;
        const bytes =
          typeof payload === "string"
            ? encoder.encode(payload)
            : new Uint8Array(payload);
        if (
          bytes.length === 0 ||
          BigInt(bytes.length) > active.transfer.descriptor.maxItemBytes
        )
          throw new YasProtocolError(
            "Channel message is empty or exceeds the negotiated limit",
          );
        const length = BigInt(bytes.length);
        if (length > facade.available(active)) return false;
        active.pendingBytes += length;
        void active.transfer.sendMessage(bytes).then(
          () => {
            active.pendingBytes -= length;
          },
          (error) => {
            active.pendingBytes -= length;
            facade.finishFromError(active, error);
          },
        );
        return true;
      },
      close(status = YAS_STATUS_OK): void {
        if (active.finished || active.localClosed) return;
        active.localClosed = true;
        if (status === YAS_STATUS_OK) active.transfer.closeWrite();
        else active.transfer.reset(status);
      },
    };
  }

  watchChannelNames(
    names: readonly string[],
    onNames: (present: ReadonlySet<string>) => void,
  ): Promise<YasNativeChannelNamesWatch> {
    this.assertOpen();
    validateWatchNames(names);
    return this.runOwned((signal) =>
      this.performWatchChannelNames(names, onNames, signal),
    );
  }

  private async performWatchChannelNames(
    names: readonly string[],
    onNames: (present: ReadonlySet<string>) => void,
    signal: AbortSignal,
  ): Promise<YasNativeChannelNamesWatch> {
    await this.client.catalogue.firstSnapshot({}, signal);
    this.assertOpen();
    const watch: NativeWatch = {
      names: new Set(names),
      present: new Set(),
      onNames,
      remove: () => undefined,
      stopped: false,
    };
    let initial = true;
    watch.remove = this.client.catalogue.subscribe((snapshot) => {
      if (watch.stopped) return;
      const next = presentNames(watch.names, snapshot);
      const changed = !sameSet(watch.present, next);
      replaceSet(watch.present, next);
      if (initial) {
        initial = false;
        return;
      }
      if (changed) {
        try {
          watch.onNames(watch.present);
        } catch {
          // One observer cannot fail State delivery for sibling watches.
        }
      }
    });
    this.watches.add(watch);
    return {
      present: watch.present,
      stop: () => this.stopWatch(watch),
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const error = new YasProtocolError("native YAS Channel facade is closed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.removeInvalidation();
    this.invalidate(new Error("native YAS Channel session closed"), false);
    if (this.ownsClient) this.client.dispose?.();
  }

  private async pump(active: NativeChannel): Promise<void> {
    try {
      while (!active.finished) {
        const message = await active.transfer.readMessage();
        if (message === null) {
          if (!active.localClosed) active.transfer.closeWrite();
          this.finish(active, YAS_STATUS_OK, "", !active.localClosed);
          return;
        }
        if (active.finished || active.localClosed) continue;
        try {
          active.options.onData?.(new Uint8Array(message));
        } catch (error) {
          const detail = error instanceof Error ? error.message : String(error);
          active.transfer.reset(
            YAS_STATUS_CANCELLED,
            encoder.encode("Channel data handler failed"),
          );
          this.finish(active, YAS_STATUS_CANCELLED, detail, true);
          return;
        }
      }
    } catch (error) {
      this.finishFromError(active, error);
    }
  }

  private available(active: NativeChannel): bigint {
    if (active.finished || active.localClosed) return 0n;
    const native = active.transfer.outgoingCreditOutstanding;
    return native > active.pendingBytes ? native - active.pendingBytes : 0n;
  }

  private finishFromError(active: NativeChannel, error: unknown): void {
    if (active.finished) return;
    if (error instanceof YasResultError) {
      this.finish(
        active,
        error.status,
        decoder.decode(error.detail),
        !active.localClosed,
      );
      return;
    }
    this.finish(
      active,
      YAS_STATUS_UNAVAILABLE,
      error instanceof Error ? error.message : String(error),
      !active.localClosed,
    );
  }

  private finish(
    active: NativeChannel,
    status: number,
    detail: string,
    notify: boolean,
  ): void {
    if (active.finished) return;
    active.finished = true;
    active.removeCredit();
    this.channels.delete(active.handle);
    if (notify) {
      try {
        active.options.onClosed?.(status, detail);
      } catch {
        // One observer cannot retain or block cleanup of other channels.
      }
    }
  }

  private stopWatch(watch: NativeWatch): void {
    if (watch.stopped) return;
    watch.stopped = true;
    watch.remove();
    this.watches.delete(watch);
  }

  private invalidate(error: Error, notify = true): void {
    for (const watch of [...this.watches]) {
      this.stopWatch(watch);
      watch.present.clear();
      if (notify) {
        try {
          watch.onNames(watch.present);
        } catch {
          // One observer cannot retain or block cleanup of other watches.
        }
      }
    }
    for (const active of [...this.channels.values()]) {
      this.finish(
        active,
        YAS_STATUS_UNAVAILABLE,
        error.message,
        notify && !active.localClosed,
      );
      try {
        active.transfer.reset(
          YAS_STATUS_CANCELLED,
          encoder.encode(error.message),
        );
      } catch {
        // The physical session may disappear between ready and send.
      }
    }
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("native YAS Channel facade is closed");
  }

  private runOwned<T>(
    operation: (signal: AbortSignal) => Promise<T>,
  ): Promise<T> {
    const controller = new AbortController();
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = (error) => {
        controller.abort(error);
        reject(error);
      };
    });
    this.pendingCancels.add(cancel);
    const running = operation(controller.signal);
    return Promise.race([running, cancelled]).finally(() => {
      this.pendingCancels.delete(cancel);
    });
  }
}

function validateName(name: string): void {
  const bytes = encoder.encode(name);
  if (
    bytes.length === 0 ||
    bytes.length > YAS_CHANNEL_MAX_NAME_BYTES ||
    name.includes("\0")
  )
    throw new YasProtocolError("Channel name is invalid");
}

function validateWatchNames(names: readonly string[]): void {
  if (
    names.length === 0 ||
    names.length > YAS_CHANNEL_MAX_LISTENERS_PER_SESSION ||
    new Set(names).size !== names.length
  )
    throw new YasProtocolError("Channel watch name set is invalid");
  for (const name of names) validateName(name);
}

function presentNames(
  wanted: ReadonlySet<string>,
  snapshot: YasChannelSnapshot,
): Set<string> {
  return new Set(
    snapshot.listeners
      .map((listener) => listener.name)
      .filter((name) => wanted.has(name)),
  );
}

function replaceSet(target: Set<string>, source: ReadonlySet<string>): void {
  target.clear();
  for (const value of source) target.add(value);
}

function sameSet(
  left: ReadonlySet<string>,
  right: ReadonlySet<string>,
): boolean {
  if (left.size !== right.size) return false;
  for (const value of left) if (!right.has(value)) return false;
  return true;
}

function connectError(name: string, error: unknown): Error {
  if (error instanceof YasResultError) {
    const detail = decoder.decode(error.detail);
    return new YasProtocolError(
      `Channel ${name} refused${detail ? `: ${detail}` : ""}`,
    );
  }
  return error instanceof Error ? error : new Error(String(error));
}

/** Preserve distinct shutdown failures when callers surface the native status. */
export function yasNativeChannelStatusIsServerLoss(status: number): boolean {
  return status === YAS_STATUS_UNAVAILABLE || status === YAS_STATUS_INTERNAL;
}
