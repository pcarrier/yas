/** Preview service worker (docs/design/net.md § Client: service worker). */

import {
  bodyStream,
  encodeRequestHead,
  parseResponseHead,
  type ResponseHead,
} from "@yas-run/core/http1";
import {
  parseBootstrapUrl,
  parsePreviewFrameUrl,
  previewKey,
  PREVIEW_PREFIX,
  type ParsedBootstrap,
  type PreviewTarget,
} from "@yas-run/core/preview";
import { CookieJarStore } from "./cookies";
import { PreviewNetBroker } from "./conn";
import {
  PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES,
  PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES,
  PREVIEW_NET_MAX_PENDING_WRITE_BYTES,
  PREVIEW_NET_MAX_PENDING_WRITES,
  type PreviewNetOpenMessage,
} from "../previewNetProtocol";
import { forgetBinding, loadBindings, rememberBinding } from "./bindings";
import { bootstrapDocument } from "./bootstrap";
import { injectIntoHtml, PREVIEW_WS_RELAY_CLOSE_GRACE_MS } from "./inject";
import {
  desktopNotificationIdentity,
  desktopNotificationImage,
  desktopNotificationSourceClientId,
  topLevelDesktopSender,
  type DesktopNotificationIdentity,
} from "./desktopNotifications";

// The bundle runs in a worker; the app's tsconfig covers both lib sets, so name the scope explicitly rather than relying on ambient inference.
declare const self: ServiceWorkerGlobalScope & typeof globalThis;

const netBroker = new PreviewNetBroker(requestNetPort);
/** clientId → target. Persisted, because after the bootstrap redirect the
 *  frame's own URL is `/` and no longer says what it is bound to. */
const bindings = new Map<string, PreviewTarget>();
/** Warm the map from storage, bounded: no request may wait on IndexedDB.
 *  A miss costs one bootstrap round trip, a hang would cost the navigation. */
const restored = Promise.race([
  loadBindings().then((saved) => {
    for (const [id, target] of saved) {
      if (!bindings.has(id)) bindings.set(id, target);
    }
  }),
  new Promise<void>((resolve) => setTimeout(resolve, 500)),
]);
/** One jar per relayed origin. */
const jars = new CookieJarStore();

self.addEventListener("install", () => {
  // Take over without a reload: a preview pane that appears before the worker is active would otherwise fetch straight past it and render the yas UI.
  void self.skipWaiting();
});

self.addEventListener("activate", (event: ExtendableEvent) => {
  event.waitUntil(Promise.all([self.clients.claim(), restored, sweep()]));
});

/** Drop bindings for clients that no longer exist. */
async function sweep(): Promise<void> {
  const live = new Set((await self.clients.matchAll()).map((c) => c.id));
  for (const id of [...bindings.keys()]) {
    if (!live.has(id)) {
      bindings.delete(id);
      void forgetBinding(id);
    }
  }
  jars.retainOnly(
    new Set([...bindings.values()].map((target) => previewKey(target))),
  );
}

/**
 * The preview a client *is*, or null when it is the app itself.
 *
 * Every message below is same-origin, and a previewed page runs on this
 * origin too — so "same-origin" says nothing about who sent it. The binding
 * map is authoritative because only the fetch handler writes it, from the
 * navigation's own URL; parsing the client's URL covers the window between
 * the navigation and the binding. A previewed SPA rewrites its own URL with
 * `pushState`, which is why the binding is consulted first.
 */
function senderPreview(source: Client | null): PreviewTarget | null {
  if (!source) return null;
  const bound = bindings.get(source.id);
  if (bound) return bound;
  try {
    const url = new URL(source.url);
    return (
      parsePreviewFrameUrl(url.pathname, url.search)?.target ??
      parseBootstrapUrl(url.pathname, url.search)?.target ??
      null
    );
  } catch {
    return null;
  }
}

type DesktopNotificationMessage = DesktopNotificationIdentity & {
  type: "yas-desktop-notification-show";
  tag: string;
  title: string;
  body: string;
  icon?: string;
  image?: string;
};

function topLevelAppClient(source: Client | null): source is WindowClient {
  return topLevelDesktopSender(
    source as WindowClient | null,
    senderPreview(source) !== null,
  );
}

