import { parseDSL } from "@yas-run/core/layout";
import type { WorkspaceLayout } from "@yas-run/core/layout";
import {
  WORKSPACE_SESSION_MAX_REFERENCE_BYTES,
  WORKSPACE_SESSION_MAX_REMOTE_BYTES,
} from "@yas-run/core";
import type { SurfaceId, TerminalId } from "@yas-run/core";

export type {
  WorkspaceLayout,
  LayoutPane,
  LayoutAssignments,
  TileAssignment,
} from "@yas-run/core/layout";
export {
  enumeratePanes,
  assignSessionsToPanes,
  carryAssignmentsToPanes,
  assignmentsAfterDrop,
  buildCandidateOrder,
  reconcileAssignments,
  adjustWeights,
  layoutFromDSL,
  leafCount,
  surfaceAssignment,
  isSurfaceAssignment,
  parseSurfaceAssignment,
  editorAssignment,
  manageAssignment,
  diffAssignment,
  parseDiffArg,
  isTileAssignment,
  parseTileAssignment,
  webAssignment,
  isWebAssignment,
  parseWebAssignment,
  isContentAssignment,
} from "@yas-run/core/layout";

import { readStorage, writeStorage } from "../storage";

export type StableWorkspaceRef =
  | {
      kind: "terminal";
      connectionId: string;
      terminalHandle: bigint;
    }
  | {
      kind: "tab";
      connectionId: string;
      tabId: string;
    }
  | {
      kind: "surface";
      connectionId: string;
      surfaceHandle: bigint;
    };

export type StableSurfaceWorkspaceRef = Extract<
  StableWorkspaceRef,
  { kind: "surface" }
>;

const workspaceRefEncoder = new TextEncoder();

function workspaceRefFieldWithinBytes(
  value: string,
  maxBytes: number,
): boolean {
  return (
    value.length > 0 &&
    !value.includes("\0") &&
    workspaceRefEncoder.encode(value).length <= maxBytes
  );
}

function encodedWorkspaceConnectionId(connectionId: string): string {
  if (
    !workspaceRefFieldWithinBytes(
      connectionId,
      WORKSPACE_SESSION_MAX_REMOTE_BYTES,
    )
  ) {
    throw new RangeError("workspace connection id exceeds its byte limit");
  }
  return encodeURIComponent(connectionId);
}

function checkedWorkspaceRef(value: string): string {
  if (
    workspaceRefEncoder.encode(value).length >
    WORKSPACE_SESSION_MAX_REFERENCE_BYTES
  ) {
    throw new RangeError("workspace reference exceeds its byte limit");
  }
  return value;
}

export function connectionAwaitingWorkspaceRestore(
  connection:
    | {
        ready: boolean;
        status: string;
      }
    | null
    | undefined,
): boolean {
  return (
    connection != null &&
    !connection.ready &&
    (connection.status === "connecting" ||
      connection.status === "authenticating" ||
      connection.status === "connected")
  );
}

/** Stable terminal identity used by the backend workspace-session store. */
export function terminalWorkspaceRef(
  connectionId: string,
  terminalHandle: bigint,
): string {
  if (terminalHandle < 0n || terminalHandle > 0xffff_ffff_ffff_ffffn) {
    throw new RangeError("terminal handle is outside u64");
  }
  return checkedWorkspaceRef(
    `terminal:${encodedWorkspaceConnectionId(connectionId)}:${terminalHandle}`,
  );
}

/** Persist the opaque native Terminal handle without allocating an alias. */
export function terminalWorkspaceRefForPtyId(
  connectionId: string,
  ptyId: TerminalId,
): string {
  return terminalWorkspaceRef(connectionId, ptyId);
}

/** Resolve a native terminal identity. */
export function ptyIdForWorkspaceRef(
  ref: Extract<StableWorkspaceRef, { kind: "terminal" }>,
): TerminalId {
  return ref.terminalHandle;
}

/** Stable server-side tab identity used for editor, diff, and web panes. */
export function tabWorkspaceRef(connectionId: string, id: string): string {
  if (
    !workspaceRefFieldWithinBytes(id, WORKSPACE_SESSION_MAX_REFERENCE_BYTES)
  ) {
    throw new RangeError("workspace tab id exceeds its byte limit");
  }
  return checkedWorkspaceRef(
    `tab:${encodedWorkspaceConnectionId(connectionId)}:${encodeURIComponent(id)}`,
  );
}

/** Stable native compositor identity used by the workspace-session store. */
export function surfaceWorkspaceRef(
  connectionId: string,
  surfaceHandle: bigint,
): string {
  if (surfaceHandle < 0n || surfaceHandle > 0xffff_ffff_ffff_ffffn) {
    throw new RangeError("surface handle is outside u64");
  }
  return checkedWorkspaceRef(
    `surface:${encodedWorkspaceConnectionId(connectionId)}:${surfaceHandle}`,
  );
}

