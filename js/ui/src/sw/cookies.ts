/** A cookie jar per relayed origin (docs/design/net.md § Clean paths inside an iframe). */

import {
  PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES,
  PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES,
} from "../previewNetProtocol";

/** One origin cannot retain more cookie data than one accepted response head. */
export const PREVIEW_COOKIE_JAR_MAX_BYTES =
  PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES;
/** Bound tiny-cookie Map overhead as well as retained string bytes. */
export const PREVIEW_COOKIE_JAR_MAX_ITEMS = Math.floor(
  PREVIEW_COOKIE_JAR_MAX_BYTES / 1024,
);
/** All retained origin jars together fit within one request-body admission. */
export const PREVIEW_COOKIE_MAX_ORIGINS = Math.max(
  1,
  Math.floor(
    PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES / PREVIEW_COOKIE_JAR_MAX_BYTES,
  ),
);
/** Aggregate retained origin-key strings; jars themselves are count-bounded. */
export const PREVIEW_COOKIE_ORIGIN_KEY_MAX_BYTES =
  PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES;
const COOKIE_ENTRY_OVERHEAD_BYTES = 64;

interface Entry {
  value: string;
  path: string;
  /** Epoch millis, or null for a session cookie. */
  expires: number | null;
  /** `HttpOnly` was set, so page script must never see this one. */
  httpOnly: boolean;
  /** Stable creation order for equal-path Cookie header ordering. */
  creation: number;
  retainedBytes: number;
}

export class CookieJar {
  private readonly entries = new Map<string, Entry>();
  private retainedBytes = 0;
  private nextCreation = 1;

  constructor(
    private readonly maxItems = PREVIEW_COOKIE_JAR_MAX_ITEMS,
    private readonly maxBytes = PREVIEW_COOKIE_JAR_MAX_BYTES,
  ) {
    if (
      !Number.isSafeInteger(maxItems) ||
      maxItems <= 0 ||
      !Number.isSafeInteger(maxBytes) ||
      maxBytes <= 0
    )
      throw new RangeError("cookie jar limits must be positive integers");
  }

  /** Apply one `Set-Cookie` value. */
  set(header: string, requestPath: string): void {
    // Network response heads are admitted at this same bound. Check before
    // split() so a forged page message cannot amplify one giant string.
    if (stringBytes(header) > PREVIEW_HTTP_MAX_RESPONSE_HEAD_BYTES) return;
    this.purgeExpired();
    const parts = header.split(";");
    const first = parts[0] ?? "";
    const eq = first.indexOf("=");
    if (eq <= 0) return;
    const name = first.slice(0, eq).trim();
    const value = first.slice(eq + 1).trim();
    if (!name) return;

    let path = defaultPath(requestPath);
    let expires: number | null = null;
    let maxAge: number | null = null;
    let httpOnly = false;
    for (const attr of parts.slice(1)) {
      const idx = attr.indexOf("=");
      const key = (idx < 0 ? attr : attr.slice(0, idx)).trim().toLowerCase();
      const val = idx < 0 ? "" : attr.slice(idx + 1).trim();
      if (key === "path" && val.startsWith("/")) path = val;
      else if (key === "httponly") httpOnly = true;
      else if (key === "expires") {
        const when = Date.parse(val);
        if (!Number.isNaN(when)) expires = when;
      } else if (key === "max-age") {
        const seconds = Number(val);
        if (Number.isFinite(seconds)) maxAge = seconds;
      }
    }
    // Max-Age wins over Expires where both appear, and a non-positive one is a deletion — the standard way servers clear a cookie.
    const entryKey = key(name, path);
    if (maxAge !== null) {
      if (maxAge <= 0) {
        this.remove(entryKey);
        return;
      }
      expires = Date.now() + maxAge * 1000;
    }
    if (expires !== null && expires <= Date.now()) {
      this.remove(entryKey);
      return;
    }
    const previous = this.entries.get(entryKey);
    const retainedBytes = cookieBytes(name, path, value);
    // Treat an over-budget replacement as absent instead of keeping stale
    // credentials under the same name and path.
    if (retainedBytes > this.maxBytes) {
      this.remove(entryKey);
      return;
    }
    if (previous) this.remove(entryKey);
    this.entries.set(entryKey, {
      value,
      path,
      expires,
      httpOnly,
      creation: previous?.creation ?? this.nextCreation++,
      retainedBytes,
    });
    this.retainedBytes += retainedBytes;
    this.prune();
  }

  /**
   * The `Cookie` header for a request path, or undefined when there is none.
   *
   * `forScript` drops `HttpOnly` entries, for the one caller that hands the
   * jar to page JS through the injected `document.cookie` shim. Without it
   * the preview gave the app a *weaker* cookie contract than its real
   * origin would: a dev server's `HttpOnly` session cookie became readable
   * by anything running on the page, which is the entire property the
   * attribute exists to provide.
   */
  header(requestPath: string, forScript = false): string | undefined {
    const path = pathOf(requestPath);
    const now = Date.now();
    const pairs: Array<{
      entryKey: string;
      entry: Entry;
      name: string;
      value: string;
      path: string;
      creation: number;
    }> = [];
    for (const [entryKey, entry] of [...this.entries]) {
      if (entry.expires !== null && entry.expires <= now) {
        this.remove(entryKey);
        continue;
      }
      if (forScript && entry.httpOnly) continue;
      if (!pathMatches(entry.path, path)) continue;
      pairs.push({
        entryKey,
        entry,
        name: entryKey.slice(0, entryKey.indexOf("\0")),
        value: entry.value,
        path: entry.path,
        creation: entry.creation,
      });
    }
    if (pairs.length === 0) return undefined;
    for (const pair of pairs) this.touch(pair.entryKey, pair.entry);
    // RFC order is longest path, then earliest creation time. LRU promotion
    // is independent of that stable wire ordering.
    pairs.sort(
      (a, b) => b.path.length - a.path.length || a.creation - b.creation,
    );
    return pairs.map((p) => `${p.name}=${p.value}`).join("; ");
  }