self.addEventListener("message", (event: ExtendableMessageEvent) => {
  const data = event.data as {
    type?: string;
    target?: PreviewTarget;
    value?: string;
    tag?: string;
    title?: string;
    body?: string;
    icon?: string;
    image?: string;
  } | null;
  if (!data || typeof data.type !== "string") return;
  if (data.type === "yas-desktop-notification-show") {
    event.waitUntil(
      (async () => {
        await restored;
        const source = event.source as Client | null;
        if (!topLevelAppClient(source)) return;
        const identity = desktopNotificationIdentity(data);
        if (
          !identity ||
          typeof data.tag !== "string" ||
          data.tag.length > 512 ||
          typeof data.title !== "string" ||
          data.title.length > 4_096 ||
          typeof data.body !== "string" ||
          data.body.length > 65_536
        ) {
          return;
        }
        const message = data as DesktopNotificationMessage;
        const options: NotificationOptions & { image?: string } = {
          body: message.body,
          tag: message.tag,
          icon: desktopNotificationImage(message.icon),
          image: desktopNotificationImage(message.image),
          data: {
            type: "yas-desktop-notification",
            sourceClientId: source.id,
            ...identity,
          },
        };
        await self.registration.showNotification(message.title, options);
      })(),
    );
    return;
  }
  if (data.type === "yas-desktop-notification-close") {
    event.waitUntil(
      (async () => {
        await restored;
        if (
          !topLevelAppClient(event.source as Client | null) ||
          typeof data.tag !== "string" ||
          data.tag.length > 512
        ) {
          return;
        }
        const notifications = await self.registration.getNotifications({
          tag: data.tag,
        });
        notifications.forEach((item) => item.close());
      })(),
    );
    return;
  }
  // The bootstrap document naming its target. `event.source` is the client
  // itself, which is how the binding gets an id without anyone guessing one.
  if (data.type === "yas-ws-open" && data.target && event.ports[0]) {
    // `waitUntil`, not a bare call: a worker with no in-flight extendable
    // event is terminated when idle (~30s), and a relayed socket lives inside
    // the worker — so a long-lived WebSocket would die silently mid-session.
    // Holding the message event open for the socket's lifetime is what keeps
    // the worker alive while it is pumping. A browser may still impose its own
    // ceiling, which is why the close sentinel matters: the app is told, and
    // reconnects.
    event.waitUntil(pipeWebSocket(data.target, event.ports[0]));
    return;
  }
  if (data.type === "yas-cookie" && typeof data.value === "string") {
    // The jar comes from the sender's own target, not from the message: a
    // frame previewing one dev server must not be able to write cookies
    // into another's jar.
    const sender = senderPreview(event.source as Client | null);
    if (!sender) return;
    const key = previewKey(sender);
    const jar = jars.obtain(key);
    jar.set(data.value, "/");
    jars.deleteIfEmpty(key, jar);
    return;
  }
  if (data.type === "yas-bind" && data.target) {
    const source = event.source as Client | null;
    if (source?.id) {
      bindings.set(source.id, data.target);
      void rememberBinding(source.id, data.target);
    }
    event.ports[0]?.postMessage({ ok: true });
  }
});

self.addEventListener("notificationclick", (event: NotificationEvent) => {
  const identity = desktopNotificationIdentity(event.notification.data);
  const sourceClientId = desktopNotificationSourceClientId(
    event.notification.data,
  );
  if (
    !identity ||
    !sourceClientId ||
    event.notification.data?.type !== "yas-desktop-notification"
  ) {
    return;
  }
  event.notification.close();
  event.waitUntil(
    restored.then(() =>
      self.clients
        .matchAll({ type: "window", includeUncontrolled: true })
        .then(async (clients) => {
          const client = clients.find(
            (candidate) =>
              candidate.id === sourceClientId && topLevelAppClient(candidate),
          ) as WindowClient | undefined;
          if (client) {
            await client.focus();
            client.postMessage({
              type: "yas-desktop-notification-click",
              ...identity,
            });
            return;
          }
          // Opening the UI is useful, but the new page has no proof that the
          // clicked record is still current. It deliberately receives no action.
          await self.clients.openWindow(self.registration.scope);
        }),
    ),
  );
});

