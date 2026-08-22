/** Shims injected into relayed HTML so a previewed app behaves as if it owned its origin (docs/design/net.md). */

import type { PreviewTarget } from "@yas-run/core";

/** Browser-compatible safety bounds for the preview WebSocket shim. */
export const PREVIEW_WS_MAX_HANDSHAKE_BYTES = 64 * 1024;
export const PREVIEW_WS_MAX_FRAME_BYTES = 16 * 1024 * 1024;
export const PREVIEW_WS_MAX_MESSAGE_BYTES = 16 * 1024 * 1024;
export const PREVIEW_WS_MAX_FRAGMENTS = 1024;
export const PREVIEW_WS_RELAY_CLOSE_GRACE_MS = 1000;

/**
 * This is a compatibility layer, **not** a security boundary. A page can delete
 * these shims, or make a fresh `about:blank` frame to get clean globals and the
 * real `top`. It exists so an app that expects to own its origin works; it does
 * not contain a hostile one. Same-origin previewing is for your own dev server.
 */
/**
 * JSON for embedding in an inline `<script>`: `<` escaped, so a value
 * containing `</script>` cannot close the element and leave the rest of the
 * shim to be parsed as markup. Both values interpolated below are
 * page-influenced — the cookie arrives from the dev server's `Set-Cookie` and
 * from the page's own `document.cookie` writes, the host from the frame URL.
 * `\u003c` is `<` to a JS string literal and inert to the HTML parser.
 */
function inlineJson(value: unknown): string {
  return JSON.stringify(value).replace(/</g, "\\u003c");
}

