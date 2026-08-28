import type { SurfaceId } from "../types";
import type {
  LayoutNode,
  LayoutSplit,
  LayoutChild,
  LayoutLeaf,
  LayoutRect,
} from "./dsl";
import { parseDSL } from "./dsl";

export interface WorkspaceLayout {
  name: string;
  dsl: string;
  root: LayoutNode;
  weight: number;
}

export interface LayoutPane {
  id: string;
  leaf: LayoutLeaf;
}

export interface LayoutAssignments {
  assignments: Record<string, string | null>;
}

/**
 * Which window manager owns the workspace.
 *
 * One tree serves all three, because what differs is how the root's children
 * are placed, not what they are:
 *
 * - `tiling`: the classic BSP tree. Every window is on screen and the weights
 *   divide the space; splits nest arbitrarily.
 * - `scrolling`: one strip of columns, allowed to be wider than the viewport,
 *   which follows the focus along it (niri). A column is a leaf or a vertical
 *   stack, and its weight is its width as a fraction of the viewport.
 * - `floating`: every window has an explicit frame and they overlap, last on
 *   top.
 *
 * The root split's direction says which is in force, so a layout carries its
 * manager in the same DSL string that already round-trips through the backend
 * session — nothing else needs a new field.
 */
export type WindowManager = "tiling" | "scrolling" | "floating";

/** Half the viewport: niri's default column width, and the width that makes a
 *  strip read as a strip rather than as a full-screen window. */
export const SCROLLING_DEFAULT_WIDTH = 0.5;

/** Where a floating window lands when it has no frame of its own, cascaded so
 *  the nth window is not exactly under the (n-1)th. */
export function cascadeRect(index: number): LayoutRect {
  const step = (index % 8) * 4;
  return { x: 6 + step, y: 6 + step, width: 58, height: 58 };
}

export function windowManagerOf(node: LayoutNode): WindowManager {
  if (node.type !== "split") return "tiling";
  if (node.direction === "scrolling") return "scrolling";
  if (node.direction === "floating") return "floating";
  return "tiling";
}

/** Every leaf, in traversal order — the order content is carried in. */
function leaves(node: LayoutNode): LayoutLeaf[] {
  if (node.type === "leaf") return [node];
  return node.children.flatMap((child) => leaves(child.node));
}

/**
 * The same windows under a different manager.
 *
 * Nesting cannot survive the trip — a strip has no room for a sub-split, and
 * a floating frame is not a fraction of anything — so the tree is flattened to
 * its leaves and rebuilt. Leaf order is preserved, which is what lets
 * `carryAssignmentsToPanes` put every occupant back where it was.
 *
 * A single window is left alone under `tiling`: a tiling manager with one window
 * is just that window, and wrapping it in a split would draw a divider with
 * nothing on the other side.
 */
export function toWindowManager(
  node: LayoutNode,
  manager: WindowManager,
): LayoutNode {
  const panes = leaves(node);
  if (manager === "tiling") {
    if (panes.length === 1) return panes[0];
    return {
      type: "split",
      direction: "horizontal",
      children: panes.map((leaf) => ({ node: leaf, weight: 1 })),
    };
  }
  if (manager === "scrolling") {
    return {
      type: "split",
      direction: "scrolling",
      children: panes.map((leaf) => ({
        node: leaf,
        weight: SCROLLING_DEFAULT_WIDTH,
      })),
    };
  }
  return {
    type: "split",
    direction: "floating",
    children: panes.map((leaf, index) => ({
      node: leaf,
      weight: 1,
      rect: cascadeRect(index),
    })),
  };
}

/** The next manager in the cycle Ctrl+B m walks. */
export function nextWindowManager(current: WindowManager): WindowManager {
  return current === "tiling"
    ? "scrolling"
    : current === "scrolling"
      ? "floating"
      : "tiling";
}

/**
 * A floating child's frame, clamped so a window can never be dragged fully
 * off-screen: at least this much of it stays reachable by the pointer.
 */
export function clampRect(rect: LayoutRect): LayoutRect {
  const width = Math.min(200, Math.max(8, rect.width));
  const height = Math.min(200, Math.max(6, rect.height));
  const keep = 6;
  return {
    width,
    height,
    x: Math.min(100 - keep, Math.max(keep - width, rect.x)),
    y: Math.min(100 - keep, Math.max(0, rect.y)),
  };
}

/** Replace one child of a split, by index, without touching the others. */
export function withChild(
  split: LayoutSplit,
  index: number,
  update: (child: LayoutChild) => LayoutChild,
): LayoutSplit {
  return {
    ...split,
    children: split.children.map((child, at) =>
      at === index ? update(child) : child,
    ),
  };
}

