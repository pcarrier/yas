import { clearStorage, readStorage, writeStorage } from "./storage";

const WORKSPACE_PART = "workspace";
// Links produced before the terminology change remain valid indefinitely.
const LEGACY_SESSION_PART = "session";
const PASSPHRASE_PART = "psk";

function hashBody(hash: string): string {
  return hash.startsWith("#") ? hash.slice(1) : hash;
}

function decodeHashComponent(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function partKey(part: string): string {
  const equals = part.indexOf("=");
  return decodeHashComponent(equals < 0 ? part : part.slice(0, equals));
}

function partValue(part: string): string {
  const equals = part.indexOf("=");
  return equals < 0 ? "" : decodeHashComponent(part.slice(equals + 1));
}

/** Backend workspace IDs are canonical, lower-case UUIDs. */
export function normalizeWorkspaceSessionId(value: string): string | null {
  const normalized = value.toLowerCase();
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    normalized,
  )
    ? normalized
    : null;
}

export interface WorkspaceSessionHashRequest {
  /** Distinguishes a malformed explicit request from an absent field. */
  present: boolean;
  id: string | null;
}

/** Read a workspace field without hiding malformed explicit requests. */
export function workspaceSessionRequestFromHash(
  hash: string,
): WorkspaceSessionHashRequest {
  const parts = hashBody(hash).split("&").filter(Boolean);
  for (const key of [WORKSPACE_PART, LEGACY_SESSION_PART]) {
    const part = parts.find((candidate) => partKey(candidate) === key);
    if (part) {
      return {
        present: true,
        id: normalizeWorkspaceSessionId(partValue(part)),
      };
    }
  }
  return { present: false, id: null };
}

export function workspaceSessionIdFromHash(hash: string): string | null {
  return workspaceSessionRequestFromHash(hash).id;
}

export interface ConsumedPassphrase {
  passphrase: string | null;
  /** Fragment after removing every `psk` field, without rewriting other data. */
  hash: string;
  found: boolean;
}

/**
 * Remove first-contact credentials from a fragment before it can be copied or
 * retained in browser history. All non-secret fields remain byte-for-byte
 * intact until workspace bootstrap writes the canonical fragment.
 */
export function consumePassphraseFromHash(hash: string): ConsumedPassphrase {
  const kept: string[] = [];
  let passphrase: string | null = null;
  let found = false;
  for (const part of hashBody(hash).split("&")) {
    if (!part) continue;
    if (partKey(part) === PASSPHRASE_PART) {
      if (!found) passphrase = partValue(part);
      found = true;
    } else {
      kept.push(part);
    }
  }
  return { passphrase, hash: kept.join("&"), found };
}

/**
 * The fragment a *share link* carries. Not what this device's address bar
 * shows: a workspace id in the URL is machine state on a line people read, copy
 * and mistype, and it made every address in the product look like a query
 * string. Handing someone a link is a deliberate act, and that is the only
 * thing this spells now.
 */
export function workspaceSessionHash(id: string): string {
  const normalized = normalizeWorkspaceSessionId(id);
  if (!normalized) throw new Error("invalid workspace ID");
  return `${WORKSPACE_PART}=${normalized}`;
}

/** Absolute, secret-free URL suitable for clipboard sharing. */
export function workspaceSessionShareUrl(
  source: Pick<Location, "origin" | "pathname">,
  id: string,
): string {
  return `${source.origin}${source.pathname}#${workspaceSessionHash(id)}`;
}

export type WorkspaceSessionHistoryMode = "push" | "replace" | "none";

/** Where this device remembers which workspace it had attached. */
export const WORKSPACE_SESSION_STORAGE_KEY = "yas.workspaceSession";

/**
 * Record the attached workspace, and leave the address bar clean.
 *
 * The attachment is per device, not per URL, so it belongs in storage: a
 * reload restores it without the id ever appearing in an address, a copied URL
 * carries no machine state, and a link someone deliberately shared still
 * works because {@link workspaceSessionRequestFromHash} is still read on first
 * contact.
 *
 * The history mode survives as the caller's statement of intent even though
 * neither entry differs now: a workspace switch replaces the address rather than
 * stacking identical entries, and callers already say which they mean.
 */
export function writeWorkspaceSessionUrl(
  id: string | null,
  mode: WorkspaceSessionHistoryMode,
): void {
  if (mode === "none") return;
  if (id) writeStorage(WORKSPACE_SESSION_STORAGE_KEY, id);
  else clearStorage(WORKSPACE_SESSION_STORAGE_KEY);
  stripWorkspaceSessionFromUrl(mode);
}

/**
 * Take the workspace — and any first-contact passphrase — out of the address,
 * without recording anything.
 *
 * Separate from {@link writeWorkspaceSessionUrl} because arriving at a link is
 * not the same as choosing a workspace: a navigation that turns out to name the
 * tab already open still has to leave a clean address behind it, and must not
 * store an id whose attachment has not happened yet.
 *
 * Everything else in the fragment is somebody else's state — connection specs,
 * debug flags — and is left byte-for-byte alone.
 */
export function stripWorkspaceSessionFromUrl(
  mode: WorkspaceSessionHistoryMode = "replace",
): void {
  if (mode === "none") return;
  const rest = hashBody(location.hash)
    .split("&")
    .filter(
      (part) =>
        part &&
        partKey(part) !== WORKSPACE_PART &&
        partKey(part) !== LEGACY_SESSION_PART &&
        partKey(part) !== PASSPHRASE_PART,
    )
    .join("&");
  if (location.hash === (rest ? `#${rest}` : "")) return;
  const url = rest ? `${location.pathname}#${rest}` : location.pathname;
  if (mode === "push") history.pushState(null, "", url);
  else history.replaceState(null, "", url);
}

/**
 * The workspace this device last attached, for a boot with no share link.
 *
 * A stored id that is no longer valid is treated as absent rather than as a
 * malformed request: nobody typed it, so there is nothing to report.
 */
export function storedWorkspaceSessionId(): string | null {
  const stored = readStorage(WORKSPACE_SESSION_STORAGE_KEY);
  return stored ? normalizeWorkspaceSessionId(stored) : null;
}
