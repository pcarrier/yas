/**
 * Private App ↔ preview-service-worker socket bridge.
 *
 * The App owns the authenticated home/Relay YAS sessions. The worker only
 * receives a demand-driven byte-stream MessagePort, so it never learns a
 * passphrase and never opens an Edge route of its own.
 */

import {
  YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
  YAS_NET_DIRECTION_DUPLEX,
  YAS_NET_DROP_NOT_APPLICABLE,
  YAS_NET_MAX_BUFFERED_PER_FLOW,
  YAS_NET_MODE_BYTE,
  YAS_NET_TLS_VERIFY_INSECURE,
  YAS_NET_TLS_VERIFY_STRICT,
  type YasNetClient,
  type YasNetFlow,
  type YasNetOpen,
} from "@yas-run/core";

export const PREVIEW_NET_MAX_PENDING_WRITES = 64;
export const PREVIEW_NET_MAX_PENDING_WRITE_BYTES =
  YAS_NET_MAX_BUFFERED_PER_FLOW;

/*
 * The same cap bounds the service worker's HTTP request materialization. It
 * matches the YAS Net family hard per-flow buffer limit, so a preview cannot
 * allocate more application data than its eventual native flow may retain.
 */
export const PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES =
  YAS_NET_MAX_BUFFERED_PER_FLOW;
/** A response head is materialized before HTTP parsing in the worker. */
export const PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES = 256 * 1024;

export const PREVIEW_NET_OPEN = "yas-preview-net-open" as const;
export const PREVIEW_NET_ACCEPTED = "yas-preview-net-accepted" as const;
export const PREVIEW_NET_OPENED = "yas-preview-net-opened" as const;
export const PREVIEW_NET_ERROR = "yas-preview-net-error" as const;
export const PREVIEW_NET_WRITE = "yas-preview-net-write" as const;
export const PREVIEW_NET_WRITE_OK = "yas-preview-net-write-ok" as const;
export const PREVIEW_NET_READ = "yas-preview-net-read" as const;
export const PREVIEW_NET_DATA = "yas-preview-net-data" as const;
export const PREVIEW_NET_END = "yas-preview-net-end" as const;
export const PREVIEW_NET_SHUTDOWN_WRITE =
  "yas-preview-net-shutdown-write" as const;
export const PREVIEW_NET_CLOSE = "yas-preview-net-close" as const;

export interface PreviewNetOpenMessage {
  type: typeof PREVIEW_NET_OPEN;
  dest: string;
  host: string;
  port: number;
  options: PreviewNetOpenOptions;
}

export interface PreviewNetOpenOptions {
  tls?: boolean;
  insecure?: boolean;
  sni?: string;
  alpn?: readonly string[];
  /** Datagram previews are intentionally unsupported by the HTTP broker. */
  udp?: boolean;
}

export type PreviewNetWorkerMessage =
  | { type: typeof PREVIEW_NET_WRITE; id: number; data: ArrayBuffer }
  | { type: typeof PREVIEW_NET_READ }
  | { type: typeof PREVIEW_NET_SHUTDOWN_WRITE }
  | { type: typeof PREVIEW_NET_CLOSE };

export type PreviewNetAppMessage =
  | { type: typeof PREVIEW_NET_ACCEPTED }
  | { type: typeof PREVIEW_NET_OPENED; alpn: string }
  | { type: typeof PREVIEW_NET_ERROR; detail: string; id?: number }
  | { type: typeof PREVIEW_NET_WRITE_OK; id: number }
  | { type: typeof PREVIEW_NET_DATA; data: ArrayBuffer }
  | { type: typeof PREVIEW_NET_END };

/** Structural native Net seam; `connection.native.net` satisfies it directly. */
export interface PreviewNetClient {
  open: YasNetClient["open"];
}

/**
 * Serve worker socket requests from the App's current connection catalogue.
 * The returned cleanup closes every stream created by this registration.
 */
export function installPreviewNetBroker(
  resolve: (dest: string) => PreviewNetClient | null,
): () => void {
  if (typeof navigator === "undefined" || !("serviceWorker" in navigator))
    return () => {};
  const sessions = new Set<() => void>();
  const onMessage = (event: MessageEvent) => {
    const value = validateOpenMessage(event.data);
    const port = event.ports[0];
    if (!value || !port) return;
    // A prompt acceptance lets a restarted worker distinguish the live App
    // from stale controlled windows before it waits on a remote connection.
    port.postMessage({
      type: PREVIEW_NET_ACCEPTED,
    } satisfies PreviewNetAppMessage);
    const net = resolve(value.dest);
    if (!net) {
      postError(port, `unknown preview destination ${value.dest}`);
      port.close();
      return;
    }
    void servePreviewNetStream(net, value, port, sessions);
  };
  navigator.serviceWorker.addEventListener("message", onMessage);
  return () => {
    navigator.serviceWorker.removeEventListener("message", onMessage);
    for (const close of [...sessions]) close();
    sessions.clear();
  };
}