  get size(): number {
    return this.entries.size;
  }

  get bytes(): number {
    return this.retainedBytes;
  }

  private purgeExpired(): void {
    const now = Date.now();
    for (const [entryKey, entry] of [...this.entries])
      if (entry.expires !== null && entry.expires <= now) this.remove(entryKey);
  }

  private touch(entryKey: string, entry: Entry): void {
    if (this.entries.get(entryKey) !== entry) return;
    this.entries.delete(entryKey);
    this.entries.set(entryKey, entry);
  }

  private remove(entryKey: string): void {
    const entry = this.entries.get(entryKey);
    if (!entry) return;
    this.entries.delete(entryKey);
    this.retainedBytes -= entry.retainedBytes;
  }

  private prune(): void {
    while (
      this.entries.size > this.maxItems ||
      this.retainedBytes > this.maxBytes
    ) {
      const oldest = this.entries.keys().next();
      if (oldest.done) return;
      this.remove(oldest.value);
    }
  }
}

/** Global content-neutral LRU for the worker's per-origin jars. */
export class CookieJarStore {
  private readonly entries = new Map<
    string,
    { jar: CookieJar; retainedBytes: number }
  >();
  private retainedBytes = 0;

  constructor(
    private readonly maxOrigins = PREVIEW_COOKIE_MAX_ORIGINS,
    private readonly maxBytes = PREVIEW_COOKIE_ORIGIN_KEY_MAX_BYTES,
  ) {
    if (
      !Number.isSafeInteger(maxOrigins) ||
      maxOrigins <= 0 ||
      !Number.isSafeInteger(maxBytes) ||
      maxBytes <= 0
    )
      throw new RangeError("cookie origin limits must be positive integers");
  }

  get(origin: string): CookieJar | undefined {
    const entry = this.entries.get(origin);
    if (!entry) return undefined;
    this.entries.delete(origin);
    this.entries.set(origin, entry);
    return entry.jar;
  }

  obtain(origin: string): CookieJar {
    const existing = this.get(origin);
    if (existing) return existing;
    const retainedBytes = COOKIE_ENTRY_OVERHEAD_BYTES + stringBytes(origin);
    // An oversized attacker-controlled route still gets a request-local jar,
    // but its key and cookies are never retained globally.
    if (retainedBytes > this.maxBytes) return new CookieJar();
    while (
      this.entries.size >= this.maxOrigins ||
      this.retainedBytes + retainedBytes > this.maxBytes
    ) {
      const oldest = this.entries.keys().next();
      if (oldest.done) break;
      this.remove(oldest.value);
    }
    const jar = new CookieJar();
    this.entries.set(origin, { jar, retainedBytes });
    this.retainedBytes += retainedBytes;
    return jar;
  }

  deleteIfEmpty(origin: string, jar: CookieJar): void {
    if (jar.size === 0 && this.entries.get(origin)?.jar === jar)
      this.remove(origin);
  }

  retainOnly(origins: ReadonlySet<string>): void {
    for (const origin of [...this.entries.keys()])
      if (!origins.has(origin)) this.remove(origin);
  }

  get size(): number {
    return this.entries.size;
  }

  keys(): IterableIterator<string> {
    return this.entries.keys();
  }

  get bytes(): number {
    return this.retainedBytes;
  }

  private remove(origin: string): void {
    const entry = this.entries.get(origin);
    if (!entry) return;
    this.entries.delete(origin);
    this.retainedBytes -= entry.retainedBytes;
  }
}

function key(name: string, path: string): string {
  return `${name}\0${path}`;
}

function stringBytes(value: string): number {
  // A JS string retains at most two bytes per code unit in current engines.
  return value.length * 2;
}

function cookieBytes(name: string, path: string, value: string): number {
  // The Map key retains name+path, and Entry retains value+path separately.
  return (
    COOKIE_ENTRY_OVERHEAD_BYTES +
    stringBytes(name) +
    stringBytes(value) +
    stringBytes(path) * 2
  );
}

/** Strip the query, then drop the last segment — the default-path rule. */
function defaultPath(requestPath: string): string {
  const path = pathOf(requestPath);
  const slash = path.lastIndexOf("/");
  if (slash <= 0) return "/";
  return path.slice(0, slash);
}

function pathOf(requestPath: string): string {
  const q = requestPath.indexOf("?");
  const path = q < 0 ? requestPath : requestPath.slice(0, q);
  return path.startsWith("/") ? path : "/" + path;
}

function pathMatches(cookiePath: string, requestPath: string): boolean {
  if (cookiePath === requestPath) return true;
  if (!requestPath.startsWith(cookiePath)) return false;
  // `/foo` matches `/foo/bar` but not `/foobar`.
  return cookiePath.endsWith("/") || requestPath[cookiePath.length] === "/";
}