function shimSource(target: PreviewTarget, cookie: string): string {
  return `(() => {
const T = ${inlineJson(target)};
const COOKIE0 = ${inlineJson(cookie)};

// The worker handle, captured before section 5 takes the API away from the
// page. Every shim below reaches the worker through this and never through
// \`navigator\`, so hiding the API from the app cannot cut the shims off from
// it — which is exactly what happened when only the cookie shim was moved
// over: relayed WebSockets, and so every dev server's HMR client, silently
// lost their transport.
const YAS_SW = navigator.serviceWorker;

// 1. Frame-busting. \`top\` and \`parent\` are [Replaceable], so assignment
// sticks; an app that checks whether it is framed now decides it is not, and
// stops navigating what it thinks is the outer page.
try { window.top = window; } catch {}
try { window.parent = window; } catch {}
try {
  Object.defineProperty(window, "frameElement", { get: () => null, configurable: true });
} catch {}

// 2. WebSocket. The handshake never reaches a service worker's fetch handler,
// so the app's socket would otherwise dial the Edge. Framing lives here
// rather than in the worker, which stays a byte pipe.
const NativeWS = window.WebSocket;
function encodeKey() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes));
}
class RelayedWebSocket extends EventTarget {
  static CONNECTING = 0; static OPEN = 1; static CLOSING = 2; static CLOSED = 3;
  CONNECTING = 0; OPEN = 1; CLOSING = 2; CLOSED = 3;
  readyState = 0; bufferedAmount = 0; extensions = ""; protocol = "";
  binaryType = "blob"; url = "";
  onopen = null; onmessage = null; onerror = null; onclose = null;
  #port = null; #handshook = false;
  #rx = new Uint8Array(0); #rxStart = 0; #rxEnd = 0; #headerScan = 0;
  #frag = []; #fragBytes = 0; #fragText = false;
  #keepalive = null; #lastRx = 0; #closeTimer = null; #handshakeTimer = null;
  #relayCloseTimer = null; #relayCloseRequested = false;
  constructor(url, protocols) {
    super();
    const abs = new URL(url, location.href);
    this.url = abs.href;
    // A socket on this frame's own origin belongs to the previewed app: it
    // built the URL from location.host, which inside a preview is the
    // Edge's, not the target's. That is the common case — every dev
    // server's HMR client does it — and without this it would dial yas.
    // A URL naming the target explicitly counts too; anything else is a third
    // party and keeps the native path.
    const ours =
      abs.host === location.host ||
      abs.hostname === T.host ||
      (T.host === "localhost" && abs.hostname === "127.0.0.1") ||
      (T.host === "127.0.0.1" && abs.hostname === "localhost");
    if ((abs.protocol !== "ws:" && abs.protocol !== "wss:") || !ours) {
      return new NativeWS(url, protocols);
    }
    const worker = YAS_SW && YAS_SW.controller;
    if (!worker) { this.#fail("no service worker controls this frame"); return; }
    const channel = new MessageChannel();
    this.#port = channel.port1;
    this.#port.onmessage = (e) => {
      // A control message rather than payload: the relay telling us the socket
      // is gone, which a closed port cannot signal by itself.
      if (e.data && e.data.yasCloseAck) { this.#finishRelayClose(); return; }
      if (e.data && e.data.yasClosed) {
        this.#finishRelayClose();
        this.#died(1006, "relay closed");
        return;
      }
      if (Object.prototype.toString.call(e.data) !== "[object ArrayBuffer]") {
        this.#died(1006, "invalid relay payload");
        return;
      }
      this.#onBytes(new Uint8Array(e.data));
    };
    worker.postMessage({ type: "yas-ws-open", target: T, port: 2 }, [channel.port2]);
    const key = encodeKey();
    const path = abs.pathname + abs.search;
    const authority = T.port === 80 || T.port === 443 ? T.host : T.host + ":" + T.port;
    const lines = [
      "GET " + path + " HTTP/1.1",
      "Host: " + authority,
      "Upgrade: websocket",
      "Connection: Upgrade",
      "Sec-WebSocket-Key: " + key,
      "Sec-WebSocket-Version: 13",
    ];
    if (protocols && protocols.length) {
      lines.push("Sec-WebSocket-Protocol: " + [].concat(protocols).join(", "));
    }
    this.#send(new TextEncoder().encode(lines.join("\\r\\n") + "\\r\\n\\r\\n"));
    // A target that accepts and then stalls would leave this in CONNECTING
    // forever; the app is better served by a close it can react to.
    this.#handshakeTimer = setTimeout(() => {
      if (this.readyState === 0) this.#died(1006, "handshake timed out");
    }, 8000);
  }
  // A relayed socket lives inside a service worker, which the browser may
  // terminate when idle; a stalled write queue looks the same from here. Either
  // way the pipe goes quiet with no close, and an app that trusts readyState
  // waits forever. So: ping, and if nothing at all comes back, declare it dead
  // so the app can reconnect.
  #startKeepalive() {
    const PING_MS = 15000, SILENCE_MS = 10000;
    this.#lastRx = Date.now();
    this.#keepalive = setInterval(() => {
      if (this.readyState !== 1) return;
      const quiet = Date.now() - this.#lastRx;
      if (quiet > PING_MS + SILENCE_MS) { this.#died(1006, "relay went quiet"); return; }
      if (quiet >= PING_MS) { try { this.#frame(0x9, new Uint8Array(0)); } catch { this.#died(1006, "relay unusable"); } }
    }, 5000);
  }
  #stopTimers() {
    if (this.#keepalive) { clearInterval(this.#keepalive); this.#keepalive = null; }
    if (this.#closeTimer) { clearTimeout(this.#closeTimer); this.#closeTimer = null; }
    if (this.#handshakeTimer) { clearTimeout(this.#handshakeTimer); this.#handshakeTimer = null; }
  }
  #finishRelayClose() {
    if (this.#relayCloseTimer) { clearTimeout(this.#relayCloseTimer); this.#relayCloseTimer = null; }
    try { if (this.#port) this.#port.close(); } catch {}
    this.#port = null;
  }
  #requestRelayClose(flush) {
    if (!this.#port || this.#relayCloseRequested) return;
    this.#relayCloseRequested = true;
    try {
      this.#port.postMessage({ yasClose: true, flush: !!flush });
      // The ACK is the normal release path. This timer handles a worker that
      // was terminated between receiving the request and acknowledging it.
      this.#relayCloseTimer = setTimeout(
        () => this.#finishRelayClose(),
        ${PREVIEW_WS_RELAY_CLOSE_GRACE_MS},
      );
    } catch {
      this.#finishRelayClose();
    }
  }
  #releaseBuffers() {
    this.#rx = new Uint8Array(0); this.#rxStart = 0; this.#rxEnd = 0;
    this.#headerScan = 0; this.#frag = []; this.#fragBytes = 0;
  }
  #died(code, reason) {
    if (this.readyState === 3) return;
    this.#stopTimers();
    this.readyState = 3;
    this.#releaseBuffers();
    this.#requestRelayClose(false);
    // The platform fires \`error\` before an abnormal \`close\`, and every one of
    // these is abnormal — a client that only wired onerror must hear it too.
    const err = new Event("error");
    this.dispatchEvent(err); if (this.onerror) this.onerror(err);
    const ev = new CloseEvent("close", { code, reason, wasClean: false });
    this.dispatchEvent(ev); if (this.onclose) this.onclose(ev);
  }
  // Never synchronous, because the constructor can fail: an app subscribes
  // after \`new WebSocket(...)\` returns, so a close dispatched during
  // construction is one nobody is listening for. The socket then sits at
  // CLOSED with no event ever fired, and every reconnect-on-close client —
  // which is every HMR client — stops reconnecting for good.
  #fail(reason) { setTimeout(() => this.#died(1006, reason), 0); }
  #send(bytes) { if (this.#port) this.#port.postMessage(bytes, [bytes.buffer]); }
  #buffered() { return this.#rxEnd - this.#rxStart; }
  #append(chunk) {
    const limit = this.#handshook
      ? ${PREVIEW_WS_MAX_FRAME_BYTES} + 14
      : ${PREVIEW_WS_MAX_HANDSHAKE_BYTES};
    const buffered = this.#buffered();
    const needed = buffered + chunk.length;
    if (needed > limit) return false;
    if (this.#rx.length - this.#rxEnd < chunk.length && this.#rxStart > 0) {
      this.#rx.copyWithin(0, this.#rxStart, this.#rxEnd);
      this.#headerScan = Math.max(0, this.#headerScan - this.#rxStart);
      this.#rxStart = 0; this.#rxEnd = buffered;
    }
    if (this.#rx.length - this.#rxEnd < chunk.length) {
      const capacity = Math.min(
        limit,
        Math.max(1024, this.#rx.length * 2, needed),
      );
      const grown = new Uint8Array(capacity);
      grown.set(this.#rx.subarray(this.#rxStart, this.#rxEnd));
      this.#headerScan = Math.max(0, this.#headerScan - this.#rxStart);
      this.#rx = grown; this.#rxStart = 0; this.#rxEnd = buffered;
    }
    this.#rx.set(chunk, this.#rxEnd);
    this.#rxEnd += chunk.length;
    return true;
  }
  #consume(count) {
    this.#rxStart += count;
    if (this.#rxStart === this.#rxEnd) {
      // Do not retain a full-sized frame buffer while fragmented payload
      // chunks are already retained separately.
      if (this.#rx.length > 64 * 1024) this.#rx = new Uint8Array(0);
      this.#rxStart = 0; this.#rxEnd = 0; this.#headerScan = 0;
    }
  }
  #findHeaderEnd() {
    const from = Math.max(this.#rxStart, this.#headerScan);
    for (let i = from; i + 3 < this.#rxEnd; i++) {
      if (this.#rx[i] === 13 && this.#rx[i + 1] === 10 &&
          this.#rx[i + 2] === 13 && this.#rx[i + 3] === 10) return i + 4;
    }
    this.#headerScan = Math.max(this.#rxStart, this.#rxEnd - 3);
    return -1;
  }
  #onBytes(chunk) {
    this.#lastRx = Date.now();
    let offset = 0;
    while (offset < chunk.length && this.readyState !== 3) {
      const limit = this.#handshook
        ? ${PREVIEW_WS_MAX_FRAME_BYTES} + 14
        : ${PREVIEW_WS_MAX_HANDSHAKE_BYTES};
      const room = limit - this.#buffered();
      if (room <= 0) {
        this.#drain();
        if (this.readyState === 3) return;
        const nextLimit = this.#handshook
          ? ${PREVIEW_WS_MAX_FRAME_BYTES} + 14
          : ${PREVIEW_WS_MAX_HANDSHAKE_BYTES};
        if (nextLimit - this.#buffered() <= 0) {
          this.#died(1006, this.#handshook ? "frame exceeds limit" : "handshake headers exceed limit");
          return;
        }
        continue;
      }
      const take = Math.min(room, chunk.length - offset);
      if (!this.#append(chunk.subarray(offset, offset + take))) {
        this.#died(1006, "receive buffer exceeds limit");
        return;
      }
      offset += take;
      this.#drain();
    }
  }
  #drain() {
    if (!this.#handshook) {
      const end = this.#findHeaderEnd();
      if (end < 0) {
        if (this.#buffered() >= ${PREVIEW_WS_MAX_HANDSHAKE_BYTES})
          this.#died(1006, "handshake headers exceed limit");
        return;
      }
      const text = new TextDecoder().decode(this.#rx.subarray(this.#rxStart, end));
      if (!/^HTTP\\/1\\.1 101(?: |\\r\\n)/i.test(text)) { this.#died(1006, "upgrade refused"); return; }
      const proto = /sec-websocket-protocol:\\s*(\\S+)/i.exec(text);
      if (proto) this.protocol = proto[1];
      this.#consume(end - this.#rxStart);
      this.#handshook = true;
      this.readyState = 1;
      if (this.#handshakeTimer) { clearTimeout(this.#handshakeTimer); this.#handshakeTimer = null; }
      this.#startKeepalive();
      const ev = new Event("open");
      this.dispatchEvent(ev); if (this.onopen) this.onopen(ev);
    }
    for (;;) {
      const frame = this.#parse();
      if (!frame) return;
      this.#handle(frame);
    }
  }
  #parse() {
    const b = this.#rx;
    const start = this.#rxStart, available = this.#buffered();
    if (available < 2) return null;
    const first = b[start], second = b[start + 1];
    const fin = (first & 0x80) !== 0, opcode = first & 0x0f;
    if ((first & 0x70) !== 0) { this.#died(1006, "unsupported WebSocket extension bits"); return null; }
    if ((second & 0x80) !== 0) { this.#died(1006, "masked server frame"); return null; }
    if (opcode !== 0x0 && opcode !== 0x1 && opcode !== 0x2 &&
        opcode !== 0x8 && opcode !== 0x9 && opcode !== 0xa) {
      this.#died(1006, "invalid WebSocket opcode"); return null;
    }
    let len = second & 0x7f, at = 2;
    if (len === 126) {
      if (available < 4) return null;
      len = (b[start + 2] << 8) | b[start + 3]; at = 4;
      if (len < 126) { this.#died(1006, "non-canonical WebSocket length"); return null; }
    }
    else if (len === 127) {
      if (available < 10) return null;
      const view = new DataView(b.buffer, b.byteOffset + start);
      const wide = view.getBigUint64(2);
      if (wide > BigInt(${PREVIEW_WS_MAX_FRAME_BYTES})) {
        this.#died(1006, "frame exceeds limit"); return null;
      }
      len = Number(wide); at = 10;
      if (len < 65536) { this.#died(1006, "non-canonical WebSocket length"); return null; }
    }
    if (len > ${PREVIEW_WS_MAX_FRAME_BYTES}) { this.#died(1006, "frame exceeds limit"); return null; }
    const control = opcode >= 0x8;
    if (control && (!fin || len > 125)) { this.#died(1006, "invalid control frame"); return null; }
    const fragmented = this.#frag.length > 0;
    if (opcode === 0x0 && !fragmented) { this.#died(1006, "unexpected continuation"); return null; }
    if (opcode === 0x0 && this.#frag.length >= ${PREVIEW_WS_MAX_FRAGMENTS}) {
      this.#died(1006, "too many message fragments"); return null;
    }
    if ((opcode === 0x1 || opcode === 0x2) && fragmented) {
      this.#died(1006, "interleaved fragmented message"); return null;
    }
    const messageBytes = opcode === 0x0 ? this.#fragBytes + len : len;
    if (!control && messageBytes > ${PREVIEW_WS_MAX_MESSAGE_BYTES}) {
      this.#died(1006, "message exceeds limit"); return null;
    }
    if (available < at + len) return null;
    const payload = b.slice(start + at, start + at + len);
    this.#consume(at + len);
    return { fin, opcode, payload };
  }
  #handle(frame) {
    if (frame.opcode === 0x8) {
      if (frame.payload.length === 1) { this.#died(1006, "invalid close frame"); return; }
      const code = frame.payload.length >= 2 ? (frame.payload[0] << 8) | frame.payload[1] : 1005;
      if (code !== 1005 && code !== 1000 &&
          !((code >= 1001 && code <= 1003) || (code >= 1007 && code <= 1014) ||
            (code >= 3000 && code <= 4999))) {
        this.#died(1006, "invalid close code"); return;
      }
      let reason = "";
      try { reason = new TextDecoder("utf-8", { fatal: true }).decode(frame.payload.subarray(2)); }
      catch { this.#died(1006, "invalid close reason"); return; }
      if (this.readyState === 1) {
        this.readyState = 2;
        this.#frame(0x8, frame.payload);
      }
      this.#stopTimers();
      this.readyState = 3;
      this.#releaseBuffers();
      this.#requestRelayClose(true);
      const ev = new CloseEvent("close", {
        code, reason, wasClean: true,
      });
      this.dispatchEvent(ev); if (this.onclose) this.onclose(ev);
      return;
    }
    if (frame.opcode === 0x9) { this.#frame(0xa, frame.payload); return; } // pong
    if (frame.opcode === 0xa) return;
    if (frame.opcode === 0x0) {
      this.#frag.push(frame.payload); this.#fragBytes += frame.payload.length;
      if (!frame.fin) return;
    } else if (!frame.fin) {
      this.#frag = [frame.payload]; this.#fragBytes = frame.payload.length;
      this.#fragText = frame.opcode === 0x1;
      return;
    }
    let full;
    if (frame.opcode !== 0x0) {
      full = frame.payload; this.#fragText = frame.opcode === 0x1;
    } else {
      full = new Uint8Array(this.#fragBytes); let at = 0;
      for (const p of this.#frag) { full.set(p, at); at += p.length; }
    }
    this.#frag = []; this.#fragBytes = 0;
    let data;
    if (this.#fragText) {
      try { data = new TextDecoder("utf-8", { fatal: true }).decode(full); }
      catch { this.#died(1006, "invalid UTF-8 message"); return; }
    }
    else if (this.binaryType === "arraybuffer") data = full.buffer;
    else data = new Blob([full]);
    const ev = new MessageEvent("message", { data, origin: this.url });
    this.dispatchEvent(ev); if (this.onmessage) this.onmessage(ev);
  }
  #frame(opcode, payload) {
    const mask = new Uint8Array(4); crypto.getRandomValues(mask);
    const n = payload.length;
    const header = n < 126 ? 6 : n < 65536 ? 8 : 14;
    const out = new Uint8Array(header + n);
    out[0] = 0x80 | opcode;
    if (n < 126) { out[1] = 0x80 | n; }
    else if (n < 65536) { out[1] = 0x80 | 126; out[2] = n >> 8; out[3] = n & 0xff; }
    else { out[1] = 0x80 | 127; new DataView(out.buffer).setBigUint64(2, BigInt(n)); }
    out.set(mask, header - 4);
    for (let i = 0; i < n; i++) out[header + i] = payload[i] ^ mask[i & 3];
    this.#send(out);
  }
  send(data) {
    if (this.readyState !== 1) throw new DOMException("not open", "InvalidStateError");
    if (typeof data === "string") this.#frame(0x1, new TextEncoder().encode(data));
    else if (data instanceof Blob) data.arrayBuffer().then((b) => this.#frame(0x2, new Uint8Array(b)));
    else if (data instanceof ArrayBuffer) this.#frame(0x2, new Uint8Array(data));
    else if (data && data.buffer) this.#frame(0x2, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  }
  close(code, reason) {
    const c = code === undefined ? 1000 : Number(code);
    if (c !== 1000 && !(c >= 3000 && c <= 4999))
      throw new DOMException("invalid close code", "InvalidAccessError");
    const text = reason === undefined ? "" : String(reason);
    const body = new TextEncoder().encode(text);
    if (body.length > 123)
      throw new DOMException("close reason exceeds 123 UTF-8 bytes", "SyntaxError");
    if (this.readyState === 3 || this.readyState === 2) return;
    // Closing before the upgrade landed: there is no frame stream to close
    // yet, so a close frame here would just be garbage ahead of the response.
    // The platform fails the connection instead, and still reports a close.
    if (this.readyState === 0) { this.readyState = 2; this.#fail("closed while connecting"); return; }
    this.readyState = 2;
    const payload = new Uint8Array(2 + body.length);
    payload[0] = c >> 8; payload[1] = c & 0xff; payload.set(body, 2);
    this.#frame(0x8, payload);
    // The reply may never come — a dead relay, a terminated worker, a target
    // that does not echo — and CLOSING dispatches nothing, so an app that
    // reconnects from onclose would wait forever. A close is owed either way.
    this.#closeTimer = setTimeout(() => this.#died(1006, text || "close timed out"), 5000);
  }
}
try { window.WebSocket = RelayedWebSocket; } catch {}

// 3. window.open. A bare target URL has no binding, so it would land on the
// worker's "no target" page; route it through a bootstrap URL instead.
const nativeOpen = window.open.bind(window);
try {
  window.open = function (url, name, features) {
    if (!url) return nativeOpen(url, name, features);
    const abs = new URL(url, location.href);
    if (abs.origin === location.origin) {
      const spec = [T.dest, T.scheme, T.host, String(T.port)].map(encodeURIComponent).join("|");
      const q = new URLSearchParams({ "yas-preview": spec, "yas-path": abs.pathname + abs.search });
      return nativeOpen("/?" + q.toString(), name, features);
    }
    return nativeOpen(url, name, features);
  };
} catch {}

// 4. document.cookie. Header cookies are the worker's jar; JS-set ones would
// otherwise land on the Edge origin and be shared with every other preview.
// The mirror is synchronous because the accessor is; the worker is told after.
let jar = COOKIE0;
try {
  Object.defineProperty(Document.prototype, "cookie", {
    configurable: true,
    get: () => jar,
    set: (value) => {
      const pair = String(value).split(";")[0];
      const eq = pair.indexOf("=");
      if (eq > 0) {
        const name = pair.slice(0, eq).trim();
        const kept = jar.split("; ").filter((c) => c && c.split("=")[0] !== name);
        kept.push(pair.trim());
        jar = kept.join("; ");
      }
      const worker = YAS_SW && YAS_SW.controller;
      if (worker) worker.postMessage({ type: "yas-cookie", target: T, value: String(value) });
    },
  });
} catch {}

// 5. navigator.serviceWorker. A previewed app registering its own worker is
// reaching for *this* origin, not its dev server: a service-worker script
// fetch bypasses the controlling worker by spec, so the request is never
// relayed. Two ways that goes wrong, and neither is the app's fault. Against
// a dev server whose SPA fallback answers /sw.js with index.html, the browser
// refuses it on its MIME type and says so on every single load. Against yas
// proper, /sw.js *is* yas's own preview worker — so the app would register
// that, at scope "/", taking a share of the origin every preview depends on.
//
// The frame therefore reports no service-worker support, which is simply
// true: it cannot have one of its own. Deleting the accessor is what makes
// the usual "serviceWorker" in navigator guard false, so an app skips
// registration instead of failing at it.
try {
  delete Navigator.prototype.serviceWorker;
} catch {}
if ("serviceWorker" in navigator) {
  // The accessor would not go. Leave one that refuses, so a register() call
  // takes the rejection path apps already have for unsupported browsers
  // rather than throwing on a missing property.
  try {
    const refuse = () =>
      Promise.reject(new Error("yas preview: this frame cannot own a service worker"));
    Object.defineProperty(navigator, "serviceWorker", {
      configurable: true,
      get: () => ({
        controller: null,
        // Spec-accurate: ready settles when a registration becomes active,
        // and none ever will here.
        ready: new Promise(() => {}),
        register: refuse,
        getRegistration: () => Promise.resolve(undefined),
        getRegistrations: () => Promise.resolve([]),
        startMessages() {},
        addEventListener() {},
        removeEventListener() {},
        dispatchEvent: () => false,
      }),
    });
  } catch {}
}
})();`;
}