async function servePreviewNetStream(
  net: PreviewNetClient,
  request: PreviewNetOpenMessage,
  port: MessagePort,
  sessions: Set<() => void>,
): Promise<void> {
  let flow: YasNetFlow;
  try {
    flow = await net.open(nativeOpen(request));
    validateByteFlow(flow);
    port.postMessage({
      type: PREVIEW_NET_OPENED,
      alpn: new TextDecoder().decode(flow.endpoint.negotiatedAlpn),
    } satisfies PreviewNetAppMessage);
  } catch (error) {
    postError(port, message(error));
    port.close();
    return;
  }

  const transfer = flow.transfer!;
  const source = readFlow(flow);
  let ended = false;
  let reading = false;
  let writeTail: Promise<void> = Promise.resolve();
  let pendingWrites = 0;
  let pendingWriteBytes = 0;
  const pendingWriteIds = new Set<number>();
  const close = () => {
    if (ended) return;
    ended = true;
    sessions.delete(close);
    void flow.close().catch(() => {});
    void source.return(undefined).catch(() => {});
    port.close();
  };
  sessions.add(close);

  port.onmessage = (event) => {
    if (ended) return;
    const value = event.data as PreviewNetWorkerMessage | null;
    if (!value || typeof value.type !== "string") return;
    if (value.type === PREVIEW_NET_WRITE) {
      const bytes = messageBytes(value.data);
      if (
        !Number.isSafeInteger(value.id) ||
        value.id <= 0 ||
        !bytes ||
        pendingWriteIds.has(value.id)
      ) {
        postError(port, "invalid preview Net write");
        close();
        return;
      }
      const id = value.id;
      if (
        pendingWrites >= PREVIEW_NET_MAX_PENDING_WRITES ||
        bytes.length > PREVIEW_NET_MAX_PENDING_WRITE_BYTES - pendingWriteBytes
      ) {
        postError(port, "preview Net write queue limit exceeded", id);
        close();
        return;
      }
      pendingWrites++;
      pendingWriteBytes += bytes.length;
      pendingWriteIds.add(id);
      writeTail = writeTail
        .then(async () => {
          try {
            if (ended) return;
            await transfer.write(bytes);
            if (!ended)
              port.postMessage({
                type: PREVIEW_NET_WRITE_OK,
                id,
              } satisfies PreviewNetAppMessage);
          } finally {
            pendingWrites--;
            pendingWriteBytes -= bytes.length;
            pendingWriteIds.delete(id);
          }
        })
        .catch((error) => {
          if (!ended) postError(port, message(error), id);
          close();
        });
      return;
    }
    if (value.type === PREVIEW_NET_READ) {
      if (reading) {
        postError(port, "concurrent preview Net reads");
        close();
        return;
      }
      reading = true;
      void source
        .next()
        .then((item) => {
          reading = false;
          if (ended) return;
          if (item.done) {
            port.postMessage({
              type: PREVIEW_NET_END,
            } satisfies PreviewNetAppMessage);
            close();
            return;
          }
          const copy = new Uint8Array(item.value);
          port.postMessage(
            {
              type: PREVIEW_NET_DATA,
              data: copy.buffer,
            } satisfies PreviewNetAppMessage,
            [copy.buffer],
          );
        })
        .catch((error) => {
          reading = false;
          if (!ended) postError(port, message(error));
          close();
        });
      return;
    }
    if (value.type === PREVIEW_NET_SHUTDOWN_WRITE) {
      transfer.closeWrite();
      return;
    }
    if (value.type === PREVIEW_NET_CLOSE) close();
  };
  port.onmessageerror = () => close();
  port.start();
}

function nativeOpen(
  request: PreviewNetOpenMessage,
): Omit<YasNetOpen, "initialReceiveCredit"> {
  const options = request.options;
  return {
    operationId: operationId(),
    address: { kind: "tcp", host: request.host, port: request.port },
    deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
    dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
    ...(options.tls
      ? {
          tls: {
            verification: options.insecure
              ? YAS_NET_TLS_VERIFY_INSECURE
              : YAS_NET_TLS_VERIFY_STRICT,
            sni: options.sni ?? request.host,
            alpn: (options.alpn ?? []).map((value) =>
              new TextEncoder().encode(value),
            ),
          },
        }
      : {}),
  };
}

function validateByteFlow(flow: YasNetFlow): void {
  if (
    flow.endpoint.mode === YAS_NET_MODE_BYTE &&
    flow.endpoint.direction === YAS_NET_DIRECTION_DUPLEX &&
    flow.transfer
  )
    return;
  void flow.close().catch(() => {});
  throw new Error("preview Net server selected an incompatible flow");
}

async function* readFlow(flow: YasNetFlow): AsyncGenerator<Uint8Array> {
  const transfer = flow.transfer;
  if (!transfer) return;
  for (;;) {
    const chunk = await transfer.read();
    if (chunk === null) return;
    yield new Uint8Array(chunk);
  }
}

function operationId(): Uint8Array {
  const crypto = globalThis.crypto;
  if (!crypto?.getRandomValues)
    throw new Error("secure randomness is required for preview Net OPEN");
  const value = crypto.getRandomValues(new Uint8Array(16));
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}

function validateOpenMessage(value: unknown): PreviewNetOpenMessage | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<PreviewNetOpenMessage>;
  if (
    candidate.type !== PREVIEW_NET_OPEN ||
    typeof candidate.dest !== "string" ||
    candidate.dest.length === 0 ||
    typeof candidate.host !== "string" ||
    candidate.host.length === 0 ||
    !Number.isInteger(candidate.port) ||
    candidate.port! <= 0 ||
    candidate.port! > 0xffff ||
    !candidate.options ||
    typeof candidate.options !== "object" ||
    candidate.options.udp === true
  )
    return null;
  return candidate as PreviewNetOpenMessage;
}

function postError(port: MessagePort, detail: string, id?: number): void {
  try {
    port.postMessage({
      type: PREVIEW_NET_ERROR,
      detail,
      id,
    } satisfies PreviewNetAppMessage);
  } catch {
    // The worker already abandoned its side.
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function messageBytes(value: unknown): Uint8Array | null {
  if (ArrayBuffer.isView(value))
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (Object.prototype.toString.call(value) === "[object ArrayBuffer]")
    return new Uint8Array(value as ArrayBuffer);
  return null;
}