self.addEventListener("fetch", (event: FetchEvent) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;

  const bootstrap =
    parsePreviewFrameUrl(url.pathname, url.search) ??
    parseBootstrapUrl(url.pathname, url.search);
  if (bootstrap) {
    // Bind only on a frame navigation. A plain `fetch()` of a preview URL is
    // still relayed — it is an explicit request for that target — but binding
    // its client would be catastrophic: the caller is usually the top-level
    // page, and a bound top-level client sends every one of the app's own
    // requests through the relay, which returns another origin's bytes for
    // them.
    const navigating =
      event.request.mode === "navigate" ||
      event.request.destination === "iframe";
    const id = navigating ? event.resultingClientId || event.clientId : "";
    if (id) {
      bindings.set(id, bootstrap.target);
      void rememberBinding(id, bootstrap.target);
    }
    event.respondWith(relay(event.request, bootstrap.target, bootstrap.path));
    return;
  }

  // Everything else: claim it only when it is certainly a preview's. Calling
  // respondWith for anything else would route the whole app through this
  // worker — every navigation and asset waiting on worker startup, and a bug
  // in here breaking the app rather than a pane. Returning without responding
  // leaves the browser's own path untouched.
  if (!isPreviewRequest(event)) return;
  event.respondWith(
    route(event).catch((err) =>
      problem(500, `preview worker failed: ${message(err)}`),
    ),
  );
});

/**
 * Synchronously decide whether a request could belong to a preview.
 *
 * Synchronous by necessity: `respondWith` must be called during dispatch, so
 * there is no awaiting `clients.get` before deciding. The in-memory bindings
 * answer for subresources; an iframe navigation is always claimed, since it is
 * either a bound frame moving or one whose binding needs recovering.
 */
function isPreviewRequest(event: FetchEvent): boolean {
  if (event.request.destination === "iframe") return true;
  const id = event.clientId || event.resultingClientId;
  return !!id && bindings.has(id);
}

/** Decide whether a request belongs to a preview. */
async function route(event: FetchEvent): Promise<Response> {
  // A navigation has no `clientId` — it is the client being created — so a
  // frame re-navigating within its target resolves through the id the
  // bootstrap bound.
  const target = await resolveTarget(event.clientId || event.resultingClientId);
  if (target) {
    const url = new URL(event.request.url);
    return relay(event.request, target, url.pathname + url.search);
  }
  // Only reachable for an iframe navigation we could not attribute.
  // An iframe navigating with no binding is a pane being opened: serve the
  // bootstrap document, which reads the target from the fragment, hands it
  // over, and replaces itself. A top-level navigation is the app and is never
  // touched — `destination` is what tells the two apart.
  if (event.request.destination === "iframe") {
    return bootstrapDocument();
  }
  return fetch(event.request);
}

async function resolveTarget(clientId: string): Promise<PreviewTarget | null> {
  if (!clientId) return null;
  await restored;
  const bound = bindings.get(clientId);
  // A navigation's resulting client does not exist yet, and `get` may reject
  // rather than resolve empty for an id it has never seen.
  const client = await self.clients.get(clientId).catch(() => undefined);
  // No client yet means a navigation in flight: only a binding can speak for
  // it, and one exists only if a bootstrap created it.
  if (!client) return bound ?? null;
  if (client.frameType !== "nested" && client.frameType !== "none") return null;
  if (bound) return bound;
  try {
    const url = new URL(client.url);
    const parsed = parseBootstrapUrl(url.pathname, url.search);
    if (parsed) {
      bindings.set(clientId, parsed.target);
      void rememberBinding(clientId, parsed.target);
      return parsed.target;
    }
  } catch {
    // Not a URL we can read.
  }
  return null;
}