/** The `<script>` to prepend, as bytes. */
export function shimTag(
  target: PreviewTarget,
  cookie: string,
): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(
    `<script>${shimSource(target, cookie)}</script>`,
  );
}

/**
 * Prepend the shims to an HTML stream without buffering the document.
 *
 * Injection happens at the first tag boundary, so only the head of the stream
 * is held — the body still streams, which is the property the whole relay
 * exists for.
 */
export function injectIntoHtml(
  body: ReadableStream<Uint8Array<ArrayBuffer>>,
  target: PreviewTarget,
  cookie: string,
): ReadableStream<Uint8Array<ArrayBuffer>> {
  const tag = shimTag(target, cookie);
  let injected = false;
  let pending: Uint8Array<ArrayBuffer> = new Uint8Array(0);
  return new ReadableStream<Uint8Array<ArrayBuffer>>({
    async start(controller) {
      const reader = body.getReader();
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          if (injected) {
            controller.enqueue(value);
            continue;
          }
          const merged = new Uint8Array(pending.length + value.length);
          merged.set(pending);
          merged.set(value, pending.length);
          pending = merged;
          const at = insertionPoint(pending);
          if (at < 0) {
            // Keep waiting unless the head is implausibly long, in which case
            // give up and pass the document through unshimmed rather than
            // stalling it.
            if (pending.length > 64 * 1024) {
              controller.enqueue(pending);
              pending = new Uint8Array(0);
              injected = true;
            }
            continue;
          }
          controller.enqueue(new Uint8Array(pending.subarray(0, at)));
          controller.enqueue(tag);
          controller.enqueue(new Uint8Array(pending.subarray(at)));
          pending = new Uint8Array(0);
          injected = true;
        }
        if (!injected) {
          // A document with no tag at all: shim first, then whatever it was.
          controller.enqueue(tag);
          if (pending.length > 0) controller.enqueue(pending);
        }
        controller.close();
      } catch (err) {
        controller.error(err);
      }
    },
  });
}

/**
 * Byte offset to insert at: after `<head…>` when there is one, otherwise
 * before the first tag that is not a doctype or comment. Placing it inside
 * `<head>` matters — the shims must run before any app script.
 */
function insertionPoint(bytes: Uint8Array): number {
  // The returned index is used as a *byte* offset into the stream, so the
  // decoding must be index-preserving: latin1 maps every byte to exactly one
  // char. UTF-8 would not — an em dash ahead of <head> is 3 bytes but 1 char,
  // and the resulting drift once split the tag itself, leaving the whole shim
  // rendered as page text. The patterns below are ASCII, so matching is
  // unaffected.
  const text = new TextDecoder("latin1").decode(bytes);
  const head = /<head[^>]*>/i.exec(text);
  if (head) return head.index + head[0].length;
  const html = /<html[^>]*>/i.exec(text);
  if (html) return html.index + html[0].length;
  const body = /<body[^>]*>/i.exec(text);
  if (body) return body.index;
  // No structural tag yet: wait unless the doctype is already past.
  const doctype = /<!doctype[^>]*>/i.exec(text);
  if (doctype && text.length > doctype.index + doctype[0].length + 64) {
    return doctype.index + doctype[0].length;
  }
  return -1;
}
