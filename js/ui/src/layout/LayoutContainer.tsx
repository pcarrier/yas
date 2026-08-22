/**
 * A sway-like container workspace. Horizontal, vertical, tabbed, and stacking
 * containers compose freely; floating windows share that workspace through a
 * Sway-style scene, and the right-hand shelf remains outside this tree.
 */
import {
  createSignal,
  createEffect,
  createMemo,
  onMount,
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
  SurfaceId,
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
  serializeDSL,
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
import type { PaneToolActions } from "../PaneTools";
import { focusedPaneAction } from "./focusedPaneAction";
import { TerminalDropTarget } from "../terminalDrop";
import { WebPaneHost, type WebPaneHostRegistrar } from "../WebPaneHost";
import {
  isTileDrag,
  paneDragSource,
  startPaneTileDrag,
  startPaneTouchDrag,
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
import { prefixChordLabel } from "../keyPrefix";
import { registerPrefixAction } from "../keyPrefix";
import type { SurfaceTouchMode, SurfaceZoomMode } from "../storage";
import {
  floatingLayerStackingStyle,
  floatingFrameNodes,
  floatingDropAppendsWindow,
  floatingPaneIds,
  floatingWindowTitle,
  addFloatingWindowToWorkspace,
  addTiledWindowToWorkspace,
  floatingFrameIndex,
  isFloatingPane as paneIsFloating,
  panesByFloatingMode,
  reusableFloatingPaneId,
  resizeFloatingRect,
  rebaseFloatingRect,
  snapFloatingRect,
  togglePaneFloating,
  type FloatingDragMode,
  type FloatingResizeEdge,
} from "./floatingWindow";
import {
  pruneUnassignedPanes,
  removePaneFromLayout,
  showEmptyPaneHint,
} from "./paneRemoval";
import { SurfaceIcon } from "../SurfaceIcon";
import { insertTabAtPane } from "./tabGrouping";
import {
  spatialNeighbor,
  type SpatialDirection,
  type SpatialPaneRect,
} from "./spatialNavigation";
import { balanceLayout, resizePaneInDirection } from "./directionalResize";
import { moveViewIntoStack, type ViewMovement } from "./viewMovement";
import {
  movePaneInDirection,
  nextTiledLayout,
  paneParentLayout,
  setPaneLayout,
  splitPaneWithAssignment,
  togglePaneSplit,
  type LayoutMutation,
  type TiledLayout,
} from "./swayLayout";
import { terminalFocusRequest } from "./terminalFocus";
import {
  mergeUniquePaneAssignments,
  uniquePaneValues,
} from "./assignmentOwnership";

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
  /** Changes when another backend workspace is attached. */
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
  onAddFloatingWindow?: (
    fn: (assignment: string) => boolean,
  ) => void | (() => void);
  /** Add a compositor toplevel without replacing any managed occupant. */
  onAddManagedWindow?: (
    fn: (assignment: string) => boolean,
  ) => void | (() => void);
  onMoveSessionToPane?: (
    fn: (sessionId: SessionId, targetPaneId: string) => void,
  ) => void;
  onMoveToPane?: (
    fn: (value: string, targetPaneId: string, fromPaneId?: string) => void,
  ) => void | (() => void);
  /** Add a parked assignment as a tab in the window containing `sourcePaneId`. */
  onTabIntoPane?: (
    fn: (value: string, sourcePaneId: string) => boolean,
  ) => void | (() => void);
  /** Open an assignment as the active view tab in a stack. */
  onOpenTabInPane?: (
    fn: (value: string, sourcePaneId: string) => boolean,
  ) => void | (() => void);
  /** Open as a sibling in the target's current container layout. */
  onOpenInContainer?: (
    fn: (value: string, targetPaneId: string) => boolean,
  ) => void | (() => void);
  /** Called with a function that splits a pane, placing `value` in a new
   *  pane beside the target's current occupant (which is preserved). */
  onSplitPane?: (
    fn: (
      value: string,
      targetPaneId: string,
      direction?: "horizontal" | "vertical",
    ) => void,
  ) => void | (() => void);
  onClearPaneAssignment?: (fn: (paneId: string) => void) => void;
  /** Reset a manager with no remaining windows to one empty tiling pane. */
  onCollapseToSingle?: (assignment: string | null) => void;
  onFocusedPaneChange?: (paneId: string | null) => void;
  /** Publish status-bar actions for the focused occupied pane. */
  onFocusedPaneActionsChange?: (actions: PaneToolActions | null) => void;
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
  /** Close a native surface. Workspace owns the close tombstone that keeps
   * the asynchronously-destroyed surface out of the parked-items panel. */
  onCloseSurface?: (connectionId: string, surfaceId: SurfaceId) => void;
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
  const [surfaceTitleRevision, setSurfaceTitleRevision] = createSignal(0);
  createEffect(() => {
    const releases: (() => void)[] = [];
    for (const snapshot of workspaceState().connections) {
      const connection = workspace.getConnection(snapshot.id);
      if (connection) {
        releases.push(
          connection.surfaceStore.onChange(() =>
            setSurfaceTitleRevision((revision) => revision + 1),
          ),
        );
      }
    }
    onCleanup(() => releases.forEach((release) => release()));
  });

  function assignmentTitle(assignment: string): string {
    if (isTileAssignment(assignment) || isWebAssignment(assignment)) {
      return floatingWindowTitle(assignment, liveSessions(), null);
    }
    const parsed = parseSurfaceAssignment(assignment);
    if (parsed) {
      surfaceTitleRevision();
      const surface = workspace
        .getConnection(parsed.connectionId)
        ?.surfaceStore.getSurfaces()
        .get(parsed.surfaceId);
      return surface
        ? floatingWindowTitle(assignment, liveSessions(), surface)
        : "";
    }
    const session = liveSessions().find(
      (candidate) => candidate.id === assignment,
    );
    return session ? floatingWindowTitle(assignment, [session], null) : "";
  }

  const [root, setRoot] = createSignal(props.layout.root);
  const panes = createMemo(() => enumeratePanes(root()));
  const paneIds = createMemo(() => panes().map((pane) => pane.id));

  // The backend store carries stable PTY/surface/tab refs. Resolve them to
  // ephemeral live assignments as their remotes arrive, while retaining every
  // unresolved ref separately so a detached remote cannot erase session state.
  let pendingRefs: Record<string, string> = uniquePaneValues(
    props.storedAssignments ?? {},
    paneIds(),
  );
  const [pendingRefsRevision, setPendingRefsRevision] = createSignal(0);
  const [resolvingRefs, setResolvingRefs] = createSignal(
    Object.keys(pendingRefs).length > 0,
  );
  const [initialPlacementPassComplete, setInitialPlacementPassComplete] =
    createSignal(false);

  // Old device-local layout records contain a tree but no assignments. Keep
  // that tree intact through the first complete multi-connection catalogue
  // turn so Workspace can place every already-live surface before empty-pane
  // compaction runs. A microtask is intentional: connection snapshots,
  // surface placement, and this component's assignment callbacks all settle
  // synchronously from the same catalogue publication.
  createEffect(() => {
    if (initialPlacementPassComplete()) return;
    const connections = workspaceState().connections;
    if (connections.length === 0 || connections.some((conn) => !conn.ready))
      return;
    queueMicrotask(() => setInitialPlacementPassComplete(true));
  });

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
        assignments: mergeUniquePaneAssignments(
          prev.assignments,
          { [paneId]: assignment },
          paneIds(),
        ),
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
    const liveSurfaceKeys = new Set(props.liveSurfaceKeys ?? []);
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
        const key = `${parsed.connectionId}:${surfaceId}`;
        // A stable ref names identity, not liveness. Restoring it while the
        // connection is ready but its catalogue does not contain the surface
        // resurrects a destroyed window as a permanent black pane. Keep the
        // ref for a future reconnect, but only materialize a live assignment.
        if (liveSurfaceKeys.has(key)) {
          resolved[paneId] = surfaceAssignment(parsed.connectionId, surfaceId);
        }
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
        assignments: mergeUniquePaneAssignments(
          prev.assignments,
          resolved,
          paneIds(),
        ),
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
  let layoutElement!: HTMLDivElement;
  let recentPaneIds: string[] = [];
  /** Sway-style layout intent, consumed by the next populated open. */
  const [nextSplitDirection, setNextSplitDirection] =
    createSignal<TiledLayout | null>(null);

  createEffect(() => {
    const paneId = focusedPaneId();
    if (!paneId) return;
    recentPaneIds = [
      paneId,
      ...recentPaneIds.filter((candidate) => candidate !== paneId),
    ];
  });

  // A new attached workspace can reuse the same component instance.
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
    pendingRefs = uniquePaneValues(props.storedAssignments ?? {}, paneIds());
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
   * Not persisted: surviving a reload is not part of presentation state.
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
  let soloRoot = root();
  createEffect(() => {
    const currentRoot = root();
    const ids = paneIds();
    const solo = untrack(soloedPaneId);
    const treeChanged = currentRoot !== soloRoot;
    soloRoot = currentRoot;
    if (solo && (treeChanged || !ids.includes(solo) || ids.length < 2)) {
      setSoloedPaneId(null);
    }
  });

  // An empty leaf is not a reusable slot. Once restore has had a chance to
  // resolve stable identities, remove every unoccupied branch before any new
  // surface placement is released by onAssignmentsResolved. The sole exception
  // is one plain leaf when the workspace has no content at all: that is the
  // empty-workspace launcher, never a gap beside a real window.
  createEffect(() => {
    if (resolvingRefs() || !initialPlacementPassComplete()) return;
    const previous = layoutState();
    const snapshots = workspaceState().connections;
    const unavailableRef = Object.entries(pendingRefs).some(([paneId, ref]) => {
      if (previous.assignments[paneId] != null) return false;
      const parsed = parseWorkspaceRef(ref);
      if (!parsed) return true;
      return !snapshots.find(
        (connection) =>
          connection.id === parsed.connectionId && connection.ready,
      );
    });
    // A disconnected remote is not an authoritative empty pane. Preserve its
    // branch and stable ref for reconnect instead of publishing a destructive
    // cross-remote layout edit. Ready connections are authoritative, so refs
    // absent from their first complete catalogue may be compacted normally.
    if (unavailableRef) return;
    const compacted = pruneUnassignedPanes(root(), previous.assignments);
    if (!compacted) return;

    const nextPending: Record<string, string> = {};
    for (const [oldPaneId, ref] of Object.entries(pendingRefs)) {
      const nextPaneId = compacted.paneIdMap.get(oldPaneId);
      if (nextPaneId && compacted.assignments[nextPaneId] != null) {
        nextPending[nextPaneId] = ref;
      }
    }
    const previousFocus = untrack(focusedPaneId);
    const nextPanes = enumeratePanes(compacted.root);
    const nextFocus =
      (previousFocus ? compacted.paneIdMap.get(previousFocus) : null) ??
      nextPanes.find(({ id }) => compacted.assignments[id] != null)?.id ??
      nextPanes[0]?.id ??
      null;
    batch(() => {
      pendingRefs = nextPending;
      touchPendingRefs();
      setSoloedPaneId(null);
      setLayoutState({ assignments: compacted.assignments });
      updateRoot(compacted.root);
      setFocusedPaneId(nextFocus);
    });
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
    const request = terminalFocusRequest(
      focusedPaneSessionId(),
      props.focusedSessionId,
    );
    if (request) props.onFocusSession(request);
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
      if (fpId && layoutState().assignments[fpId] != null) {
        openInContainer(sessionId, fpId);
      } else if (fpId) {
        moveToPane(sessionId, fpId);
      }
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
    const directSource =
      fromPaneId &&
      fromPaneId !== pane &&
      valid.includes(fromPaneId) &&
      layoutState().assignments[fromPaneId] === value
        ? fromPaneId
        : null;
    if (directSource && layoutState().assignments[pane] != null) {
      const movement = moveViewIntoStack(
        root(),
        layoutState().assignments,
        directSource,
        pane,
      );
      if (movement) {
        applyLayoutMutation(movement);
        return;
      }
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
    const unregister = props.onMoveToPane?.(moveToPane);
    if (unregister) onCleanup(unregister);
  });

  /** Add one view tab. Root + assignment rekeying is one batch so no
   * intermediate frame can duplicate or lose either surface. */
  function addTabToPane(
    value: string,
    sourcePaneId: string,
    activate: boolean,
  ): boolean {
    const previous = layoutState();
    const sourceValue = previous.assignments[sourcePaneId] ?? null;
    if (!sourceValue || sourceValue === value) return false;
    if (
      Object.entries(previous.assignments).some(
        ([paneId, assignment]) =>
          paneId !== sourcePaneId && assignment === value,
      )
    ) {
      return false;
    }
    const inserted = insertTabAtPane(root(), sourcePaneId);
    if (!inserted) return false;

    const sourcePending = pendingRefs[sourcePaneId];
    batch(() => {
      forgetPendingRef(inserted.newPaneId);
      if (sourcePending && inserted.sourcePaneId !== sourcePaneId) {
        delete pendingRefs[sourcePaneId];
        pendingRefs[inserted.sourcePaneId] = sourcePending;
        touchPendingRefs();
      }
      setLayoutState((state) => {
        const assignments = { ...state.assignments };
        if (inserted.sourcePaneId !== sourcePaneId) {
          delete assignments[sourcePaneId];
          assignments[inserted.sourcePaneId] = sourceValue;
        }
        assignments[inserted.newPaneId] = value;
        return { ...state, assignments };
      });
      updateRoot(inserted.root);
      setFocusedPaneId(activate ? inserted.newPaneId : inserted.sourcePaneId);
    });
    return true;
  }

  /** A parked-card drop keeps the dragged source active. */
  function tabIntoPane(value: string, sourcePaneId: string): boolean {
    return addTabToPane(value, sourcePaneId, false);
  }

  /** Explicit center grouping makes the new view the active tab. */
  function openTabInPane(value: string, sourcePaneId: string): boolean {
    return addTabToPane(value, sourcePaneId, true);
  }

  createEffect(() => {
    const unregister = props.onTabIntoPane?.(tabIntoPane);
    if (unregister) onCleanup(unregister);
  });

  /** Ordinary opens add a child using the container layout the user selected.
   * A lone root defaults to a geometric split. Tabs are therefore only made by
   * an explicit tabbed container or an explicit center-drop grouping action. */
  function openInContainer(value: string, targetPaneId: string): boolean {
    const ids = paneIds();
    if (!ids.includes(targetPaneId)) return false;
    const shown = ids.find(
      (paneId) => layoutState().assignments[paneId] === value,
    );
    if (shown) {
      setFocusedPaneId(shown);
      return true;
    }
    if (layoutState().assignments[targetPaneId] == null) {
      const occupied = ids.find(
        (paneId) => layoutState().assignments[paneId] != null,
      );
      if (occupied) targetPaneId = occupied;
    }
    if (layoutState().assignments[targetPaneId] == null) {
      moveToPane(value, targetPaneId);
      return true;
    }
    if (paneIsFloating(root(), targetPaneId)) {
      return addFloatingWindow(value);
    }
    if (windowManagerOf(root()) === "floating") {
      return addFloatingWindow(value);
    }
    const targetElement = layoutElement?.querySelector<HTMLElement>(
      `[data-yas-pane-id="${targetPaneId}"]`,
    );
    const targetRect = targetElement?.getBoundingClientRect();
    const inherited = paneParentLayout(root(), targetPaneId);
    const inheritedTiled: TiledLayout | null =
      inherited === "horizontal" ||
      inherited === "vertical" ||
      inherited === "tabs" ||
      inherited === "stacking"
        ? inherited
        : null;
    const direction: TiledLayout =
      nextSplitDirection() ??
      inheritedTiled ??
      (targetRect && targetRect.height > targetRect.width
        ? "vertical"
        : "horizontal");
    const mutation = splitPaneWithAssignment(
      root(),
      layoutState().assignments,
      targetPaneId,
      value,
      direction,
    );
    if (!mutation) return false;
    setNextSplitDirection(null);
    applyLayoutMutation(mutation);
    return true;
  }

  createEffect(() => {
    const unregister = props.onOpenInContainer?.(openInContainer);
    if (unregister) onCleanup(unregister);
  });
  createEffect(() => {
    const unregister = props.onOpenTabInPane?.(openTabInPane);
    if (unregister) onCleanup(unregister);
  });

  // Open a populated container beside the target. The pure engine extends a
  // matching parent (sway's flat split containers) and nests only when the
  // requested axis differs.
  function splitPane(
    value: string,
    targetPaneId: string,
    requestedDirection?: "horizontal" | "vertical",
  ) {
    const targetElement = layoutElement?.querySelector<HTMLElement>(
      `[data-yas-pane-id="${targetPaneId}"]`,
    );
    const targetRect = targetElement?.getBoundingClientRect();
    const direction =
      requestedDirection ??
      nextSplitDirection() ??
      (targetRect && targetRect.height > targetRect.width
        ? "vertical"
        : "horizontal");
    const cur = root();
    if (
      cur.type === "split" &&
      cur.direction === "workspace" &&
      paneIsFloating(cur, targetPaneId)
    ) {
      const mutation = addTiledWindowToWorkspace(
        cur,
        layoutState().assignments,
        value,
      );
      if (mutation) applyLayoutMutation(mutation);
      return;
    }
    // A floating child is a window, not a miniature tiling workspace. Content
    // opened through a split-capable caller still gets its own top-level frame;
    // an empty keyboard split has no meaningful floating representation.
    if (windowManagerOf(cur) === "floating") {
      addFloatingWindow(value);
      return;
    }
    const mutation = splitPaneWithAssignment(
      cur,
      layoutState().assignments,
      targetPaneId,
      value,
      direction,
    );
    if (!mutation) {
      moveToPane(value, targetPaneId);
      return;
    }
    setNextSplitDirection(null);
    applyLayoutMutation(mutation);
  }

  createEffect(() => {
    props.onSplitPane?.(splitPane);
  });

  /** A parked item entering floating mode is a new window, never a replace. */
  function addFloatingWindow(value: string): boolean {
    const shown = paneIds().find(
      (paneId) => layoutState().assignments[paneId] === value,
    );
    if (shown) {
      setFocusedPaneId(shown);
      return true;
    }
    const current = root();
    // Closing a floating frame deliberately leaves its top-level leaf in the
    // live tree. Reuse that stable slot before appending: the pane ids and DOM
    // owners of every other window then remain untouched for their lifetime.
    if (
      current.type === "split" &&
      (current.direction === "floating" || current.direction === "workspace")
    ) {
      const paneId = reusableFloatingPaneId(
        current,
        layoutState().assignments,
        new Set(Object.keys(pendingRefs)),
      );
      if (paneId) {
        batch(() => {
          setLayoutState((previous) => ({
            ...previous,
            assignments: {
              ...previous.assignments,
              [paneId]: value,
            },
          }));
          setFocusedPaneId(paneId);
          raisePane(paneId);
        });
        return true;
      }
    }
    const mutation = addFloatingWindowToWorkspace(
      current,
      layoutState().assignments,
      value,
      cascadeRect(panesByFloatingMode(current).floating.length),
    );
    if (!mutation) return false;
    applyLayoutMutation(mutation);
    raisePane(mutation.focusedPaneId);
    return true;
  }

  /** Add one compositor toplevel beside the focused view, inheriting its
   * explicitly selected container layout. */
  function addManagedWindow(value: string): boolean {
    if (windowManagerOf(root()) === "floating") return addFloatingWindow(value);
    const shown = paneIds().find(
      (paneId) => layoutState().assignments[paneId] === value,
    );
    if (shown) {
      setFocusedPaneId(shown);
      return true;
    }
    const ids = paneIds();
    const focused = focusedPaneId();
    const currentRoot = root();
    const occupied = ids.filter(
      (paneId) =>
        layoutState().assignments[paneId] != null &&
        !paneIsFloating(currentRoot, paneId),
    );
    if (
      occupied.length === 0 &&
      currentRoot.type === "split" &&
      currentRoot.direction === "workspace"
    ) {
      const mutation = addTiledWindowToWorkspace(
        currentRoot,
        layoutState().assignments,
        value,
      );
      if (!mutation) return false;
      applyLayoutMutation(mutation);
      return true;
    }
    const targetPaneId =
      (focused && occupied.includes(focused) ? focused : null) ??
      occupied[0] ??
      ids[0];
    if (!targetPaneId) return false;
    if (layoutState().assignments[targetPaneId] == null) {
      moveToPane(value, targetPaneId);
    } else if (!openInContainer(value, targetPaneId)) {
      return false;
    }
    return true;
  }

  function clearPaneAssignment(paneId: string) {
    removePane(paneId, false);
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
        if (props.onCloseSurface) {
          props.onCloseSurface(parsed.connectionId, parsed.surfaceId);
        } else {
          workspace.closeSurface(parsed.connectionId, parsed.surfaceId);
        }
      }
      return;
    }
    const session = liveSessions().find((item) => item.id === assign);
    if (session) void workspace.closeSession(session.id);
  }

  /**
   * Remove a displayed pane. `closeContent=false` parks its occupant; true
   * closes it. Tiled trees shrink immediately. Floating children are cleared
   * first so their live siblings retain identity during the close dispatch;
   * the empty frame is then removed by the common compaction effect.
   */
  function removePane(paneId: string, closeContent: boolean) {
    const currentRoot = root();
    const currentPanes = enumeratePanes(currentRoot);
    const previous = layoutState();
    const removed = previous.assignments[paneId] ?? null;

    // Clear a floating frame before closing its resource. The compactor removes
    // the empty leaf while preserving the surviving child objects, avoiding a
    // transient reassignment during synchronous catalogue/focus updates.
    if (paneIsFloating(currentRoot, paneId)) {
      const retained = currentPanes.filter(
        (pane) =>
          pane.id !== paneId &&
          (previous.assignments[pane.id] != null ||
            pendingRefs[pane.id] != null),
      );
      if (retained.length === 0) {
        batch(() => {
          forgetPendingRef(paneId);
          setLayoutState((state) => ({
            ...state,
            assignments: { ...state.assignments, [paneId]: null },
          }));
        });
        props.onCollapseToSingle?.(null);
        if (closeContent) closeAssignment(removed);
        return;
      }

      const focusedBefore = untrack(focusedPaneId);
      const nextFocus =
        focusedBefore &&
        focusedBefore !== paneId &&
        previous.assignments[focusedBefore] != null
          ? focusedBefore
          : (retained.find((pane) => previous.assignments[pane.id] != null)
              ?.id ??
            retained[0]?.id ??
            null);
      batch(() => {
        forgetPendingRef(paneId);
        if (untrack(soloedPaneId) === paneId) setSoloedPaneId(null);
        setRaiseOrder((order) => order.filter((id) => id !== paneId));
        setLayoutState((state) => ({
          ...state,
          assignments: { ...state.assignments, [paneId]: null },
        }));
        setFocusedPaneId(nextFocus);
      });
      // Publish the stable visual state before a synchronous catalogue/focus
      // update from the close can arrive.
      if (closeContent) closeAssignment(removed);
      return;
    }

    if (closeContent) closeAssignment(removed);

    // With no other content, retain one launcher leaf. It is the empty
    // workspace itself, not a structural gap next to another pane.
    if (currentPanes.length <= 1) {
      batch(() => {
        forgetPendingRef(paneId);
        setLayoutState((state) => ({
          ...state,
          assignments: { ...state.assignments, [paneId]: null },
        }));
        setFocusedPaneId(paneId);
      });
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

    const removedIndex = Math.max(
      0,
      currentPanes.findIndex((pane) => pane.id === paneId),
    );
    const paneIdMap = new Map<string, string>();
    for (let index = 0; index < survivingPanes.length; index++) {
      const target = nextPanes[index];
      if (target) paneIdMap.set(survivingPanes[index].id, target.id);
    }
    const focusedBefore = untrack(focusedPaneId);
    const retainedFocus = focusedBefore
      ? paneIdMap.get(focusedBefore)
      : undefined;
    const nextFocus =
      retainedFocus ??
      nextPanes[Math.min(removedIndex, nextPanes.length - 1)]?.id ??
      nextPanes[0]?.id ??
      null;
    const nextRaiseOrder = untrack(raiseOrder).flatMap((raisedPaneId) => {
      const mapped = paneIdMap.get(raisedPaneId);
      if (mapped) return [mapped];

      // Old nested floating state can use the top-level child id for stacking
      // while enumeratePanes returns only descendant leaves. Map that frame
      // through its first surviving leaf and retain the same path depth.
      const prefix = `${raisedPaneId}.`;
      const survivorIndex = survivingPanes.findIndex((pane) =>
        pane.id.startsWith(prefix),
      );
      const target = nextPanes[survivorIndex];
      if (!target) return [];
      const depth = raisedPaneId.split(".").length;
      return [target.id.split(".").slice(0, depth).join(".")];
    });
    batch(() => {
      pendingRefs = nextPending;
      touchPendingRefs();
      setSoloedPaneId(null);
      setRaiseOrder([...new Set(nextRaiseOrder)]);
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

  function visiblePaneRects(): SpatialPaneRect[] {
    if (!layoutElement) return [];
    const assignments = layoutState().assignments;
    return Array.from(
      layoutElement.querySelectorAll<HTMLElement>("[data-yas-pane-id]"),
    ).flatMap((element) => {
      const paneId = element.dataset.yasPaneId;
      if (!paneId || assignments[paneId] == null) return [];
      const rect = element.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return [];
      return [
        {
          paneId,
          left: rect.left,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
        },
      ];
    });
  }

  function directionalNeighbor(direction: SpatialDirection): string | null {
    const paneId = focusedPaneId();
    if (!paneId) return null;
    return spatialNeighbor(
      visiblePaneRects(),
      paneId,
      direction,
      recentPaneIds,
    );
  }

  function focusInDirection(direction: SpatialDirection) {
    const target = directionalNeighbor(direction);
    if (target) focusPane(target);
  }

  function applyLayoutMutation(movement: ViewMovement | LayoutMutation) {
    const nextPending: Record<string, string> = {};
    for (const [oldPaneId, ref] of Object.entries(pendingRefs)) {
      const newPaneId = movement.paneIdMap.get(oldPaneId);
      if (newPaneId) nextPending[newPaneId] = ref;
    }
    batch(() => {
      pendingRefs = nextPending;
      touchPendingRefs();
      setLayoutState((previous) => ({
        ...previous,
        assignments: movement.assignments,
      }));
      updateRoot(movement.root);
      setFocusedPaneId(movement.focusedPaneId);
      setRaiseOrder((order) =>
        order.flatMap((paneId) => {
          const mapped = movement.paneIdMap.get(paneId);
          return mapped ? [mapped] : [];
        }),
      );
    });
  }

  function suggestedFloatingRect(paneId: string): LayoutRect {
    const workspaceRect = layoutElement?.getBoundingClientRect();
    const paneRect = layoutElement
      ?.querySelector<HTMLElement>(`[data-yas-pane-id="${paneId}"]`)
      ?.getBoundingClientRect();
    if (
      !workspaceRect ||
      !paneRect ||
      workspaceRect.width <= 0 ||
      workspaceRect.height <= 0
    ) {
      return cascadeRect(panesByFloatingMode(root()).floating.length);
    }
    const minWidth = percentOf(75, workspaceRect.width);
    const minHeight = percentOf(50, workspaceRect.height);
    const width = Math.max(
      minWidth,
      Math.min(72, percentOf(paneRect.width, workspaceRect.width)),
    );
    const height = Math.max(
      minHeight,
      Math.min(72, percentOf(paneRect.height, workspaceRect.height)),
    );
    const centerX = percentOf(
      paneRect.left + paneRect.width / 2 - workspaceRect.left,
      workspaceRect.width,
    );
    const centerY = percentOf(
      paneRect.top + paneRect.height / 2 - workspaceRect.top,
      workspaceRect.height,
    );
    return clampRect({
      x: centerX - width / 2,
      y: centerY - height / 2,
      width,
      height,
    });
  }

  function toggleFloating(paneId = focusedPaneId()) {
    if (!paneId) return;
    const wasFloating = paneIsFloating(root(), paneId);
    const mutation = togglePaneFloating(
      root(),
      layoutState().assignments,
      paneId,
      suggestedFloatingRect(paneId),
    );
    if (!mutation) return;
    applyLayoutMutation(mutation);
    if (!wasFloating) raisePane(mutation.focusedPaneId);
  }

  function changeFloatingRect(
    paneId: string,
    update: (rect: LayoutRect, box: DOMRect) => LayoutRect,
  ): boolean {
    const currentRoot = root();
    const index = floatingFrameIndex(currentRoot, paneId);
    if (index == null || currentRoot.type !== "split" || !layoutElement)
      return false;
    const child = currentRoot.children[index];
    const box = layoutElement.getBoundingClientRect();
    if (!child || box.width <= 0 || box.height <= 0) return false;
    handleRectChange(
      currentRoot,
      index,
      clampRect(update(child.rect ?? cascadeRect(index), box)),
    );
    raisePane(paneId.split(".")[0]);
    return true;
  }

  function moveFloatingInDirection(
    paneId: string,
    direction: SpatialDirection,
  ): boolean {
    return changeFloatingRect(paneId, (rect, box) => ({
      ...rect,
      x:
        rect.x +
        (direction === "left"
          ? -percentOf(10, box.width)
          : direction === "right"
            ? percentOf(10, box.width)
            : 0),
      y:
        rect.y +
        (direction === "up"
          ? -percentOf(10, box.height)
          : direction === "down"
            ? percentOf(10, box.height)
            : 0),
    }));
  }

  function resizeFloatingInDirection(
    paneId: string,
    direction: SpatialDirection,
  ): boolean {
    return changeFloatingRect(paneId, (rect, box) =>
      resizeFloatingRect(
        rect,
        direction === "left"
          ? -percentOf(10, box.width)
          : direction === "right"
            ? percentOf(10, box.width)
            : 0,
        direction === "up"
          ? -percentOf(10, box.height)
          : direction === "down"
            ? percentOf(10, box.height)
            : 0,
        direction === "left" || direction === "right" ? "e" : "s",
      ),
    );
  }

  function focusModeToggle() {
    const current = focusedPaneId();
    if (!current) return;
    const modes = panesByFloatingMode(root());
    const targets = paneIsFloating(root(), current)
      ? modes.tiled
      : modes.floating;
    if (targets.length === 0) return;
    const target =
      recentPaneIds.find((paneId) => targets.includes(paneId)) ??
      targets[targets.length - 1];
    focusPane(target);
    if (paneIsFloating(root(), target)) raisePane(target.split(".")[0]);
  }

  function moveViewInDirection(direction: SpatialDirection) {
    const sourcePaneId = focusedPaneId();
    if (!sourcePaneId) return;
    if (moveFloatingInDirection(sourcePaneId, direction)) return;
    const targetPaneId = directionalNeighbor(direction);
    const movement = movePaneInDirection(
      root(),
      layoutState().assignments,
      sourcePaneId,
      targetPaneId,
      direction,
    );
    if (movement) applyLayoutMutation(movement);
  }

  function dropAssignmentBeside(
    assignment: string,
    targetPaneId: string,
    sourcePaneId: string | undefined,
    direction: SpatialDirection,
  ) {
    const assignments = layoutState().assignments;
    const recoveredSource =
      sourcePaneId ??
      paneIds().find(
        (paneId) =>
          paneId !== targetPaneId && assignments[paneId] === assignment,
      );
    const mutation =
      recoveredSource && assignments[recoveredSource] === assignment
        ? movePaneInDirection(
            root(),
            assignments,
            recoveredSource,
            targetPaneId,
            direction,
          )
        : splitPaneWithAssignment(
            root(),
            assignments,
            targetPaneId,
            assignment,
            direction === "left" || direction === "right"
              ? "horizontal"
              : "vertical",
            direction === "right" || direction === "down",
          );
    if (mutation) applyLayoutMutation(mutation);
  }

  function changeContainerLayout(direction: TiledLayout) {
    const paneId = focusedPaneId();
    if (!paneId) return;
    const mutation = setPaneLayout(
      root(),
      layoutState().assignments,
      paneId,
      direction,
    );
    if (mutation) {
      applyLayoutMutation(mutation);
    } else if (paneParentLayout(root(), paneId) == null) {
      // A one-leaf tree has no parent whose layout can change. Remember the
      // explicit choice so the next open creates that container.
      setNextSplitDirection(direction);
    }
  }

  function toggleContainerSplit() {
    const paneId = focusedPaneId();
    if (!paneId) return;
    const mutation = togglePaneSplit(root(), layoutState().assignments, paneId);
    if (mutation) applyLayoutMutation(mutation);
  }

  function cycleContainerLayout() {
    const paneId = focusedPaneId();
    if (!paneId) return;
    const direction = paneParentLayout(root(), paneId);
    changeContainerLayout(nextTiledLayout(direction ?? "horizontal"));
  }

  function resizeInDirection(direction: SpatialDirection) {
    const paneId = focusedPaneId();
    if (!paneId) return;
    if (resizeFloatingInDirection(paneId, direction)) return;
    const resized = resizePaneInDirection(root(), paneId, direction);
    if (resized) updateRoot(resized, true);
  }

  // Report focused pane changes.
  createEffect(() => {
    props.onFocusedPaneChange?.(focusedPaneId());
  });

  createEffect(() => {
    const paneId = focusedPaneId();
    const assignment = paneId
      ? (layoutState().assignments[paneId] ?? null)
      : null;
    if (!paneId || !assignment) {
      props.onFocusedPaneActionsChange?.(null);
      return;
    }
    const multiPane = paneIds().length > 1;
    props.onFocusedPaneActionsChange?.({
      drag: { assignment, paneId },
      floating: {
        active: paneIsFloating(root(), paneId),
        onToggle: focusedPaneAction(focusedPaneId, toggleFloating),
      },
      solo: multiPane
        ? {
            active: soloedPaneId() === paneId,
            onToggle: focusedPaneAction(focusedPaneId, toggleSolo),
          }
        : undefined,
      onPark: focusedPaneAction(focusedPaneId, backgroundPane),
      onClose: focusedPaneAction(focusedPaneId, closePane),
    });
  });

  onCleanup(() => props.onFocusedPaneActionsChange?.(null));

  createEffect(() => {
    props.onFocusPane?.(focusPane);
  });

  createEffect(() => {
    const unregister = props.onAddFloatingWindow?.(addFloatingWindow);
    if (unregister) onCleanup(unregister);
  });

  createEffect(() => {
    const unregister = props.onAddManagedWindow?.(addManagedWindow);
    if (unregister) onCleanup(unregister);
  });

  // Remember last active tab per tabs container so switching away doesn't reset.
  const tabMemory: Record<string, number> = {};

  // Stack keys are registered only while a layout is mounted. This keeps the
  // action map honest: spatial commands do not appear without a tree to walk.
  createEffect(() => {
    const ids = paneIds().filter(
      (paneId) => layoutState().assignments[paneId] != null,
    );
    const fpId = focusedPaneId();
    const cyclePane = (delta: 1 | -1) => {
      if (ids.length === 0) return;
      const index = fpId ? ids.indexOf(fpId) : -1;
      focusPane(ids[(index + delta + ids.length) % ids.length]);
    };
    const directionMenu = {
      token: "← ↑ ↓ →",
      group: "focus-direction",
    };
    const moveMenu = {
      token: "Shift+← ↑ ↓ →",
      group: "move-direction",
    };
    const resizeMenu = {
      token: "Alt+← ↑ ↓ →",
      group: "resize-direction",
    };
    const numberMenu = {
      token: ids.length === 1 ? "1" : `1–${Math.min(9, ids.length)}`,
      group: "focus-number",
    };
    const bindings: Parameters<typeof registerPrefixAction>[] = [
      ["Tab", () => cyclePane(1), t("help.nextPane")],
      ["Shift+Tab", () => cyclePane(-1), t("help.previousPane")],
      ["z", () => fpId && toggleSolo(fpId), t("help.soloPaneShort")],
      [
        "h",
        () => setNextSplitDirection("horizontal"),
        t("help.splitHorizontal"),
      ],
      ["v", () => setNextSplitDirection("vertical"), t("help.splitVertical")],
      ["b", toggleContainerSplit, t("help.toggleSplit")],
      ["t", () => changeContainerLayout("tabs"), t("help.layoutTabbed")],
      ["s", () => changeContainerLayout("stacking"), t("help.layoutStacking")],
      ["l", cycleContainerLayout, t("help.cycleContainerLayout")],
      ["Space", focusModeToggle, t("help.focusModeToggle")],
      [
        "Shift+Space",
        () => fpId && toggleFloating(fpId),
        t("help.toggleFloating"),
      ],
      [
        "ArrowLeft",
        () => focusInDirection("left"),
        t("help.focusStack"),
        directionMenu,
      ],
      [
        "ArrowRight",
        () => focusInDirection("right"),
        t("help.focusStack"),
        directionMenu,
      ],
      [
        "ArrowUp",
        () => focusInDirection("up"),
        t("help.focusStack"),
        directionMenu,
      ],
      [
        "ArrowDown",
        () => focusInDirection("down"),
        t("help.focusStack"),
        directionMenu,
      ],
      [
        "Shift+ArrowLeft",
        () => moveViewInDirection("left"),
        t("help.moveView"),
        moveMenu,
      ],
      [
        "Shift+ArrowRight",
        () => moveViewInDirection("right"),
        t("help.moveView"),
        moveMenu,
      ],
      [
        "Shift+ArrowUp",
        () => moveViewInDirection("up"),
        t("help.moveView"),
        moveMenu,
      ],
      [
        "Shift+ArrowDown",
        () => moveViewInDirection("down"),
        t("help.moveView"),
        moveMenu,
      ],
      [
        "Alt+ArrowLeft",
        () => resizeInDirection("left"),
        t("help.resizeStack"),
        resizeMenu,
      ],
      [
        "Alt+ArrowRight",
        () => resizeInDirection("right"),
        t("help.resizeStack"),
        resizeMenu,
      ],
      [
        "Alt+ArrowUp",
        () => resizeInDirection("up"),
        t("help.resizeStack"),
        resizeMenu,
      ],
      [
        "Alt+ArrowDown",
        () => resizeInDirection("down"),
        t("help.resizeStack"),
        resizeMenu,
      ],
      [
        "=",
        () => updateRoot(balanceLayout(root())),
        t("help.balanceWorkspace"),
      ],
      ["q", () => fpId && backgroundPane(fpId), t("help.removeFromPane")],
      ["x", () => fpId && closePane(fpId), t("pane.close")],
      ...ids
        .slice(0, 9)
        .map(
          (paneId, index): Parameters<typeof registerPrefixAction> => [
            String(index + 1),
            () => focusPane(paneId),
            t("help.focusContainerNumber"),
            numberMenu,
          ],
        ),
    ];
    const unbind = bindings.map((binding) => registerPrefixAction(...binding));
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

  function handleRectsChange(
    split: LayoutSplit,
    rects: readonly (LayoutRect | null)[],
  ) {
    updateRoot(
      replaceSplit(split, {
        ...split,
        children: split.children.map((child, index) => ({
          ...child,
          ...(rects[index]
            ? { rect: clampRect(rects[index]) }
            : child.rect
              ? { rect: child.rect }
              : {}),
        })),
      }),
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
    const live = liveSessions();
    const fpId = focusedPaneId();
    const fsId = fpId ? (layoutState().assignments[fpId] ?? null) : null;
    const handler = (event: KeyboardEvent) => {
      if (!fpId || !fsId) return;
      const session = live.find((item) => item.id === fsId);
      if (!session || session.state !== "exited") return;
      // Enter restarts the terminal named by the banner. Escape dismisses what
      // is in front of you; neither action needs the workspace prefix.
      if (
        event.key === "Enter" &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        !event.shiftKey &&
        !(event.target instanceof Element && event.target.closest("button"))
      ) {
        event.preventDefault();
        event.stopImmediatePropagation();
        const supportsRestart = workspaceState().connections.find(
          (connection) => connection.id === session.connectionId,
        )?.supportsRestart;
        if (supportsRestart) workspace.restartSession(fsId);
        else removePane(fpId, true);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        // Remove the leaf before closing the retained backend terminal so the
        // surviving panes reflow immediately instead of waiting for the next
        // catalogue snapshot to clear an empty assignment.
        removePane(fpId, true);
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  const multiPane = () => leafCount(root()) > 1;
  const hasAssignedPane = () =>
    Object.values(layoutState().assignments).some((value) => value !== null);

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
    get hasAssignedPane() {
      return hasAssignedPane();
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
    onDropAssignmentBeside: dropAssignmentBeside,
    get soloedPaneId() {
      return soloedPaneId();
    },
    onToggleSolo: toggleSolo,
    assignmentTitle,
    onRectChange: handleRectChange,
    onRectsChange: handleRectsChange,
    onColumnWidth: handleColumnWidth,
    floatingDepth,
    onRaisePane: raisePane,
    isFloatingPane: (paneId) => paneIsFloating(root(), paneId),
    onAddFloatingWindow: addFloatingWindow,
  };
  return (
    <LayoutTreeContext.Provider value={ctxValue}>
      <div
        ref={layoutElement}
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          position: "relative",
        }}
      >
        <PaneNode
          node={root()}
          assignments={layoutState().assignments}
          focusedPaneId={focusedPaneId()}
          visible={props.manageVisibility ?? true}
          // Expose/overlay visibility is not a structural hide. Keep the
          // mounted surface's size claim alive there so choosing its card
          // does not destroy and recreate the encoder.
          surfaceSizingVisible
        />
        <Show when={nextSplitDirection()}>
          {(direction) => (
            <div
              role="status"
              style={{
                position: "absolute",
                top: `${uiScale(props.fontSize).gap}px`,
                left: "50%",
                transform: "translateX(-50%)",
                padding: `${uiScale(props.fontSize).controlY}px ${uiScale(props.fontSize).controlX}px`,
                "border-radius": `${uiScale(props.fontSize).tightGap}px`,
                border: `1px solid ${themeFor(props.palette).border}`,
                background: themeFor(props.palette).solidPanelBg,
                color: themeFor(props.palette).fg,
                "font-size": `${uiScale(props.fontSize).sm}px`,
                "z-index": z.exitedBanner,
                "pointer-events": "none",
              }}
            >
              {direction() === "horizontal"
                ? "↔"
                : direction() === "vertical"
                  ? "↕"
                  : direction() === "tabs"
                    ? "▤"
                    : "☰"}{" "}
              {t("layout.nextSplit")}
            </div>
          )}
        </Show>
      </div>
    </LayoutTreeContext.Provider>
  );
}

function PaneNode(props: {
  node: LayoutNode;
  assignments: Record<string, SessionId | null>;
  focusedPaneId: string | null;
  visible: boolean;
  surfaceSizingVisible: boolean;
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
          surfaceSizingVisible={props.surfaceSizingVisible}
        />
      }
    >
      {(split) => (
        <Show
          when={
            split().direction === "tabs" || split().direction === "stacking"
          }
          fallback={
            <Show
              when={
                split().direction === "scrolling" ||
                split().direction === "floating" ||
                split().direction === "workspace"
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
                              surfaceSizingVisible={
                                props.surfaceSizingVisible && !hidden()
                              }
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
                surfaceSizingVisible={props.surfaceSizingVisible}
                path={path()}
              />
            </Show>
          }
        >
          {(() => {
            const theme = () => themeFor(ctx.palette);
            const scale = () => uiScale(ctx.fontSize);
            const tabKey = () => path().join(".") || "root";
            let tabContainer!: HTMLDivElement;

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

            const childPaneIds = (
              child: LayoutChild,
              index: number,
            ): string[] => {
              const prefix = [...path(), index];
              if (child.node.type === "leaf") return [prefix.join(".")];
              return enumeratePanes(child.node).map(({ id }) =>
                [...prefix, ...id.split(".")].join("."),
              );
            };

            const paneForChild = (
              child: LayoutChild,
              index: number,
            ): string => {
              const ids = childPaneIds(child, index);
              const focused = props.focusedPaneId;
              return (
                (focused && ids.includes(focused) ? focused : null) ??
                ids.find((id) => props.assignments[id] != null) ??
                ids[0]
              );
            };

            const tabLabel = (child: LayoutChild, index: number): string => {
              if (child.label) return child.label;
              const assignment = props.assignments[paneForChild(child, index)];
              if (assignment) {
                const title = ctx.assignmentTitle(assignment);
                if (title) return title;
              }
              return tp("pane.tab", { index: index + 1 });
            };

            const tabEdge = (event: DragEvent): SpatialDirection | null => {
              const rect = tabContainer.getBoundingClientRect();
              const x = event.clientX - rect.left;
              const y = event.clientY - rect.top;
              const distances = [
                ["left", x],
                ["right", rect.width - x],
                ["up", y],
                ["down", rect.height - y],
              ] as const;
              const [direction, distance] = distances.reduce(
                (best, candidate) =>
                  candidate[1] < best[1] ? candidate : best,
              );
              return distance <= Math.min(rect.width, rect.height) * 0.24
                ? direction
                : null;
            };

            return (
              <div
                ref={tabContainer}
                style={{
                  display: "flex",
                  "flex-direction": "column",
                  width: "100%",
                  height: "100%",
                }}
                onDragOver={(event) => {
                  if (!isTileDrag(event) || !tabEdge(event)) return;
                  event.preventDefault();
                  event.dataTransfer!.dropEffect = "move";
                }}
                onDrop={(event) => {
                  const assignment = tileDragAssignment(event);
                  const direction = tabEdge(event);
                  if (!assignment || !direction) return;
                  const active = activeTab();
                  const target = split().children[active];
                  if (!target) return;
                  event.preventDefault();
                  event.stopPropagation();
                  ctx.onDropAssignmentBeside(
                    assignment,
                    paneForChild(target, active),
                    paneDragSource(event) ?? undefined,
                    direction,
                  );
                }}
              >
                <div
                  style={{
                    display: "flex",
                    "flex-direction":
                      split().direction === "stacking" ? "column" : "row",
                    gap: "1px",
                    "flex-shrink": 0,
                    "background-color": theme().solidPanelBg,
                    "border-bottom": `1px solid ${theme().subtleBorder}`,
                    "font-size": `${scale().sm}px`,
                  }}
                >
                  <For each={split().children}>
                    {(child, index) => {
                      const sourcePaneId = () => paneForChild(child, index());
                      const assignment = () =>
                        props.assignments[sourcePaneId()] ?? null;
                      const label = () => tabLabel(child, index());
                      return (
                        <button
                          title={label()}
                          draggable={assignment() != null}
                          onDragStart={(event) => {
                            const value = assignment();
                            if (value)
                              startPaneTileDrag(event, value, sourcePaneId());
                          }}
                          onPointerDown={(event) => {
                            const value = assignment();
                            if (value)
                              startPaneTouchDrag(event, value, sourcePaneId());
                          }}
                          onClick={() => ctx.onFocusPane(sourcePaneId())}
                          style={{
                            ...ui.btn,
                            flex:
                              split().direction === "stacking" ? "0 0 auto" : 1,
                            "min-width": 0,
                            padding: `${scale().controlY}px ${scale().controlX}px`,
                            "font-size": `${scale().sm}px`,
                            "text-align":
                              split().direction === "stacking"
                                ? "left"
                                : "center",
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
                          {label()}
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
                            surfaceSizingVisible={
                              props.surfaceSizingVisible && active()
                            }
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
  surfaceSizingVisible: boolean;
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
  // Center groups as a tab; edges make an explicit structural split.
  const [tileDropZone, setTileDropZone] = createSignal<
    SpatialDirection | "center" | null
  >(null);
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

  function dropZone(event: DragEvent): SpatialDirection | "center" {
    const rect = paneContainer.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const distances = [
      ["left", x],
      ["right", rect.width - x],
      ["up", y],
      ["down", rect.height - y],
    ] as const;
    const [direction, distance] = distances.reduce((best, candidate) =>
      candidate[1] < best[1] ? candidate : best,
    );
    return distance <= Math.min(rect.width, rect.height) * 0.24
      ? direction
      : "center";
  }

  function dropZoneStyle(): JSX.CSSProperties {
    const zone = tileDropZone();
    if (zone === "left") return { inset: "0 66% 0 0" };
    if (zone === "right") return { inset: "0 0 0 66%" };
    if (zone === "up") return { inset: "0 0 66% 0" };
    if (zone === "down") return { inset: "66% 0 0 0" };
    return { inset: "18%" };
  }

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
        "box-sizing": "border-box",
        border:
          ctx.multiPane && !ctx.isFloatingPane(props.paneId)
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
      onDragOver={(e) => {
        if (!ctx.onDropTile || !isTileDrag(e)) return;
        e.preventDefault(); // allow the drop
        e.dataTransfer!.dropEffect = "copy";
        const zone = dropZone(e);
        if (tileDropZone() !== zone) setTileDropZone(zone);
      }}
      onDragLeave={(e) => {
        // Ignore leaves into child elements; only clear when truly leaving.
        if (!e.currentTarget.contains(e.relatedTarget as Node | null))
          setTileDropZone(null);
      }}
      onDrop={(e) => {
        const assignment = tileDragAssignment(e);
        const zone = tileDropZone() ?? dropZone(e);
        setTileDropZone(null);
        if (assignment && ctx.onDropTile) {
          e.preventDefault();
          e.stopPropagation();
          const sourcePaneId = paneDragSource(e) ?? undefined;
          // A parked item is not being moved from another pane. In floating
          // mode it therefore becomes an independent window even when the
          // pointer lands over an existing frame; treating that frame as an
          // ordinary pane target replaces its occupant and briefly renders
          // the displaced terminal both there and in the dock.
          if (zone !== "center") {
            ctx.onDropAssignmentBeside(
              assignment,
              props.paneId,
              sourcePaneId,
              zone,
            );
          } else if (
            ctx.isFloatingPane(props.paneId) &&
            floatingDropAppendsWindow(sourcePaneId)
          ) {
            ctx.onAddFloatingWindow(assignment);
          } else {
            ctx.onDropTile(assignment, props.paneId, sourcePaneId);
          }
        }
      }}
    >
      <Show when={tileDropZone()}>
        <div
          style={{
            position: "absolute",
            "z-index": 5,
            "pointer-events": "none",
            background: `color-mix(in srgb, ${theme().accent} 14%, transparent)`,
            border: `2px solid ${theme().accent}`,
            "box-sizing": "border-box",
            ...dropZoneStyle(),
          }}
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
                  showHint={showEmptyPaneHint(
                    ctx.multiPane,
                    ctx.hasAssignedPane,
                    props.isFocused,
                  )}
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
                      {t("pane.restart")}{" "}
                      <kbd style={ui.kbd}>{t("keyboard.enter")}</kbd>
                    </button>
                  </Show>
                  <button
                    onClick={() => ctx.onClosePane(props.paneId)}
                    style={mergeStyle(ui.btn, {
                      "font-size": `${scale().sm}px`,
                      opacity: 0.5,
                    })}
                  >
                    {t("pane.close")}{" "}
                    <kbd style={ui.kbd}>{t("keyboard.esc")}</kbd>
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
                showHint={showEmptyPaneHint(
                  ctx.multiPane,
                  ctx.hasAssignedPane,
                  props.isFocused,
                )}
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
                // Hidden tab/stack leaves stay mounted for instant return,
                // but must withdraw their stale size/DPI offer while they are
                // not participating in the visible window-manager layout.
                resizable={props.surfaceSizingVisible}
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
 * Both use the same renderer as the tiling manager. A scrolling child may be
 * a stack; a floating child remains a single leaf so one frame has exactly one
 * assignment. `floatingPaneIds` still handles old nested state defensively.
 */
function ManagedSplit(props: {
  split: LayoutSplit;
  assignments: Record<string, string | null>;
  focusedPaneId: string | null;
  visible: boolean;
  surfaceSizingVisible: boolean;
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
          surfaceSizingVisible={props.surfaceSizingVisible}
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
        surfaceSizingVisible={props.surfaceSizingVisible}
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
  surfaceSizingVisible: boolean;
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
              surfaceSizingVisible={props.surfaceSizingVisible}
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
 * Free-positioned windows, last one raised on top. A `workspace` split also
 * renders its one rect-less child as the tiled base beneath those frames.
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
  surfaceSizingVisible: boolean;
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
  const isFrame = (child: LayoutChild) =>
    props.split.direction === "floating" || child.rect != null;
  const frameNodes = () =>
    floatingFrameNodes(props.split.children.filter(isFrame));
  const baseNode = () =>
    props.split.direction === "workspace"
      ? props.split.children.find((child) => child.rect == null)?.node
      : undefined;
  const baseIndex = () => {
    const node = baseNode();
    return node
      ? props.split.children.findIndex((child) => child.node === node)
      : -1;
  };
  let layer!: HTMLDivElement;
  // The frame being dragged, so the window follows the pointer without a
  // round trip through the serialized layout on every pointermove.
  const [dragging, setDragging] = createSignal<{
    index: number;
    rect: LayoutRect;
  } | null>(null);
  let previousLayerBox: DOMRect | null = null;
  let layerResizeFrame = 0;
  onMount(() => {
    previousLayerBox = layer.getBoundingClientRect();
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(layerResizeFrame);
      layerResizeFrame = requestAnimationFrame(() => {
        layerResizeFrame = 0;
        const next = layer.getBoundingClientRect();
        const previous = previousLayerBox;
        previousLayerBox = next;
        if (
          !previous ||
          dragging() ||
          (previous.left === next.left &&
            previous.top === next.top &&
            previous.width === next.width &&
            previous.height === next.height)
        )
          return;
        ctx.onRectsChange(
          props.split,
          props.split.children.map((child, index) =>
            isFrame(child)
              ? rebaseFloatingRect(
                  clampRect(child.rect ?? cascadeRect(index)),
                  previous,
                  next,
                )
              : null,
          ),
        );
      });
    });
    observer.observe(layer);
    onCleanup(() => {
      observer.disconnect();
      cancelAnimationFrame(layerResizeFrame);
    });
  });

  const rectOf = (child: LayoutChild, index: number): LayoutRect => {
    const live = dragging();
    if (live && live.index === index) return live.rect;
    return clampRect(child.rect ?? cascadeRect(index));
  };

  const drag = (
    event: PointerEvent,
    index: number,
    start: LayoutRect,
    mode: FloatingDragMode,
  ) => {
    if (event.button !== 0 && !(event.altKey && event.button === 2)) return;
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
          : resizeFloatingRect(start, dx, dy, mode);
      // Use a fixed physical capture radius so snapping feels the same on a
      // narrow phone and a desktop viewport. The stored geometry remains in
      // percentages.
      const snapX = percentOf(12, box.width);
      const snapY = percentOf(12, box.height);
      const neighbors = props.split.children.flatMap((child, at) => {
        if (at === index || !isFrame(child)) return [];
        const occupied = floatingPaneIds(child.node, [...props.path, at]).some(
          (paneId) => props.assignments[paneId] != null,
        );
        return occupied ? [rectOf(child, at)] : [];
      });
      setDragging({
        index,
        rect: snapFloatingRect(next, mode, snapX, snapY, neighbors),
      });
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

  // Sway's floating_modifier applies over the whole window, not just its
  // decoration. Capture before the surface/terminal sees the press so an
  // Alt-drag moves the frame without also clicking the remote application.
  onMount(() => {
    const modifierDrag = (event: PointerEvent) => {
      if (
        !event.altKey ||
        (event.button !== 0 && event.button !== 2) ||
        !(event.target instanceof Element)
      )
        return;
      const frame = event.target.closest<HTMLElement>(
        "[data-yas-floating-frame-index]",
      );
      if (!frame || !layer.contains(frame)) return;
      const index = Number(frame.dataset.yasFloatingFrameIndex);
      const child = props.split.children[index];
      if (!Number.isInteger(index) || !child) return;
      const ids = floatingPaneIds(child.node, [...props.path, index]);
      if (ctx.soloedPaneId && ids.includes(ctx.soloedPaneId)) return;
      const paneId =
        (props.focusedPaneId && ids.includes(props.focusedPaneId)
          ? props.focusedPaneId
          : null) ??
        ids.find((candidate) => props.assignments[candidate] != null) ??
        ids[0];
      if (paneId) ctx.onFocusPane(paneId);
      event.stopPropagation();
      if (event.button === 0) {
        drag(event, index, rectOf(child, index), "move");
        return;
      }
      const box = frame.getBoundingClientRect();
      const horizontal = event.clientX < box.left + box.width / 2 ? "w" : "e";
      const vertical = event.clientY < box.top + box.height / 2 ? "n" : "s";
      drag(
        event,
        index,
        rectOf(child, index),
        `${vertical}${horizontal}` as FloatingResizeEdge,
      );
    };
    layer.addEventListener("pointerdown", modifierDrag, true);
    onCleanup(() =>
      layer.removeEventListener("pointerdown", modifierDrag, true),
    );
  });

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
      onDragOver={(event) => {
        if (!isTileDrag(event)) return;
        event.preventDefault();
        event.dataTransfer!.dropEffect = "copy";
      }}
      onDrop={(event) => {
        // Moving an existing floating pane keeps the ordinary pane-drop
        // semantics. A sidebar card has no pane source and becomes a new
        // top-level window even when it lands in the empty space between two.
        if (paneDragSource(event)) return;
        const value = tileDragAssignment(event);
        if (!value) return;
        event.preventDefault();
        event.stopPropagation();
        ctx.onAddFloatingWindow(value);
      }}
    >
      <Show when={baseNode()}>
        {(node) => {
          const index = () => baseIndex();
          const prefix = () => props.paneIdAt(index());
          const hidden = () => {
            const solo = ctx.soloedPaneId;
            return (
              !!solo && solo !== prefix() && !solo.startsWith(prefix() + ".")
            );
          };
          return (
            <div
              style={{
                position: "absolute",
                inset: 0,
                display: hidden() ? "none" : "block",
                overflow: "hidden",
                "z-index": 0,
              }}
            >
              <PaneNode
                node={node()}
                assignments={props.assignments}
                focusedPaneId={props.focusedPaneId}
                visible={props.visible && !hidden()}
                surfaceSizingVisible={props.surfaceSizingVisible && !hidden()}
                path={[...props.path, index()]}
              />
            </div>
          );
        }}
      </Show>
      {/* Key floating windows by the node inside each layout child. The child
          wrapper is rewritten whenever its rect is rebased, so keying by the
          wrapper remounts every terminal/surface once per viewport resize.
          The node survives both rect changes and sibling removal, while still
          disposing exactly the subtree that is actually removed. */}
      <For each={frameNodes()}>
        {(node) => {
          const index = () =>
            props.split.children.findIndex((child) => child.node === node);
          const child = () => props.split.children[index()];
          const paneIds = () => floatingPaneIds(node, [...props.path, index()]);
          const paneId = () => {
            const ids = paneIds();
            const focused = props.focusedPaneId;
            if (focused && ids.includes(focused)) return focused;
            return (
              ids.find((id) => props.assignments[id] != null) ??
              ids[0] ??
              props.paneIdAt(index())
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
              : rectOf(child(), index());
          const focused = () => props.holdsFocus(index());
          const title = () => {
            const value = assignment();
            return value
              ? floatingWindowTitle(value, sessions(), surfaceFor(value))
              : "";
          };
          const surface = () => {
            const value = assignment();
            return value ? surfaceFor(value) : null;
          };
          const resizeHandles: readonly {
            edge: FloatingResizeEdge;
            cursor: string;
            style: JSX.CSSProperties;
          }[] = [
            {
              edge: "n",
              cursor: "ns-resize",
              style: { top: 0, left: "10px", right: "10px", height: "6px" },
            },
            {
              edge: "e",
              cursor: "ew-resize",
              style: { top: "10px", right: 0, bottom: "10px", width: "6px" },
            },
            {
              edge: "s",
              cursor: "ns-resize",
              style: { right: "10px", bottom: 0, left: "10px", height: "6px" },
            },
            {
              edge: "w",
              cursor: "ew-resize",
              style: { top: "10px", bottom: "10px", left: 0, width: "6px" },
            },
            {
              edge: "nw",
              cursor: "nwse-resize",
              style: { top: 0, left: 0, width: "10px", height: "10px" },
            },
            {
              edge: "ne",
              cursor: "nesw-resize",
              style: { top: 0, right: 0, width: "10px", height: "10px" },
            },
            {
              edge: "se",
              cursor: "nwse-resize",
              style: { right: 0, bottom: 0, width: "10px", height: "10px" },
            },
            {
              edge: "sw",
              cursor: "nesw-resize",
              style: { bottom: 0, left: 0, width: "10px", height: "10px" },
            },
          ];
          return (
            <Show when={occupied()}>
              <div
                data-yas-floating-frame-index={index()}
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
                    ctx.floatingDepth(props.paneIdAt(index())),
                }}
                onContextMenu={(event) => {
                  if (event.altKey) event.preventDefault();
                }}
                onPointerDown={() => {
                  ctx.onRaisePane(props.paneIdAt(index()));
                  ctx.onFocusPane(paneId());
                }}
              >
                <div
                  role="presentation"
                  onPointerDown={(event) => {
                    if (!soloed()) drag(event, index(), rect(), "move");
                  }}
                  style={{
                    height: `${scale().md * 2}px`,
                    flex: "0 0 auto",
                    display: "flex",
                    "align-items": "center",
                    cursor: soloed() ? "default" : "move",
                    "background-color": theme().bg,
                    color: theme().fg,
                    "border-bottom": `1px solid ${theme().border}`,
                    "touch-action": "none",
                    "font-size": `${scale().sm}px`,
                  }}
                >
                  <Show when={surface()}>
                    {(value) => (
                      <span
                        style={{
                          display: "flex",
                          "align-items": "center",
                          padding: `0 0 0 ${scale().controlY}px`,
                          "flex-shrink": 0,
                        }}
                      >
                        <SurfaceIcon
                          surface={value()}
                          theme={theme()}
                          scale={scale()}
                          size={Math.round(scale().md * 1.45)}
                        />
                      </span>
                    )}
                  </Show>
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
                </div>
                <div style={{ flex: 1, position: "relative", "min-height": 0 }}>
                  <PaneNode
                    node={node}
                    assignments={props.assignments}
                    focusedPaneId={props.focusedPaneId}
                    visible={props.visible && !hidden()}
                    surfaceSizingVisible={
                      props.surfaceSizingVisible && !hidden()
                    }
                    path={[...props.path, index()]}
                  />
                </div>
                <Show when={!soloed()}>
                  <For each={resizeHandles}>
                    {(handle) => (
                      <div
                        role="presentation"
                        aria-label={tp("pane.resizeEdge", {
                          edge: t(`direction.${handle.edge}`),
                        })}
                        onPointerDown={(event) =>
                          drag(event, index(), rect(), handle.edge)
                        }
                        style={{
                          position: "absolute",
                          cursor: handle.cursor,
                          "touch-action": "none",
                          "z-index": 2,
                          ...handle.style,
                        }}
                      />
                    )}
                  </For>
                </Show>
              </div>
            </Show>
          );
        }}
      </For>
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
      <Show when={props.showHint !== false}>
        <button
          type="button"
          aria-label={t("workspace.newTerminal")}
          onClick={() => props.onCreateInPane?.(props.paneId)}
          style={{
            border: "none",
            padding: 0,
            color: "inherit",
            background: "transparent",
            cursor: "pointer",
            font: "inherit",
          }}
        >
          {tp("pane.startWith", { shortcut: prefixChordLabel() })}
        </button>
      </Show>
    </div>
  );
}