// ---------------------------------------------------------------------------
// Surface assignment helpers
// ---------------------------------------------------------------------------

const SURFACE_PREFIX = "surface:";

/** Create a layout assignment value representing a compositor surface.
 *  Format: "surface:<connectionId>:<surfaceId>" */
export function surfaceAssignment(
  connectionId: string,
  surfaceId: SurfaceId,
): string {
  if (surfaceId <= 0n || surfaceId > 0xffff_ffff_ffff_ffffn) {
    throw new RangeError("surface id is outside the native u64 range");
  }
  return `${SURFACE_PREFIX}${connectionId}:${surfaceId}`;
}

/** Check whether a layout assignment value represents a surface. */
export function isSurfaceAssignment(value: string | null): boolean {
  return value != null && value.startsWith(SURFACE_PREFIX);
}

/** Extract the opaque native surface ID from an assignment string, or null. */
export function parseSurfaceAssignment(
  value: string | null,
): { connectionId: string; surfaceId: SurfaceId } | null {
  if (value == null || !value.startsWith(SURFACE_PREFIX)) return null;
  const rest = value.slice(SURFACE_PREFIX.length);
  const colon = rest.lastIndexOf(":");
  if (colon <= 0) return null;
  const connectionId = rest.slice(0, colon);
  const id = rest.slice(colon + 1);
  if (id.length === 0 || id.length > 20 || !/^\d+$/.test(id)) return null;
  const surfaceId = BigInt(id);
  if (surfaceId === 0n || surfaceId > 0xffff_ffff_ffff_ffffn) return null;
  return { connectionId, surfaceId };
}

// IDE tiles (docs/ide-plan.md PR-6/7): non-session, non-surface pane content
// — a CodeMirror editor or a git diff — dispatched by assignment shape like
// surfaces. The argument is a filesystem path, so it may contain ":" and "/":
// the parser splits only the leading "<kind>:<conn>:" and keeps the rest
// verbatim (unlike the surface parser's lastIndexOf, which would corrupt it).
const EDITOR_PREFIX = "editor:";
const DIFF_PREFIX = "diff:";
const COMMIT_PREFIX = "commit:";
const PREVIEW_PREFIX = "preview:";
const MANAGE_PREFIX = "manage:";

/** layout assignment for an editor tile: "editor:<connectionId>:<path>". */
export function editorAssignment(connectionId: string, path: string): string {
  return `${EDITOR_PREFIX}${connectionId}:${path}`;
}

/** layout assignment for a rendered preview of a file:
 *  "preview:<connectionId>:<path>". Same shape as an editor tile — it is
 *  the same file, shown rendered instead of as source, and the view
 *  switcher flips between them. */
export function previewAssignment(connectionId: string, path: string): string {
  return `${PREVIEW_PREFIX}${connectionId}:${path}`;
}

/** layout assignment for a git diff tile: "diff:<connectionId>:<path>" for the
 *  unstaged (INDEX×WORKTREE) diff, or ":staged:<path>" for the staged
 *  (HEAD×INDEX) diff. `path` is absolute (starts with "/"), so the "staged:"
 *  marker is unambiguous. */
/** Which endpoints a diff tile compares.
 *  - "unstaged":  INDEX×WORKTREE (tracked, unstaged edits)
 *  - "staged":    HEAD×INDEX (git diff --cached)
 *  - "untracked": INDEX×WORKTREE + untracked walk (a new file, shown added)
 *  - "worktree":  HEAD×WORKTREE (all changes since HEAD, staged + unstaged) */
export type DiffSide = "unstaged" | "staged" | "untracked" | "worktree";

export function diffAssignment(
  connectionId: string,
  path: string,
  side: DiffSide = "unstaged",
): string {
  const prefix = side === "unstaged" ? "" : `${side}:`;
  return `${DIFF_PREFIX}${connectionId}:${prefix}${path}`;
}

/** Decode a diff tile's arg into { side, staged, path }. `staged` is kept as a
 *  convenience alias for `side === "staged"`. */
export function parseDiffArg(arg: string): {
  side: DiffSide;
  staged: boolean;
  path: string;
} {
  for (const side of ["staged", "untracked", "worktree"] as const) {
    const prefix = `${side}:`;
    if (arg.startsWith(prefix)) {
      return {
        side,
        staged: side === "staged",
        path: arg.slice(prefix.length),
      };
    }
  }
  return { side: "unstaged", staged: false, path: arg };
}