/** Speak HTTP/1.1 to the target over a relayed socket. */
async function relay(
  request: Request,
  target: PreviewTarget,
  path: string,
): Promise<Response> {
  const key = previewKey(target);
  const jar = jars.obtain(key);

  let body: Uint8Array | null;
  try {
    body = await requestBody(request);
  } catch (error) {
    if (error instanceof PreviewRequestBodyTooLarge)
      return problem(413, error.message);
    throw error;
  }
  let stream;
  try {
    stream = await netBroker.open(target.dest, target.host, target.port, {
      tls: target.scheme === "https",
      // Offer only what we can speak.
      alpn: target.scheme === "https" ? ["http/1.1"] : undefined,
    });
    await stream.opened;
  } catch (err) {
    jars.deleteIfEmpty(key, jar);
    return problem(502, `Cannot reach ${describe(target)}: ${message(err)}`);
  }

  const host = target.host.includes(":") ? `[${target.host}]` : target.host;
  const authority =
    (target.scheme === "https" && target.port === 443) ||
    (target.scheme === "http" && target.port === 80)
      ? host
      : `${host}:${target.port}`;
  const head = encodeRequestHead({
    method: request.method,
    path,
    host: authority,
    headers: request.headers,
    contentLength: body ? body.length : undefined,
    cookie: jar.header(path),
    origin: `${target.scheme}://${authority}`,
    referer: request.referrer || undefined,
  });

  try {
    await stream.write(head);
    if (body && body.length > 0) await stream.write(body);
    // Half-close only when there is nothing more to send and the response is all we want: keep-alive requests must not shut the write side, or a pooled stream could not carry a second request.
    const source = stream.read();
    const { responseHead, prefix } = await readHead(source, request.method);
    for (const cookie of responseHead.setCookie) jar.set(cookie, path);
    jars.deleteIfEmpty(key, jar);
    const rewritten = rewriteHeaders(responseHead, target);
    let stream2 = bodyStream(responseHead, prefix, source, () =>
      stream.close(),
    );
    const isHtml = (rewritten.get("content-type") ?? "").includes("text/html");
    if (stream2 && isHtml && request.destination === "iframe") {
      // Only navigations, and only HTML: injecting into a fetched payload
      // would corrupt it.
      // `forScript`: the shim exposes this through `document.cookie`, so
      // HttpOnly entries must not be in it.
      stream2 = injectIntoHtml(stream2, target, jar.header(path, true) ?? "");
      // CSP is already dropped in rewriteHeaders, which the injected inline
      // script also depends on: a strict policy withholds `unsafe-inline`.
    }
    return new Response(stream2, {
      status: responseHead.status,
      statusText: responseHead.statusText,
      headers: rewritten,
    });
  } catch (err) {
    stream.close();
    jars.deleteIfEmpty(key, jar);
    return problem(502, `${describe(target)}: ${message(err)}`);
  }
}

/**
 * Relay a WebSocket as raw bytes between the page's shim and the target.
 *
 * Framing lives in the shim, not here: this end stays a byte pipe, which is
 * all the `NET` family is. The handshake bytes arrive from the shim like any
 * other payload.
 */
