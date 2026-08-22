/** Content-addressed faces fetched through the native YAS FONT family. */

const DB = "yas-fonts";
const STORE = "faces";

/** How much face byte content is worth keeping. An entry larger than this
 * is usable for its current load but is never persisted: exempting it would
 * let one hostile family defeat the store's entire memory/quota bound. */
export const FONT_STORE_BUDGET_BYTES = 64 * 1024 * 1024;
export const FONT_STORE_MAX_ENTRIES = 128;
export const FONT_STORE_MAX_KEY_CHARS = 4_096;
export const FONT_LIST_MAX_FAMILIES = 2_048;
export const FONT_LIST_MAX_FAMILY_CHARS = 512;
export const FONT_LIST_MAX_TOTAL_CHARS = 256 * 1024;

export interface StoredFontFace {
  /** Exact standalone font bytes, keyed by their server-published BLAKE3. */
  data: Uint8Array;
  savedAt: number;
  usedAt: number;
}

/** What a stored entry costs and when it was last wanted. */
export interface FontStoreEntry {
  key: string;
  bytes: number;
  usedAt: number;
}

function keyBytes(key: string): number {
  return 64 + key.length * 2;
}

function faceEntryBytes(key: string, data: Uint8Array): number {
  return keyBytes(key) + 64 + data.byteLength;
}

function validStoreKey(key: string): boolean {
  return key.length > 0 && key.length <= FONT_STORE_MAX_KEY_CHARS;
}

let cached: Promise<IDBDatabase | null> | null = null;

/** One connection, reused: a fresh `open` per call can block behind the
 *  others, and a blocked open never settles. */
function open(): Promise<IDBDatabase | null> {
  cached ??= openOnce();
  return cached;
}

function openOnce(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    let request: IDBOpenDBRequest;
    try {
      request = indexedDB.open(DB, 1);
    } catch {
      resolve(null);
      return;
    }
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => resolve(null);
    request.onblocked = () => resolve(null);
  });
}

function store(db: IDBDatabase, mode: IDBTransactionMode): IDBObjectStore {
  return db.transaction(STORE, mode).objectStore(STORE);
}

function request<T>(req: IDBRequest<T>): Promise<T | null> {
  return new Promise((resolve) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => resolve(null);
  });
}

/** Load content-addressed face bytes. Hash keys are global rather than tied
 * to an edge URL: identical bytes served by another trusted server reuse the
 * same cache entry. */
export async function loadFontFace(
  hash: string,
): Promise<StoredFontFace | null> {
  const db = await open();
  if (!db) return null;
  const key = `hash:${hash}`;
  if (!validStoreKey(key)) return null;
  const entry = await request<StoredFontFace | undefined>(
    store(db, "readonly").get(key),
  );
  if (!entry || !(entry.data instanceof Uint8Array) || entry.data.length === 0)
    return null;
  if (faceEntryBytes(key, entry.data) > FONT_STORE_BUDGET_BYTES) {
    try {
      store(db, "readwrite").delete(key);
    } catch {}
    return null;
  }
  const now = Date.now();
  if (now - entry.usedAt > 60_000) {
    try {
      store(db, "readwrite").put({ ...entry, usedAt: now }, key);
    } catch {}
  }
  return entry;
}

export async function saveFontFace(
  hash: string,
  data: Uint8Array,
): Promise<void> {
  const db = await open();
  if (!db || data.length === 0) return;
  const key = `hash:${hash}`;
  if (
    !validStoreKey(key) ||
    faceEntryBytes(key, data) > FONT_STORE_BUDGET_BYTES
  ) {
    try {
      store(db, "readwrite").delete(key);
    } catch {}
    return;
  }
  const now = Date.now();
  try {
    const transaction = db.transaction(STORE, "readwrite");
    const committed = new Promise<boolean>((resolve) => {
      transaction.oncomplete = () => resolve(true);
      transaction.onabort = () => resolve(false);
      transaction.onerror = () => resolve(false);
    });
    transaction.objectStore(STORE).put(
      {
        data: data.slice(),
        savedAt: now,
        usedAt: now,
      } satisfies StoredFontFace,
      key,
    );
    if (!(await committed)) return;
  } catch {
    return;
  }
  await prune(db, key);
}

/** Remove bytes that failed content-hash verification. */
export async function forgetFontFace(hash: string): Promise<void> {
  const db = await open();
  if (!db) return;
  const key = `hash:${hash}`;
  if (!validStoreKey(key)) return;
  try {
    store(db, "readwrite").delete(key);
  } catch {}
}

/**
 * Which entries to evict so the rest fits `budget`.
 *
 * Least-recently-used first. `keep` is evicted last, but cannot override the
 * hard byte or item bound (save paths reject an oversized `keep` up front).
 */
export function selectEvictions(
  entries: readonly FontStoreEntry[],
  budget: number,
  keep?: string,
  maxEntries: number = FONT_STORE_MAX_ENTRIES,
): string[] {
  const total = entries.reduce((sum, e) => sum + e.bytes, 0);
  if (total <= budget && entries.length <= maxEntries) return [];
  const evictable = [...entries].sort(
    (a, b) =>
      Number(a.key === keep) - Number(b.key === keep) || a.usedAt - b.usedAt,
  );
  const evicted: string[] = [];
  let held = total;
  let heldEntries = entries.length;
  for (const entry of evictable) {
    if (held <= budget && heldEntries <= maxEntries) break;
    evicted.push(entry.key);
    held -= entry.bytes;
    heldEntries--;
  }
  return evicted;
}

async function prune(db: IDBDatabase, keep: string): Promise<void> {
  const invalid: string[] = [];
  const entries = await new Promise<FontStoreEntry[]>((resolve) => {
    const out: FontStoreEntry[] = [];
    let cursor: IDBRequest<IDBCursorWithValue | null>;
    try {
      cursor = store(db, "readonly").openCursor();
    } catch {
      resolve(out);
      return;
    }
    cursor.onsuccess = () => {
      const c = cursor.result;
      if (!c) {
        resolve(out);
        return;
      }
      const value = c.value as Partial<StoredFontFace>;
      if (!(value.data instanceof Uint8Array) || value.data.length === 0) {
        invalid.push(String(c.key));
        c.continue();
        return;
      }
      out.push({
        key: String(c.key),
        bytes: faceEntryBytes(String(c.key), value.data),
        usedAt: value.usedAt ?? 0,
      });
      c.continue();
    };
    cursor.onerror = () => resolve(out);
  });
  const evicted = selectEvictions(
    entries,
    FONT_STORE_BUDGET_BYTES,
    keep,
    FONT_STORE_MAX_ENTRIES,
  );
  if (evicted.length === 0 && invalid.length === 0) return;
  try {
    const s = store(db, "readwrite");
    for (const key of [...invalid, ...evicted]) s.delete(key);
  } catch {}
}

/** Sanitize a server-published family list before retaining it. */
export function boundedFontList(fonts: readonly unknown[]): string[] {
  const bounded: string[] = [];
  const seen = new Set<string>();
  let chars = 0;
  for (const value of fonts) {
    if (typeof value !== "string") continue;
    const family = value.trim();
    if (
      family.length === 0 ||
      family.length > FONT_LIST_MAX_FAMILY_CHARS ||
      chars + family.length > FONT_LIST_MAX_TOTAL_CHARS ||
      seen.has(family)
    ) {
      continue;
    }
    bounded.push(family);
    seen.add(family);
    chars += family.length;
    if (bounded.length >= FONT_LIST_MAX_FAMILIES) break;
  }
  return bounded;
}