/** layout assignment for a server's own panels — what its session supervisor
 *  runs, who is connected, its units, its extensions: "manage:<connectionId>:".
 *
 *  The trailing colon is not decoration: `parseTileAssignment` splits on the
 *  first ":" after the prefix, and a manage tile has nothing to say after its
 *  connection. Keeping the shape means every kind-agnostic path (the hash
 *  writer, the tab registry, drop handling) treats it like any other tile.
 *
 *  There is one per connection by construction, so opening Manage twice lands
 *  on the same tile rather than accumulating panels that each hold a live
 *  client watch. */
export function manageAssignment(connectionId: string): string {
  return `${MANAGE_PREFIX}${connectionId}:`;
}

/** layout assignment for a commit tile: "commit:<connectionId>:<oid>:<repoPath>".
 *  `oid` is hex (no ":"), so the first ":" of the arg splits oid from repo. */
export function commitAssignment(
  connectionId: string,
  oid: string,
  repoPath: string,
): string {
  return `${COMMIT_PREFIX}${connectionId}:${oid}:${repoPath}`;
}

/** True when the assignment is an editor/diff/commit/manage tile (not a
 *  session). */
export function isTileAssignment(value: string | null): boolean {
  return (
    value != null &&
    (value.startsWith(EDITOR_PREFIX) ||
      value.startsWith(DIFF_PREFIX) ||
      value.startsWith(COMMIT_PREFIX) ||
      value.startsWith(PREVIEW_PREFIX) ||
      value.startsWith(MANAGE_PREFIX))
  );
}

/** True when the assignment names pane content rather than a terminal session
 *  — a surface, an IDE tile, or a web pane. Anything that answers true here
 *  must be kept out of session assignment and focus bookkeeping. */
export function isContentAssignment(value: string | null): boolean {
  return (
    isSurfaceAssignment(value) ||
    isTileAssignment(value) ||
    isWebAssignment(value)
  );
}

export interface TileAssignment {
  kind: "editor" | "diff" | "commit" | "preview" | "manage";
  connectionId: string;
  /** Verbatim argument (a path, "<oid>:<repoPath>" for commit, empty for
   *  manage — the connection is the whole address). */
  arg: string;
}

/** Parse an editor/diff/commit tile assignment, or null. */
export function parseTileAssignment(
  value: string | null,
): TileAssignment | null {
  let kind: TileAssignment["kind"];
  let prefix: string;
  if (value != null && value.startsWith(EDITOR_PREFIX)) {
    kind = "editor";
    prefix = EDITOR_PREFIX;
  } else if (value != null && value.startsWith(DIFF_PREFIX)) {
    kind = "diff";
    prefix = DIFF_PREFIX;
  } else if (value != null && value.startsWith(COMMIT_PREFIX)) {
    kind = "commit";
    prefix = COMMIT_PREFIX;
  } else if (value != null && value.startsWith(PREVIEW_PREFIX)) {
    kind = "preview";
    prefix = PREVIEW_PREFIX;
  } else if (value != null && value.startsWith(MANAGE_PREFIX)) {
    kind = "manage";
    prefix = MANAGE_PREFIX;
  } else {
    return null;
  }
  const rest = value.slice(prefix.length);
  const colon = rest.indexOf(":");
  if (colon <= 0) return null;
  return {
    kind,
    connectionId: rest.slice(0, colon),
    arg: rest.slice(colon + 1),
  };
}

// ---------------------------------------------------------------------------
// Web pane assignment helpers
// ---------------------------------------------------------------------------

// A web pane is an iframe onto something the server can reach — a dev server,
// an internal dashboard — served through the preview service worker
// (docs/design/net.md). Same dispatch-by-assignment-shape as surfaces and IDE
// tiles. The argument is a URL, so it contains ":" and "/" and the parser
// splits only the leading "web:<conn>:" and keeps the rest verbatim.
const WEB_PREFIX = "web:";

/** layout assignment for a web pane: "web:<connectionId>:<url>". */
export function webAssignment(connectionId: string, url: string): string {
  return `${WEB_PREFIX}${connectionId}:${url}`;
}

/** Check whether a layout assignment value represents a web pane. */
export function isWebAssignment(value: string | null): boolean {
  return value != null && value.startsWith(WEB_PREFIX);
}

/** Parse a web pane assignment into its connection and URL, or null. */
export function parseWebAssignment(
  value: string | null,
): { connectionId: string; url: string } | null {
  if (value == null || !value.startsWith(WEB_PREFIX)) return null;
  const rest = value.slice(WEB_PREFIX.length);
  const colon = rest.indexOf(":");
  if (colon <= 0) return null;
  const url = rest.slice(colon + 1);
  if (!url) return null;
  return { connectionId: rest.slice(0, colon), url };
}

