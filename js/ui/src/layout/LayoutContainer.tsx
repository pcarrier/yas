/**
 * The workspace layout: one tree, three window managers.
 *
 * What a split does with its children is the manager. `line`/`col`/`tabs`
 * tile; `scroll` lays a strip out wider than the viewport and follows the
 * focus along it; `float` places each child by its own frame. Everything
 * below the root is the same recursion either way, which is why a strip
 * column can be a stack and a floating window can hold a tiling tree.
 *
 * Ctrl+B m hands the same windows to the next manager (`cycleWindowManager`),
 * and the keys that only mean something with a tree on screen are registered
 * from here so they are unbound when there is not one.
 */
import {
  createSignal,
  createEffect,
  createMemo,
  onCleanup,
  untrack,
  batch,
  Show,
  For,
  Index,
  type JSX,
} from "solid-js";
import {
  YasTerminal,
  YasSurfaceView,
  createYasWorkspace,
  createYasSessions,
  createYasWorkspaceState,
} from "@yas-run/solid";
import type {
  YasTerminalSurface,
  SessionId,
  TerminalPalette,
  YasSurface,
} from "@yas-run/core";
import type {
  LayoutNode,
  LayoutChild,
  LayoutSplit,
  LayoutLeaf,
  LayoutRect,
} from "@yas-run/core/layout";
import {
  cascadeRect,
  clampRect,
  leafCount,
  nextWindowManager,
  serializeDSL,
  toWindowManager,
  windowManagerOf,
  withChild,
} from "@yas-run/core/layout";
import type { LayoutAssignments, WorkspaceLayout } from "./store";
import {
  adjustWeights,
  assignSessionsToPanes,
  assignmentsAfterDrop,
  buildCandidateOrder,
  carryAssignmentsToPanes,
  connectionAwaitingWorkspaceRestore,
  enumeratePanes,
  parseWorkspaceRef,
  ptyIdForWorkspaceRef,
  reconcileAssignments,
  saveActiveLayout,
  surfaceIdForWorkspaceRef,
  surfaceAssignment,
  surfaceWorkspaceRefForId,
  terminalWorkspaceRefForPtyId,
  isContentAssignment,
  isSurfaceAssignment,
  isWebAssignment,
  parseSurfaceAssignment,
  parseWebAssignment,
  isTileAssignment,
  parseTileAssignment,
} from "./store";
import { YasTile } from "../ide/YasTile";
import { PaneTools } from "../PaneTools";
import { TerminalDropTarget } from "../terminalDrop";
import { WebPaneHost, type WebPaneHostRegistrar } from "../WebPaneHost";
import {
  isTileDrag,
  paneDragSource,
  tileDragAssignment,
} from "../ide/tileDrag";
import { resolveTab } from "../ide/tabRegistry";
import { ResizeHandle } from "./ResizeHandle";
import {
  LayoutTreeContext,
  autoFocusPaneTarget,
  useLayoutTree,
  type LayoutTreeCtx,
} from "./treeContext";
import type { Theme } from "../theme";
import { mergeStyle, themeFor, ui, uiScale, z } from "../theme";
import { t, tp } from "../i18n";
import { registerPrefixAction } from "../keyPrefix";
import type { SurfaceTouchMode, SurfaceZoomMode } from "../storage";
import {
  floatingLayerStackingStyle,
  floatingPaneIds,
  floatingWindowTitle,
} from "./floatingWindow";
import { removePaneFromLayout } from "./paneRemoval";

// The tree context lives in ./treeContext so its identity survives hot
// reloads of this module (see that file).

function resolveLeafFontSize(leaf: LayoutLeaf, baseFontSize: number): number {
  const raw = leaf.fontSize;
  if (raw == null) return baseFontSize;
  let resolved: number;
  if (typeof raw === "number") {
    resolved = raw;
  } else if (raw.endsWith("%")) {
    resolved = Math.round((baseFontSize * parseFloat(raw)) / 100);
  } else if (raw.endsWith("pt")) {
    resolved = Math.round((parseFloat(raw) * 4) / 3);
  } else if (raw.endsWith("px")) {
    resolved = parseFloat(raw);
  } else {
    resolved = baseFontSize;
  }
  return Math.max(6, Math.min(72, Math.round(resolved)));
}

function sameAssignments(
  left: LayoutAssignments,
  right: LayoutAssignments,
): boolean {
  const leftKeys = Object.keys(left.assignments);
  const rightKeys = Object.keys(right.assignments);
  if (leftKeys.length !== rightKeys.length) return false;
  for (const key of leftKeys) {
    if (left.assignments[key] !== right.assignments[key]) return false;
  }
  return true;
}

/** Resolve a pane id (child-index path, `enumeratePanes` scheme) to the index
 *  path of its leaf, or null when it doesn't name a leaf. */
function leafPath(node: LayoutNode, paneId: string): number[] | null {
  if (node.type === "leaf") return paneId === "0" ? [] : null;
  const path = paneId.split(".").map(Number);
  if (path.some((n) => !Number.isInteger(n))) return null;
  let cur: LayoutNode = node;
  for (const idx of path) {
    if (cur.type !== "split" || !cur.children[idx]) return null;
    cur = cur.children[idx].node;
  }
  return cur.type === "leaf" ? path : null;
}

/** Return a copy of `node` with the subtree at `path` replaced. */
function replaceNodeAtPath(
  node: LayoutNode,
  path: readonly number[],
  replacement: LayoutNode,
): LayoutNode {
  if (path.length === 0) return replacement;
  if (node.type !== "split") return node;
  const [head, ...rest] = path;
  return {
    ...node,
    children: node.children.map((child, i) =>
      i === head
        ? { ...child, node: replaceNodeAtPath(child.node, rest, replacement) }
        : child,
    ),
  };
}