export async function pipeWebSocket(
  target: PreviewTarget,
  port: MessagePort,
): Promise<void> {
  const reportClosed = (): void => {
    try {
      port.postMessage({ yasClosed: true });
    } catch {
      // Port already unusable.
    }
    port.close();
  };
  let stream;
  try {
    stream = await netBroker.open(target.dest, target.host, target.port, {
      tls: target.scheme === "https",
    });
  } catch {
    // The sentinel matters most here: a refused connect must reach the shim, or
    // the socket sits in CONNECTING until its own timeout.
    reportClosed();
    return;
  }
  // Serialize writes: `write` chunks and waits on credit, so two concurrent
  // calls can interleave their bytes and corrupt the frame stream.
  let queue: Promise<void> = Promise.resolve();
  let pendingWrites = 0;
  let pendingWriteBytes = 0;
  let closing: Promise<void> | null = null;
  const closeFromShim = (flush: boolean): void => {
    if (closing) return;
    closing = (async () => {
      if (flush) {
        // A clean WebSocket close first queues its reply frame. Give that
        // ordered write a short chance to reach the target, but do not let a
        // target withholding Net credit retain the native stream forever.
        await Promise.race([
          queue.catch(() => {}),
          new Promise<void>((resolve) =>
            setTimeout(resolve, PREVIEW_WS_RELAY_CLOSE_GRACE_MS),
          ),
        ]);
      }
      try {
        stream.close();
      } finally {
        try {
          port.postMessage({ yasCloseAck: true });
        } catch {
          // The page already discarded its side.
        }
        port.close();
      }
    })();
  };
  port.onmessage = (event) => {
    const message = event.data as
      | ArrayBuffer
      | ArrayBufferView
      | { yasClose?: boolean; flush?: boolean }
      | null;
    if (message && typeof message === "object" && "yasClose" in message) {
      if (message.yasClose === true && typeof message.flush === "boolean")
        closeFromShim(message.flush);
      else closeFromShim(false);
      return;
    }
    const bytes = webSocketPortBytes(message);
    if (closing || !bytes) {
      if (!bytes) closeFromShim(false);
      return;
    }
    if (
      pendingWrites >= PREVIEW_NET_MAX_PENDING_WRITES ||
      bytes.length > PREVIEW_NET_MAX_PENDING_WRITE_BYTES - pendingWriteBytes
    ) {
      closeFromShim(false);
      return;
    }
    pendingWrites++;
    pendingWriteBytes += bytes.length;
    queue = queue
      .then(async () => {
        try {
          if (closing) return;
          await stream.write(bytes);
        } finally {
          pendingWrites--;
          pendingWriteBytes -= bytes.length;
        }
      })
      .catch(() => closeFromShim(false));
  };
  port.onmessageerror = () => closeFromShim(false);
  try {
    await stream.opened;
  } catch {
    if (closing) {
      await closing;
      return;
    }
    stream.close();
    reportClosed();
    return;
  }
  try {
    for await (const chunk of stream.read()) {
      if (closing) break;
      const copy = new Uint8Array(chunk);
      port.postMessage(copy.buffer, [copy.buffer]);
    }
  } catch {
    // Target closed or reset.
  }
  if (closing) {
    await closing;
    return;
  }
  // A closed MessagePort fires no event on the other side, so the shim would
  // never learn the socket is dead — and an app that reconnects on close (every
  // HMR client) would hang instead. Say so explicitly before closing.
  stream.close();
  reportClosed();
}

function webSocketPortBytes(value: unknown): Uint8Array | null {
  if (ArrayBuffer.isView(value))
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (Object.prototype.toString.call(value) === "[object ArrayBuffer]")
    return new Uint8Array(value as ArrayBuffer);
  return null;
}

/** The target named by a referrer, when it is a preview URL. */
function fromReferrer(referrer: string): ParsedBootstrap | null {
  if (!referrer) return null;
  try {
    const url = new URL(referrer);
    if (url.origin !== self.location.origin) return null;
    return (
      parsePreviewFrameUrl(url.pathname, url.search) ??
      parseBootstrapUrl(url.pathname, url.search)
    );
  } catch {
    return null;
  }
}

/**
 * Hand a fresh MessagePort to one controlled top-level App. The App resolves
 * `dest` against its live home/Relay catalogue and opens exact native YAS Net;
 * the worker never receives a credential or opens an edge route.
 */
async function requestNetPort(
  request: PreviewNetOpenMessage,
): Promise<MessagePort> {
  await restored;
  const windows = (
    await self.clients.matchAll({
      type: "window",
      includeUncontrolled: true,
    })
  )
    .filter((client): client is WindowClient => topLevelAppClient(client))
    .sort(
      (left, right) =>
        Number(right.focused) - Number(left.focused) ||
        Number(right.visibilityState === "visible") -
          Number(left.visibilityState === "visible"),
    );
  const app = windows[0];
  if (!app)
    throw new Error(
      "No yas App is available to broker native YAS Net. Open the yas UI in a top-level tab, then reload this pane.",
    );
  const channel = new MessageChannel();
  app.postMessage(request, [channel.port2]);
  return channel.port1;
}

/** Read until the response head is complete, keeping the body's first bytes. */
async function readHead(
  source: AsyncGenerator<Uint8Array, void, void>,
  method: string,
): Promise<{ responseHead: ResponseHead; prefix: Uint8Array }> {
  let buffer = new Uint8Array(0);
  for (;;) {
    const { done, value } = await source.next();
    if (done) throw new Error("closed before sending a response");
    const merged = new Uint8Array(buffer.length + value.length);
    merged.set(buffer, 0);
    merged.set(value, buffer.length);
    buffer = merged;
    const parsed = parseResponseHead(buffer, method);
    if (parsed) {
      if (
        buffer.length - parsed.rest.length >
        PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES
      )
        throw new Error("response head too large");
      return { responseHead: parsed, prefix: parsed.rest };
    }
    // A head this large is a broken target, not a slow one.
    if (buffer.length > PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES) {
      throw new Error("response head too large");
    }
  }
}