/** Persist the opaque native Surface handle without allocating an alias. */
export function surfaceWorkspaceRefForId(
  connectionId: string,
  surfaceId: SurfaceId,
): string {
  return surfaceWorkspaceRef(connectionId, surfaceId);
}

/** Resolve a native Surface identity. */
export function surfaceIdForWorkspaceRef(
  ref: StableSurfaceWorkspaceRef,
): SurfaceId {
  return ref.surfaceHandle;
}

/** Parse the deliberately small, opaque workspace-reference wire form. */
export function parseWorkspaceRef(value: string): StableWorkspaceRef | null {
  // The persisted DTO admits a 16 KiB ASCII reference. Percent encoding can
  // expand each byte of a valid 1 KiB Unicode Relay name to three characters,
  // so a smaller character cap rejects otherwise valid remote identities.
  if (
    workspaceRefEncoder.encode(value).length >
    WORKSPACE_SESSION_MAX_REFERENCE_BYTES
  )
    return null;
  const first = value.indexOf(":");
  const second = value.indexOf(":", first + 1);
  if (first <= 0 || second <= first + 1 || value.indexOf(":", second + 1) >= 0)
    return null;
  const kind = value.slice(0, first);
  let connectionId: string;
  let id: string;
  try {
    connectionId = decodeURIComponent(value.slice(first + 1, second));
    id = decodeURIComponent(value.slice(second + 1));
  } catch {
    return null;
  }
  if (
    !workspaceRefFieldWithinBytes(
      connectionId,
      WORKSPACE_SESSION_MAX_REMOTE_BYTES,
    ) ||
    !workspaceRefFieldWithinBytes(id, WORKSPACE_SESSION_MAX_REFERENCE_BYTES)
  )
    return null;
  if (kind === "tab") return { kind: "tab", connectionId, tabId: id };
  // u64 has at most 20 decimal digits. Bound before BigInt parsing so the
  // larger persisted-reference envelope cannot become a CPU-heavy integer.
  if (id.length > 20) return null;
  if (!/^\d+$/.test(id)) return null;
  if (kind === "surface" || kind === "terminal") {
    const handle = BigInt(id);
    if (handle > 0xffff_ffff_ffff_ffffn) return null;
    return kind === "surface"
      ? { kind: "surface", connectionId, surfaceHandle: handle }
      : { kind: "terminal", connectionId, terminalHandle: handle };
  }
  return null;
}

const LAYOUT_KEY = "yas.layout";
export const LAYOUT_HISTORY_KEY = "yas.layouts";
interface StoredRecentLayout {
  name: string;
  dsl: string;
}

function layoutFromDSLString(
  dsl: string,
  name?: string,
): WorkspaceLayout | null {
  try {
    const { root, weight } = parseDSL(dsl);
    return { name: name ?? dsl, dsl, root, weight };
  } catch {
    return null;
  }
}

export function loadActiveLayout(): WorkspaceLayout | null {
  try {
    const raw = readStorage(LAYOUT_KEY);
    if (!raw) return null;
    const saved = JSON.parse(raw) as { name: string; dsl: string };
    return layoutFromDSLString(saved.dsl, saved.name);
  } catch {
    return null;
  }
}

export function saveActiveLayout(layout: WorkspaceLayout | null): void {
  if (layout) {
    writeStorage(
      LAYOUT_KEY,
      JSON.stringify({ name: layout.name, dsl: layout.dsl }),
    );
  } else {
    try {
      localStorage.removeItem(LAYOUT_KEY);
    } catch {}
  }
}

export function saveToHistory(layout: WorkspaceLayout): void {
  pushRecentLayout(layout);
}

/** Remove a layout from the recent history by its DSL string. */
export function removeFromHistory(dsl: string): void {
  try {
    const raw = readStorage(LAYOUT_HISTORY_KEY);
    if (!raw) return;
    const existing: StoredRecentLayout[] = JSON.parse(raw);
    const next = existing.filter((entry) => entry.dsl !== dsl);
    writeStorage(LAYOUT_HISTORY_KEY, JSON.stringify(next));
  } catch {}
}

export function loadRecentLayouts(): WorkspaceLayout[] {
  try {
    const raw = readStorage(LAYOUT_HISTORY_KEY);
    if (!raw) return [];
    const stored: StoredRecentLayout[] = JSON.parse(raw);
    return stored.flatMap((entry) => {
      try {
        const { root, weight } = parseDSL(entry.dsl);
        return [{ name: entry.name, dsl: entry.dsl, root, weight }];
      } catch {
        return [];
      }
    });
  } catch {
    return [];
  }
}

function pushRecentLayout(layout: WorkspaceLayout): void {
  const record = { name: layout.name, dsl: layout.dsl };
  try {
    const raw = readStorage(LAYOUT_HISTORY_KEY);
    const existing: StoredRecentLayout[] = raw ? JSON.parse(raw) : [];
    const next = [
      record,
      ...existing.filter((entry) => entry.dsl !== record.dsl),
    ].slice(0, 10);
    writeStorage(LAYOUT_HISTORY_KEY, JSON.stringify(next));
  } catch {}
}