export function LayoutContainer(props: {
  layout: WorkspaceLayout;
  onLayoutChange: (
    layout: WorkspaceLayout | null,
    options?: { debounceHistory?: boolean },
  ) => void;
  connectionId: string;
  connectionLabels?: Map<string, string>;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
  /** Surface zoom factor. Defaults to 1. */
  surfaceZoom?: number;
  /** Whether surface zoom is relative to display DPI or an exact scale. */
  surfaceZoomMode?: SurfaceZoomMode;
  /** Native Wayland multitouch contacts or pointer-gesture compatibility. */
  surfaceTouchMode?: SurfaceTouchMode;

  focusedSessionId: SessionId | null;
  lruSessionIds: readonly SessionId[];
  /** Live surface keys ("connectionId:surfaceId") for cleanup of dead surface assignments. */
  liveSurfaceKeys?: readonly string[];
  /** Additional session IDs to keep visible (e.g. side panel thumbnails). */
  extraVisibleSessions?: readonly SessionId[];
  manageVisibility?: boolean;
  onAssignmentsChange?: (assignments: LayoutAssignments) => void;
  /** Stable backend-session refs used to hydrate pane occupants. */
  storedAssignments?: Readonly<Record<string, string>>;
  storedFocusedPaneId?: string | null;
  /** Changes when another backend workspace session is attached. */
  restoreKey?: string;
  /** Authoritative stable refs are reported intact, including after resolve. */
  onUnresolvedAssignmentsChange?: (
    assignments: Readonly<Record<string, string>>,
  ) => void;
  /** Called when the current restore pass settles (or immediately when empty). */
  onAssignmentsResolved?: (resolved: boolean) => void;
  onFocusSession: (id: SessionId | null) => void;
  onCreateInPane?: (
    paneId: string,
    command?: string,
    connectionId?: string,
  ) => void;
  onSwitcher?: () => void;
  onHelp?: () => void;
  /** Called with control functions so the parent can direct pane focus/assignments. */
  onFocusBySession?: (fn: (sessionId: SessionId) => void) => void;
  onFocusPane?: (fn: (paneId: string) => void) => void;
  onMoveSessionToPane?: (
    fn: (sessionId: SessionId, targetPaneId: string) => void,
  ) => void;
  onMoveToPane?: (
    fn: (value: string, targetPaneId: string, fromPaneId?: string) => void,
  ) => void;
  /** Called with a function that splits a pane, placing `value` in a new
   *  pane beside the target's current occupant (which is preserved). */
  onSplitPane?: (fn: (value: string, targetPaneId: string) => void) => void;
  onClearPaneAssignment?: (fn: (paneId: string) => void) => void;
  /** Leave layout mode with its last occupant in the single main view. */
  onCollapseToSingle?: (assignment: string | null) => void;
  onFocusedPaneChange?: (paneId: string | null) => void;
  onRender?: (renderMs?: number) => void;
  /** Receives each terminal pane's surface as it mounts, so hyperlink hover
   *  and activation work in every split. */
  onTerminalSurface?: (surface: YasTerminalSurface | null) => void;
  /** Open an IDE tile from within a tile (commit view → editor). */
  onOpenTile?: (assignment: string) => void;
  /** Register visual hosts for Workspace-owned persistent web panes. */
  registerWebPaneHost?: WebPaneHostRegistrar;
  /** Drop a dragged IDE tile assignment into a specific pane. */
  onDropTile?: (
    assignment: string,
    paneId: string,
    sourcePaneId?: string,
  ) => void;
  /** Coarse pointer — keeps each pane's ✕ visible without a hover. */
  isMobileTouch?: boolean;
  /** Is this pane assignment lit by an activation (see LayoutTreeCtx)? */
  hasAttention?: (assignment: string) => boolean;
  /** Whether a session's connection is read-only (see LayoutTreeCtx). */
  isSessionReadOnly?: (sessionId: string) => boolean;
  /** Whether a whole connection is read-only (see LayoutTreeCtx). */
  isConnectionReadOnly?: (connectionId: string) => boolean;
  /** Close an IDE/web tab host-wide (Workspace owns the tab registry). */
  onCloseTab?: (assignment: string) => void;
}) {
  const workspace = createYasWorkspace();
  const workspaceState = createYasWorkspaceState(workspace);
  const sessions = createYasSessions(workspace);

  const connection = createMemo(() => {
    const snap = workspaceState();
    return snap.connections.find((c) => c.id === props.connectionId) ?? null;
  });
  // Include "authenticating" so reconciliation can run while the native HELLO
  // catalogue is settling. The per-connection `readyConnectionIds`
  // filter inside reconcileAssignments preserves assignments for connections
  // that haven't completed the handshake, so this is safe and lets surfaces
  // propagate to the UI (e.g. PreviewPanel) before the snapshot is ready.
  const connected = () => {
    const status = connection()?.status;
    return status === "connected" || status === "authenticating";
  };

  const liveSessions = createMemo(() =>
    sessions().filter((session) => session.state !== "closed"),
  );
  const liveSessionIds = createMemo(() =>
    liveSessions().map((session) => session.id),
  );

  const [root, setRoot] = createSignal(props.layout.root);
  const panes = createMemo(() => enumeratePanes(root()));
  const paneIds = createMemo(() => panes().map((pane) => pane.id));

  // The backend store carries stable PTY/surface/tab refs. Resolve them to
  // ephemeral live assignments as their remotes arrive, while retaining every
  // unresolved ref separately so a detached remote cannot erase session state.
  let pendingRefs: Record<string, string> = {
    ...(props.storedAssignments ?? {}),
  };
  const [pendingRefsRevision, setPendingRefsRevision] = createSignal(0);
  const [resolvingRefs, setResolvingRefs] = createSignal(
    Object.keys(pendingRefs).length > 0,
  );

  function touchPendingRefs(): void {
    setPendingRefsRevision((revision) => revision + 1);
  }

  function forgetPendingRef(paneId: string): void {
    if (!(paneId in pendingRefs)) return;
    delete pendingRefs[paneId];
    touchPendingRefs();
  }

  const [layoutState, setLayoutState] = createSignal<LayoutAssignments>(
    (() => {
      // Don't resolve stable assignments yet — sessions haven't arrived.
      // Start with empty assignments; the effect below will resolve them.
      if (Object.keys(pendingRefs).length > 0) {
        const assignments: Record<string, SessionId | null> = {};
        for (const paneId of paneIds()) {
          assignments[paneId] = null;
        }
        return { assignments };
      }
      const orderedSessionIds = buildCandidateOrder({
        liveSessionIds: liveSessionIds(),
        focusedSessionId: props.focusedSessionId,
        lruSessionIds: props.lruSessionIds,
      });
      return assignSessionsToPanes(panes(), orderedSessionIds);
    })(),
  );

  let lastDsl = props.layout.dsl;
  let lastLayout = props.layout;

  // React to external layout changes.
  createEffect(() => {
    const layout = props.layout;
    if (layout === lastLayout) return;

    const currentPanes = enumeratePanes(root());
    const nextRoot = layout.root;
    const nextPanes = enumeratePanes(nextRoot);
    const nextPending: Record<string, string> = {};
    for (let index = 0; index < currentPanes.length; index++) {
      const ref = pendingRefs[currentPanes[index].id];
      const target = nextPanes[index];
      if (ref && target) nextPending[target.id] = ref;
    }
    pendingRefs = nextPending;
    touchPendingRefs();

    lastLayout = layout;
    lastDsl = layout.dsl;
    setRoot(nextRoot);
    setLayoutState(
      carryAssignmentsToPanes({
        currentPanes,
        nextPanes,
        previous: layoutState(),
        liveSessionIds: liveSessionIds(),
      }),
    );
  });

  const knownSessionIds = createMemo(() => sessions().map((s) => s.id));

  // Resolve stable backend-session refs progressively. The stable ref remains
  // beside its ephemeral live assignment even after success, so a transport
  // generation can remap native handles to new browser-local aliases. Only an
  // explicit pane edit removes or moves it.
  const tabFetchesInFlight = new Set<string>();
  const settledTabLookups = new Map<string, string>();
  let restoreGeneration = 0;
  function applyResolvedTab(
    paneId: string,
    ref: string,
    generation: number,
    assignment: string | null,
  ) {
    if (generation !== restoreGeneration) return;
    if (pendingRefs[paneId] !== ref) return;
    if (assignment) {
      setLayoutState((prev) => ({
        assignments: { ...prev.assignments, [paneId]: assignment },
      }));
    }
  }

  createEffect(() => {
    pendingRefsRevision();
    const entries = Object.entries(pendingRefs);
    if (entries.length === 0) {
      setResolvingRefs(false);
      return;
    }
    const live = liveSessions();
    const snap = workspaceState();
    const resolved: Record<string, string> = {};
    let waitingForInitialRemoteState = false;
    for (const [paneId, ref] of entries) {
      const parsed = parseWorkspaceRef(ref);
      if (!parsed) continue; // Preserve refs introduced by a newer UI.
      const conn = snap.connections.find(
        (candidate) => candidate.id === parsed.connectionId,
      );
      if (connectionAwaitingWorkspaceRestore(conn)) {
        waitingForInitialRemoteState = true;
      }
      if (parsed.kind === "surface") {
        const surfaceId = surfaceIdForWorkspaceRef(parsed);
        const assignment = surfaceAssignment(parsed.connectionId, surfaceId);
        resolved[paneId] = assignment;
        continue;
      }
      if (parsed.kind === "terminal") {
        const ptyId = ptyIdForWorkspaceRef(parsed);
        const session = live.find(
          (candidate) =>
            candidate.connectionId === parsed.connectionId &&
            candidate.ptyId === ptyId,
        );
        if (session) {
          resolved[paneId] = session.id;
        }
        continue;
      }
      if (
        conn?.supportsKv &&
        (conn.status === "connected" || conn.status === "authenticating") &&
        !tabFetchesInFlight.has(paneId) &&
        pendingRefs[paneId] === ref &&
        settledTabLookups.get(paneId) !== `${ref}\u0000${conn.generation}`
      ) {
        const generation = restoreGeneration;
        const lookupKey = `${ref}\u0000${conn.generation}`;
        tabFetchesInFlight.add(paneId);
        waitingForInitialRemoteState = true;
        resolveTab(workspace, parsed.connectionId, parsed.tabId)
          .then((assignment) => {
            settledTabLookups.set(paneId, lookupKey);
            applyResolvedTab(paneId, ref, generation, assignment);
          })
          .catch(() => {
            // A reconnect will change the workspace snapshot and retry. The
            // stable ref remains authoritative meanwhile.
            settledTabLookups.set(paneId, lookupKey);
          })
          .finally(() => {
            if (generation !== restoreGeneration) return;
            tabFetchesInFlight.delete(paneId);
            touchPendingRefs();
          });
      }
    }

    if (Object.keys(resolved).length > 0) {
      setLayoutState((prev) => ({
        assignments: { ...prev.assignments, ...resolved },
      }));
    }
    setResolvingRefs(
      waitingForInitialRemoteState || tabFetchesInFlight.size > 0,
    );
  });

  // Capture identities for assignments created after hydration as soon as
  // their native mapping is available. This closes the reconnect window
  // between a user's pane edit and the next backend patch.
  createEffect(() => {
    pendingRefsRevision();
    const liveById = new Map(
      liveSessions().map((session) => [session.id, session]),
    );
    let changed = false;
    for (const [paneId, assignment] of Object.entries(
      layoutState().assignments,
    )) {
      if (!assignment || paneId in pendingRefs) continue;
      const surface = parseSurfaceAssignment(assignment);
      if (surface) {
        pendingRefs[paneId] = surfaceWorkspaceRefForId(
          surface.connectionId,
          surface.surfaceId,
        );
        changed = true;
        continue;
      }
      const session = liveById.get(assignment);
      if (!session || session.ptyId == null) continue;
      pendingRefs[paneId] = terminalWorkspaceRefForPtyId(
        session.connectionId,
        session.ptyId,
      );
      changed = true;
    }
    if (changed) touchPendingRefs();
  });

  // Durable mapping from session ID → native terminal identity. Survives
  // connection removal so that when a remote is re-added we can remap stale
  // pane assignments to newly created browser aliases for the same terminal.
  const durableSessionKeys = new Map<
    string,
    { connectionId: string; ref: string }
  >();

  // Single memo that builds both the session-replacement map (closed →
  // live session ID for the same PTY) and the session→connectionId map
  // (including entries for removed connections).  Both share the same
  // durableSessionKeys bookkeeping, so computing them together avoids
  // iterating sessions() twice.
  const sessionMaps = createMemo(() => {
    const allSessions = sessions();
    // Record every session we've ever seen so we can remap after a
    // remove-then-readd of a connection.
    for (const s of allSessions) {
      if (s.ptyId != null) {
        durableSessionKeys.set(s.id, {
          connectionId: s.connectionId,
          ref: terminalWorkspaceRefForPtyId(s.connectionId, s.ptyId),
        });
      }
    }
    const liveByKey = new Map<string, string>();
    const connectionIds = new Map<string, string>();
    for (const s of allSessions) {
      connectionIds.set(s.id, s.connectionId);
      if (s.state !== "closed") {
        const key = durableSessionKeys.get(s.id)?.ref;
        if (key) liveByKey.set(key, s.id);
      }
    }
    const replacements = new Map<string, string>();
    for (const s of allSessions) {
      if (s.state === "closed") {
        const key = durableSessionKeys.get(s.id)?.ref;
        const replacement = key ? liveByKey.get(key) : undefined;
        if (replacement && replacement !== s.id) {
          replacements.set(s.id, replacement);
        }
      }
    }
    // Remap sessions that were completely removed (connection destroyed)
    // but whose underlying PTY now has a live session again.  Also fill
    // in connectionIds for removed sessions.
    const currentIds = new Set(allSessions.map((s) => s.id));
    for (const [oldId, key] of durableSessionKeys) {
      if (!currentIds.has(oldId)) {
        if (!replacements.has(oldId)) {
          const replacement = liveByKey.get(key.ref);
          if (replacement) replacements.set(oldId, replacement);
        }
        connectionIds.set(oldId, key.connectionId);
      }
    }
    return { replacements, connectionIds };
  });

  createEffect(() => {
    if (!connected()) return;
    // Skip reconciliation while the initial backend-session restore is still
    // waiting on a present remote's first catalogue.
    if (resolvingRefs()) return;
    const p = panes();
    const live = liveSessionIds();
    const known = knownSessionIds();
    const surfaceKeys = props.liveSurfaceKeys;
    const { replacements, connectionIds: sessionConns } = sessionMaps();
    // Only include connections that are both present AND ready.  A
    // connection that is present but not ready (reconnecting) has its
    // surface list momentarily empty — treating it as "ready" would
    // cause reconciliation to nuke surface assignments that will
    // reappear once the handshake finishes.
    const readyConns = new Set(
      workspaceState()
        .connections.filter((c) => c.ready)
        .map((c) => c.id),
    );
    setLayoutState((previous) => {
      const next = reconcileAssignments({
        panes: p,
        previous,
        liveSessionIds: live,
        knownSessionIds: known,
        liveSurfaceKeys: surfaceKeys,
        readyConnectionIds: readyConns,
        sessionReplacements: replacements,
        sessionConnectionIds: sessionConns,
      });
      return sameAssignments(previous, next) ? previous : next;
    });
  });

  // LayoutContainer does not discover surfaces on its own; callers assign them
  // explicitly via moveToPane.

  const assignedInPaneOrder = createMemo(() =>
    paneIds()
      .map((paneId) => layoutState().assignments[paneId])
      .filter((v): v is SessionId => v != null && !isContentAssignment(v)),
  );

  // focusedPaneId is the single source of truth for which pane is active.
  const [focusedPaneId, setFocusedPaneId] = createSignal<string | null>(
    (() => {
      const stored = props.storedFocusedPaneId;
      if (stored && paneIds().includes(stored)) return stored;
      if (!props.focusedSessionId) return paneIds()[0] ?? null;
      return (
        paneIds().find(
          (id) => layoutState().assignments[id] === props.focusedSessionId,
        ) ??
        paneIds()[0] ??
        null
      );
    })(),
  );

  // A new attached workspace session can reuse the same component instance.
  // Reset its ephemeral assignments atomically and begin a fresh resolution
  // generation so a late tab lookup from the previous session cannot land.
  let lastRestoreKey = props.restoreKey;
  createEffect(() => {
    const restoreKey = props.restoreKey;
    if (restoreKey === lastRestoreKey) return;
    lastRestoreKey = restoreKey;
    restoreGeneration += 1;
    tabFetchesInFlight.clear();
    settledTabLookups.clear();
    pendingRefs = { ...(props.storedAssignments ?? {}) };
    touchPendingRefs();
    setResolvingRefs(Object.keys(pendingRefs).length > 0);
    const ids = paneIds();
    const assignments: Record<string, null> = {};
    for (const id of ids) assignments[id] = null;
    batch(() => {
      setLayoutState({ assignments });
      const storedFocus = props.storedFocusedPaneId;
      setFocusedPaneId(
        storedFocus && ids.includes(storedFocus)
          ? storedFocus
          : (ids[0] ?? null),
      );
    });
  });

  /**
   * The soloed pane: rendered filling the workspace, siblings hidden.
   *
   * Hidden, not unmounted, and the tree is never rewritten. Both matter.
   * Replacing `root()` with the soloed subtree would renumber every pane id
   * (they are positional paths — see `enumeratePanes`) and unmount the
   * siblings, disposing terminal surfaces and resetting editors; a one-child
   * split is not even expressible in the DSL. Hiding costs nothing to undo.
   *
   * Not persisted, like the PaneTools corner: outliving a hover is the point,
   * surviving a reload is not.
   */
  const [soloedPaneId, setSoloedPaneId] = createSignal<string | null>(null);
  function toggleSolo(paneId: string) {
    // Nothing to solo against in a single-pane layout.
    if (paneIds().length < 2) return;
    setSoloedPaneId((cur) => (cur === paneId ? null : paneId));
    focusPane(paneId);
  }
  // A pane id only means something against the tree that minted it, so any
  // change of shape drops the solo rather than soloing whatever now sits at
  // that path.
  createEffect(() => {
    const ids = paneIds();
    const solo = untrack(soloedPaneId);
    if (solo && (!ids.includes(solo) || ids.length < 2)) setSoloedPaneId(null);
  });

  // Derive the focused session from the focused pane.
  // Returns null if the pane holds a surface rather than a session.
  const focusedPaneSessionId = createMemo(() => {
    const fpId = focusedPaneId();
    if (!fpId) return null;
    const value = layoutState().assignments[fpId] ?? null;
    return value && !isContentAssignment(value) ? value : null;
  });

  // Keep focusedPaneId valid when panes change.
  createEffect(() => {
    const fpId = focusedPaneId();
    if (fpId != null && !paneIds().includes(fpId)) {
      setFocusedPaneId(paneIds()[0] ?? null);
    }
  });

  // Push our derived session up to Workspace.
  createEffect(() => {
    if (resolvingRefs()) return;
    const fpSessionId = focusedPaneSessionId();
    if (fpSessionId !== props.focusedSessionId) {
      props.onFocusSession(fpSessionId);
    }
  });

  // Allow Workspace to focus a specific session's pane (e.g. from menu).
  // If the session is already visible in a pane, focus that pane.
  // Otherwise swap it into the currently focused pane so sidebar clicks work.
  function focusBySession(sessionId: SessionId) {
    const paneId = paneIds().find(
      (id) => layoutState().assignments[id] === sessionId,
    );
    if (paneId) {
      setFocusedPaneId(paneId);
    } else {
      const fpId = focusedPaneId();
      if (fpId) moveToPane(sessionId, fpId);
    }
  }

  createEffect(() => {
    props.onFocusBySession?.(focusBySession);
  });

  function moveToPane(
    value: string,
    targetPaneId: string,
    fromPaneId?: string,
  ) {
    // Guard against a stale pane id (e.g. a caller still holding a pane path
    // from a previous layout): writing the tile to a non-existent pane would
    // silently render nothing. Fall back to the focused pane, then the first.
    const valid = paneIds();
    let pane = targetPaneId;
    if (!valid.includes(pane)) {
      const fp = focusedPaneId();
      pane = fp && valid.includes(fp) ? fp : (valid[0] ?? targetPaneId);
    }
    const markedSource =
      fromPaneId &&
      fromPaneId !== pane &&
      valid.includes(fromPaneId) &&
      layoutState().assignments[fromPaneId] === value
        ? fromPaneId
        : undefined;
    const effectiveSource =
      markedSource ??
      (isSurfaceAssignment(value)
        ? valid.find(
            (paneId) =>
              paneId !== pane && layoutState().assignments[paneId] === value,
          )
        : undefined);
    const sourcePending = effectiveSource
      ? pendingRefs[effectiveSource]
      : undefined;
    const targetPending = pendingRefs[pane];
    // Batched (like splitPane): unbatched, the assignment write flushes
    // independently from its stable identity and lets the identity-capture
    // effect restore the old occupant over a drop. The still-focused OLD
    // pane's focus effect can also re-assert DOM focus before focus moves,
    // stealing the caret on every cross-pane open (Explorer click, dock
    // restore). Publish the ref edit, assignment and focus as one state.
    batch(() => {
      forgetPendingRef(pane);
      if (effectiveSource && sourcePending) {
        pendingRefs[pane] = sourcePending;
        if (targetPending) pendingRefs[effectiveSource] = targetPending;
        else delete pendingRefs[effectiveSource];
        touchPendingRefs();
      }
      setLayoutState((prev) => {
        const assignments = assignmentsAfterDrop(
          prev.assignments,
          value,
          pane,
          fromPaneId,
          valid,
        );
        return assignments ? { ...prev, assignments } : prev;
      });
      setFocusedPaneId(pane);
    });
  }

  function moveSessionToPane(sessionId: SessionId, targetPaneId: string) {
    moveToPane(sessionId, targetPaneId);
  }

  createEffect(() => {
    props.onMoveSessionToPane?.(moveSessionToPane);
  });
  createEffect(() => {
    props.onMoveToPane?.(moveToPane);
  });

  // Split the target pane in two, keeping its current occupant in the first
  // child and placing `value` in a new second child (so opening a tile never
  // evicts the terminal). Inserting a split at a leaf's path only changes that
  // leaf's own pane id (siblings keep their index paths), so the only
  // assignment that must be rekeyed is the target's — from `targetPaneId` to
  // `<path>.0`; the new pane `<path>.1` gets `value`.
  function splitPane(
    value: string | null,
    targetPaneId: string,
    direction: "horizontal" | "vertical" = "horizontal",
  ) {
    const cur = root();
    const path = leafPath(cur, targetPaneId);
    if (path === null) {
      // Can't locate the pane in the tree — fall back to a plain replace.
      if (value != null) moveToPane(value, targetPaneId);
      return;
    }
    let oldLeaf: LayoutNode = cur;
    for (const idx of path) {
      oldLeaf = (oldLeaf as LayoutSplit).children[idx].node;
    }
    const newLeaf: LayoutLeaf = { type: "leaf" };
    const split: LayoutSplit = {
      type: "split",
      direction,
      children: [
        { node: oldLeaf, weight: 1 },
        { node: newLeaf, weight: 1 },
      ],
    };
    const newRoot = replaceNodeAtPath(cur, path, split);
    const oldNewId = [...path, 0].join(".");
    const newId = [...path, 1].join(".");
    const targetPending = pendingRefs[targetPaneId];
    forgetPendingRef(newId);
    if (targetPending && oldNewId !== targetPaneId) {
      delete pendingRefs[targetPaneId];
      pendingRefs[oldNewId] = targetPending;
      touchPendingRefs();
    }
    // Batch: the target pane's id changes during the split, so root and
    // assignments must flush in one reactive cycle. Otherwise the intermediate
    // state (rekeyed assignment, stale panes) transiently hides the sibling
    // terminal and flip-flops focus (delegated event handlers aren't batched).
    batch(() => {
      setLayoutState((prev) => {
        const assignments = { ...prev.assignments };
        const occupant = assignments[targetPaneId];
        if (oldNewId !== targetPaneId) {
          delete assignments[targetPaneId];
          if (occupant != null) assignments[oldNewId] = occupant;
        }
        assignments[newId] = value;
        return { ...prev, assignments };
      });
      updateRoot(newRoot);
      setFocusedPaneId(newId);
    });
  }

  createEffect(() => {
    props.onSplitPane?.(splitPane);
  });

  function clearPaneAssignment(paneId: string) {
    // The stable-ref capture effect observes both signals. Publish their
    // removal atomically or it can see the old occupant after the ref is
    // forgotten and immediately recreate the ref we just removed.
    batch(() => {
      forgetPendingRef(paneId);
      if (untrack(soloedPaneId) === paneId) setSoloedPaneId(null);
      setLayoutState((prev) => {
        if (prev.assignments[paneId] == null) return prev;
        return {
          ...prev,
          assignments: { ...prev.assignments, [paneId]: null },
        };
      });
    });
  }

  createEffect(() => {
    props.onClearPaneAssignment?.(clearPaneAssignment);
  });

  /** Close one assignment using the same resource-specific dispatch as the
   * global close action. The caller removes its layout leaf separately. */
  function closeAssignment(assign: string | null) {
    if (assign == null) return;
    if (isTileAssignment(assign) || isWebAssignment(assign)) {
      props.onCloseTab?.(assign);
      return;
    }
    if (isSurfaceAssignment(assign)) {
      const parsed = parseSurfaceAssignment(assign);
      if (parsed) {
        workspace.closeSurface(parsed.connectionId, parsed.surfaceId);
      }
      return;
    }
    const session = liveSessions().find((item) => item.id === assign);
    if (session) void workspace.closeSession(session.id);
  }

  /**
   * Remove a leaf from the layout. `closeContent=false` parks its occupant;
   * true closes it. In either case the layout tree itself shrinks, so floating
   * windows cannot turn into purposeless empty shells and tiled layouts do not
   * accumulate dead slots.
   */
  function removePane(paneId: string, closeContent: boolean) {
    const currentRoot = root();
    const currentPanes = enumeratePanes(currentRoot);
    const previous = layoutState();
    const removed = previous.assignments[paneId] ?? null;
    if (closeContent) closeAssignment(removed);

    // A layout-mounted container should have at least two leaves, but keep the
    // operation well-defined during a concurrent external layout collapse.
    if (currentPanes.length <= 1) {
      if (!closeContent) clearPaneAssignment(paneId);
      return;
    }

    const nextRoot = removePaneFromLayout(currentRoot, paneId);
    if (!nextRoot || nextRoot === currentRoot) return;
    const survivingPanes = currentPanes.filter((pane) => pane.id !== paneId);
    const nextPanes = enumeratePanes(nextRoot);
    const nextAssignments = carryAssignmentsToPanes({
      currentPanes: survivingPanes,
      nextPanes,
      previous,
      liveSessionIds: liveSessionIds(),
    });
    const nextPending: Record<string, string> = {};
    for (let index = 0; index < survivingPanes.length; index++) {
      const ref = pendingRefs[survivingPanes[index].id];
      const target = nextPanes[index];
      if (ref && target) nextPending[target.id] = ref;
    }

    if (nextPanes.length === 1) {
      props.onCollapseToSingle?.(
        nextAssignments.assignments[nextPanes[0].id] ?? null,
      );
      return;
    }

    const removedIndex = Math.max(
      0,
      currentPanes.findIndex((pane) => pane.id === paneId),
    );
    const nextFocus =
      nextPanes[Math.min(removedIndex, nextPanes.length - 1)]?.id ??
      nextPanes[0]?.id ??
      null;
    batch(() => {
      pendingRefs = nextPending;
      touchPendingRefs();
      setSoloedPaneId(null);
      setRaiseOrder([]);
      setLayoutState(nextAssignments);
      updateRoot(nextRoot);
      setFocusedPaneId(nextFocus);
    });
  }

  function closePane(paneId: string) {
    removePane(paneId, true);
  }

  function backgroundPane(paneId: string) {
    removePane(paneId, false);
  }

  function focusPane(paneId: string) {
    setFocusedPaneId(paneId);
  }

  // Report focused pane changes.
  createEffect(() => {
    props.onFocusedPaneChange?.(focusedPaneId());
  });

  createEffect(() => {
    props.onFocusPane?.(focusPane);
  });

  // Remember last active tab per tabs container so switching away doesn't reset.
  const tabMemory: Record<string, number> = {};

  // Pane keys, bound behind Ctrl+B only while a layout is on screen. They
  // live here rather than in createKeyboardShortcuts because every one of
  // them is meaningless without a tree to walk: with no layout up the tokens are
  // unbound, which is a better answer than a handler that has to ask whether
  // a layout exists. Ctrl+B h / Ctrl+B v deliberately shadow the workspace's
  // own bindings for the same two keys — splitting one pane of a tree and
  // turning a single view into a tree are different operations.
  createEffect(() => {
    const ids = paneIds();
    const fpId = focusedPaneId();
    const cyclePane = (delta: 1 | -1) => {
      if (ids.length === 0) return;
      const index = fpId ? ids.indexOf(fpId) : -1;
      focusPane(ids[(index + delta + ids.length) % ids.length]);
    };
    const split = (direction: "horizontal" | "vertical") => {
      if (fpId) splitPane(null, fpId, direction);
    };
    // Narrower/wider applies to the focused column of a strip, where a column
    // has a width of its own. Under tiling the dividers already do this with
    // the pointer, and a floating window is resized by its corner.
    const resizeColumn = (factor: number) => {
      const strip = root();
      if (strip.type !== "split" || strip.direction !== "scrolling") return;
      const index = strip.children.findIndex(
        (_, at) => String(at) === (fpId ?? "").split(".")[0],
      );
      if (index < 0) return;
      handleColumnWidth(strip, index, strip.children[index].weight * factor);
    };
    const bindings: [string, () => void, string][] = [
      ["Tab", () => cyclePane(1), t("help.nextPane")],
      ["Shift+Tab", () => cyclePane(-1), t("help.previousPane")],
      ["z", () => fpId && toggleSolo(fpId), t("help.soloPaneShort")],
      // tmux's axes: -h puts the new pane beside this one, -v below it.
      ["h", () => split("horizontal"), t("help.splitBeside")],
      ["v", () => split("vertical"), t("help.splitBelow")],
      ["m", cycleWindowManager, t("help.windowManager")],
      ["-", () => resizeColumn(1 / 1.25), t("help.narrowColumn")],
      ["=", () => resizeColumn(1.25), t("help.widenColumn")],
      ["q", () => fpId && backgroundPane(fpId), t("help.removeFromPane")],
      ["x", () => fpId && closePane(fpId), t("pane.close")],
    ];
    const unbind = bindings.map(([token, run, label]) =>
      registerPrefixAction(token, run, label),
    );
    onCleanup(() => {
      for (const drop of unbind) drop();
    });
  });

  createEffect(() => {
    const state = layoutState();
    // Always report assignments so that Workspace can derive the focused
    // surface (for the status bar) and filter offScreenSurfaces even
    // while workspace-session reference resolution is in progress. Workspace
    // guards against persisting unresolved entries via onAssignmentsResolved.
    props.onAssignmentsChange?.(state);
  });

  createEffect(() => {
    pendingRefsRevision();
    props.onUnresolvedAssignmentsChange?.({ ...pendingRefs });
  });

  createEffect(() => {
    props.onAssignmentsResolved?.(!resolvingRefs());
  });

  createEffect(() => {
    const manageVisibility = props.manageVisibility ?? true;
    if (!manageVisibility) return;
    const ids = assignedInPaneOrder();
    const extra = props.extraVisibleSessions;
    if (extra && extra.length > 0) {
      workspace.setVisibleSessions([...ids, ...extra]);
    } else {
      workspace.setVisibleSessions(ids);
    }
  });

  // Floating stacking order, most recently raised last. Ephemeral by design
  // (see LayoutTreeCtx.floatingDepth).
  const [raiseOrder, setRaiseOrder] = createSignal<string[]>([]);
  const floatingDepth = (paneId: string) => raiseOrder().indexOf(paneId) + 1;
  const raisePane = (paneId: string) =>
    setRaiseOrder((order) => [...order.filter((id) => id !== paneId), paneId]);

  function replaceSplit(target: LayoutSplit, updated: LayoutSplit): LayoutNode {
    const walk = (node: LayoutNode): LayoutNode => {
      if (node === target) return updated;
      if (node.type === "leaf") return node;
      return {
        ...node,
        children: node.children.map((child) => ({
          ...child,
          node: walk(child.node),
        })),
      };
    };
    return walk(root());
  }

  function handleRectChange(
    split: LayoutSplit,
    index: number,
    rect: LayoutRect,
  ) {
    updateRoot(
      replaceSplit(
        split,
        withChild(split, index, (child) => ({
          ...child,
          rect: clampRect(rect),
        })),
      ),
      true,
    );
  }

  function handleColumnWidth(
    split: LayoutSplit,
    index: number,
    weight: number,
  ) {
    updateRoot(
      replaceSplit(
        split,
        withChild(split, index, (child) => ({
          ...child,
          weight: Math.min(2, Math.max(0.15, weight)),
        })),
      ),
      true,
    );
  }

  /**
   * Hand the same windows to the next window manager.
   *
   * The tree is rebuilt rather than re-flagged (see `toWindowManager`), so
   * every pane id changes and the occupants have to be carried across
   * explicitly — `updateRoot` marks the layout as ours, which is exactly what
   * stops the external-layout effect from doing that carry for us.
   */
  function cycleWindowManager() {
    const current = root();
    const next = toWindowManager(
      current,
      nextWindowManager(windowManagerOf(current)),
    );
    const currentPanes = enumeratePanes(current);
    const nextPanes = enumeratePanes(next);
    const focusedIndex = Math.max(
      0,
      currentPanes.findIndex((pane) => pane.id === focusedPaneId()),
    );
    const nextPending: Record<string, string> = {};
    for (let index = 0; index < currentPanes.length; index++) {
      const ref = pendingRefs[currentPanes[index].id];
      const target = nextPanes[index];
      if (ref && target) nextPending[target.id] = ref;
    }
    batch(() => {
      pendingRefs = nextPending;
      touchPendingRefs();
      setLayoutState(
        carryAssignmentsToPanes({
          currentPanes,
          nextPanes,
          previous: layoutState(),
          liveSessionIds: liveSessionIds(),
        }),
      );
      setRaiseOrder([]);
      updateRoot(next);
      setFocusedPaneId(nextPanes[focusedIndex]?.id ?? nextPanes[0]?.id ?? null);
    });
  }

  function updateRoot(next: LayoutNode, debounceHistory = false) {
    setRoot(next);
    const dsl = serializeDSL(next);
    const updated: WorkspaceLayout = { ...props.layout, root: next, dsl };
    lastLayout = updated;
    lastDsl = dsl;
    saveActiveLayout(updated);
    props.onLayoutChange(
      updated,
      debounceHistory ? { debounceHistory: true } : undefined,
    );
  }

  function handleResize(
    split: LayoutSplit,
    indexA: number,
    indexB: number,
    fraction: number,
  ) {
    const updated = adjustWeights(split, indexA, indexB, fraction);
    const replaceNode = (node: LayoutNode): LayoutNode => {
      if (node === split) return updated;
      if (node.type === "leaf") return node;
      return {
        ...node,
        children: node.children.map((child) => ({
          ...child,
          node: replaceNode(child.node),
        })),
      };
    };
    updateRoot(replaceNode(root()), true);
  }

  createEffect(() => {
    const fsId = props.focusedSessionId;
    const live = liveSessions();
    const fpId = focusedPaneId();
    const handler = (event: KeyboardEvent) => {
      if (!fsId) return;
      const session = live.find((item) => item.id === fsId);
      if (!session || session.state !== "exited") return;
      // Enter restarts the terminal named by the banner. Escape dismisses what
      // is in front of you; neither action needs the workspace prefix.
      if (event.key === "Escape") {
        event.preventDefault();
        // Immediately clear the pane assignment so the exited terminal
        // disappears without waiting for the server round-trip.
        if (fpId) {
          setLayoutState((prev) => {
            if (prev.assignments[fpId] !== fsId) return prev;
            return {
              assignments: { ...prev.assignments, [fpId]: null },
            };
          });
        }
        void workspace.closeSession(fsId);
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  const multiPane = () => leafCount(root()) > 1;

  // Each reactive field is exposed via a getter so consumers reading
  // `ctx.foo` see the current value.  Solid's Provider captures `props.value`
  // once under `untrack`, so a plain-object literal would freeze every field
  // to the mount-time snapshot — breaking e.g. connectionLabels when a new
  // remote is added after LayoutContainer mounts.
  const ctxValue: LayoutTreeCtx = {
    get connectionId() {
      return props.connectionId;
    },
    get connectionLabels() {
      return props.connectionLabels;
    },
    get multiPane() {
      return multiPane();
    },
    get windowManager() {
      return windowManagerOf(root());
    },
    get isMobileTouch() {
      return props.isMobileTouch;
    },
    get hasAttention() {
      return props.hasAttention;
    },
    get isSessionReadOnly() {
      return props.isSessionReadOnly;
    },
    get isConnectionReadOnly() {
      return props.isConnectionReadOnly;
    },
    onFocusPane: focusPane,
    onClosePane: closePane,
    onBackgroundPane: backgroundPane,
    get onCreateInPane() {
      return props.onCreateInPane;
    },
    get onSwitcher() {
      return props.onSwitcher;
    },
    get onHelp() {
      return props.onHelp;
    },
    onResize: handleResize,
    get palette() {
      return props.palette;
    },
    get fontFamily() {
      return props.fontFamily;
    },
    get fontSize() {
      return props.fontSize;
    },
    get surfaceZoom() {
      return props.surfaceZoom ?? 1;
    },
    get surfaceZoomMode() {
      return props.surfaceZoomMode ?? "relative";
    },
    get surfaceTouchMode() {
      return props.surfaceTouchMode ?? "direct";
    },
    tabMemory,
    get onRender() {
      return props.onRender;
    },
    get onTerminalSurface() {
      return props.onTerminalSurface;
    },
    get registerWebPaneHost() {
      return props.registerWebPaneHost;
    },
    get onOpenTile() {
      return props.onOpenTile;
    },
    get onDropTile() {
      return props.onDropTile;
    },
    get soloedPaneId() {
      return soloedPaneId();
    },
    onToggleSolo: toggleSolo,
    onRectChange: handleRectChange,
    onColumnWidth: handleColumnWidth,
    floatingDepth,
    onRaisePane: raisePane,
  };
  return (
    <LayoutTreeContext.Provider value={ctxValue}>
      <div style={{ width: "100%", height: "100%", display: "flex" }}>
        <PaneNode
          node={root()}
          assignments={layoutState().assignments}
          focusedPaneId={focusedPaneId()}
          visible={props.manageVisibility ?? true}
        />
      </div>
    </LayoutTreeContext.Provider>
  );
}

function PaneNode(props: {
  node: LayoutNode;
  assignments: Record<string, SessionId | null>;
  focusedPaneId: string | null;
  visible: boolean;
  path?: number[];
}) {
  const ctx = useLayoutTree();
  // All branching uses <Show> so Solid re-evaluates when props.node changes
  // (e.g. on layout switch or resize).  <Index> is used for split children
  // so that components persist by position — only the item signal updates,
  // avoiding unnecessary recreation during resize drags.

  const path = () => props.path ?? [];
  const paneId = () => {
    const p = path();
    return p.length > 0 ? p.join(".") : "0";
  };

  /**
   * Index of the child containing the soloed pane, or -1 when this split has
   * no say. Matching by path prefix is what lets a solo deep in the tree
   * clear every ancestor's siblings on the way down.
   */
  const soloChild = (children: readonly LayoutChild[]): number => {
    const solo = ctx.soloedPaneId;
    if (!solo) return -1;
    for (let i = 0; i < children.length; i++) {
      const prefix = [...path(), i].join(".");
      if (solo === prefix || solo.startsWith(prefix + ".")) return i;
    }
    return -1;
  };

  return (
    <Show
      when={
        props.node.type === "split" ? (props.node as LayoutSplit) : undefined
      }
      fallback={
        <LeafPane
          paneId={paneId()}
          leaf={props.node as LayoutLeaf}
          sessionId={props.assignments[paneId()] ?? null}
          isFocused={paneId() === props.focusedPaneId}
          visible={props.visible}
        />
      }
    >
      {(split) => (
        <Show
          when={split().direction === "tabs"}
          fallback={
            <Show
              when={
                split().direction === "scrolling" ||
                split().direction === "floating"
              }
              fallback={
                <div
                  style={{
                    display: "flex",
                    "flex-direction":
                      split().direction === "horizontal" ? "row" : "column",
                    width: "100%",
                    height: "100%",
                  }}
                >
                  <Index each={split().children}>
                    {(child, index) => {
                      const solo = () => soloChild(split().children);
                      const hidden = () => solo() >= 0 && index !== solo();
                      return (
                        <>
                          {/* No handle to drag while one pane fills the split. */}
                          <Show when={index > 0 && solo() < 0}>
                            <ResizeHandle
                              direction={
                                split().direction as "horizontal" | "vertical"
                              }
                              onDrag={(fraction) =>
                                ctx.onResize(
                                  split(),
                                  index - 1,
                                  index,
                                  fraction,
                                )
                              }
                            />
                          </Show>
                          <div
                            style={{
                              // The soloed branch takes the whole split; its
                              // siblings keep their weights for the moment the
                              // solo is lifted.
                              flex: solo() >= 0 ? 1 : child().weight,
                              display: hidden() ? "none" : undefined,
                              overflow: "hidden",
                              position: "relative",
                              "min-width": 0,
                              "min-height": 0,
                            }}
                          >
                            <PaneNode
                              node={child().node}
                              assignments={props.assignments}
                              focusedPaneId={props.focusedPaneId}
                              // Not merely cosmetic: `visible` gates
                              // `resizable`, and a hidden-but-resizable terminal
                              // measures 0×0. The client sends the *minimum*
                              // across a session's views, so leaving these true
                              // would pin the soloed PTY to 1×1.
                              visible={props.visible && !hidden()}
                              path={[...(props.path ?? []), index]}
                            />
                          </div>
                        </>
                      );
                    }}
                  </Index>
                </div>
              }
            >
              <ManagedSplit
                split={split()}
                assignments={props.assignments}
                focusedPaneId={props.focusedPaneId}
                visible={props.visible}
                path={path()}
              />
            </Show>
          }
        >
          {(() => {
            const theme = () => themeFor(ctx.palette);
            const scale = () => uiScale(ctx.fontSize);
            const tabKey = () => path().join(".") || "root";

            const activeTab = () => {
              const focusedPrefix = props.focusedPaneId ?? "";
              const s = split();
              let active = -1;
              for (let i = 0; i < s.children.length; i++) {
                const childPrefix = [...path(), i].join(".");
                if (
                  focusedPrefix === childPrefix ||
                  focusedPrefix.startsWith(childPrefix + ".")
                ) {
                  active = i;
                  break;
                }
              }
              if (active >= 0) {
                ctx.tabMemory[tabKey()] = active;
                return active;
              }
              return Math.min(
                ctx.tabMemory[tabKey()] ?? 0,
                s.children.length - 1,
              );
            };

            const tabLabel = (child: LayoutChild, index: number): string => {
              if (child.label) return child.label;
              return tp("pane.tab", { index: index + 1 });
            };

            return (
              <div
                style={{
                  display: "flex",
                  "flex-direction": "column",
                  width: "100%",
                  height: "100%",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    gap: "1px",
                    "flex-shrink": 0,
                    "background-color": theme().solidPanelBg,
                    "border-bottom": `1px solid ${theme().subtleBorder}`,
                    "font-size": `${scale().sm}px`,
                  }}
                >
                  <For each={split().children}>
                    {(child, index) => {
                      const childPath = () => [...path(), index()].join(".");
                      return (
                        <button
                          onClick={() => ctx.onFocusPane(childPath())}
                          style={{
                            ...ui.btn,
                            flex: 1,
                            "min-width": 0,
                            padding: `${scale().controlY}px ${scale().controlX}px`,
                            "font-size": `${scale().sm}px`,
                            "text-align": "center",
                            overflow: "hidden",
                            "text-overflow": "ellipsis",
                            "white-space": "nowrap",
                            opacity: index() === activeTab() ? 1 : 0.5,
                            "border-bottom":
                              index() === activeTab()
                                ? `1px solid ${theme().accent}`
                                : "1px solid transparent",
                          }}
                        >
                          {tabLabel(child, index())}
                        </button>
                      );
                    }}
                  </For>
                </div>
                <div
                  style={{
                    flex: 1,
                    overflow: "hidden",
                    position: "relative",
                    "min-height": 0,
                  }}
                >
                  {/* Keep every tab body mounted. In particular, a restored web
                      pane must create its iframe and bind to the preview worker
                      without waiting for the user to focus its tab. Persistent
                      bodies also preserve in-frame navigation when switching
                      between tabs; inactive bodies are only hidden. */}
                  <For each={split().children}>
                    {(child, index) => {
                      const active = () => index() === activeTab();
                      return (
                        <div
                          style={{
                            position: "absolute",
                            inset: 0,
                            display: active() ? "block" : "none",
                          }}
                        >
                          <PaneNode
                            node={child.node}
                            assignments={props.assignments}
                            focusedPaneId={props.focusedPaneId}
                            visible={props.visible && active()}
                            path={[...path(), index()]}
                          />
                        </div>
                      );
                    }}
                  </For>
                </div>
              </div>
            );
          })()}
        </Show>
      )}
    </Show>
  );
}

function LeafPane(props: {
  paneId: string;
  leaf: LayoutLeaf;
  sessionId: SessionId | null;
  isFocused: boolean;
  visible: boolean;
}) {
  const ctx = useLayoutTree();
  const theme = () => themeFor(ctx.palette);
  const scale = () => uiScale(ctx.fontSize);
  const workspace = createYasWorkspace();
  const sessions = createYasSessions(workspace);
  const workspaceState = createYasWorkspaceState(workspace);

  const surfaceParsed = () => parseSurfaceAssignment(props.sessionId);
  const isSurface = () => surfaceParsed() != null;
  const tileParsed = () => parseTileAssignment(props.sessionId);
  const webParsed = () => parseWebAssignment(props.sessionId);
  const surfaceId = () => surfaceParsed()?.surfaceId ?? null;
  // Highlighted while a tile drag hovers this pane (a valid drop target).
  const [tileDragOver, setTileDragOver] = createSignal(false);
  // Reveals the corner tools on pointer devices (see PaneTools).
  const [hovered, setHovered] = createSignal(false);
  // The pane's live terminal surface, for the drop target's post-upload paste.
  const [termSurface, setTermSurface] = createSignal<YasTerminalSurface | null>(
    null,
  );
  const surfaceConnectionId = () =>
    surfaceParsed()?.connectionId ?? ctx.connectionId;

  /** True when the surface's owning connection is present in the workspace.
   *  When the remote is removed the connection disappears — we hide the
   *  surface view (the assignment is still preserved so it can reattach
   *  once the remote is re-added). */
  const surfaceConnPresent = () => {
    const parsed = surfaceParsed();
    if (!parsed) return false;
    const snap = workspaceState();
    return snap.connections.some((c) => c.id === parsed.connectionId);
  };

  const session = () =>
    isSurface()
      ? null
      : (sessions().find((item) => item.id === props.sessionId) ?? null);

  const connection = () => {
    const snap = workspaceState();
    return snap.connections.find((c) => c.id === ctx.connectionId) ?? null;
  };

  let paneContainer!: HTMLDivElement;
  let autoCreated = false;

  createEffect(() => {
    // Tabs keep their bodies mounted so web panes can materialize eagerly, but
    // an inactive command leaf should retain the old lazy-start behavior.
    if (!props.visible) return;
    if (props.sessionId || !props.leaf.command || autoCreated) return;
    if (connection()?.status !== "connected") return;
    autoCreated = true;
    ctx.onCreateInPane?.(props.paneId, props.leaf.command);
  });

  // Per-pane memos (default equality): the raw props read through the shared
  // assignments object, whose identity changes on ANY pane's reassignment —
  // without the memo, every pane's focus effect re-runs on every layout
  // mutation and the focused pane re-asserts DOM focus it never lost.
  const paneSession = createMemo(() => props.sessionId);
  const paneVisible = createMemo(() => props.visible);
  const paneAttention = createMemo(() => {
    const assignment = props.sessionId;
    return assignment != null && (ctx.hasAttention?.(assignment) ?? false);
  });
  createEffect(() => {
    // Track these dependencies
    const focused = props.isFocused;
    const _sid = paneSession();
    const _vis = paneVisible();
    if (!focused || !paneContainer) return;

    // Focus the pane container's focusable child. An editable CodeMirror
    // content div comes FIRST: a comma-list querySelector returns the
    // first match in *document* order, and an editor tile has [tabindex]
    // elements (the scroller) before `.cm-content` — focusing those
    // leaves the editor without keyboard focus or a visible cursor.
    // Read-only CM contents (diff views) are contenteditable=false and
    // unfocusable, so they fall through to the [tabindex] pass (the
    // diff root). Bare "canvas" is excluded — the terminal canvas has
    // no tabindex so focus() is a no-op; surface canvases have tabindex.
    const pick = (): HTMLElement | null =>
      paneContainer.querySelector<HTMLElement>(
        '.cm-content[contenteditable="true"]',
      ) ??
      paneContainer.querySelector<HTMLElement>("[tabindex], input, textarea");
    autoFocusPaneTarget(() => props.isFocused, pick);
  });

  return (
    <div
      ref={paneContainer}
      data-yas-pane-id={props.paneId}
      data-yas-pane-focused={props.isFocused ? "true" : undefined}
      style={{
        width: "100%",
        height: "100%",
        position: "relative",
        border: ctx.multiPane
          ? `1px solid ${
              paneAttention()
                ? theme().errorText
                : props.isFocused
                  ? theme().accent
                  : "transparent"
            }`
          : "none",
      }}
      onPointerDown={() => ctx.onFocusPane(props.paneId)}
      onFocusIn={() => ctx.onFocusPane(props.paneId)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onDragOver={(e) => {
        if (!ctx.onDropTile || !isTileDrag(e)) return;
        e.preventDefault(); // allow the drop
        e.dataTransfer!.dropEffect = "copy";
        if (!tileDragOver()) setTileDragOver(true);
      }}
      onDragLeave={(e) => {
        // Ignore leaves into child elements; only clear when truly leaving.
        if (!e.currentTarget.contains(e.relatedTarget as Node | null))
          setTileDragOver(false);
      }}
      onDrop={(e) => {
        const assignment = tileDragAssignment(e);
        setTileDragOver(false);
        if (assignment && ctx.onDropTile) {
          e.preventDefault();
          ctx.onDropTile(
            assignment,
            props.paneId,
            paneDragSource(e) ?? undefined,
          );
        }
      }}
    >
      <Show when={tileDragOver()}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            "z-index": 5,
            "pointer-events": "none",
            background: `color-mix(in srgb, ${theme().accent} 14%, transparent)`,
            border: `2px solid ${theme().accent}`,
            "box-sizing": "border-box",
          }}
        />
      </Show>
      {/* Every occupied pane gets the ✕, whatever it holds. Gated on something
          actually being rendered rather than on the assignment being non-null:
          a pane still resolving a tab ref falls through to EmptyPane, which
          has nothing to close. */}
      <Show
        when={
          ctx.windowManager !== "floating" &&
          (tileParsed() || webParsed() || isSurface() || session() != null)
        }
      >
        <PaneTools
          theme={theme()}
          scale={scale()}
          alwaysVisible={ctx.isMobileTouch ?? false}
          hovered={hovered()}
          drag={
            props.sessionId
              ? { assignment: props.sessionId, paneId: props.paneId }
              : undefined
          }
          solo={
            ctx.multiPane
              ? {
                  active: ctx.soloedPaneId === props.paneId,
                  onToggle: () => ctx.onToggleSolo(props.paneId),
                }
              : undefined
          }
          onClose={() => ctx.onClosePane(props.paneId)}
        />
      </Show>
      {/* IDE tile (editor/diff/commit) overlays the pane; its value is mutually
          exclusive with sessions and surfaces (docs/ide-plan.md PR-6). Rendered
          via the shared YasTile so panes and the single-view focused-tile view
          never drift. */}
      <Show when={tileParsed()}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            overflow: "hidden",
            "background-color": theme().bg,
          }}
        >
          <YasTile
            workspace={workspace}
            assignment={props.sessionId!}
            focused={props.isFocused}
            theme={theme()}
            palette={ctx.palette}
            scale={scale()}
            fontFamily={ctx.fontFamily}
            fontSize={ctx.fontSize}
            onOpenTile={(a) => ctx.onOpenTile?.(a)}
            isConnectionReadOnly={ctx.isConnectionReadOnly}
          />
        </div>
      </Show>
      <Show when={webParsed()}>
        {(_) => (
          <div
            style={{
              position: "absolute",
              inset: 0,
              overflow: "hidden",
              "background-color": theme().bg,
            }}
          >
            <WebPaneHost
              assignment={props.sessionId!}
              hostId={`pane:${props.paneId}`}
              register={ctx.registerWebPaneHost!}
              focused={props.isFocused}
              onFocusRequest={() => ctx.onFocusPane(props.paneId)}
            />
          </div>
        )}
      </Show>
      {/* Terminal / surface / empty layer. Gated on !tileParsed(): a tile is
          mutually exclusive with sessions and surfaces, and because EmptyPane
          is position:relative and follows the tile overlay in the DOM it would
          otherwise paint *over* the tile instead of the editor/diff/commit. */}
      <Show when={!tileParsed() && !webParsed()}>
        <Show
          when={isSurface()}
          fallback={
            <Show
              when={props.sessionId && session()}
              fallback={
                <EmptyPane
                  paneId={props.paneId}
                  isFocused={props.isFocused}
                  showHint={!ctx.multiPane}
                  theme={theme()}
                  palette={ctx.palette}
                  fontSize={ctx.fontSize}
                  connectionId={ctx.connectionId}
                  connectionLabels={ctx.connectionLabels}
                  onCreateInPane={ctx.onCreateInPane}
                  onSwitcher={ctx.onSwitcher}
                  onHelp={ctx.onHelp}
                />
              }
            >
              <TerminalDropTarget
                workspace={workspace}
                sessionId={props.sessionId!}
                connectionId={session()?.connectionId ?? ctx.connectionId}
                surface={termSurface}
                theme={theme()}
                scale={scale()}
              >
                <YasTerminal
                  sessionId={props.sessionId}
                  readOnly={
                    (props.sessionId !== null &&
                      ctx.isSessionReadOnly?.(props.sessionId)) ||
                    false
                  }
                  resizable={props.visible}
                  fontSize={resolveLeafFontSize(props.leaf, ctx.fontSize)}
                  fontFamily={ctx.fontFamily}
                  palette={ctx.palette}
                  style={{ width: "100%", height: "100%" }}
                  showCursor={props.isFocused}
                  onRender={ctx.onRender}
                  surfaceRef={(s) => {
                    setTermSurface(s);
                    ctx.onTerminalSurface?.(s);
                  }}
                />
              </TerminalDropTarget>
              <Show when={session()?.state === "exited"}>
                <div
                  style={{
                    position: "absolute",
                    bottom: "8px",
                    left: "50%",
                    transform: "translateX(-50%)",
                    background: theme().solidPanelBg,
                    border: `1px solid ${theme().border}`,
                    padding: `${scale().controlY}px ${scale().controlX}px`,
                    "font-size": `${scale().sm}px`,
                    display: "flex",
                    "align-items": "center",
                    gap: `${scale().gap}px`,
                    // Above the terminal's scroll surface (z-index 1), which
                    // otherwise hit-tests over the banner and swallows the
                    // tap — invisible, but the top layer. Same treatment as
                    // the single-view banner in Workspace.
                    "z-index": z.exitedBanner,
                  }}
                >
                  <mark
                    style={{
                      ...ui.badge,
                      "background-color": "rgba(255,100,100,0.3)",
                    }}
                  >
                    {t("pane.exited")}
                  </mark>
                  <Show when={connection()?.supportsRestart}>
                    <button
                      onClick={() => workspace.restartSession(props.sessionId!)}
                      style={{ ...ui.btn, "font-size": `${scale().sm}px` }}
                    >
                      {t("pane.restart")} <kbd style={ui.kbd}>Enter</kbd>
                    </button>
                  </Show>
                  <button
                    onClick={() =>
                      void workspace.closeSession(props.sessionId!)
                    }
                    style={mergeStyle(ui.btn, {
                      "font-size": `${scale().sm}px`,
                      opacity: 0.5,
                    })}
                  >
                    {t("pane.close")} <kbd style={ui.kbd}>Esc</kbd>
                  </button>
                </div>
              </Show>
            </Show>
          }
        >
          <Show
            when={surfaceConnPresent()}
            fallback={
              <EmptyPane
                paneId={props.paneId}
                isFocused={props.isFocused}
                showHint={!ctx.multiPane}
                theme={theme()}
                palette={ctx.palette}
                fontSize={ctx.fontSize}
                connectionId={ctx.connectionId}
                connectionLabels={ctx.connectionLabels}
                onCreateInPane={ctx.onCreateInPane}
                onSwitcher={ctx.onSwitcher}
                onHelp={ctx.onHelp}
              />
            }
          >
            <div style={{ width: "100%", height: "100%" }}>
              <YasSurfaceView
                connectionId={surfaceConnectionId()}
                surfaceId={surfaceId()!}
                focus={props.isFocused}
                resizable
                zoom={ctx.surfaceZoom}
                zoomMode={ctx.surfaceZoomMode}
                touchMode={ctx.surfaceTouchMode}
                style={{ width: "100%", height: "100%" }}
              />
            </div>
          </Show>
        </Show>
      </Show>
    </div>
  );
}

/**
 * The scrolling and floating managers.
 *
 * Both are one level of the same tree the tiling manager renders, so a child
 * is still a `PaneNode` and can still be a split — a strip column is a stack
 * when you divide it, and a floating window can hold a tiling tree. Only the
 * placement of the root's children differs.
 */
function ManagedSplit(props: {
  split: LayoutSplit;
  assignments: Record<string, string | null>;
  focusedPaneId: string | null;
  visible: boolean;
  path: readonly number[];
}) {
  const ctx = useLayoutTree();
  const paneIdAt = (index: number) => [...props.path, index].join(".");
  /** True while the focus is inside this child's subtree. */
  const holdsFocus = (index: number) => {
    const prefix = paneIdAt(index);
    const focused = props.focusedPaneId ?? "";
    return focused === prefix || focused.startsWith(prefix + ".");
  };

  return (
    <Show
      when={props.split.direction === "scrolling"}
      fallback={
        <FloatingLayer
          split={props.split}
          assignments={props.assignments}
          focusedPaneId={props.focusedPaneId}
          visible={props.visible}
          path={props.path}
          paneIdAt={paneIdAt}
          holdsFocus={holdsFocus}
        />
      }
    >
      <ScrollingStrip
        split={props.split}
        assignments={props.assignments}
        focusedPaneId={props.focusedPaneId}
        visible={props.visible}
        path={props.path}
        holdsFocus={holdsFocus}
      />
    </Show>
  );
}

/**
 * niri's model: one row of columns, as wide as it needs to be, with the
 * viewport following the focus along it.
 *
 * A column's weight is its width as a fraction of the viewport rather than a
 * share of a fixed total, which is the whole difference from a tiling row —
 * three half-width columns are 150% wide and that is not an error, it is the
 * strip.
 */
function ScrollingStrip(props: {
  split: LayoutSplit;
  assignments: Record<string, string | null>;
  focusedPaneId: string | null;
  visible: boolean;
  path: readonly number[];
  holdsFocus: (index: number) => boolean;
}) {
  let strip!: HTMLDivElement;
  const columns: (HTMLDivElement | undefined)[] = [];

  // Follow the focus. `nearest` rather than `center` so moving one column
  // over scrolls by one column instead of recentring the whole strip, which
  // is what makes a strip readable as a place rather than a carousel.
  createEffect(() => {
    const focused = props.focusedPaneId;
    if (!focused) return;
    const index = props.split.children.findIndex((_, at) =>
      props.holdsFocus(at),
    );
    if (index < 0) return;
    const column = columns[index];
    if (!column || !strip) return;
    queueMicrotask(() =>
      column.scrollIntoView({ inline: "nearest", block: "nearest" }),
    );
  });

  return (
    <div
      ref={strip}
      style={{
        display: "flex",
        "flex-direction": "row",
        width: "100%",
        height: "100%",
        "overflow-x": "auto",
        "overflow-y": "hidden",
        "scroll-behavior": "smooth",
      }}
    >
      <Index each={props.split.children}>
        {(child, index) => (
          <div
            ref={(element: HTMLDivElement) => (columns[index] = element)}
            style={{
              flex: `0 0 ${Math.round(Math.min(2, Math.max(0.15, child().weight)) * 100)}%`,
              position: "relative",
              overflow: "hidden",
              "min-width": 0,
              "min-height": 0,
            }}
          >
            <PaneNode
              node={child().node}
              assignments={props.assignments}
              focusedPaneId={props.focusedPaneId}
              visible={props.visible}
              path={[...props.path, index]}
            />
          </div>
        )}
      </Index>
    </div>
  );
}

/** Percent of a box, from a pixel delta. */
function percentOf(pixels: number, total: number): number {
  return total > 0 ? (pixels / total) * 100 : 0;
}

/**
 * Free-positioned windows, last one raised on top.
 *
 * A window is moved by its title strip and resized by its bottom-right
 * corner, both through pointer capture so the drag survives the pointer
 * leaving the window — which it will, because the point of dragging is to put
 * the window where it is not.
 */
function FloatingLayer(props: {
  split: LayoutSplit;
  assignments: Record<string, string | null>;
  focusedPaneId: string | null;
  visible: boolean;
  path: readonly number[];
  paneIdAt: (index: number) => string;
  holdsFocus: (index: number) => boolean;
}) {
  const ctx = useLayoutTree();
  const theme = () => themeFor(ctx.palette);
  const scale = () => uiScale(ctx.fontSize);
  const workspace = createYasWorkspace();
  const sessions = createYasSessions(workspace);
  const workspaceState = createYasWorkspaceState(workspace);
  const connectionKey = createMemo(() =>
    workspaceState()
      .connections.map((connection) => connection.id)
      .sort()
      .join("\u0000"),
  );
  const [surfaceRevision, setSurfaceRevision] = createSignal(0);
  createEffect(() => {
    connectionKey();
    const releases: (() => void)[] = [];
    for (const connection of untrack(() => workspaceState().connections)) {
      const live = workspace.getConnection(connection.id);
      if (live) {
        releases.push(
          live.surfaceStore.onChange(() =>
            setSurfaceRevision((revision) => revision + 1),
          ),
        );
      }
    }
    onCleanup(() => releases.forEach((release) => release()));
  });
  const surfaceFor = (assignment: string): YasSurface | null => {
    surfaceRevision();
    const parsed = parseSurfaceAssignment(assignment);
    if (!parsed) return null;
    return (
      workspace
        .getConnection(parsed.connectionId)
        ?.surfaceStore.getSurfaces()
        .get(parsed.surfaceId) ?? null
    );
  };
  let layer!: HTMLDivElement;
  // The frame being dragged, so the window follows the pointer without a
  // round trip through the serialized layout on every pointermove.
  const [dragging, setDragging] = createSignal<{
    index: number;
    rect: LayoutRect;
  } | null>(null);

  const rectOf = (child: LayoutChild, index: number): LayoutRect => {
    const live = dragging();
    if (live && live.index === index) return live.rect;
    return clampRect(child.rect ?? cascadeRect(index));
  };

  const drag = (
    event: PointerEvent,
    index: number,
    start: LayoutRect,
    mode: "move" | "resize",
  ) => {
    if (event.button !== 0) return;
    const target = event.currentTarget as HTMLElement;
    const box = layer.getBoundingClientRect();
    const originX = event.clientX;
    const originY = event.clientY;
    event.preventDefault();
    target.setPointerCapture(event.pointerId);
    ctx.onRaisePane(props.paneIdAt(index));

    const move = (moved: PointerEvent) => {
      const dx = percentOf(moved.clientX - originX, box.width);
      const dy = percentOf(moved.clientY - originY, box.height);
      const next =
        mode === "move"
          ? { ...start, x: start.x + dx, y: start.y + dy }
          : {
              ...start,
              width: start.width + dx,
              height: start.height + dy,
            };
      setDragging({ index, rect: clampRect(next) });
    };
    const done = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", done);
      target.removeEventListener("pointercancel", done);
      const live = dragging();
      setDragging(null);
      if (live) ctx.onRectChange(props.split, index, live.rect);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", done);
    target.addEventListener("pointercancel", done);
  };

  return (
    <div
      ref={layer}
      style={{
        position: "relative",
        width: "100%",
        height: "100%",
        overflow: "hidden",
        ...floatingLayerStackingStyle,
      }}
    >
      <Index each={props.split.children}>
        {(child, index) => {
          const paneIds = () =>
            floatingPaneIds(child().node, [...props.path, index]);
          const paneId = () => {
            const ids = paneIds();
            const focused = props.focusedPaneId;
            if (focused && ids.includes(focused)) return focused;
            return (
              ids.find((id) => props.assignments[id] != null) ??
              ids[0] ??
              props.paneIdAt(index)
            );
          };
          const assignment = () => props.assignments[paneId()] ?? null;
          const occupied = () =>
            paneIds().some((id) => props.assignments[id] != null);
          const soloed = () =>
            ctx.soloedPaneId != null && paneIds().includes(ctx.soloedPaneId);
          const hidden = () =>
            ctx.soloedPaneId != null && !paneIds().includes(ctx.soloedPaneId);
          const rect = () =>
            soloed()
              ? { x: 0, y: 0, width: 100, height: 100 }
              : rectOf(child(), index);
          const focused = () => props.holdsFocus(index);
          const title = () => {
            const value = assignment();
            return value
              ? floatingWindowTitle(value, sessions(), surfaceFor(value))
              : "";
          };
          const frameButton = (): JSX.CSSProperties => ({
            ...ui.btn,
            display: "flex",
            "align-items": "center",
            "justify-content": "center",
            width: `${scale().md * 2}px`,
            height: "100%",
            padding: 0,
            color: theme().fg,
            "background-color": theme().solidPanelBg,
            border: "none",
            "border-left": `1px solid ${theme().border}`,
            "border-radius": "0",
            opacity: 1,
            "font-size": `${scale().md}px`,
            "touch-action": "manipulation",
          });
          return (
            <Show when={occupied()}>
              <div
                style={{
                  position: "absolute",
                  left: `${rect().x}%`,
                  top: `${rect().y}%`,
                  width: `${rect().width}%`,
                  height: `${rect().height}%`,
                  display: hidden() ? "none" : "flex",
                  "flex-direction": "column",
                  overflow: "hidden",
                  "border-radius": soloed() ? "0" : `${scale().tightGap}px`,
                  border: `1px solid ${focused() ? theme().accent : theme().border}`,
                  "box-shadow": "none",
                  background: theme().bg,
                  // Focus wins over recency, so the window you are typing in is
                  // never behind the one you last dragged.
                  "z-index":
                    (focused() ? 1_000 : 0) +
                    ctx.floatingDepth(props.paneIdAt(index)),
                }}
                onPointerDown={() => ctx.onRaisePane(props.paneIdAt(index))}
              >
                <div
                  role="presentation"
                  onPointerDown={(event) => {
                    if (!soloed()) drag(event, index, rect(), "move");
                  }}
                  style={{
                    height: `${scale().md * 2}px`,
                    flex: "0 0 auto",
                    display: "flex",
                    "align-items": "center",
                    cursor: soloed() ? "default" : "move",
                    "background-color": focused()
                      ? theme().selectedBg
                      : theme().solidPanelBg,
                    color: theme().fg,
                    "border-bottom": `1px solid ${theme().border}`,
                    "touch-action": "none",
                    "font-size": `${scale().sm}px`,
                  }}
                >
                  <div
                    title={title()}
                    style={{
                      flex: 1,
                      "min-width": 0,
                      padding: `0 ${scale().controlX}px`,
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                      "white-space": "nowrap",
                      "font-weight": focused() ? 600 : 400,
                    }}
                  >
                    {title()}
                  </div>
                  <button
                    type="button"
                    title={t("help.removeFromPane")}
                    aria-label={t("help.removeFromPane")}
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={(event) => {
                      event.stopPropagation();
                      ctx.onBackgroundPane(paneId());
                    }}
                    style={frameButton()}
                  >
                    −
                  </button>
                  <button
                    type="button"
                    title={soloed() ? t("pane.unsolo") : t("pane.solo")}
                    aria-label={soloed() ? t("pane.unsolo") : t("pane.solo")}
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={(event) => {
                      event.stopPropagation();
                      ctx.onToggleSolo(paneId());
                    }}
                    style={frameButton()}
                  >
                    {soloed() ? "❐" : "□"}
                  </button>
                  <button
                    type="button"
                    title={t("pane.close")}
                    aria-label={t("pane.close")}
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={(event) => {
                      event.stopPropagation();
                      ctx.onClosePane(paneId());
                    }}
                    style={frameButton()}
                  >
                    ×
                  </button>
                </div>
                <div style={{ flex: 1, position: "relative", "min-height": 0 }}>
                  <PaneNode
                    node={child().node}
                    assignments={props.assignments}
                    focusedPaneId={props.focusedPaneId}
                    visible={props.visible && !hidden()}
                    path={[...props.path, index]}
                  />
                </div>
                <Show when={!soloed()}>
                  <div
                    role="presentation"
                    onPointerDown={(event) =>
                      drag(event, index, rect(), "resize")
                    }
                    style={{
                      position: "absolute",
                      right: 0,
                      bottom: 0,
                      width: `${scale().controlX * 2}px`,
                      height: `${scale().controlX * 2}px`,
                      cursor: "nwse-resize",
                      "touch-action": "none",
                    }}
                  />
                </Show>
              </div>
            </Show>
          );
        }}
      </Index>
    </div>
  );
}

export function EmptyPane(props: {
  paneId: string;
  isFocused: boolean;
  showHint?: boolean;
  theme: Theme;
  palette: TerminalPalette;
  fontSize: number;
  connectionId: string;
  connectionLabels?: Map<string, string>;
  onCreateInPane?: (
    paneId: string,
    command?: string,
    connectionId?: string,
  ) => void;
  onSwitcher?: () => void;
  onHelp?: () => void;
}) {
  let paneRef!: HTMLDivElement;
  const scale = () => uiScale(props.fontSize);

  createEffect(() => {
    autoFocusPaneTarget(
      () => props.isFocused,
      () => paneRef ?? null,
    );
  });

  return (
    <div
      ref={paneRef}
      tabIndex={0}
      style={{
        width: "100%",
        height: "100%",
        position: "relative",
        "background-color": `rgb(${props.palette.bg[0]},${props.palette.bg[1]},${props.palette.bg[2]})`,
        display: "flex",
        "flex-direction": "column",
        "align-items": "center",
        "justify-content": "center",
        color: props.theme.fg,
        "font-size": `${scale().md}px`,
        outline: "none",
      }}
    >
      <Show when={props.showHint !== false}>Start with C-b</Show>
    </div>
  );
}