export function enumeratePanes(
  node: LayoutNode,
  path: readonly number[] = [],
): LayoutPane[] {
  if (node.type === "leaf") {
    return [
      {
        id: path.length > 0 ? path.join(".") : "0",
        leaf: node,
      },
    ];
  }
  return node.children.flatMap((child, index) =>
    enumeratePanes(child.node, [...path, index]),
  );
}

export function assignSessionsToPanes(
  panes: readonly LayoutPane[],
  orderedSessionIds: readonly string[],
): LayoutAssignments {
  const assignments: Record<string, string | null> = {};
  let sessionIdx = 0;
  for (const pane of panes) {
    if (pane.leaf.command) {
      assignments[pane.id] = null;
    } else {
      assignments[pane.id] = orderedSessionIds[sessionIdx++] ?? null;
    }
  }
  return { assignments };
}

/** Carry the occupants of an existing layout into a replacement layout.
 *
 * Content migrates in pane traversal order. Every assignment has one visual
 * owner, so duplicate records left by an old restore or a racy drag collapse
 * to their first pane. Crucially, this does not append other live sessions:
 * panes added by the replacement layout stay empty until the user puts
 * something in them. */
export function carryAssignmentsToPanes({
  currentPanes,
  nextPanes,
  previous,
  liveSessionIds,
}: {
  currentPanes: readonly LayoutPane[];
  nextPanes: readonly LayoutPane[];
  previous: LayoutAssignments;
  liveSessionIds: readonly string[];
}): LayoutAssignments {
  const live = new Set(liveSessionIds);
  const seen = new Set<string>();
  const carried: string[] = [];

  for (const pane of currentPanes) {
    const value = previous.assignments[pane.id];
    if (value == null || seen.has(value)) continue;
    if (isContentAssignment(value)) {
      seen.add(value);
      carried.push(value);
    } else if (live.has(value)) {
      seen.add(value);
      carried.push(value);
    }
  }

  return assignSessionsToPanes(nextPanes, carried);
}

export function buildCandidateOrder({
  liveSessionIds,
  focusedSessionId,
  currentAssignedInPaneOrder = [],
  lruSessionIds = [],
}: {
  liveSessionIds: readonly string[];
  focusedSessionId: string | null;
  currentAssignedInPaneOrder?: readonly string[];
  lruSessionIds?: readonly string[];
}): string[] {
  const live = new Set(liveSessionIds);
  const seen = new Set<string>();
  const ordered: string[] = [];

  const push = (sessionId: string | null | undefined) => {
    if (!sessionId || !live.has(sessionId) || seen.has(sessionId)) return;
    seen.add(sessionId);
    ordered.push(sessionId);
  };

  push(focusedSessionId);
  currentAssignedInPaneOrder.forEach(push);
  lruSessionIds.forEach(push);
  liveSessionIds.forEach(push);

  return ordered;
}

/**
 * The assignments after a dropped `value` lands in `targetPaneId`, or `null`
 * when nothing changes.
 *
 * A drop that names the pane the drag left (`fromPaneId` — a pane's ✕
 * doubling as its drag handle) is a *move*, not another open: the source
 * pane takes what the target held, so the content lands in exactly one pane,
 * and dropping on an empty pane is a plain move. Gated on the source still
 * holding the dragged value — a layout change mid-drag must not evict
 * whatever else got there since.
 *
 * Every assignment is a unique visual owner. Recover its source from the
 * current assignments if a browser omits the secondary source-pane drag MIME
 * (or if that pane id went stale); otherwise a dropped terminal, surface, or
 * panel becomes two floating windows.
 */
export function assignmentsAfterDrop(
  prev: Readonly<Record<string, string | null>>,
  value: string,
  targetPaneId: string,
  fromPaneId: string | undefined,
  validPaneIds: readonly string[],
): Record<string, string | null> | null {
  const markedSourceIsCurrent =
    fromPaneId !== undefined &&
    fromPaneId !== targetPaneId &&
    validPaneIds.includes(fromPaneId) &&
    prev[fromPaneId] === value;
  const sourcePaneId = markedSourceIsCurrent
    ? fromPaneId
    : validPaneIds.find(
        (paneId) => paneId !== targetPaneId && prev[paneId] === value,
      );
  const swap = sourcePaneId !== undefined;
  if (prev[targetPaneId] === value && !swap) return null;
  const next: Record<string, string | null> = {
    ...prev,
    [targetPaneId]: value,
  };
  if (sourcePaneId !== undefined)
    next[sourcePaneId] = prev[targetPaneId] ?? null;
  return next;
}

