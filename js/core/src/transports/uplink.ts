import type { YasTransportOptions } from "../types";
import { UplinkEvents, YasNoiseTransport } from "./noise";
import { decodeKey, publicKey } from "./uplink-crypto";

interface UplinkTarget {
  attach: string;
  token: string;
  server: string;
  identity: string;
}
/** Parse secrets locally; only the routing token is sent to the control plane. */
export function parseUplinkUri(uri: string, identity?: string): UplinkTarget {
  let url: URL;
  try {
    url = new URL(uri.startsWith("uplink:") ? uri.slice(7) : uri);
  } catch {
    throw new Error("invalid uplink control URL");
  }
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    url.search ||
    !url.hash
  )
    throw new Error(
      "uplink control URL requires HTTPS, a key fragment, and no userinfo or query",
    );
  const fields = new Map<string, string>();
  for (const [key, value] of new URLSearchParams(url.hash.slice(1))) {
    if (
      !["token", "server", "identity"].includes(key) ||
      fields.has(key) ||
      !value
    )
      throw new Error("unknown, duplicate, or empty uplink fragment field");
    fields.set(key, value);
  }
  const token = fields.get("token"),
    server = fields.get("server"),
    privateKey = fields.get("identity") ?? identity;
  if (!token || !token.trim() || /[\x00-\x1f\x7f]/.test(token))
    throw new Error("uplink routing token is missing or invalid");
  if (!server || !privateKey)
    throw new Error(
      "uplink requires a pinned server key and a private identity",
    );
  publicKey(server);
  decodeKey(privateKey).fill(0);
  url.hash = "";
  url.pathname = url.pathname.replace(/\/+$/, "") + "/attach";
  return { attach: url.href, token, server, identity: privateKey };
}

/** Direct browser consumer. The private key is supplied separately and retained
 * only in memory. The control endpoint must permit CORS from the trusted UI origin. */
export class YasUplinkTransport extends YasNoiseTransport {
  constructor(
    uri: string,
    identity?: string,
    options: YasTransportOptions = {},
  ) {
    const target = parseUplinkUri(uri, identity);
    super(
      new UplinkWebSocketCarrier(target.attach, target.token, options),
      target.identity,
      target.server,
      options,
    );
  }
}

/** Routing-only carrier. Noise owns retries, including routing failures, so a
 * fresh carrier cannot overtake decryption of the previous session's tail. */
class UplinkWebSocketCarrier extends UplinkEvents {
  private ws: WebSocket | null = null;
  private attempt: AbortController | null = null;
  private disposed = false;
  constructor(
    private readonly attach: string,
    private readonly token: string,
    private readonly options: YasTransportOptions,
  ) {
    super();
  }
  get bufferedAmount(): number {
    return this.ws?.bufferedAmount ?? 0;
  }
  connect(): void {
    if (this.disposed || this.attempt || this.ws) return;
    const attempt = new AbortController();
    this.attempt = attempt;
    this.setStatus("connecting");
    void this.open(attempt);
  }
  send(data: Uint8Array): void {
    if (!this.ws || this.status !== "connected")
      throw new Error("uplink carrier is not connected");
    // Match the native opaque carrier's 16 KiB outgoing chunks.
    for (let offset = 0; offset < data.length; offset += 16384)
      this.ws.send(data.slice(offset, offset + 16384));
  }
  reconnect(): void {
    this.suspend();
    this.authRejected = false;
    this.connect();
  }
  suspend(): void {
    this.cleanup();
    this.setStatus("disconnected");
  }
  close(): void {
    this.disposed = true;
    this.cleanup();
    this.setStatus("closed");
  }
  private cleanup(): void {
    this.attempt?.abort();
    this.attempt = null;
    const ws = this.ws;
    this.ws = null;
    if (ws) {
      ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null;
      ws.close();
    }
  }
  private failed(attempt: AbortController, rejected = false): void {
    if (this.attempt !== attempt) return;
    this.lastError = rejected
      ? "uplink routing token rejected"
      : "uplink carrier connection failed";
    this.authRejected = rejected;
    this.cleanup();
    this.setStatus(rejected ? "error" : "disconnected");
  }
  private async open(attempt: AbortController): Promise<void> {
    const timeout = setTimeout(
      () => this.failed(attempt),
      this.options.connectTimeoutMs ?? 10000,
    );
    try {
      const response = await fetch(this.attach, {
        headers: {
          authorization: `Bearer ${this.token}`,
          accept: "application/json",
        },
        redirect: "error",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        signal: attempt.signal,
      });
      if (response.status === 401 || response.status === 403) {
        this.failed(attempt, true);
        return;
      }
      if (!response.ok) throw new Error("attach failed");
      // The control plane is untrusted; bound its response before parsing it.
      const reader = response.body?.getReader();
      if (!reader) throw new Error("missing attach response");
      let text = "",
        size = 0;
      const decoder = new TextDecoder();
      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          size += value.length;
          if (size > 65536) throw new Error("oversized attach response");
          text += decoder.decode(value, { stream: true });
        }
        text += decoder.decode();
      } finally {
        await reader.cancel().catch(() => {});
        reader.releaseLock();
      }
      const body: unknown = JSON.parse(text);
      if (
        !body ||
        typeof body !== "object" ||
        !("ws" in body) ||
        typeof body.ws !== "string"
      )
        throw new Error("missing worker URL");
      const worker = new URL(body.ws);
      if (
        worker.protocol !== "wss:" ||
        worker.username ||
        worker.password ||
        worker.hash
      )
        throw new Error("invalid worker URL");
      if (this.attempt !== attempt || attempt.signal.aborted) return;
      const ws = new WebSocket(worker.href);
      this.ws = ws;
      ws.binaryType = "arraybuffer";
      await new Promise<void>((resolve, reject) => {
        const abort = () => reject(new Error("uplink connection aborted"));
        attempt.signal.addEventListener("abort", abort, { once: true });
        ws.onopen = () => {
          this.setStatus("authenticating");
          ws.send(this.token);
        };
        ws.onerror = () => reject(new Error("worker connection failed"));
        ws.onclose = () => reject(new Error("worker connection closed"));
        ws.onmessage = (event) => {
          if (event.data !== "ok") {
            this.authRejected = true;
            reject(new Error("worker authentication failed"));
            return;
          }
          attempt.signal.removeEventListener("abort", abort);
          resolve();
        };
      });
      if (this.attempt !== attempt) return;
      ws.onmessage = (event) => {
        if (this.ws !== ws) return;
        if (
          !(event.data instanceof ArrayBuffer) ||
          event.data.byteLength > 65536
        ) {
          this.failed(attempt);
          return;
        }
        this.emit("message", event.data);
      };
      ws.onclose = () => this.failed(attempt);
      ws.onerror = () => this.failed(attempt);
      this.authRejected = false;
      this.lastError = null;
      this.setStatus("connected");
    } catch {
      this.failed(attempt, this.authRejected);
    } finally {
      clearTimeout(timeout);
    }
  }
}