/** Fix up the headers the browser will act on. */
export function rewriteHeaders(
  head: ResponseHead,
  target: PreviewTarget,
): Headers {
  const headers = new Headers(head.headers);
  const location = headers.get("location");
  if (location) {
    // A redirect off the previewed origin is delivered unchanged, so the frame
    // follows it and leaves the relay — deliberate, because a dev server that
    // bounces you to an identity provider should still get you there. The
    // trade is worth stating: the browser resolves it directly, so a *remote*
    // target answering `Location: //localhost:9000` reaches the viewer's own
    // machine and not the server's.
    const rewritten = rewriteLocation(location, target);
    if (rewritten) headers.set("location", rewritten);
  }
  // Content-Length would contradict the stream we actually deliver (chunked decoded, or truncated); the browser computes what it needs.
  headers.delete("content-length");
  // A preview must not claim authority over the Edge origin's other paths.
  headers.delete("clear-site-data");
  headers.delete("service-worker-allowed");
  // Framing refusals are enforced by the browser against *this* response, and
  // this response is ours — so dropping them is all it takes to preview a site
  // that says it does not want to be framed. Deliberate: those headers are a
  // site's clickjacking defence, and removing them is only defensible because a
  // preview is the operator looking at their own target on their own screen.
  headers.delete("x-frame-options");
  headers.delete("content-security-policy");
  headers.delete("content-security-policy-report-only");
  return headers;
}

export function rewriteLocation(
  location: string,
  target: PreviewTarget,
): string | null {
  // `//host/path` is an authority, not a path, so it must not be taken for a
  // clean same-origin one. Resolved against the target's scheme instead: one
  // naming the target becomes a path and the frame stays in the preview, and
  // one naming anything else is left to the browser like any other off-target
  // redirect. Treating it as a path sent even an on-target `//host` straight
  // out of the relay.
  if (location.startsWith("//")) {
    return targetPath(`${target.scheme}:${location}`, target);
  }
  if (location.startsWith("/")) return location; // already clean-path
  return targetPath(location, target);
}

/**
 * `absolute` as a path on `target`, or null when it names somewhere else.
 *
 * Null means the header is delivered unchanged and the browser follows it out
 * of the relay — see `rewriteHeaders`.
 */
function targetPath(absolute: string, target: PreviewTarget): string | null {
  let url: URL;
  try {
    url = new URL(absolute);
  } catch {
    return null;
  }
  const sameHost =
    url.hostname.replace(/^\[|\]$/g, "") === target.host &&
    (url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80) ===
      target.port;
  return sameHost ? url.pathname + url.search + url.hash : null;
}

class PreviewRequestBodyTooLarge extends Error {
  constructor() {
    super(
      `request body exceeds the ${PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES}-byte preview limit`,
    );
    this.name = "PreviewRequestBodyTooLarge";
  }
}

export async function requestBody(
  request: Request,
): Promise<Uint8Array | null> {
  if (request.method === "GET" || request.method === "HEAD") return null;
  const declaredLength = request.headers.get("content-length");
  if (
    declaredLength !== null &&
    /^\d+$/.test(declaredLength) &&
    BigInt(declaredLength) > BigInt(PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES)
  )
    throw new PreviewRequestBodyTooLarge();
  const body = request.clone().body;
  if (!body) return null;
  const reader = body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    for (;;) {
      const item = await reader.read();
      if (item.done) break;
      const chunk = new Uint8Array(item.value);
      if (chunk.length > PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES - length) {
        void reader.cancel("preview request body limit exceeded");
        throw new PreviewRequestBodyTooLarge();
      }
      chunks.push(chunk);
      length += chunk.length;
    }
  } finally {
    reader.releaseLock();
  }
  if (length === 0) return null;
  const result = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

function describe(target: PreviewTarget): string {
  return `${target.scheme}://${target.host}:${target.port} on ${target.dest}`;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** A legible failure. */
function problem(status: number, text: string): Response {
  return new Response(`yas preview: ${text}\n`, {
    status,
    headers: {
      "content-type": "text/plain; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

export { PREVIEW_PREFIX };