export function reconcileAssignments({
  panes,
  previous,
  liveSessionIds,
  knownSessionIds,
  liveSurfaceKeys,
  readyConnectionIds,
  sessionReplacements,
  sessionConnectionIds,
}: {
  panes: readonly LayoutPane[];
  previous: LayoutAssignments;
  liveSessionIds: readonly string[];
  knownSessionIds: readonly string[];
  /** When provided, surface assignments for destroyed surfaces are cleared.
   *  Each key is "connectionId:surfaceId". */
  liveSurfaceKeys?: readonly string[];
  /** Connections that are both present AND ready.  Surface assignments
   *  whose connection is absent OR not yet ready (reconnecting) are
   *  preserved — the surface may reappear once the connection finishes
   *  its handshake or is re-added. */
  readyConnectionIds?: ReadonlySet<string>;
  /** Maps old (closed) session IDs to replacement live session IDs.
   *  Used to re-map pane assignments after a reconnect where PTYs get
   *  new session IDs but represent the same underlying terminal. */
  sessionReplacements?: ReadonlyMap<string, string>;
  /** Maps session IDs to their owning connection ID.  Used together with
   *  `readyConnectionIds` to preserve terminal assignments whose
   *  connection is absent or still reconnecting — mirroring the surface
   *  assignment protection so terminals survive reconnect cycles too. */
  sessionConnectionIds?: ReadonlyMap<string, string>;
}): LayoutAssignments {
  const live = new Set(liveSessionIds);
  const known = new Set(knownSessionIds);
  const liveSurfaces = liveSurfaceKeys ? new Set(liveSurfaceKeys) : null;
  const assignments: Record<string, string | null> = {};
  const seen = new Set<string>();

  for (const pane of panes) {
    const value = previous.assignments[pane.id];
    if (value != null && seen.has(value)) {
      assignments[pane.id] = null;
      continue;
    }
    if (value != null && isSurfaceAssignment(value)) {
      if (liveSurfaces) {
        const parsed = parseSurfaceAssignment(value);
        const key =
          parsed != null ? `${parsed.connectionId}:${parsed.surfaceId}` : null;
        if (key != null && liveSurfaces.has(key)) {
          // Surface is live — keep.
          assignments[pane.id] = value;
          seen.add(value);
        } else if (
          parsed &&
          readyConnectionIds &&
          !readyConnectionIds.has(parsed.connectionId)
        ) {
          // Surface's connection is absent or still reconnecting —
          // preserve the assignment so it survives until the connection
          // is fully ready (or re-added).
          assignments[pane.id] = value;
          seen.add(value);
        } else {
          // Surface is gone and its connection is present+ready — clear.
          assignments[pane.id] = null;
        }
      } else {
        assignments[pane.id] = value;
        seen.add(value);
      }
      continue;
    }
    if (value != null && !live.has(value)) {
      // The assigned session is gone. Try to replace it with a live
      // session for the same underlying PTY (reconnect gave it a new ID).
      const replacement = sessionReplacements?.get(value);
      if (replacement && live.has(replacement)) {
        if (seen.has(replacement)) {
          assignments[pane.id] = null;
        } else {
          assignments[pane.id] = replacement;
          seen.add(replacement);
        }
        continue;
      }
      // Session's connection is absent or still reconnecting — preserve
      // the assignment so it survives until the connection is fully
      // ready (or re-added), mirroring the surface protection above.
      if (readyConnectionIds && sessionConnectionIds) {
        const connId = sessionConnectionIds.get(value);
        if (connId != null && !readyConnectionIds.has(connId)) {
          assignments[pane.id] = value;
          seen.add(value);
          continue;
        }
      }
    }
    const keep = value != null && (live.has(value) || !known.has(value));
    assignments[pane.id] = keep ? value : null;
    if (keep) seen.add(value);
  }

  return { assignments };
}

export function adjustWeights(
  split: LayoutSplit,
  indexA: number,
  indexB: number,
  fraction: number,
): LayoutSplit {
  const totalWeight =
    split.children[indexA].weight + split.children[indexB].weight;
  const delta = fraction * totalWeight;
  const minWeight = 0.1;

  const newA = Math.max(minWeight, split.children[indexA].weight + delta);
  const newB = Math.max(minWeight, split.children[indexB].weight - delta);

  const children: LayoutChild[] = split.children.map((c, i) => {
    if (i === indexA) return { ...c, weight: newA };
    if (i === indexB) return { ...c, weight: newB };
    return c;
  });

  return { ...split, children };
}

export function layoutFromDSL(dsl: string): WorkspaceLayout {
  const { root, weight } = parseDSL(dsl);
  return { name: dsl, dsl, root, weight };
}
