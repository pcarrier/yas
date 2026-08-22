import {
  createSignal,
  createEffect,
  createMemo,
  batch,
  onMount,
  onCleanup,
  untrack,
  Show,
  For,
  Index,
  type JSX,
  type Accessor,
} from "solid-js";
import {
  YasTerminal,
  YasSurfaceView,
  YasWorkspaceProvider,
  createYasWorkspace,
  createYasSessions,
  createYasWorkspaceState,
  createYasWorkspaceConnection,
} from "@yas-run/solid";
import {
  YAS_SURFACE_TEXT_INPUT_EVENT,
  YasWorkspace,
  PALETTES,
  LSP_STATUS_OK,
  detectCodecSupport,
  getProbedCodecSupport,
  setAllowedCodecSupport,
  isIOS,
} from "@yas-run/core";
import type {
  YasTransport,
  YasSession,
  YasSurface,
  YasTerminalSurface,
  YasWasmModule,
  SessionId,
  SurfaceId,
  TerminalId,
  TerminalPalette,
  ConnectionId,
  LinkHover,
  UrlAssessment,
  YasActivity,
  YasSurfaceTextInputEvent,
  YasRelayRoute,
  WorkspaceSessionWorkspace,
  WorkspaceSessionProjectSelection,
} from "@yas-run/core";
import type { ConnectionSpec } from "./App";
import { createMetrics } from "./createMetrics";
import { dropConnectionTabState } from "./connectionTab";
import { cardAspectRatio, surfaceCardSignature } from "./surfaceAspect";
import {
  createFontLoader,
  fontProtocolSourceKey,
  protocolFontFamilies,
  type FontProtocolSource,
} from "./createFontLoader";
import { createKeyboardShortcuts } from "./createKeyboardShortcuts";
import { truncateDocumentEntityTitle } from "./documentTitle";
import {
  PALETTE_KEY,
  FONT_KEY,
  FONT_SIZE_KEY,
  TEXT_GAMMA_KEY,
  AUDIO_BITRATE_KEY,
  AUDIO_MUTED_KEY,
  VIDEO_BANDWIDTH_KEY,
  VIDEO_SPEED_KEY,
  SURFACE_STREAMING_KEY,
  SURFACE_SMOOTHING_KEY,
  SURFACE_MAX_FPS_KEY,
  SURFACE_ZOOM_KEY,
  SURFACE_ZOOM_MODE_KEY,
  SURFACE_TOUCH_MODE_KEY,
  SURFACE_CODECS_KEY,
  WAYLAND_KEYBOARD_REQUESTS_KEY,
  MIN_SURFACE_ZOOM,
  MAX_SURFACE_ZOOM,
  LEFT_DOCK_WIDTH_KEY,
  PREVIEW_PANEL_WIDTH_KEY,
  readStorage,
  writeStorage,
  useStoredValue,
  preferredPalette,
  defaultFont,
  preferredFont,
  preferredFontSize,
  preferredTextGamma,
  preferredAudioBitrate,
  preferredAudioMuted,
  preferredVideoBandwidth,
  preferredVideoSpeed,
  preferredSurfaceStreaming,
  preferredSurfaceSmoothing,
  preferredSurfaceMaxFps,
  preferredSurfaceZoom,
  preferredSurfaceZoomMode,
  preferredSurfaceTouchMode,
  preferredSurfaceCodecs,
  preferredWaylandKeyboardRequests,
  preferredLeftDockWidth,
  preferredPreviewPanelWidth,
  preferredPreviewPanelOpen,
  MIN_PREVIEW_PANEL_WIDTH,
  preferredLeftDockOpen,
  preferredCollapsedSections,
  LEFT_DOCK_OPEN_KEY,
  PREVIEW_PANEL_OPEN_KEY,
  LEFT_COLLAPSED_KEY,
  yasHost,
  type SurfaceZoomMode,
  type SurfaceTouchMode,
} from "./storage";
import type { UIScale, Theme } from "./theme";
import {
  mergeStyle,
  sessionName,
  sessionPrefix,
  scrollbarStyle,
  themeFor,
  layout,
  ui,
  uiScale,
  z,
} from "./theme";
import { t, tp } from "./i18n";
import { TerminalDropTarget } from "./terminalDrop";
import { StatusBar } from "./StatusBar";
import { DesktopChrome } from "./DesktopChrome";
import { mprisSurfaceMatchScore } from "./desktopPresentation";
import { LeftDock, LEFT_PANELS, type LeftPanel } from "./LeftDock";
import { foldedSections, liveOverrides, toggleSection } from "./dockSections";
import { settleAttention } from "./surfaceAttention";
import {
  observeTopLevelSurface,
  pendingSurfacePlacementIsRetired,
  restoredSurfaceAssignments,
  surfacePlacementIdentity,
} from "./layout/surfacePlacement";
import { isParkedTabDropTarget } from "./layout/tabGrouping";
import {
  groupMusterPreviewResources,
  isMusterSession,
  musterStackKey,
  previewSessionsToWatch,
} from "./musterPreview";
import { displayHandle } from "./muster";
import { fontCatalog } from "./fontCatalog";
import { ExplorerPanel } from "./ide/ExplorerPanel";
import { BranchesPanel } from "./ide/BranchesPanel";
import { LogPanel } from "./ide/LogPanel";
import { SearchPanel } from "./ide/SearchPanel";
import { ResizeHandle } from "./layout/ResizeHandle";
import { searchInputFocused } from "./ide/searchStore";
import { ProblemsPanel } from "./ide/ProblemsPanel";
import { YasTile } from "./ide/YasTile";
import { tileDisplay } from "./ide/tileDisplay";
import {
  startTileDrag,
  startTouchDrag,
  fillTileDrag,
  isTileDrag,
  isPaneDrag,
  paneDragSource,
  tileDragAssignment,
  MAIN_PANE_SOURCE,
} from "./ide/tileDrag";
import {
  tabId,
  stripConn,
  registerTab,
  unregisterTab,
  resolveTab,
} from "./ide/tabRegistry";
import { createOpenTabs } from "./ide/openTabs";
import { createTabCloseTracker } from "./ide/tabCloseTracker";
import {
  allServerRoots,
  ensureServerRoots,
  dropServerRoots,
  addServerRoot,
  removeServerRoot,
  toggleServerRoot,
  reorderServerRoots,
  type Root,
} from "./ide/rootsStore";
import {
  dropSessionCatalog,
  ensureSessionCatalog,
  startApplication,
} from "./sessionCatalogs";
import { useIdeSession, type IdeSessionDescriptor } from "./ide/session";
import {
  currentSourceSessionForPty,
  sourceSessionCanResolveCwd,
} from "./ide/followTerminal";
import {
  dropFileIndexes,
  localFileIndex,
  searchFileIndex,
} from "./ide/fileIndex";
import { dropCachedCommits } from "./ide/commitCache";
import {
  dropEditorPositions,
  editorRecencySnapshot,
} from "./ide/editorPositions";
import { SwitcherOverlay } from "./SwitcherOverlay";
import { PaletteOverlay } from "./PaletteOverlay";
import { FontOverlay } from "./FontOverlay";
import { HelpOverlay } from "./HelpOverlay";
import { LinkOverlay } from "./LinkOverlay";
import { RemotesOverlay } from "./RemotesOverlay";
import { shellCapabilities } from "./shellCapabilities";
import { RootsOverlay } from "./RootsOverlay";
import { MediaOverlay } from "./MediaOverlay";
import { createMediaDevices } from "./mediaDevices";
import { LayoutContainer, EmptyPane } from "./layout/LayoutContainer";
import {
  newlyLaunchedSurface,
  shouldOpenAsFloatingWindow,
} from "./layout/floatingWindow";
import { autoFocusPaneTarget } from "./layout/treeContext";
import { WebOverlay } from "./WebOverlay";
import type { WebPaneHandle } from "./WebPane";
import { WebPaneHost } from "./WebPaneHost";
import {
  PersistentWebPanes,
  createWebPaneHostRegistry,
} from "./PersistentWebPanes";
import { WebPaneNav } from "./WebPaneNav";
import {
  ensurePreviewWorker,
  loadLocations,
  looksLikeWebLocation,
  previewSupported,
  saveLocations,
  withLocation,
  type WebLocation,
} from "./preview";

import { MobileToolbar } from "./MobileToolbar";
import type { PaneToolActions } from "./PaneTools";
import type { LayoutAssignments, WorkspaceLayout } from "./layout/store";
import {
  loadActiveLayoutState,
  saveActiveLayout,
  saveActiveLayoutState,
  saveToHistory,
  removeFromHistory,
  loadRecentLayouts,
  LAYOUT_HISTORY_KEY,
  surfaceAssignment,
  isSurfaceAssignment,
  isWebAssignment,
  parseWebAssignment,
  webAssignment,
  isTileAssignment,
  parseTileAssignment,
  parseDiffArg,
  parseSurfaceAssignment,
  connectionAwaitingWorkspaceRestore,
  parseWorkspaceRef,
  ptyIdForWorkspaceRef,
  surfaceIdForWorkspaceRef,
  surfaceWorkspaceRefForId,
  tabWorkspaceRef,
  terminalWorkspaceRefForPtyId,
  editorAssignment,
  manageAssignment,
  layoutFromDSL,
  leafCount,
} from "./layout/store";
import { setReveal } from "./ide/reveal";
import { debugPanelOpenFromHash } from "./workspaceUrl";
import type {
  WorkspaceSessionBinding,
  WorkspaceSessionController,
} from "./workspaceSession";
import { WorkspaceSessionPatchSequencer } from "./workspaceSessionPersistence";
import {
  addStoredRemote,
  ensureStoredRemotes,
  removeStoredRemote,
  remotesFor,
  toggleStoredRemote,
} from "./remotesStore";
import { WorkspaceSessionTabs } from "./WorkspaceSessionTabs";
import { WorkspaceSessionOverlay } from "./WorkspaceSessionOverlay";
import { PrefixMap } from "./PrefixMap";
import { previewPanelState as derivePreviewPanelState } from "./previewPanelState";
import {
  mergeWorkspaceSessionRemotes,
  storeAndActivateWorkspaceSessionRemote,
  type Remote,
  workspaceSessionRemoteMembershipSetter,
} from "./workspaceSessionRemotes";
import {
  cancelHmrRelease,
  claimHmrLease,
  deferHmrRelease,
  type HmrLeaseState,
} from "./hmrLease";
import {
  removeOwnedWorkspaceConnection,
  removeOwnedWorkspaceConnections,
  type WorkspaceTransportOwnership,
} from "./workspaceConnectionOwnership";
import { workspaceSessionBoundary } from "./workspaceSessionBoundary";

export type Overlay =
  | "expose"
  | "palette"
  | "font"
  | "help"
  | "remotes"
  | "roots"
  | "media"
  | "web"
  | "link"
  | null;

type HmrWorkspaceData = HmrLeaseState & {
  workspace: YasWorkspace;
  /** Module-local identity. A new object means this module was hot-reloaded. */
  owner: object;
  /** Transport generation owned by the parent ConnectedApp. */
  key: object;
  /** Whether this workspace or its parent closes the transport objects. */
  transportOwnership: WorkspaceTransportOwnership;
};

const hmrWorkspaceOwner = {};
const WORKSPACE_SESSION_PATCH_DEBOUNCE_MS = 250;
const UI_SESSION_HISTORY_MAX_ITEMS = 4_096;
const UI_SESSION_HISTORY_MAX_ENTRY_CHARS = 8_192;
const UI_SESSION_HISTORY_MAX_BYTES = 16 * 1024 * 1024;
const boundedStringLruBytes = new WeakMap<Map<string, string>, number>();

function setBoundedStringLru(
  cache: Map<string, string>,
  key: string,
  value: string,
  maxItems: number = UI_SESSION_HISTORY_MAX_ITEMS,
): void {
  if (key.length + value.length > UI_SESSION_HISTORY_MAX_ENTRY_CHARS) return;
  let bytes = boundedStringLruBytes.get(cache) ?? 0;
  const previous = cache.get(key);
  if (previous !== undefined) {
    cache.delete(key);
    bytes -= (key.length + previous.length) * 2 + 64;
  }
  cache.set(key, value);
  bytes += (key.length + value.length) * 2 + 64;
  while (cache.size > maxItems || bytes > UI_SESSION_HISTORY_MAX_BYTES) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    const oldestValue = cache.get(oldest);
    cache.delete(oldest);
    if (oldestValue !== undefined) {
      bytes -= (oldest.length + oldestValue.length) * 2 + 64;
    }
  }
  boundedStringLruBytes.set(cache, bytes);
}

/** Reset terminal view leases before handing an HMR workspace to new views. */
function resetHmrViewSizes(workspace: YasWorkspace): void {
  workspace.resetViewSizes();
}

function getHmrWorkspace(
  wasm: YasWasmModule,
  key: object,
  leaseOwner: object,
  transportOwnership: WorkspaceTransportOwnership,
): HmrWorkspaceData {
  const raw = import.meta.hot?.data?.workspace as
    | HmrWorkspaceData
    | YasWorkspace
    | undefined;
  // Accept the raw YasWorkspace stored by versions before HmrWorkspaceData.
  const prev = raw && "workspace" in raw ? raw.workspace : raw;
  const previousOwner = raw && "workspace" in raw ? raw.owner : null;
  const previousKey = raw && "workspace" in raw ? raw.key : null;
  if (prev && previousKey === key) {
    // Solid normally disposes every old terminal surface, but HMR is allowed to
    // replace a component boundary without visiting all of those cleanups. The
    // preserved workspace would then retain the vanished pane's size forever,
    // and the minimum-size policy would leave most of a larger pane blank.
    // Reset once per module generation; the replacement surfaces immediately
    // register their real boxes while transports and terminal state stay live.
    if (previousOwner !== hmrWorkspaceOwner) resetHmrViewSizes(prev);
    const data = raw as HmrWorkspaceData;
    data.owner = hmrWorkspaceOwner;
    data.transportOwnership = transportOwnership;
    claimHmrLease(data, leaseOwner);
    if (import.meta.hot) import.meta.hot.data.workspace = data;
    return data;
  }
  if (prev) {
    if (raw && "workspace" in raw) cancelHmrRelease(raw);
    removeOwnedWorkspaceConnections(
      prev,
      raw && "workspace" in raw
        ? (raw.transportOwnership ?? "workspace")
        : "workspace",
    );
  }
  const ws = new YasWorkspace({ wasm });
  const data = claimHmrLease<HmrWorkspaceData>(
    { workspace: ws, owner: hmrWorkspaceOwner, key, transportOwnership },
    leaseOwner,
  );
  if (import.meta.hot) {
    import.meta.hot.data.workspace = data;
  }
  return data;
}

export function Workspace(props: {
  connections: ConnectionSpec[] | (() => ConnectionSpec[]);
  wasm: YasWasmModule;
  hmrKey?: object;
  onAuthError: () => void;
  relayRoutes?: () => readonly YasRelayRoute[];
  workspaceSession?:
    | WorkspaceSessionBinding
    | Accessor<WorkspaceSessionBinding | null>;
  workspaceSessions?: WorkspaceSessionController;
  transportOwnership?: WorkspaceTransportOwnership;
}) {
  const hmrLeaseOwner = {};
  const hmrKey = props.hmrKey ?? {};
  const transportOwnership = props.transportOwnership ?? "workspace";
  const hmrData = getHmrWorkspace(
    props.wasm,
    hmrKey,
    hmrLeaseOwner,
    transportOwnership,
  );
  const workspace = hmrData.workspace;
  const sessionBoundary = workspaceSessionBoundary(props.workspaceSession);

  // Normalise: accept either a static array or a reactive accessor.
  const getConnections =
    typeof props.connections === "function"
      ? props.connections
      : () => props.connections as ConnectionSpec[];

  const notifiedConnections = new Map<
    string,
    {
      callback: NonNullable<ConnectionSpec["onConnection"]>;
      connection: NonNullable<ReturnType<YasWorkspace["getConnection"]>>;
    }
  >();

  // Reactively reconcile workspace connections whenever the list changes.
  createEffect(() => {
    const next = getConnections();
    const nextIds = new Set(next.map((c) => c.id));
    const nextById = new Map(next.map((c) => [c.id, c]));

    // Remove connections no longer in the list, or whose transport was
    // replaced under a stable route name. Relay generations deliberately
    // preserve the UI id while replacing the nested byte stream.
    const existing = workspace.getSnapshot().connections;
    for (const conn of existing) {
      const replacement = nextById.get(conn.id);
      const materialized = workspace.getConnection(conn.id);
      const replacementTransport =
        replacement?.connection?.transport ?? replacement?.transport;
      if (
        !nextIds.has(conn.id) ||
        (replacement && materialized?.transport !== replacementTransport)
      ) {
        notifiedConnections.get(conn.id)?.callback(null);
        notifiedConnections.delete(conn.id);
        removeOwnedWorkspaceConnection(workspace, conn.id, transportOwnership);
      }
    }

    // Add new connections (snapshot may have changed after removals).
    const existingIds = new Set(
      workspace.getSnapshot().connections.map((c) => c.id),
    );
    for (const conn of next) {
      if (!existingIds.has(conn.id)) {
        workspace.addConnection({
          id: conn.id,
          connection: conn.connection,
          transport: conn.transport,
        });
      }
    }

    // Give transport owners access to the concrete YasConnection without
    // constructing a second protocol parser on the same transport. This is
    // how the outer home connection publishes RelayTransport instances.
    for (const spec of next) {
      const connection = workspace.getConnection(spec.id);
      const previous = notifiedConnections.get(spec.id);
      if (!spec.onConnection || !connection) {
        previous?.callback(null);
        notifiedConnections.delete(spec.id);
        continue;
      }
      if (
        previous?.callback === spec.onConnection &&
        previous.connection === connection
      ) {
        continue;
      }
      previous?.callback(null);
      spec.onConnection(connection);
      notifiedConnections.set(spec.id, {
        callback: spec.onConnection,
        connection,
      });
    }
  });

  onCleanup(() => {
    for (const { callback } of notifiedConnections.values()) callback(null);
    notifiedConnections.clear();
    if (import.meta.hot) {
      deferHmrRelease(
        hmrData,
        hmrLeaseOwner,
        () => import.meta.hot?.data?.workspace === hmrData,
        () =>
          removeOwnedWorkspaceConnections(
            workspace,
            hmrData.transportOwnership,
          ),
        () => {
          if (import.meta.hot?.data?.workspace === hmrData) {
            delete import.meta.hot.data.workspace;
          }
        },
      );
    } else {
      removeOwnedWorkspaceConnections(workspace, transportOwnership);
    }
  });

  const connectionSpecs = createMemo(() => getConnections());

  return (
    <YasWorkspaceProvider workspace={workspace}>
      {sessionBoundary.managed ? (
        <Show
          when={sessionBoundary.current()}
          keyed
          fallback={
            <WorkspaceScreen
              connectionSpecs={connectionSpecs}
              wasm={props.wasm}
              onAuthError={props.onAuthError}
              relayRoutes={props.relayRoutes}
              workspaceSessions={props.workspaceSessions}
            />
          }
        >
          {(workspaceSession) => (
            <WorkspaceScreen
              connectionSpecs={connectionSpecs}
              wasm={props.wasm}
              onAuthError={props.onAuthError}
              relayRoutes={props.relayRoutes}
              workspaceSession={workspaceSession}
              workspaceSessions={props.workspaceSessions}
            />
          )}
        </Show>
      ) : (
        <WorkspaceScreen
          connectionSpecs={connectionSpecs}
          wasm={props.wasm}
          onAuthError={props.onAuthError}
          relayRoutes={props.relayRoutes}
          workspaceSessions={props.workspaceSessions}
        />
      )}
    </YasWorkspaceProvider>
  );
}

function WorkspaceScreen(props: {
  connectionSpecs: () => ConnectionSpec[];
  wasm: YasWasmModule;
  onAuthError: () => void;
  relayRoutes?: () => readonly YasRelayRoute[];
  workspaceSession?: WorkspaceSessionBinding;
  workspaceSessions?: WorkspaceSessionController;
}) {
  const workspace = createYasWorkspace();
  const setSessionRemoteActive = workspaceSessionRemoteMembershipSetter(
    props.workspaceSession,
  );
  const wsState = createYasWorkspaceState(workspace);
  const sessions = createYasSessions(workspace);
  const initialSessionWorkspace =
    props.workspaceSession?.current().workspace ?? null;
  const clientFocusedPaneKey = props.workspaceSession
    ? `yas.workspaceSession.${props.workspaceSession.id}.focusedPane`
    : null;
  const unknownStoredExpandedSections =
    initialSessionWorkspace?.panels.expandedSections.filter(
      (section) => !(LEFT_PANELS as readonly string[]).includes(section),
    ) ?? [];
  const [activities, setActivities] = createSignal<readonly YasActivity[]>(
    workspace.activities.getSnapshot(),
  );
  const unsubscribeActivities = workspace.activities.subscribe(() =>
    setActivities(workspace.activities.getSnapshot()),
  );
  onCleanup(unsubscribeActivities);

  /** Connection ID labels from the CLI config — reactive. */
  const connectionLabels = createMemo(
    () =>
      new Map<string, string>(
        props.connectionSpecs().map((c) => [c.id, c.label]),
      ),
  );
  const multiConnection = createMemo(() => props.connectionSpecs().length > 1);
  const defaultConnectionId = createMemo(
    () => props.connectionSpecs()[0]?.id ?? "main",
  );

  // Read-only connections (an `.ro` share): their terminals render without
  // input affordances instead of swallowing keystrokes the server refuses.
  const readOnlyConnections = createMemo(
    () =>
      new Set(
        props
          .connectionSpecs()
          .filter((c) => c.readOnly)
          .map((c) => c.id),
      ),
  );
  const isSessionReadOnly = (sessionId: string): boolean => {
    if (readOnlyConnections().size === 0) return false;
    const s = wsState().sessions.find((x) => x.id === sessionId);
    return !!s && readOnlyConnections().has(s.connectionId);
  };
  /** The same answer about a whole connection, which is what a manage tile
   *  needs: read-only shares drop the client-control family, so its clients
   *  panel must not be offered rather than sit unanswered. */
  const isConnectionReadOnly = (connectionId: string): boolean =>
    readOnlyConnections().has(connectionId as ConnectionId);

  const focusedSession = () => {
    const snap = wsState();
    if (!snap.focusedSessionId) return null;
    return snap.sessions.find((s) => s.id === snap.focusedSessionId) ?? null;
  };
  const focusedSessionId = createMemo(() => wsState().focusedSessionId);

  /** The connection that owns the currently focused session (or the first). */
  const activeConnectionId = (): ConnectionId => {
    const fs = focusedSession();
    return fs?.connectionId ?? defaultConnectionId();
  };

  const connection = () => {
    const snap = wsState();
    return snap.connections.find((c) => c.id === activeConnectionId()) ?? null;
  };

  /**
   * The connection whose KV store holds the Relay catalogue: the home server,
   * which is the default connection. Editing remotes on a route would be
   * editing that other machine's catalogue, which is not what this panel does.
   */
  const homeConnectionId = (): ConnectionId => defaultConnectionId();
  const homeStoredRemotes = () => remotesFor(homeConnectionId());

  /** All connections from snapshot. */
  const allConnections = () => wsState().connections;

  // Viewer camera/microphone/screen sharing. Owned here, not by the media
  // panel: the capability advertisement and the encoder probes have to run
  // whether or not the panel is open, and the status bar reads the same
  // state to decide whether to light its media glyph.
  const mediaDevices = createMediaDevices({
    workspace,
    get connections() {
      return allConnections();
    },
    get connectionLabels() {
      return connectionLabels();
    },
    get readOnlyConnections() {
      return readOnlyConnections();
    },
  });

  const [surfaces, setSurfaces] = createSignal<YasSurface[]>([]);

  // Per-surface signature of the fields that drive reactive surface consumers
  // (parent, title, appId, origin, and both size pairs — see
  // surfaceCardSignature).
  // <For each> keys by reference, while surface metadata is held in plain
  // objects. Track a per-surface signature so every field a card renders gets
  // a fresh item when it changes, while unchanged surfaces keep their child.
  const surfaceSigs = new Map<string, string>();

  // Track the set of available connection IDs so the surface aggregation
  // effect re-runs when connections are added or removed.  The joined-string
  // comparison ensures the memo value only changes when the actual set of
  // IDs changes, not on every workspace snapshot update (which is frequent
  // due to terminal output, pings, etc.).
  const availableConnIds = createMemo(() =>
    wsState()
      .connections.map((c) => c.id)
      .sort()
      .join(","),
  );

  // Connections that completed the handshake.  Same joined-string trick as
  // above so consumers only re-run when readiness actually flips, not on
  // every snapshot.
  const readyConnIdsKey = createMemo(() =>
    wsState()
      .connections.filter((c) => c.ready)
      .map((c) => c.id)
      .sort()
      .join(","),
  );
  const readyConnIds = createMemo(
    () => new Set(readyConnIdsKey().split(",").filter(Boolean)),
  );

  // Aggregate surfaces from all connections.
  // When surface streaming is disabled the list is emptied, which cascades
  // through every derived view (focused surface, panes, preview panel,
  // status bar count, switcher) so windows disappear immediately.
  createEffect(() => {
    // Re-run when connection specs change OR when the set of live
    // connections changes (a connection that was absent when we first ran
    // may now be available, and we need its surfaceStore.onChange listener).
    const _connIds = availableConnIds();
    const streaming = surfaceStreaming();
    const cleanups: (() => void)[] = [];
    const syncAll = () => {
      if (!streaming) {
        if (untrack(() => surfaces()).length !== 0) {
          surfaceSigs.clear();
          setSurfaces([]);
        }
        return;
      }
      const all: YasSurface[] = [];
      const seenKeys = new Set<string>();
      let anyChanged = false;
      for (const spec of props.connectionSpecs()) {
        const conn = workspace.getConnection(spec.id);
        if (!conn) continue;
        for (const s of conn.surfaceStore.getSurfaces().values()) {
          const key = `${s.connectionId}:${s.surfaceId}`;
          seenKeys.add(key);
          const sig = surfaceCardSignature(s);
          if (surfaceSigs.get(key) !== sig) {
            surfaceSigs.set(key, sig);
            // Shallow copy: a new ref forces <For> to rebuild this
            // item's child, which is the only way a downstream
            // `props.surface.width` JSX read picks up the fresh value
            // (SolidJS doesn't track property access on plain objects).
            all.push({ ...s });
            anyChanged = true;
          } else {
            all.push(s);
          }
        }
      }
      // Prune sigs for surfaces that no longer exist so stale entries
      // don't forever block a new surface with the same id from
      // getting a fresh ref on first frame.
      if (surfaceSigs.size !== seenKeys.size) {
        for (const key of surfaceSigs.keys()) {
          if (!seenKeys.has(key)) {
            surfaceSigs.delete(key);
            anyChanged = true;
          }
        }
      }
      const prev = untrack(() => surfaces());
      if (!anyChanged && prev.length === all.length) return;
      setSurfaces(all);

      // If the user just started an app from the switcher, place its new
      // surface into the active panel as soon as it appears.
      const pending = untrack(() => pendingAppStart());
      if (pending) {
        const match = newlyLaunchedSurface(
          all,
          pending.connectionId,
          pending.appId,
          pending.existingSurfaceKeys,
        );
        if (match) {
          if (pendingAppStartTimer) {
            clearTimeout(pendingAppStartTimer);
            pendingAppStartTimer = undefined;
          }
          setPendingAppStart(null);
          // An application launched while floating owns a new window. The
          // generic focus path replaces the focused pane unless this flag is
          // explicit, which can leave the new surface parked until a later
          // layout remount happens to reconcile it.
          focusSurface(match.surfaceId, match.connectionId, true);
        }
      }
    };
    for (const spec of props.connectionSpecs()) {
      const conn = workspace.getConnection(spec.id);
      if (!conn) continue;
      cleanups.push(conn.surfaceStore.onChange(syncAll));
      // A client asking to be activated (xdg_activation_v1 — e.g. an Electron
      // app reacting to a notification click) gets the same treatment as
      // picking its surface in the switcher.
      cleanups.push(
        conn.surfaceStore.onActivated((surfaceId) =>
          activateSurface(surfaceId, spec.id),
        ),
      );
    }
    // Also refresh on workspace state changes (connection status
    // transitions) so the surface list stays in sync after reconnects
    // and initial connection setup.  The equality check in syncAll
    // prevents <For> churn on unrelated snapshot changes (terminal
    // frames, pacing, ping).
    cleanups.push(workspace.subscribe(syncAll));
    syncAll();
    onCleanup(() => cleanups.forEach((fn) => fn()));
  });

  // Relay state contains presentation metadata only. Connector targets and
  // credentials stay server-side; sessions store only selected route names.
  const remotes = createMemo<Remote[]>(() =>
    mergeWorkspaceSessionRemotes(
      props.relayRoutes?.() ?? [],
      homeStoredRemotes(),
    ),
  );
  const activeRemoteNames = () =>
    props.workspaceSession?.current().activeRemotes ?? [];

  /** Map remote name → connection status (derived from workspace snapshot). */
  // Content equality: the snapshot fires on every frame/ping, and a fresh Map
  // reference each tick would churn everything downstream that reads statuses.
  const remoteStatuses = createMemo(
    () => {
      const map = new Map<string, import("@yas-run/core").ConnectionStatus>();
      for (const conn of allConnections()) {
        map.set(conn.id, conn.status);
      }
      return map;
    },
    undefined,
    {
      equals: (a, b) =>
        a != null &&
        a.size === b.size &&
        [...b].every(([name, status]) => a.get(name) === status),
    },
  );

  const [palette, setPalette] =
    createSignal<TerminalPalette>(preferredPalette());
  const [font, setFont] = createSignal(preferredFont());
  const [fontSize, setFontSize] = createSignal(preferredFontSize());
  const [textGamma, setTextGamma] = createSignal(preferredTextGamma());
  const [overlay, setOverlay] = createSignal<Overlay>(null);
  // Whether the active connection serves the systemd watcher. Probed rather
  // than assumed: it is an extension somebody installed, not a server family,
  // and the status bar should not offer a panel with nothing behind it.
  const [openInNewTerminalMode, setOpenInNewTerminalMode] = createSignal(false);
  const [newTerminalTargetPaneId, setNewTerminalTargetPaneId] = createSignal<
    string | null
  >(null);
  const [debugPanel, setDebugPanel] = createSignal(
    initialSessionWorkspace?.panels.debugOpen ??
      debugPanelOpenFromHash(location.hash),
  );
  const [audioMuted, setAudioMuted] = createSignal(preferredAudioMuted());
  const [audioBitrate, setAudioBitrate] = createSignal(preferredAudioBitrate());
  const [videoBandwidth, setVideoBandwidth] = createSignal(
    preferredVideoBandwidth(),
  );
  const [videoSpeed, setVideoSpeed] = createSignal(preferredVideoSpeed());
  const [surfaceStreaming, setSurfaceStreaming] = createSignal(
    preferredSurfaceStreaming(),
  );
  const [surfaceSmoothing, setSurfaceSmoothing] = createSignal(
    preferredSurfaceSmoothing(),
  );
  const [surfaceMaxFps, setSurfaceMaxFps] = createSignal(
    preferredSurfaceMaxFps(),
  );
  const [surfaceZoom, setSurfaceZoom] = createSignal(preferredSurfaceZoom());
  const [surfaceZoomMode, setSurfaceZoomMode] = createSignal(
    preferredSurfaceZoomMode(),
  );
  const [surfaceTouchMode, setSurfaceTouchMode] = createSignal(
    preferredSurfaceTouchMode(),
  );
  const [waylandKeyboardRequests, setWaylandKeyboardRequests] = createSignal(
    preferredWaylandKeyboardRequests(),
  );
  // Applied to the cached probe result before the first native Surface view is
  // opened, avoiding a broad initial format offer.
  const [surfaceCodecs, setSurfaceCodecs] = createSignal(
    preferredSurfaceCodecs(),
  );
  setAllowedCodecSupport(surfaceCodecs());
  const [probedSurfaceCodecs, setProbedSurfaceCodecs] = createSignal(
    getProbedCodecSupport(),
  );
  // The media panel can only offer codecs the decode probe confirmed, and on
  // a terminal-only workspace nothing else ever runs it — a surface view does
  // it on mount. Kicked when the panel opens rather than at startup, since
  // the probe instantiates real decoders. The promise is cached, so a page
  // that already probed answers immediately.
  createEffect(() => {
    if (overlay() !== "media" || probedSurfaceCodecs()) return;
    void detectCodecSupport().then(() =>
      setProbedSurfaceCodecs(getProbedCodecSupport()),
    );
  });
  const [previewPanelOpen, setPreviewPanelOpen] = createSignal(
    preferredPreviewPanelOpen(
      initialSessionWorkspace?.panels.previewOpen ?? true,
    ),
  );
  const [musterPreviewExpanded, setMusterPreviewExpanded] = createSignal(
    initialSessionWorkspace?.panels.musterExpanded ?? false,
  );
  const [expandedMusterStacks, setExpandedMusterStacks] = createSignal<
    ReadonlySet<string>
  >(new Set());
  const [previewPanelWidth, setPreviewPanelWidth] = createSignal(
    preferredPreviewPanelWidth(),
  );
  // Left dock (docs/ide.md): one dock, opened/closed from the status bar,
  // stacking the IDE sections as a collapsible accordion.
  // Project search: a transient top pane, not persisted — it opens on
  // Ctrl+B f and closes on Escape or its own dismiss button.
  const [searchOpen, setSearchOpen] = createSignal(false);
  // Bumped on every invoke so the panel refocuses its input even when
  // the pane was already open — the shortcut should always land you in
  // the field, not just reveal it.
  const [searchFocus, setSearchFocus] = createSignal(0);
  // null = size to content (capped at half the column); a number pins an
  // explicit fraction after the user drags the handle.
  const [searchHeight, setSearchHeight] = createSignal<number | null>(null);
  let middleWorkspaceColumn: HTMLDivElement | null = null;
  /** Dismiss the search pane and hand focus back to whatever was using it.
   *  Closing chrome should return you to the thing underneath — otherwise
   *  focus is left on `document.body` and the next keystroke goes nowhere.
   *  A tile pane owns its own focus, so only a terminal needs the nudge. */
  function closeSearch() {
    setSearchOpen(false);
    queueMicrotask(() => focusedKeyboardInput()?.focus());
  }

  /** Where a drag starts from when the pane was still auto-sized: its
   *  measured share of the column, so the handle does not jump. */
  const autoSearchFraction = () => {
    const el = document.querySelector("[data-yas-search-pane]");
    const parent = middleWorkspaceColumn;
    return el && parent && parent.clientHeight > 0
      ? el.clientHeight / parent.clientHeight
      : 0.32;
  };
  const [leftDockOpen, setLeftDockOpen] = createSignal(
    initialSessionWorkspace?.panels.leftOpen ?? preferredLeftDockOpen(),
  );
  const [collapsedSections, setCollapsedSections] = createSignal<
    Set<LeftPanel>
  >(
    initialSessionWorkspace
      ? new Set(
          LEFT_PANELS.filter(
            (panel) =>
              !initialSessionWorkspace.panels.expandedSections.includes(panel),
          ),
        )
      : new Set(preferredCollapsedSections() as LeftPanel[]),
  );
  // Sections auto-folded because they don't apply here, which the user asked
  // to see anyway. Not persisted: it is an override of a fold this root
  // caused, not a preference about the dock.
  const [foldOverrides, setFoldOverrides] = createSignal<
    ReadonlySet<LeftPanel>
  >(new Set());
  const [sectionWeights, setSectionWeights] = createSignal<
    Record<LeftPanel, number>
  >({ explorer: 1, branches: 1, log: 1, problems: 1 });
  const [leftDockWidth, setLeftDockWidth] = createSignal(
    preferredLeftDockWidth(),
  );

  // Which root the IDE dock is showing: a server KV root, or the focused
  // terminal (follow-cd via fromSessionId).
  // Module caches outlive a component and several intentionally retain state
  // across tile remounts. Reconcile them against route removal explicitly;
  // connection close callbacks are not guaranteed to fire after an owner has
  // already detached its transport.
  let retainedConnections = new Map<
    string,
    { generation: number; identity: object | null }
  >();
  createEffect(() => {
    const next = new Map(
      wsState().connections.map((connection) => [
        connection.id,
        {
          generation: connection.generation,
          identity: workspace.getConnection(connection.id),
        },
      ]),
    );
    for (const [connectionId, retained] of retainedConnections) {
      const current = next.get(connectionId);
      if (
        current?.generation === retained.generation &&
        current.identity === retained.identity
      ) {
        continue;
      }
      dropServerRoots(connectionId as ConnectionId);
      dropSessionCatalog(connectionId as ConnectionId);
      dropFileIndexes(connectionId);
      dropCachedCommits(connectionId);
      dropConnectionTabState(connectionId);
      dropEditorPositions(connectionId);
    }
    retainedConnections = next;
  });
  onCleanup(() => {
    for (const connectionId of retainedConnections.keys()) {
      dropServerRoots(connectionId as ConnectionId);
      dropSessionCatalog(connectionId as ConnectionId);
      dropFileIndexes(connectionId);
      dropCachedCommits(connectionId);
      dropConnectionTabState(connectionId);
      dropEditorPositions(connectionId);
    }
    retainedConnections.clear();
  });
  // Each connected KV-capable server owns its `roots` document.
  createEffect(() => {
    for (const c of wsState().connections) {
      if (c.status !== "connected" || !c.supportsKv) continue;
      ensureServerRoots(workspace, c.id, c.generation);
      ensureStoredRemotes(workspace, c.id, c.generation);
    }
  });
  // Each connected server's application catalog, held open so the switcher can
  // filter it from the first keystroke instead of fetching one when it opens.
  // Armed like the roots watch above, and re-armed on the generation for the
  // same reason: a channel does not survive a re-establish.
  createEffect(() => {
    for (const c of wsState().connections) {
      if (c.status !== "connected") continue;
      ensureSessionCatalog(workspace, c.id, c.generation);
    }
  });
  const roots = createMemo<Root[]>(allServerRoots);
  // A worktree selection is deliberately not a declared root: it is a
  // navigation, not a configured place. It carries its own connection so it
  // survives the focus moving, and a label so the picker can name it without
  // re-deriving a basename.
  const [rootSel, setRootSel] = createSignal<WorkspaceSessionProjectSelection>(
    initialSessionWorkspace?.panels.project ?? { kind: "focused" },
  );
  // Live cwd of the focused terminal, fed by the cwd poll below: it labels
  // the root-picker's focused-terminal option and shows in the status bar.
  // `sessionId` is what the reading is *about* — a poll that comes back
  // empty leaves the last value in place, so consumers need it to tell a
  // live cwd from one belonging to the terminal they just left.
  const [focusedTerm, setFocusedTerm] = createSignal<{
    sessionId: SessionId;
    conn: string;
    ptyId: TerminalId;
    cwd: string;
  } | null>(null);
  // Unlike `focusedTerm`, this survives focusless reconnect windows. It is
  // only a fallback for a sticky terminal anchor after that PTY is confirmed
  // gone, and is bounded by the terminals seen during this workspace mount.
  const lastTerminalCwds = new Map<string, string>();
  const terminalCwdKey = (connectionId: string, ptyId: TerminalId): string =>
    `${connectionId}\u0000${ptyId}`;
  // A `cd` OUTSIDE the current session root re-roots the dock there (Files and
  // Log follow the terminal, not just the label). Inside the root, the poll
  // only expands the tree — re-rooting on every subdirectory cd would narrow
  // the view constantly. Set by the poll, consumed by ideDescriptor.
  const [termCwdOverride, setTermCwdOverride] = createSignal<{
    sessionId: SessionId;
    connectionId: ConnectionId;
    cwd: string;
  } | null>(null);

  // What the focused *pane* anchors the IDE root on. A terminal anchors on its
  // live cwd; an editor/diff tile on its file's directory; a commit tile on its
  // repo. So the dock follows whatever pane you focus — not just terminals.
  type FocusAnchor =
    | { kind: "terminal"; session: YasSession }
    | { kind: "path"; connectionId: ConnectionId; path: string; label: string };

  const dirOf = (abs: string): string => {
    const s = abs.replace(/\/+$/, "");
    const i = s.lastIndexOf("/");
    return i <= 0 ? "/" : s.slice(0, i);
  };

  // Resolve the currently-focused pane to an anchor, or null when the pane has
  // no root to show (a surface, an empty pane) — in which case the last anchor
  // sticks, so the dock never flickers to nothing.
  const focusedPaneAnchor = (): FocusAnchor | null => {
    const assign = inLayout()
      ? (layoutAssignments()?.assignments[layoutFocusedPaneId() ?? ""] ?? null)
      : activeTile();
    if (typeof assign === "string" && isTileAssignment(assign)) {
      const t = parseTileAssignment(assign);
      if (t) {
        // A manage tile is a server's panels, not a place in a filesystem: it
        // has no root to anchor on, so the last one sticks.
        if (t.kind === "manage") return null;
        if (t.kind === "commit") {
          const repoPath = t.arg.slice(t.arg.indexOf(":") + 1);
          return {
            kind: "path",
            connectionId: t.connectionId as ConnectionId,
            path: repoPath,
            label: repoPath,
          };
        }
        const file = t.kind === "diff" ? parseDiffArg(t.arg).path : t.arg;
        return {
          kind: "path",
          connectionId: t.connectionId as ConnectionId,
          path: dirOf(file),
          label: file,
        };
      }
    }
    const term = focusedSession();
    // An exited terminal has no live cwd to anchor on — a follow-terminal
    // open against its dead pty can only fail ("source terminal has no
    // working directory"). Treat it as rootless: the last live anchor
    // sticks, or the dock shows no root at all (first open).
    return term && term.state !== "exited"
      ? { kind: "terminal", session: term }
      : null;
  };

  // A stable identity for an anchor, so we only re-emit when the focused pane
  // meaningfully changes — not on every workspace snapshot (terminal frames
  // fire those constantly, and focusedPaneAnchor() allocates a fresh object
  // each call, which would otherwise churn ideDescriptor every frame).
  const anchorKey = (a: FocusAnchor | null): string =>
    !a
      ? ""
      : a.kind === "terminal"
        ? `t:${a.session.id}`
        : `p:${a.connectionId}:${a.path}`;

  // Sticky: keep the last derivable anchor when focus lands on a rootless pane.
  const [lastAnchor, setLastAnchor] = createSignal<FocusAnchor | null>(null);
  createEffect(() => {
    const a = focusedPaneAnchor();
    if (!a) return;
    const k = anchorKey(a);
    setLastAnchor((prev) => (anchorKey(prev) === k ? prev : a));
  });

  // Hoisted declaration: the server-roots memo above runs at component
  // setup, before this point in source order.
  function connectionForRemote(remote: string): ConnectionId {
    return (remote || defaultConnectionId()) as ConnectionId;
  }

  const ideDescriptor = createMemo<IdeSessionDescriptor | null>(() => {
    // No session while the dock is closed — the fs/git syncs would be pure
    // overhead. Editor/diff tiles own their own handles, so they are
    // unaffected. The 30s idle cache keeps a session warm across quick
    // close/reopen and pane switches.
    if (!leftDockOpen()) return null;
    // Nor while every section in it is collapsed. An open dock showing only
    // its three headers reads nothing from the tree, the log or the problem
    // list, so the fs sync and git repo behind them are as much dead weight
    // as when the dock is shut — and collapsing the sections is how a dock
    // gets emptied in practice, since it leaves the pane where it is.
    //
    // Deliberately the user's own collapse set, not the folded set the dock
    // renders: that one counts sections auto-folded for having nothing to
    // show, and those are derived from the session this decides whether to
    // open.
    if (LEFT_PANELS.every((panel) => collapsedSections().has(panel))) {
      return null;
    }
    const sel = rootSel();
    if (sel.kind === "declared") {
      const r = roots().find((x) => x.name === sel.name && !x.disabled);
      if (!r) return null;
      const connectionId = connectionForRemote(r.remote);
      return { key: `d ${connectionId} ${r.path}`, connectionId, path: r.path };
    }
    if (sel.kind === "worktree") {
      // No `preferRepoRoot`: a linked worktree IS the repo root the server
      // resolves for it, and asking to be re-rooted at "the enclosing repo"
      // is exactly how a click on a worktree would snap back to whichever
      // one we came from.
      return {
        key: `w ${sel.connectionId} ${sel.path}`,
        connectionId: sel.connectionId,
        path: sel.path,
      };
    }
    const a = lastAnchor();
    if (!a) return null;
    if (a.kind === "terminal") {
      // A cd outside the session's root re-keys the descriptor at the new
      // cwd (set by the cwd poll), so Files and Log follow the terminal
      // instead of staying on the root resolved at first open.
      const ov = termCwdOverride();
      if (ov && ov.sessionId === a.session.id) {
        return {
          key: `f ${ov.connectionId} ${ov.cwd}`,
          connectionId: ov.connectionId,
          path: ov.cwd,
        };
      }
      // The terminal may exit after becoming the sticky last anchor. Keep the
      // useful root, but stop issuing PTY-relative opens that can only return
      // the server's internal "source terminal has no working directory"
      // diagnostic. The cwd poll gives us the same root as an absolute path;
      // without even one successful poll there is no root to retain.
      const source = currentSourceSessionForPty(
        wsState().sessions,
        a.session.connectionId,
        a.session.ptyId,
      );
      const sourceConnectionReady =
        wsState().connections.find(
          (connection) => connection.id === a.session.connectionId,
        )?.ready ?? false;
      if (!sourceSessionCanResolveCwd(source, sourceConnectionReady)) {
        const last = focusedTerm();
        const lastCwd =
          last &&
          last.conn === a.session.connectionId &&
          last.ptyId === a.session.ptyId
            ? last.cwd
            : lastTerminalCwds.get(
                terminalCwdKey(a.session.connectionId, a.session.ptyId),
              );
        if (!lastCwd) return null;
        return {
          key: `f ${a.session.connectionId} ${lastCwd}`,
          connectionId: a.session.connectionId,
          path: lastCwd,
        };
      }
      return {
        key: `f ${a.session.connectionId} pty${a.session.ptyId}`,
        connectionId: a.session.connectionId,
        path: "",
        fromSessionId: a.session.id,
        // Keyed by pty, so the session survives reconnects that replace every
        // SessionId — the pty is what its opens keep following.
        fromPtyId: a.session.ptyId,
      };
    }
    // Tile-anchored: the fs sync starts at the file's directory (or the
    // commit's repo), but preferRepoRoot re-roots the tree at the enclosing
    // repo once git discovers it — so opening a file shows the whole project.
    return {
      key: `p ${a.connectionId} ${a.path}`,
      connectionId: a.connectionId,
      path: a.path,
      preferRepoRoot: true,
    };
  });
  const activeSession = useIdeSession(workspace, ideDescriptor);

  // Sections with nothing to show for this root: a commit log over a directory
  // that is not a repository (or a remote with no git at all), problems from a
  // remote that cannot run a language server. They fold away rather than
  // sitting open on a message — the space belongs to the panels that do apply —
  // and unfold by themselves once they have something to say.
  const inapplicableSections = createMemo<ReadonlySet<LeftPanel>>(() => {
    const set = new Set<LeftPanel>();
    const s = activeSession();
    // No session at all — nothing picked yet, or a share still connecting —
    // is as empty as a root without a repository, and folds the same way.
    // Now that the log's fold comes from here rather than from a seeded
    // preference, this case has to be named or the log sits open on nothing.
    if (!s || s.noRepo()) set.add("log");
    // Branches folds on exactly the same condition as the log: both are
    // views of a repository, and neither has anything to say without one.
    if (!s || s.noRepo()) set.add("branches");
    if (s?.noLsp()) set.add("problems");
    return set;
  });
  // An override lapses once its section applies again.
  createEffect(() => {
    const inapplicable = inapplicableSections();
    setFoldOverrides((cur) => {
      const next = liveOverrides(cur, inapplicable);
      return next.size === cur.size ? cur : next;
    });
  });
  const collapsedForDock = createMemo(() =>
    foldedSections(
      collapsedSections(),
      inapplicableSections(),
      foldOverrides(),
    ),
  );

  // --- Mobile touch detection & virtual keyboard tracking ---
  const [isMobileTouch, setIsMobileTouch] = createSignal(false);
  const [terminalSurface, setTerminalSurface] =
    createSignal<YasTerminalSurface | null>(null);

  // --- Terminal hyperlinks ---
  // `hoveredLink` drives the status-bar preview; `pendingLink` is the target
  // awaiting a decision in the confirmation overlay.
  const [hoveredLink, setHoveredLink] = createSignal<LinkHover | null>(null);
  const [pendingLink, setPendingLink] = createSignal<{
    assessment: UrlAssessment;
    text: string;
  } | null>(null);

  onMount(() => {
    const isTouch = () =>
      "ontouchstart" in window ||
      navigator.maxTouchPoints > 0 ||
      matchMedia("(pointer: coarse)").matches;
    const check = () => isTouch();
    setIsMobileTouch(check());
    // Recheck when the coarse pointer media query changes (e.g.
    // DevTools device-mode toggle).
    const mq = matchMedia("(pointer: coarse)");
    const handler = () => setIsMobileTouch(check());
    mq.addEventListener?.("change", handler);
    onCleanup(() => {
      mq.removeEventListener?.("change", handler);
    });
  });

  // Track visualViewport to detect keyboard open/close on mobile.
  const [vpHeight, setVpHeight] = createSignal<number | null>(null);
  const [vpOffset, setVpOffset] = createSignal(0);
  const [vpBaseHeight, setVpBaseHeight] = createSignal(0);
  onMount(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    let baseWidth = 0;
    const update = () => {
      const height = vv.height;
      const width = vv.width;
      const fullHeight = Math.max(height, window.innerHeight);
      setVpHeight(height);
      setVpOffset(vv.offsetTop);
      setVpBaseHeight((prev) => {
        // A large width change means rotation or device-mode resize; reset the
        // baseline instead of carrying a portrait height into landscape.
        if (baseWidth === 0 || Math.abs(width - baseWidth) > 48) {
          baseWidth = width;
          return fullHeight;
        }

        // Grow with browser chrome collapse.  Also allow small decreases so
        // address-bar changes do not look like a keyboard; never learn a
        // keyboard-shrunken viewport (>150px) as the new baseline.
        if (fullHeight > prev || prev - height <= 150) {
          baseWidth = width;
          return fullHeight;
        }
        return prev;
      });
    };
    update(); // initialise immediately
    vv.addEventListener("resize", update);
    vv.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    const onOrientationChange = () => setTimeout(update, 150);
    screen.orientation?.addEventListener("change", onOrientationChange);
    onCleanup(() => {
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      screen.orientation?.removeEventListener("change", onOrientationChange);
    });
  });

  // How much of the layout viewport something is parked over: a software
  // keyboard, but also iPadOS's ~55px shortcut bar when a hardware keyboard is
  // attached, and the floating keyboard.  Only a full keyboard clears 150px,
  // and gating the viewport pin on that number left <main> at its full 100dvh
  // for the smaller two — with the footer, and the keyboard toggle in it,
  // sitting underneath and untappable.  Anything beyond the deadband is also
  // keyboard-open state: the shortcut bar is still an input panel the toggle
  // must be able to dismiss.  The deadband keeps momentum-scroll jitter from
  // thrashing the layout.
  const occlusion = createMemo(() => {
    if (!isMobileTouch()) return 0;
    const h = vpHeight();
    const full = vpBaseHeight();
    if (h === null || full === 0) return 0;
    return Math.max(0, full - h);
  });
  const viewportOccluded = createMemo(() => occlusion() > 32);

  // Sticky virtual keyboard: track explicit user intent so the keyboard
  // isn't dismissed when tapping elsewhere on the page.
  const [keyboardWanted, setKeyboardWanted] = createSignal(false);
  // A remote Wayland enable may raise the keyboard automatically. An
  // explicit status-bar toggle outranks its later disable until the user
  // dismisses or toggles the keyboard again.
  let keyboardManualOverride = false;
  let automaticKeyboardInput: HTMLTextAreaElement | null = null;
  const terminalInputSelector =
    'textarea[aria-label="Terminal input"][tabindex]:not([readonly])';
  // A surface pane's IME textarea (YasSurfaceCanvas creates it next to the
  // canvas).  It routes keydown/keyup and composition into the surface, so
  // it is what the software keyboard has to rest on — the canvas itself is
  // not editable and an IME will not stay up for it.
  const surfaceInputSelector = 'textarea[aria-label="Surface input"]';
  // CodeMirror 6's focused contenteditable. Including it lets the mobile
  // toolbar's paste button reach the editor and lets hide-keyboard blur the
  // right element instead of falling back to a different pane's textarea.
  const editorInputSelector = '.cm-content[contenteditable="true"]';
  const keyboardInputSelector = `${terminalInputSelector}, ${surfaceInputSelector}, ${editorInputSelector}`;

  // The software keyboard rises only from the status-bar toggle, never from a
  // tap: while it isn't wanted, every terminal and surface textarea carries
  // inputmode="none", which keeps focus semantics (hardware keys, scrollback
  // navigation, paste) but tells the browser not to bring up an IME.  The
  // observer exists because the textareas are created whenever a pane
  // mounts, and the attribute has to be in place before the tap that focuses
  // them — stamping on focus is too late for the IME decision.
  const stampSelector =
    'textarea[aria-label="Terminal input"], textarea[aria-label="Surface input"]';
  createEffect(() => {
    // `suppress` is false when leaving touch mode too (a DevTools device-mode
    // flip), so that pass strips stale stamps before bailing.
    const suppress = isMobileTouch() && !keyboardWanted();
    const stampOne = (el: Element) => {
      if (suppress) el.setAttribute("inputmode", "none");
      else {
        const desired = (el as HTMLElement).dataset.yasInputmode;
        if (desired) el.setAttribute("inputmode", desired);
        else el.removeAttribute("inputmode");
      }
    };
    const stamp = (root: ParentNode) => {
      for (const el of root.querySelectorAll(stampSelector)) stampOne(el);
    };
    stamp(document);
    if (!isMobileTouch()) return;
    const mo = new MutationObserver((records) => {
      for (const r of records) {
        for (const n of r.addedNodes) {
          if (!(n instanceof HTMLElement)) continue;
          if (n.matches(stampSelector)) stampOne(n);
          else stamp(n);
        }
      }
    });
    mo.observe(document.body, { childList: true, subtree: true });
    onCleanup(() => mo.disconnect());
  });

  // The focused pane's terminal or surface input, else the first one on
  // screen that can take focus.  Every fallback matters: a pane holding an
  // editor or a web view has no keyboard input at all, and until something
  // is tapped no pane carries the focused marker.  Resolving to null there
  // left the keyboard toggle dead for good — it returns before flipping
  // `keyboardWanted`, so every later tap took the same branch and did
  // nothing.  Reaching into another pane is safe: that pane's own focusin
  // moves layout focus to match, so the caret never lands out of sight.
  function focusedKeyboardInput(): HTMLElement | null {
    // A soloed-away pane and a background tab are `display:none`, which
    // leaves the input with no client rects.  focus() there is a silent
    // no-op, so returning one lit the icon over a keyboard that never came
    // up.  offsetParent can't be the test: the IME textareas are
    // position:fixed (pinned to the screen top, always clear of the
    // keyboard), and offsetParent is null on fixed elements even when
    // rendered.  Parked thumbnails are `inert` — same silent no-op, but
    // with boxes still laid out, so they need their own check.
    const focusable = (el: HTMLElement | null | undefined) =>
      el && el.getClientRects().length > 0 && !el.closest("[inert]")
        ? el
        : null;
    const active = document.activeElement;
    if (
      active instanceof HTMLElement &&
      active.matches(keyboardInputSelector)
    ) {
      const focused = focusable(active);
      if (focused) return focused;
    }
    const focusedPane = document.querySelector<HTMLElement>(
      '[data-yas-pane-focused="true"]',
    );
    return (
      focusable(
        focusedPane?.querySelector<HTMLElement>(terminalInputSelector),
      ) ??
      focusable(
        focusedPane?.querySelector<HTMLElement>(surfaceInputSelector),
      ) ??
      focusable(focusedPane?.querySelector<HTMLElement>(editorInputSelector)) ??
      [
        ...document.querySelectorAll<HTMLElement>(
          `section ${terminalInputSelector}`,
        ),
        ...document.querySelectorAll<HTMLElement>(
          `section ${surfaceInputSelector}`,
        ),
        ...document.querySelectorAll<HTMLElement>(
          `section ${editorInputSelector}`,
        ),
      ].find((el) => focusable(el)) ??
      null
    );
  }

  function enableKeyboardInput(el: HTMLElement): void {
    const desired = el.dataset.yasInputmode;
    if (desired) el.setAttribute("inputmode", desired);
    else el.removeAttribute("inputmode");
  }

  // A committed Wayland text-input enable is the remote field asking for an
  // input panel. Only honor it for the surface this viewer already focused;
  // another viewer shares the same Wayland seat and must not pop keyboards on
  // every connected phone. Browser policy still makes showing best-effort.
  onMount(() => {
    const handler = (raw: Event) => {
      const event = raw as YasSurfaceTextInputEvent;
      const input = event.target;
      if (!(input instanceof HTMLTextAreaElement)) return;
      if (!input.matches(surfaceInputSelector) || !isMobileTouch()) return;
      if (!waylandKeyboardRequests()) return;
      // A modal overlay owns keyboard focus. The pane remains marked focused
      // behind it, so accepting a late Wayland text-input update here would
      // pull focus out of inputs such as the C-b k search box.
      if (overlay()) return;

      const state = event.detail;
      if (!state.enabled) {
        if (automaticKeyboardInput !== input || keyboardManualOverride) return;
        queueMicrotask(() => {
          // An old surface's disable can be immediately followed by the new
          // focused field's enable. Let that handoff replace the owner before
          // deciding whether the keyboard should go away.
          if (automaticKeyboardInput !== input || keyboardManualOverride)
            return;
          automaticKeyboardInput = null;
          batch(() => {
            setKeyboardWanted(false);
            if (document.activeElement === input) input.blur();
          });
        });
        return;
      }
      if (!state.requested) return;
      const locallyFocused =
        document.activeElement === input ||
        !!input.closest('[data-yas-pane-focused="true"]');
      if (!locallyFocused) return;

      if (!keyboardWanted()) {
        keyboardManualOverride = false;
        automaticKeyboardInput = input;
        setKeyboardWanted(true);
      } else if (!keyboardManualOverride) {
        automaticKeyboardInput = input;
      }
      enableKeyboardInput(input);
      input.focus({ preventScroll: true });
      try {
        (
          navigator as { virtualKeyboard?: { show?: () => void } }
        ).virtualKeyboard?.show?.();
      } catch {
        // Safari/Chromium may reject programmatic show without a sufficiently
        // recent user activation; focusing the editable target is the
        // portable best effort.
      }
    };
    document.addEventListener(YAS_SURFACE_TEXT_INPUT_EVENT, handler);
    onCleanup(() =>
      document.removeEventListener(YAS_SURFACE_TEXT_INPUT_EVENT, handler),
    );
  });

  function focusSettledElsewhere(): boolean {
    const active = document.activeElement;
    if (!(active instanceof HTMLElement)) return false;
    if (active.matches(terminalInputSelector)) return true;
    if (!active.closest("section")) return false;
    // CodeMirror focuses a contenteditable div, not a textarea.  Without it
    // here the sticky re-focus reads an editor as "nothing took focus" and
    // now that the terminal lookup falls back across panes, drags the caret
    // out of the editor the user just tapped into.
    return active.matches(
      'input, textarea, select, canvas[tabindex], [contenteditable="true"]',
    );
  }

  // The keyboard going away is the user putting it away — iPadOS has a
  // dedicated dismiss key, which produces a blur we cannot tell apart from
  // "tapped a button", so intent has to be read off the viewport instead.
  // Latching on the first occlusion keeps the gap between the tap and the
  // keyboard animating in from counting as a dismissal.  This is also what
  // stops the icon lying: it tracks intent, and intent now expires when the
  // keyboard does.
  let keyboardSeen = false;
  createEffect(() => {
    if (!keyboardWanted()) {
      keyboardSeen = false;
      // inputmode="none" means taps no longer raise the IME, but the OS still
      // can (a keyboard-show gesture, stylus handwriting input).  If an input
      // panel is genuinely up over a focused terminal, latch intent from
      // reality so the icon and toolbar match what's on screen.  This includes
      // iPadOS's shortcut bar: although it is not a full software keyboard, it
      // must take the same hide path.  Focus gating keeps the drain after an
      // explicit hide (the toggle blurred, occlusion not yet gone) from
      // re-latching.
      if (
        viewportOccluded() &&
        document.activeElement instanceof HTMLElement &&
        document.activeElement.matches(keyboardInputSelector)
      ) {
        keyboardManualOverride = true;
        automaticKeyboardInput = null;
        setKeyboardWanted(true);
      }
      return;
    }
    if (viewportOccluded()) keyboardSeen = true;
    else if (keyboardSeen) {
      keyboardManualOverride = false;
      automaticKeyboardInput = null;
      setKeyboardWanted(false);
    }
  });

  // While the keyboard is wanted, focus landing on a surface canvas would
  // dismiss the IME — a canvas is not editable.  YasSurfaceCanvas hands its
  // own canvas focus to the textarea beside it (an IME will not start a
  // composition otherwise, on any platform), so this capture-phase pass is
  // the net beneath it: it catches a canvas in a pane whatever put it there,
  // and runs first, which makes the two agree rather than compete.  Keys
  // still reach the surface because the textarea routes keydown/keyup and
  // composition through the same handlers as the canvas.
  createEffect(() => {
    if (!isMobileTouch() || !keyboardWanted()) return;
    const handler = (e: FocusEvent) => {
      const t = e.target;
      if (!(t instanceof HTMLCanvasElement) || !t.closest("section")) return;
      t.parentElement
        ?.querySelector<HTMLElement>(surfaceInputSelector)
        ?.focus();
    };
    document.addEventListener("focusin", handler, true);
    onCleanup(() => document.removeEventListener("focusin", handler, true));
  });

  // Re-focus the keyboard-holding textarea when it blurs while the user
  // wants the keyboard open, unless an overlay is active.
  createEffect(() => {
    if (!isMobileTouch() || !keyboardWanted()) return;
    const handler = (e: FocusEvent) => {
      if (!(e.target instanceof HTMLTextAreaElement)) return;
      if (!e.target.matches(keyboardInputSelector)) return;
      if (!(e.target as Element).closest?.("section")) return;
      if (overlay()) return;
      // Long enough to outlast the dismiss animation, so the effect above has
      // cleared `keyboardWanted` and this bails rather than shoving the
      // keyboard back up.  A tap that merely stole focus never lowers the
      // keyboard, so nothing is visibly slower for the case this exists for.
      setTimeout(() => {
        if (!keyboardWanted() || overlay()) return;
        if (focusSettledElsewhere()) return;
        focusedKeyboardInput()?.focus();
      }, 300);
    };
    document.addEventListener("focusout", handler, true);
    onCleanup(() => document.removeEventListener("focusout", handler, true));
  });

  /** Toggle the virtual keyboard on mobile. */
  // Completes the iPadOS focus hop (see toggleMobileKeyboard): the real
  // target gets focus back once something is genuinely parked over the
  // viewport — the only signal that WebKit accepted the host's assist.
  let pendingHopLand: (() => void) | null = null;
  createEffect(() => {
    // Read viewportOccluded() unconditionally: short-circuiting it behind
    // pendingHopLand would subscribe to nothing on the first run, and the
    // effect would never fire.
    const covered = viewportOccluded();
    const land = pendingHopLand;
    if (land && covered) {
      pendingHopLand = null;
      land();
    }
  });

  // The focus-hop host for iPadOS (see toggleMobileKeyboard): a plain 1px
  // textarea at the document level.  It must stay outside `section` (so the
  // sticky-refocus net reads it as "nothing took focus") and outside the
  // inputmode-stamping selectors (so it keeps a real inputmode and the IME
  // will assist it).
  let keyboardHost: HTMLTextAreaElement | null = null;
  function keyboardHostEl(): HTMLTextAreaElement {
    if (!keyboardHost || !keyboardHost.isConnected) {
      keyboardHost = document.createElement("textarea");
      keyboardHost.setAttribute("aria-label", t("keyboard.host"));
      Object.assign(keyboardHost.style, {
        position: "fixed",
        top: "0",
        left: "0",
        width: "1px",
        height: "1px",
        opacity: "0",
        padding: "0",
        border: "none",
        outline: "none",
        resize: "none",
        overflow: "hidden",
      });
      document.body.appendChild(keyboardHost);
    }
    return keyboardHost;
  }

  // What held focus just before the current tap.  Whether a tapped button
  // takes focus differs by engine (iPadOS: no; Chromium: yes, during the
  // tap's click), so the already-focused decision in the toggle reads this
  // snapshot — the state *before* the tap's own focus churn — instead of
  // the live activeElement at handler time.
  let preTapFocus: Element | null = null;
  const snapshotPreTapFocus = () => {
    preTapFocus = document.activeElement;
  };
  document.addEventListener("pointerdown", snapshotPreTapFocus, true);
  onCleanup(() =>
    document.removeEventListener("pointerdown", snapshotPreTapFocus, true),
  );

  function toggleMobileKeyboard() {
    // A tap means "put it away" when any keyboard input panel is genuinely
    // up, including iPadOS's shortcut bar.  While intent is lit but no panel
    // rose — the IME refused the focus transition, or the tap landed while the
    // last keyboard was still draining — the tap asks for the keyboard again,
    // and taking the hide branch is exactly backwards.
    if (keyboardWanted() && viewportOccluded()) {
      keyboardManualOverride = false;
      automaticKeyboardInput = null;
      // Clear intent and focus together. Otherwise the viewport effect can
      // observe the still-focused input and immediately latch intent again.
      batch(() => {
        setKeyboardWanted(false);
        // Blur whatever actually holds the keyboard, including an editor.
        const active = document.activeElement;
        if (active instanceof HTMLElement && active.closest("section")) {
          active.blur();
        } else {
          focusedKeyboardInput()?.blur();
        }
      });
    } else {
      const el = focusedKeyboardInput();
      if (!el) return;
      keyboardManualOverride = true;
      automaticKeyboardInput = null;
      setKeyboardWanted(true);
      // The stamping effect above has cleared inputmode="none" by now (Solid
      // runs it synchronously on the write), but the IME decision happens on
      // this very element in this very gesture — clear it directly rather
      // than trust effect ordering.
      enableKeyboardInput(el);
      if (el === preTapFocus) {
        // A keyboard already up for this very element was only missing the
        // intent — adopt it without any focus churn, which would just
        // flicker the keyboard.
        if (viewportOccluded()) return;
        if (isIOS()) {
          // iPadOS only answers a focus CHANGE: focus() on the element that
          // already holds focus is a no-op, and blur+focus within one tap
          // nets to zero — no keyboard.  (The tell: switching panes raised
          // the keyboard, because that lands focus on a *different*
          // element.)  Hop focus through a neutral host the IME freshly
          // assists, then hand it to the real target — editable→editable
          // moves keep the keyboard.  The host lives outside any pane, so
          // it never holds focus when a show tap happens and every hop is
          // a real change.
          const host = keyboardHostEl();
          el.blur();
          host.focus();
          // The handback runs when the keyboard is actually rising — the
          // occlusion reading is the only proof WebKit accepted the assist —
          // with a timeout as the fallback for a keyboard that never shows,
          // so focus isn't parked on the host forever.
          pendingHopLand = () => {
            if (keyboardWanted() && document.activeElement === host) el.focus();
          };
          setTimeout(() => {
            const land = pendingHopLand;
            pendingHopLand = null;
            land?.();
          }, 600);
          return;
        }
        // Android leaves the textarea focused with no keyboard up — the
        // pane-focus effect focuses it at load with no user gesture (Chrome
        // moves focus but raises no IME), and the Back gesture dismisses the
        // IME without a blur.  focus() on the already-focused element is a
        // spec'd no-op no keyboard answers, so force a real transition.  This
        // must not be gated on the occlusion reading: a keyboard still
        // draining after an OS dismiss sits over 150px for a moment, and
        // skipping the blur there made this focus() a no-op — the tap lit
        // the icon over a keyboard that never rose, and the keyboard then
        // took extra taps to appear.
        el.blur();
      }
      el.focus();
      // Chromium's IME can stay down for a programmatic focus() even inside
      // a tap; where this API exists (Chrome on Android) it raises the
      // keyboard directly, and it fails silently everywhere else.  Safari
      // has no virtualKeyboard object.
      (
        navigator as { virtualKeyboard?: { show?: () => void } }
      ).virtualKeyboard?.show?.();
    }
  }

  // Initial main-view focus uses only durable native resource handles from
  // the workspace-session record.
  const storedMain = initialSessionWorkspace?.main ?? null;
  const storedMainRef = storedMain ? parseWorkspaceRef(storedMain) : null;
  const pendingStoredSurface = (() => {
    return storedMainRef?.kind === "surface" ? storedMainRef : null;
  })();

  const [focusedSurfaceId, setFocusedSurfaceId] =
    createSignal<SurfaceId | null>(null);
  // Track the connectionId for the focused surface so we don't re-derive
  // it reactively (which causes thrashing when surface list changes).
  const [focusedSurfaceConnId, setFocusedSurfaceConnId] =
    createSignal<ConnectionId | null>(null);

  // Surfaces that asked to come forward (xdg_activation_v1) and were answered
  // with a mark rather than the view — see ./surfaceAttention.ts. One set for
  // every place a mark can appear (dock card, pane, surface count, switcher) and
  // no timers anywhere: a mark waits until the window is looked at.
  //
  // Only the signal lives here. What settles it needs `inLayout` and
  // `layoutFocusedSurface`, both declared far below, and Solid runs a memo body
  // eagerly at setup — so a memo reading them from here dies in their temporal
  // dead zone before the first render finishes. The reader is down beside them
  // instead; see `frontSurfaceAssignment`.
  const [pendingAttention, setPendingAttention] = createSignal<
    ReadonlySet<string>
  >(new Set());
  /** Is this window still asking? Read by every mark there is. */
  const hasAttention = (assignment: string) =>
    pendingAttention().has(assignment);

  /**
   * An application the user just started from the switcher. The surface it
   * opens is not known until the server creates it, so we watch the surface
   * list for the first new surface with a matching appId on this connection
   * and place it in the active panel.
   */
  type PendingAppStart = {
    connectionId: ConnectionId;
    appId: string;
    /** Surface identities present before launch. Metadata for a new surface
     * can arrive after CREATE; comparing against the previous render would
     * misclassify that second-stage update as an old window. */
    existingSurfaceKeys: ReadonlySet<string>;
  };
  const [pendingAppStart, setPendingAppStart] =
    createSignal<PendingAppStart | null>(null);
  let pendingAppStartTimer: ReturnType<typeof setTimeout> | undefined;
  function startAppFromSwitcher(connectionId: ConnectionId, appId: string) {
    if (pendingAppStartTimer) clearTimeout(pendingAppStartTimer);
    if (!startApplication(connectionId, appId)) return false;
    setPendingAppStart({
      connectionId,
      appId,
      existingSurfaceKeys: new Set(
        surfaces().map(
          (surface) => `${surface.connectionId}:${surface.surfaceId}`,
        ),
      ),
    });
    // If the surface never appears, don't leave this hanging forever.
    pendingAppStartTimer = setTimeout(() => {
      pendingAppStartTimer = undefined;
      setPendingAppStart((cur) =>
        cur?.connectionId === connectionId && cur?.appId === appId ? null : cur,
      );
    }, 30_000);
    return true;
  }

  /** Set or clear the focused surface, always keeping the connectionId
   *  in sync so the layout view uses the correct connection.
   *  When `connectionId` is provided it is used directly, avoiding a
   *  potentially ambiguous lookup by numeric surfaceId alone. */
  function focusSurfaceById(
    surfaceId: SurfaceId | null,
    connectionId?: ConnectionId | null,
  ) {
    setFocusedSurfaceId(surfaceId);
    if (surfaceId != null) {
      const connId =
        connectionId ??
        surfaces().find((x) => x.surfaceId === surfaceId)?.connectionId ??
        null;
      setFocusedSurfaceConnId(connId);
      setPendingMainRef(
        connId ? surfaceWorkspaceRefForId(connId, surfaceId) : null,
      );
    } else {
      setFocusedSurfaceConnId(null);
    }
  }

  // Restore surface focus once the native catalogue contains it (one-shot).
  // Only into the single main view: under a multi-pane layout the surface is
  // owned by its pane assignment instead, and filling the single-view slot with it
  // would leave a focused surface nothing renders — which every shortcut gated on
  // hasFocusedWaylandSurface would then obey for the rest of the session.
  if (pendingStoredSurface != null) {
    let surfaceRestoreCancelled = false;
    createEffect(() => {
      if (surfaceRestoreCancelled) return;
      if (storedMain && pendingMainRef() !== storedMain) {
        // Explicit navigation supersedes a late native-catalog resolution.
        surfaceRestoreCancelled = true;
        setMainRestoreResolved(true);
        return;
      }
      if (inLayout()) {
        surfaceRestoreCancelled = true;
        setMainRestoreResolved(true);
        return;
      }
      const ss = surfaces();
      const currentSurfaceId = surfaceIdForWorkspaceRef(pendingStoredSurface);
      if (
        currentSurfaceId != null &&
        ss.some(
          (s) =>
            s.surfaceId === currentSurfaceId &&
            s.connectionId === pendingStoredSurface.connectionId,
        )
      ) {
        if (
          focusedSurfaceId() !== currentSurfaceId ||
          focusedSurfaceConnId() !== pendingStoredSurface.connectionId
        ) {
          focusSurfaceById(
            currentSurfaceId,
            pendingStoredSurface.connectionId as ConnectionId,
          );
        }
        setPendingMainRef(
          surfaceWorkspaceRefForId(
            pendingStoredSurface.connectionId,
            currentSurfaceId,
          ),
        );
        if (!storedMain) surfaceRestoreCancelled = true;
        setMainRestoreResolved(true);
        return;
      }
      const owningConnection = wsState().connections.find(
        (candidate) => candidate.id === pendingStoredSurface.connectionId,
      );
      if (!connectionAwaitingWorkspaceRestore(owningConnection)) {
        // Missing/detached or authoritatively absent: retain the stable ref,
        // but let the rest of the session become writable.
        setMainRestoreResolved(true);
      }
    });
  }

  // Restore terminal focus once the native catalogue contains it (one-shot).
  // Only if no surface focus was requested.
  const pendingStoredTerminal =
    storedMainRef?.kind === "terminal" ? storedMainRef : null;
  if (pendingStoredTerminal && pendingStoredSurface == null) {
    let terminalRestoreCancelled = false;
    createEffect(() => {
      if (terminalRestoreCancelled) return;
      if (storedMain && pendingMainRef() !== storedMain) {
        terminalRestoreCancelled = true;
        setMainRestoreResolved(true);
        return;
      }
      const ss = sessions();
      const storedPtyId = ptyIdForWorkspaceRef(pendingStoredTerminal);
      const match = ss.find(
        (candidate) =>
          candidate.connectionId === pendingStoredTerminal.connectionId &&
          candidate.ptyId === storedPtyId,
      );
      if (match) {
        if (wsState().focusedSessionId !== match.id) {
          workspace.focusSession(match.id);
        }
        retainMainTerminalRef(match.id);
        if (!storedMain) terminalRestoreCancelled = true;
        setMainRestoreResolved(true);
        return;
      }
      if (pendingStoredTerminal) {
        const owningConnection = wsState().connections.find(
          (candidate) => candidate.id === pendingStoredTerminal.connectionId,
        );
        if (!connectionAwaitingWorkspaceRestore(owningConnection)) {
          setMainRestoreResolved(true);
        }
      }
    });
  }
  const activeFontSource = createMemo<FontProtocolSource | null>(
    () => {
      const snapshot = connection();
      if (!snapshot) return null;
      const active = workspace.getConnection(snapshot.id);
      if (!active) return null;
      return {
        key: fontProtocolSourceKey(snapshot.id, snapshot.generation, active),
        connected: snapshot.status === "connected",
        connection: active.fontProtocol,
        hashFont: (data) => props.wasm.blake3_hash(data),
      };
    },
    null,
    {
      equals: (left, right) =>
        left?.key === right?.key &&
        left?.connected === right?.connected &&
        left?.connection === right?.connection,
    },
  );

  const [serverFonts, setServerFonts] = createSignal<string[]>([]);
  let serverFontsSourceKey: string | null = null;
  let serverFontsLoaded = false;
  let serverFontsRequest: { key: string; promise: Promise<void> } | null = null;

  function loadServerFonts(): void {
    const source = activeFontSource();
    const sourceKey = source
      ? `${source.connected ? (source.connection ? "font" : "no-font") : "waiting"}:${source.key}`
      : "waiting:none";
    if (sourceKey !== serverFontsSourceKey) {
      serverFontsSourceKey = sourceKey;
      serverFontsLoaded = false;
      setServerFonts([]);
    }
    if (!source?.connected) return;
    if (serverFontsLoaded || serverFontsRequest?.key === sourceKey) return;
    if (!source.connection) {
      serverFontsLoaded = true;
      return;
    }

    const promise = source.connection
      .listFonts()
      .then(protocolFontFamilies)
      .then((fonts) => {
        if (serverFontsSourceKey !== sourceKey) return;
        setServerFonts(fonts);
        serverFontsLoaded = true;
      })
      .catch(() => {
        // Retry while this server remains active when the picker next opens.
      })
      .finally(() => {
        if (serverFontsRequest?.key === sourceKey) serverFontsRequest = null;
      });
    serverFontsRequest = { key: sourceKey, promise };
  }

  const { resolvedFont, fontLoading, advanceRatio } = createFontLoader(
    font,
    defaultFont(),
    activeFontSource,
  );

  // Switching the focused session changes the active server. If the font
  // picker is open, replace its catalogue immediately rather than leaving the
  // previously focused server's families visible until it is reopened.
  createEffect(() => {
    activeFontSource();
    if (overlay() === "font") loadServerFonts();
  });
  const localLayoutState = !initialSessionWorkspace
    ? loadActiveLayoutState()
    : null;
  const initialLayout = (() => {
    const stored = initialSessionWorkspace?.layout;
    if (!stored) {
      return (
        localLayoutState?.layout ?? {
          ...layoutFromDSL("_"),
          name: t("windowManager.tiling"),
        }
      );
    }
    try {
      return { ...layoutFromDSL(stored.dsl), name: stored.name };
    } catch {
      return { ...layoutFromDSL("_"), name: t("windowManager.tiling") };
    }
  })();
  // Import the former standalone main ref into pane 0 once. Normal state is
  // now always a managed layout, including one-pane tiling.
  const initialPaneAssignments: Readonly<Record<string, string>> =
    Object.keys(initialSessionWorkspace?.assignments ?? {}).length > 0
      ? initialSessionWorkspace!.assignments
      : initialSessionWorkspace?.main
        ? { "0": initialSessionWorkspace.main }
        : (localLayoutState?.assignments ?? {});
  const [activeLayout, setActiveLayoutSignal] =
    createSignal<WorkspaceLayout | null>(initialLayout);
  const freshTilingLayout = (): WorkspaceLayout => ({
    ...layoutFromDSL("_"),
    name: t("windowManager.tiling"),
  });
  function setActiveLayout(layout: WorkspaceLayout | null) {
    setActiveLayoutSignal(layout ?? freshTilingLayout());
  }
  function setWorkspaceLayout(
    layout: WorkspaceLayout | null,
    _options?: { debounceHistory?: boolean },
  ) {
    setActiveLayoutSignal(layout ?? freshTilingLayout());
  }
  // Hot updates and an already-open pre-unification client can retain the old
  // null single-view state. Enforce the managed-one-pane invariant in place;
  // requiring a page reload here would leave exactly the newly launched
  // surface that motivated the unification without an owner.
  createEffect(() => {
    if (activeLayout() !== null) return;
    const layout = freshTilingLayout();
    setActiveLayoutSignal(layout);
    saveActiveLayout(layout);
  });
  const [recentLayouts, setRecentLayouts] = createSignal(loadRecentLayouts());
  const [layoutAssignments, setLayoutAssignments] =
    createSignal<LayoutAssignments | null>(null);
  const [unresolvedLayoutAssignments, setUnresolvedLayoutAssignments] =
    createSignal<Readonly<Record<string, string>>>(initialPaneAssignments);
  /** True once LayoutContainer's initial stable-reference pass has settled. */
  const [assignmentsResolved, setAssignmentsResolved] = createSignal(
    Object.keys(initialPaneAssignments).length === 0,
  );

  // Single-view "focused tile": an IDE tile (editor/diff/commit) shown in place of
  // the terminal when the user isn't in a multi-pane layout. Opening a tile
  // must NOT swap the user into a layout — it just replaces the main view, and the
  // terminal returns when the tile is dismissed.
  // A workspace's main tab reference resolves asynchronously against
  // the server's tabs registry once the connection reports KV capability.
  const [activeTile, setActiveTileSignal] = createSignal<string | null>(null);
  const [pendingMainRef, setPendingMainRef] = createSignal<string | null>(
    initialSessionWorkspace?.main ?? null,
  );
  const [mainRestoreResolved, setMainRestoreResolved] = createSignal(
    initialSessionWorkspace?.main == null ||
      parseWorkspaceRef(initialSessionWorkspace.main) == null,
  );
  function setActiveTile(assignment: string | null): void {
    if (assignment) setPendingMainRef(null);
    setActiveTileSignal(assignment);
  }

  function retainMainTerminalRef(sessionId: SessionId): void {
    const session = sessions().find((candidate) => candidate.id === sessionId);
    setPendingMainRef(
      session?.ptyId != null
        ? terminalWorkspaceRefForPtyId(session.connectionId, session.ptyId)
        : null,
    );
  }

  // Keep a resolved main ref live across remote transport generations. The
  // native handle is authoritative; the focused presentation state is replaceable.
  createEffect(() => {
    const value = pendingMainRef();
    if (!value || inLayout()) return;
    const parsed = parseWorkspaceRef(value);
    if (parsed?.kind === "surface") {
      const surfaceId = surfaceIdForWorkspaceRef(parsed);
      if (
        surfaces().some(
          (surface) =>
            surface.connectionId === parsed.connectionId &&
            surface.surfaceId === surfaceId,
        ) &&
        (focusedSurfaceId() !== surfaceId ||
          focusedSurfaceConnId() !== parsed.connectionId)
      ) {
        setFocusedSurfaceId(surfaceId);
        setFocusedSurfaceConnId(parsed.connectionId as ConnectionId);
      }
      return;
    }
    if (parsed?.kind !== "terminal") return;
    const ptyId = ptyIdForWorkspaceRef(parsed);
    const session = sessions().find(
      (candidate) =>
        candidate.connectionId === parsed.connectionId &&
        candidate.ptyId === ptyId,
    );
    if (session && wsState().focusedSessionId !== session.id) {
      workspace.focusSession(session.id);
    }
  });

  const [pendingActiveTileRef, setPendingActiveTileRef] = createSignal<{
    connectionId: ConnectionId;
    id: string;
  } | null>(
    (() => {
      const stored = initialSessionWorkspace?.main;
      if (!stored) return null;
      const parsed = parseWorkspaceRef(stored);
      return parsed?.kind === "tab"
        ? {
            connectionId: parsed.connectionId as ConnectionId,
            id: parsed.tabId,
          }
        : null;
    })(),
  );
  let activeTileFetchInFlight = false;
  let activeTileFetchRetries = 0;
  // Retry rides a signal: the in-flight early-return narrows this effect's
  // dependencies to the ref alone, so a plain-variable reset in the catch
  // would never re-trigger it (Solid re-tracks per run).
  const [activeTileRetry, setActiveTileRetry] = createSignal(0);
  createEffect(() => {
    activeTileRetry();
    const ref = pendingActiveTileRef();
    if (!ref || activeTileFetchInFlight) return;
    const conn = wsState().connections.find((c) => c.id === ref.connectionId);
    if (!conn) {
      // A detached remote is a settled unresolved ref, not a reason to block
      // unrelated workspace changes or erase the stored main value.
      setMainRestoreResolved(true);
      return;
    }
    if (
      conn.status === "disconnected" ||
      conn.status === "closed" ||
      conn.status === "error"
    ) {
      setMainRestoreResolved(true);
      return;
    }
    if (!conn.supportsKv) {
      if (conn.ready) {
        setPendingActiveTileRef(null); // ready and no kv: leave ref unresolved
        setMainRestoreResolved(true);
      }
      return;
    }
    // The ref stays set until the fetch settles definitively, so session
    // persistence keeps the main reference alive for the whole flight. Transient
    // failures (a boot-time re-establish rejects in-flight requests) re-arm
    // and retry on the next snapshot change, bounded like every other
    // re-establish retry in the tree.
    activeTileFetchInFlight = true;
    resolveTab(workspace, ref.connectionId, ref.id)
      .then((assignment) => {
        // Apply only if the user hasn't opened anything meanwhile.
        if (assignment && !activeTile()) {
          setActiveTile(assignment);
          setPendingMainRef(null);
        }
        setPendingActiveTileRef(null);
        setMainRestoreResolved(true);
      })
      .catch(() => {
        activeTileFetchInFlight = false;
        if (++activeTileFetchRetries > 20) {
          setPendingActiveTileRef(null);
          setMainRestoreResolved(true);
        } else setActiveTileRetry((n) => n + 1);
      });
  });
  // Every tile this client has displayed, most-recent first. This is the
  // FALLBACK ordering/source for the dock: the server registry below is the
  // real one, but a host without the native KV family contributes nothing, and
  // this list keeps the dock usable there.
  // Session-only; explicit closes prune it.
  const [localTabs, setLocalTabs] = createSignal<string[]>([]);
  const closingTabs = createTabCloseTracker();
  // A pane disappears synchronously, but xdg_toplevel.close is only a request:
  // the Wayland client destroys its surface later. Tombstone it during that
  // interval so closing a floating window can never look like parking it.
  const closingSurfaces = createTabCloseTracker();
  // Recording pushes one entry per file navigated past, so the list is
  // LRU-capped — an unbounded dock also meant unbounded live fs syncs,
  // which is how YAS_FS_MAX_SYNCS got exhausted in normal browsing.
  const BACKGROUND_TILES_MAX = 50;
  // Only the most recent cards render as live tiles (each live editor holds
  // a content sync of its parent dir); the rest are title-only.
  const LIVE_DOCK_PREVIEWS = 6;
  function recordLocalTab(assignment: string) {
    closingTabs.reopen(assignment);
    setLocalTabs((prev) =>
      [assignment, ...prev.filter((a) => a !== assignment)].slice(
        0,
        BACKGROUND_TILES_MAX,
      ),
    );
  }
  /** Close a tab everywhere: drop the server registry record and the local
   *  fallback entry. The counterpart to `registerTab`, and now the ONLY thing
   *  that unregisters — see the effect below. */
  function closeTab(assignment: string) {
    const operation = closingTabs.begin(assignment);
    setLocalTabs((prev) => prev.filter((a) => a !== assignment));
    void unregisterTab(workspace, assignment).then(
      () =>
        closingTabs.settle(
          assignment,
          operation,
          true,
          openTabs().some((tab) => tab.assignment === assignment),
        ),
      () => closingTabs.settle(assignment, operation, false, false),
    );
  }
  // The host-wide open-tab list, mirrored from every connected server's `tabs/`
  // prefix (docs/design/kv.md, ./ide/openTabs.ts).
  const openTabs = createOpenTabs(workspace, () => wsState().connections);
  /**
   * The dock: every open tab, on every connected host, that this client is not
   * currently displaying. DERIVED, not stored — which is the whole point:
   * defocusing a tile can no longer lose it (it merely stops being displayed,
   * and reappears here), and a tab opened in another frontend shows up here
   * without this one having done anything.
   */
  const backgroundTiles = createMemo<string[]>(() => {
    const displayed = new Set<string>();
    for (const v of Object.values(layoutAssignments()?.assignments ?? {})) {
      if (typeof v === "string") displayed.add(v);
    }
    const at = activeTile();
    if (at) displayed.add(at);
    const out: string[] = [];
    const seen = new Set<string>();
    const take = (a: string) => {
      if (displayed.has(a) || seen.has(a) || closingTabs.isClosing(a)) return;
      if (!isTileAssignment(a) && !isWebAssignment(a)) return;
      seen.add(a);
      out.push(a);
    };
    // Registry first (mtime order — registration is a put on every open, so
    // newest-touched sorts first); the local list then appends anything the
    // registry doesn't know about, which on a kv-less host is all of it.
    for (const tab of openTabs()) take(tab.assignment);
    for (const a of localTabs()) take(a);
    return out.slice(0, BACKGROUND_TILES_MAX);
  });
  createEffect(() => {
    closingTabs.reconcile(new Set(openTabs().map((tab) => tab.assignment)));
  });
  /**
   * Everything open, in the order Ctrl+B [ / ] walks it: terminals, then
   * surfaces, then tabs — the dock's own top-to-bottom order, so the chord
   * agrees with what the eye already scanned. Terminals and surfaces are
   * listed in their arrival order, which is what those two signals already
   * hold.
   *
   * The tab block cannot simply follow `openTabs`, which is ordered by
   * recency: displaying a tab re-registers it (the effect below), so walking
   * the ring would float each tab to the front as it was reached and leave the
   * chord ping-ponging between the last two it touched. So a tab keeps the
   * slot it had on the previous pass and only newcomers append — the sequence
   * they were opened in, which holds still because opening is the only thing
   * that changes it. Solid hands the previous value to the memo, so the order
   * is carried without a signal of its own.
   */
  const cycleRing = createMemo<string[]>((prev) => {
    const out: string[] = [];
    for (const s of sessions()) if (s.state !== "closed") out.push(s.id);
    // Subsurfaces are composited into their parent — only a top-level window
    // is somewhere focus can land.
    for (const s of surfaces()) {
      if (s.parentId === 0n) {
        out.push(surfaceAssignment(s.connectionId, s.surfaceId));
      }
    }
    const tabs = new Set<string>();
    for (const tab of openTabs()) tabs.add(tab.assignment);
    for (const a of localTabs()) tabs.add(a);
    // `delete` returns whether it was there, so this both keeps the old order
    // and leaves only the newcomers behind — and a tab that has closed drops
    // out, rather than holding its slot forever.
    for (const a of prev) if (tabs.delete(a)) out.push(a);
    out.push(...tabs);
    return out;
  }, []);
  // One prev/next pass over the displayed set (pane assignments plus the
  // single-view active tile) serves two jobs:
  //
  //  - registration: a tile ENTERING the set is written to the server's tabs/
  //    registry (docs/design/kv.md) so workspace-session refs resolve anywhere,
  //    and
  //    recorded in the local fallback list;
  //  - in-place replacement: the Edit⇄Staged⇄Unstaged switcher REPLACES a tab
  //    rather than opening a second one beside it, so the outgoing view is
  //    closed — otherwise it would linger in the dock as a stale card.
  //
  // Departures are otherwise NOT unregistered. Deletion is an explicit close
  // now, because the registry is shared: driving it from one client's
  // displayed set let that client delete a record another client's workspace
  // session points at, making its tile vanish on reload.
  //
  // Gated on workspace-session reference resolution so boot churn never writes.
  // Two tiles view "the same file" when their connection + path match —
  // the in-pane Edit⇄Staged⇄Unstaged switcher. Commits never match (their
  // identity is an oid, not a file).
  const tileFileKey = (a: string): string | null => {
    const t = parseTileAssignment(a);
    if (!t) return null;
    // A preview keys the same as its editor: they are one file in two
    // views, which is what makes the switcher replace the tile in place
    // instead of opening a second one beside it.
    if (t.kind === "editor" || t.kind === "preview")
      return `${t.connectionId}:${t.arg}`;
    if (t.kind === "diff")
      return `${t.connectionId}:${parseDiffArg(t.arg).path}`;
    return null;
  };
  const sameTileFile = (a: string, b: string): boolean => {
    const ka = tileFileKey(a);
    return ka !== null && ka === tileFileKey(b);
  };
  let prevPaneAssignments: Record<string, string | null | undefined> = {};
  let prevActiveTile: string | null = null;
  let prevOpenTiles = new Set<string>();
  createEffect(() => {
    const la = layoutAssignments();
    const resolved = assignmentsResolved() && !pendingActiveTileRef();
    const next: Record<string, string | null | undefined> =
      la?.assignments ?? {};
    if (!resolved) return;
    const shown = new Set<string>();
    for (const v of Object.values(next)) {
      if (
        typeof v === "string" &&
        (isTileAssignment(v) || isWebAssignment(v))
      ) {
        shown.add(v);
      }
    }
    const at = activeTile();
    if (at && (isTileAssignment(at) || isWebAssignment(at))) shown.add(at);
    // In-place view switches are the one departure that closes a tab: the
    // switcher swapped which view of ONE file the pane holds, so the outgoing
    // view is not a second open tab, it is the same tab in a different shape.
    // Every other departure — displaced by a terminal, pane cleared, layout
    // torn down, the fullscreen slot dismissed — leaves the tab registered and
    // the dock picks it up.
    if (la) {
      for (const [paneId, prev] of Object.entries(prevPaneAssignments)) {
        if (typeof prev !== "string" || !isTileAssignment(prev)) continue;
        const now = next[paneId];
        if (
          typeof now === "string" &&
          now !== prev &&
          isTileAssignment(now) &&
          !shown.has(prev) &&
          sameTileFile(prev, now)
        ) {
          closeTab(prev);
        }
      }
    }
    // The single-view flavor of the same rule. Web panes have no file identity,
    // so they never match and are never closed implicitly.
    if (
      prevActiveTile &&
      at &&
      at !== prevActiveTile &&
      isTileAssignment(at) &&
      !shown.has(prevActiveTile) &&
      sameTileFile(prevActiveTile, at)
    ) {
      closeTab(prevActiveTile);
    }
    // Web panes are registered like every other tab: a workspace keeps
    // a reference to the KV record, not the URL itself.
    for (const a of shown) {
      if (prevOpenTiles.has(a)) continue;
      recordLocalTab(a);
      registerTab(workspace, a);
    }
    prevPaneAssignments = { ...next };
    // Remember a web pane here too, or the rules above can never see one
    // leave the fullscreen slot.
    prevActiveTile =
      at && (isTileAssignment(at) || isWebAssignment(at)) ? at : null;
    prevOpenTiles = shown;
  });
  // A manager owns its sole pane too. There is no alternate "single view"
  // window model: one-pane tiling uses the same assignment and lifecycle path
  // as every larger tree.
  const inLayout = createMemo(() => activeLayout() != null);

  // Clear focused surface if it was destroyed.  A grace period avoids
  // flickering during reconnect cycles where the surface list is temporarily
  // empty before being re-populated — but it only applies while the owning
  // connection is absent or mid-handshake.  Once the connection is ready its
  // surface list is authoritative, so a missing surface means it really is
  // gone and we clear immediately.  Mirrors reconcileAssignments'
  // `readyConnectionIds` gate, which is why panes empty on the ack while
  // the main view used to sit on a dead surface for the full grace period.
  let clearFocusedTimer: ReturnType<typeof setTimeout> | null = null;
  createEffect(() => {
    const fid = focusedSurfaceId();
    const fConnId = focusedSurfaceConnId();
    if (fid == null) {
      if (clearFocusedTimer) {
        clearTimeout(clearFocusedTimer);
        clearFocusedTimer = null;
      }
      return;
    }
    const exists = surfaces().some(
      (s) =>
        s.surfaceId === fid && (fConnId == null || s.connectionId === fConnId),
    );
    // Unknown connection id: we can't tell a destroy from a reconnect blip,
    // so keep the grace period.
    const connReady = fConnId != null && readyConnIds().has(fConnId);
    if (!exists && connReady) {
      if (clearFocusedTimer) {
        clearTimeout(clearFocusedTimer);
        clearFocusedTimer = null;
      }
      focusSurfaceById(null);
    } else if (!exists) {
      if (!clearFocusedTimer) {
        clearFocusedTimer = setTimeout(() => {
          clearFocusedTimer = null;
          // Re-check after the grace period.
          const stillGone = !surfaces().some(
            (s) =>
              s.surfaceId === fid &&
              (fConnId == null || s.connectionId === fConnId),
          );
          if (stillGone) focusSurfaceById(null);
        }, 2000);
      }
    } else if (clearFocusedTimer) {
      clearTimeout(clearFocusedTimer);
      clearFocusedTimer = null;
    }
  });

  const offScreenSurfaces = createMemo<YasSurface[]>(() => {
    // A tile covers the main view (it is drawn ahead of the focused surface),
    // so the surface underneath is off-screen and belongs in the panel — the
    // same rule the sessions memo below applies to a displaced terminal.
    // Without this, tapping a tile's dock card hid the surface it covered from
    // everywhere at once: the tile is on top of it, and this filter dropped it
    // from the panel because focusedSurfaceId still named it. It came back
    // only by closing the tile. The slot is deliberately still *set* — that is
    // what brings the surface back when the tile closes — so what changes here
    // is only whether it is also offered as a card.
    const covered = activeTile() != null;
    const fid = covered ? null : focusedSurfaceId();
    const fConnId = covered ? null : focusedSurfaceConnId();
    // Collect surface keys assigned to panes.
    const al = activeLayout();
    const la = layoutAssignments();
    if (al) {
      // An initial empty snapshot precedes stable-reference resolution. Do
      // not open thumbnail streams for windows about to fill the main panes.
      // A missing assignment snapshot cannot prove that anything is parked.
      // Returning a stale previous list kept the empty shelf mounted forever
      // when a layout remount never published another value.
      if (!la || !assignmentsResolved()) return [];
    }
    const inPane = new Set<string>();
    if (la) {
      for (const v of Object.values(la.assignments)) {
        if (v && isSurfaceAssignment(v)) {
          const parsed = parseSurfaceAssignment(v);
          if (parsed) inPane.add(`${parsed.connectionId}:${parsed.surfaceId}`);
        }
      }
    }
    return surfaces().filter(
      (s) =>
        !closingSurfaces.isClosing(
          surfaceAssignment(s.connectionId, s.surfaceId),
        ) &&
        !(
          s.surfaceId === fid &&
          (fConnId == null || s.connectionId === fConnId)
        ) &&
        !inPane.has(`${s.connectionId}:${s.surfaceId}`),
    );
  });

  /**
   * The session the user parked out of the main view, which then shows
   * nothing.
   *
   * UI-level state, because the core cannot express it: `focusSession(null)`
   * does not stick — `resolveFocusedSessionId` falls back to the connection's
   * focus and finally to the first live session, so *some* session is always
   * focused (which is what keeps focus alive across reconnects). Parking is a
   * statement about this view, not about which session holds focus.
   *
   * Holding the id rather than a flag is what keeps it honest: parking only
   * applies while that exact session is still the focused one, so anything
   * that moves focus — a new terminal, a dock card, the session closing —
   * un-parks by construction, with no clear-it-here call to forget.
   *
   * Declared here, above `offScreenSessions`: that memo reads it and Solid
   * runs a memo body eagerly at setup, so a later `const` is still in its
   * temporal dead zone when the first render reaches it.
   */
  const [parkedSessionId, setParkedSessionId] = createSignal<SessionId | null>(
    null,
  );
  const mainTerminalParked = () => {
    const fid = wsState().focusedSessionId;
    return fid != null && fid === parkedSessionId();
  };
  // Focus moving elsewhere ends the park outright, rather than leaving the id
  // set and merely inactive. Holding it would let the park resurrect: the core
  // always resolves *some* focus, so closing the session that displaced a
  // parked one hands focus back to it — and it would silently re-park, with
  // its dock card the only way out. A null focus is not "elsewhere": catalogue
  // reconciliation can report null briefly before restoring the same session,
  // and clearing then would make a successful park immediately resurrect.
  createEffect(() => {
    const fid = wsState().focusedSessionId;
    const parked = untrack(parkedSessionId);
    if (parked != null && fid != null && fid !== parked) {
      setParkedSessionId(null);
    }
  });
  /** The session the single main view displays: none while parked. */
  const mainViewSessionId = () =>
    mainTerminalParked() ? null : wsState().focusedSessionId;

  const offScreenSessions = createMemo<YasSession[]>(() => {
    const al = activeLayout();
    const la = layoutAssignments();
    const sess = sessions();
    if (al) {
      // Do not let a previous layout's parked list keep an empty shelf alive.
      // The mounted LayoutContainer must resolve its restored assignments
      // before a missing session assignment means the terminal is parked.
      if (!la || !assignmentsResolved()) return [];
      const assigned = new Set<SessionId>(
        Object.values(la.assignments).filter(
          (id): id is SessionId => id != null && !isSurfaceAssignment(id),
        ),
      );
      return sess.filter((s) => s.state !== "closed" && !assigned.has(s.id));
    }
    // When a surface or a tile is focused the terminal it displaced is
    // off-screen — as is the parked one, which is the whole point of
    // parking it.  focusedSessionId still points at that terminal, so
    // without this branch it would be filtered out below while nothing
    // renders it.
    if (
      focusedSurfaceId() != null ||
      activeTile() != null ||
      mainTerminalParked()
    ) {
      return sess.filter((s) => s.state !== "closed");
    }
    return sess.filter(
      (s) => s.state !== "closed" && s.id !== wsState().focusedSessionId,
    );
  });
  const watchedPreviewSessions = createMemo(() =>
    previewSessionsToWatch(
      offScreenSessions(),
      musterPreviewExpanded(),
      expandedMusterStacks(),
    ),
  );

  function toggleMusterStack(key: string) {
    setExpandedMusterStacks((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function toggleDebug() {
    setDebugPanel((v) => !v);
  }
  function togglePreviewPanel() {
    const next = !previewPanelOpen();
    setPreviewPanelOpen(next);
    writeStorage(PREVIEW_PANEL_OPEN_KEY, next ? "1" : "0");
  }
  function persistCollapsed(next: Set<LeftPanel>) {
    writeStorage(LEFT_COLLAPSED_KEY, [...next].join(","));
  }
  function toggleLeftDock() {
    const next = !leftDockOpen();
    setLeftDockOpen(next);
    writeStorage(LEFT_DOCK_OPEN_KEY, next ? "1" : "0");
  }
  function toggleSectionCollapse(panel: LeftPanel) {
    const cur = collapsedSections();
    const next = toggleSection(
      panel,
      cur,
      inapplicableSections(),
      foldOverrides(),
    );
    setFoldOverrides(next.overridden);
    // A toggle moves exactly one panel, so an unchanged size means this click
    // went to the override instead of the preference.
    if (next.userCollapsed.size !== cur.size) {
      setCollapsedSections(next.userCollapsed);
      persistCollapsed(next.userCollapsed);
    }
  }
  // Keyboard entry point: open the dock and reveal a section.
  function focusSection(panel: LeftPanel) {
    if (!leftDockOpen()) toggleLeftDock();
    setCollapsedSections((cur) => {
      if (!cur.has(panel)) return cur;
      const next = new Set(cur);
      next.delete(panel);
      persistCollapsed(next);
      return next;
    });
  }
  function resizeSectionWeight(
    a: LeftPanel,
    b: LeftPanel,
    deltaWeight: number,
  ) {
    setSectionWeights((w) => ({
      ...w,
      [a]: Math.max(0.1, w[a] + deltaWeight),
      [b]: Math.max(0.1, w[b] - deltaWeight),
    }));
  }

  // Turn an absolute path into one relative to the active session's root, or
  // null when it isn't under that root.
  function relToActiveRoot(abs: string | null): string | null {
    const root = activeSession()?.root();
    if (!root || !abs) return null;
    if (abs === root) return "";
    if (abs.startsWith(`${root}/`)) return abs.slice(root.length + 1);
    return null;
  }

  // The file shown in the focused tile pane (editor or diff), as a root-rel
  // path — so the Explorer can highlight and reveal it. Commit tiles and
  // non-file panes yield null.
  const focusedTileFile = (): string | null => {
    const assign = inLayout()
      ? (layoutAssignments()?.assignments[layoutFocusedPaneId() ?? ""] ?? null)
      : activeTile();
    if (!assign || typeof assign !== "string" || !isTileAssignment(assign))
      return null;
    const t = parseTileAssignment(assign);
    if (!t) return null;
    if (t.kind === "editor" || t.kind === "preview")
      return relToActiveRoot(t.arg);
    if (t.kind === "diff") return relToActiveRoot(parseDiffArg(t.arg).path);
    return null; // commit
  };

  // The terminal cwd as a root-rel directory, for the Explorer's follow-cd
  // highlight (null when the cwd is outside the active root).
  const cwdRelToRoot = (): string | null => {
    const f = focusedTerm();
    return f ? relToActiveRoot(f.cwd) : null;
  };

  // Reactive prop bag shared by every left-dock panel: they are pure views
  // over the one active IdeSession (getters keep them live).
  const leftPanelProps = {
    get session() {
      return activeSession();
    },
    get theme() {
      return theme();
    },
    get palette() {
      return palette();
    },
    get scale() {
      return chromeScale();
    },
    get fontFamily() {
      return resolvedFontWithFallback();
    },
    get fontSize() {
      return fontSize();
    },
    get activeFile() {
      return focusedTileFile();
    },
    get cwd() {
      return cwdRelToRoot();
    },
    onOpenTile: openTile,
  };

  // Re-root the dock at a worktree. The connection comes from the session
  // the list was read through, so navigating cannot silently land on another
  // server's path of the same name.
  function openWorktree(path: string) {
    const connectionId = activeSession()?.connectionId;
    if (!connectionId) return;
    const label = path.split("/").filter(Boolean).pop() ?? path;
    setRootSel({ kind: "worktree", connectionId, path, label });
  }

  function panelBody(panel: LeftPanel): JSX.Element {
    if (panel === "branches")
      return (
        <BranchesPanel
          {...leftPanelProps}
          onOpenWorktree={openWorktree}
          onOpenTerminalIn={(path) => void openTerminalIn(path)}
        />
      );
    if (panel === "log") return <LogPanel {...leftPanelProps} />;
    if (panel === "problems") return <ProblemsPanel {...leftPanelProps} />;
    return <ExplorerPanel {...leftPanelProps} />;
  }

  // The root the dock is showing: the focused terminal, or a declared
  // yas.roots entry. Sits at the top of the dock.
  function rootPickerHeader(): JSX.Element {
    const declared = () => roots().filter((r) => !r.disabled);
    // Label the "focused" option with the root actually being explored:
    // the session's resolved root (the repo workdir once git discovers
    // it), never the anchoring file or the terminal's live cwd. A `cd`
    // into a subdirectory expands the tree in place rather than
    // re-rooting it (see the cwd poll), so a cwd label would drift away
    // from the tree it sits above. Collapsed to a declared root's name
    // when the two name the same place.
    const focusedLabel = () => {
      const a = lastAnchor();
      if (!a) return t("workspace.focusedPane");
      const s = rootSel().kind === "focused" ? activeSession() : null;
      const root = s?.root();
      const f = a.kind === "terminal" ? focusedTerm() : null;
      const path = root ?? (a.kind === "path" ? a.path : f?.cwd);
      const connectionId =
        (root ? s?.connectionId : null) ??
        (a.kind === "path" ? a.connectionId : f?.conn);
      if (!path || !connectionId) return t("workspace.focusedPane");
      const match = declared().find(
        (r) =>
          r.path === path && connectionForRemote(r.remote) === connectionId,
      );
      return match ? match.name : `${connectionId}:${path}`;
    };
    const worktreeSel = () => {
      const s = rootSel();
      return s.kind === "worktree" ? s : null;
    };
    const value = () => {
      const s = rootSel();
      if (s.kind === "declared") return s.name;
      if (s.kind === "worktree") return "__worktree__";
      return "__focused__";
    };
    return (
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: `${chromeScale().tightGap}px`,
          padding: `${chromeScale().controlY}px ${chromeScale().panelPadding}px`,
          "border-bottom": `1px solid ${theme().subtleBorder}`,
        }}
      >
        <select
          // NOT `value={value()}`: Solid compiles that to a render effect
          // tracking only `value()`, which runs *before* the `<Show>` below
          // has added the `__worktree__` option. The browser drops an
          // assignment naming an option that does not exist yet, and the
          // select silently falls back to the first one — so navigating to a
          // worktree changed the root but left the picker reading "Focused
          // pane". Re-assigning from an effect that reads the option set
          // explicitly runs after the children exist.
          ref={(el) => {
            createEffect(() => {
              worktreeSel();
              declared();
              el.value = value();
            });
          }}
          onChange={(e) => {
            const v = e.currentTarget.value;
            if (v === "__focused__") setRootSel({ kind: "focused" });
            // Re-picking the worktree we are already on is a no-op; without
            // this it would fall through and mint a declared root named
            // "__worktree__" that resolves to nothing.
            else if (v !== "__worktree__")
              setRootSel({ kind: "declared", name: v });
          }}
          title={t("workspace.root")}
          style={{
            flex: 1,
            "min-width": 0,
            background: theme().panelBg,
            color: theme().fg,
            border: `1px solid ${theme().subtleBorder}`,
            "border-radius": "3px",
            padding: `1px ${chromeScale().tightGap}px`,
            "font-size": `${chromeScale().sm}px`,
            "font-family": resolvedFontWithFallback(),
          }}
        >
          <option value="__focused__">◐ {focusedLabel()}</option>
          {/* The worktree navigated to from the Branches panel. Only present
              while one is selected: it is a place you went, not a place you
              configured, so it does not accumulate in the list. */}
          <Show when={worktreeSel()}>
            {(sel) => <option value="__worktree__">⌥ {sel().label}</option>}
          </Show>
          <For each={declared()}>
            {(r) => <option value={r.name}>{r.name}</option>}
          </For>
        </select>
        <button
          onClick={() => toggleOverlay("roots")}
          title={t("workspace.manageRoots")}
          style={mergeStyle(ui.btn, {
            "flex-shrink": 0,
            "font-size": `${chromeScale().sm}px`,
            padding: `0 ${chromeScale().tightGap}px`,
            opacity: 0.7,
          })}
        >
          {"⚙"}
        </button>
      </div>
    );
  }
  // Open an IDE tile (editor/diff/commit).
  //
  //  - In a multi-pane layout: REPLACE the focused pane (never split,
  //    never destroy the layout), queueing if LayoutContainer isn't wired yet.
  //  - Otherwise (no layout, or a degenerate single-pane one): show the tile
  //    in place of the terminal via activeTile — do NOT swap into a layout. Drop any
  //    stale single-pane layout so the single-view view actually renders.
  // Which pane an IDE tile should open into: the focused pane if it already
  // holds a tile (so switching views / navigating replaces in place), otherwise
  // an existing editor/diff/commit pane (so clicking a file while a *terminal*
  // is focused swaps the file pane, not the terminal), otherwise the focused
  // pane. moveToPane focuses the target, so the highlight follows.
  // Where a file/diff/commit opens: ALWAYS the focused pane. No preference
  // for existing editor panes, no empty-pane special case — what you open
  // lands where you are, tiling-WM style. A terminal occupying the pane is
  // simply replaced (it stays alive off-screen in the preview panel).
  function preferredTilePane(): string {
    return layoutFocusedPaneId() ?? "0";
  }

  // ── Navigation history: browser-like back/forward per tile pane. openTile
  //    records the tile it replaces; navHistory walks the per-pane stacks.
  type NavStacks = { back: string[]; forward: string[] };
  const navHistory = new Map<string, NavStacks>();
  // The single-view activeTile slot, keyed in navHistory alongside real pane ids
  // (and used as a web-pane host id, same namespace). It cannot collide with a
  // pane: ids come from enumeratePanes (js/core/src/layout/tree.ts) as
  // dot-joined child indices, so every real one matches /^\d+(\.\d+)*$/.
  const NAV_NONBSP = "non-bsp";
  const navFor = (key: string): NavStacks => {
    let h = navHistory.get(key);
    if (!h) {
      h = { back: [], forward: [] };
      navHistory.set(key, h);
    }
    return h;
  };
  const navKeyFor = (paneId: string | null): string =>
    inLayout() && paneId ? paneId : NAV_NONBSP;
  const currentTileIn = (key: string): string | null => {
    if (key === NAV_NONBSP) return activeTile();
    const v = layoutAssignments()?.assignments[key];
    return typeof v === "string" ? v : null;
  };
  // Push the pane's current tile onto its back stack before it's replaced by a
  // *different* tile (a fresh navigation clears the forward stack).
  const recordNav = (key: string, next: string) => {
    const cur = currentTileIn(key);
    if (!cur || cur === next || !isTileAssignment(cur)) return;
    const h = navFor(key);
    h.back.push(cur);
    h.forward.length = 0;
  };
  // Place a tile into a pane without recording history (a history move itself).
  // Nothing evicts it from the dock: the dock is derived as "open minus
  // displayed", so showing a tile drops it from there by construction.
  const placeTile = (assignment: string, paneId: string | null) => {
    if (navKeyFor(paneId) === NAV_NONBSP) {
      if (activeLayout()) {
        exitLayout();
        saveActiveLayout(null); // persist, or a remount resurrects it
      }
      setActiveTile(assignment);
    } else if (paneId) {
      if (moveToPaneFn) moveToPaneFn(assignment, paneId);
      else queueTilePlacement(assignment, paneId);
    }
  };
  function navigateHistory(dir: "back" | "forward") {
    const paneId = inLayout() ? layoutFocusedPaneId() : null;
    const key = navKeyFor(paneId);
    const h = navFor(key);
    const from = dir === "back" ? h.back : h.forward;
    const to = dir === "back" ? h.forward : h.back;
    const target = from.pop();
    if (!target) return;
    const cur = currentTileIn(key);
    if (cur && isTileAssignment(cur) && cur !== target) to.push(cur);
    placeTile(target, paneId);
  }

  // ── Web panes ──

  const [webLocations, setWebLocations] = createSignal<WebLocation[]>([]);
  const [webUnavailable, setWebUnavailable] = createSignal<string | null>(null);
  // Which server a web pane resolves against; defaults to the active one and
  // is switchable in the picker, since a URL means different things per remote.
  const [webDest, setWebDest] = createSignal<string | null>(null);
  const webDestId = () => webDest() ?? activeConnectionId();
  // WebPane instances live in one persistent overlay and publish handles by
  // assignment, so moving a frame between a pane and the dock keeps the same
  // browsing context and navigation history.
  const webPaneHosts = createWebPaneHostRegistry();
  const [webHandles, setWebHandles] = createSignal<
    Record<string, WebPaneHandle>
  >({});
  const persistentWebAssignments = createMemo(() => {
    const assignments = new Set<string>();
    const active = activeTile();
    if (active && isWebAssignment(active)) assignments.add(active);
    for (const value of Object.values(layoutAssignments()?.assignments ?? {})) {
      if (typeof value === "string" && isWebAssignment(value)) {
        assignments.add(value);
      }
    }
    // Keep the dock's live-resource budget intact: older web cards remain
    // title-only and reload if restored, just like older editor cards.
    for (const value of backgroundTiles().slice(0, LIVE_DOCK_PREVIEWS)) {
      if (isWebAssignment(value)) assignments.add(value);
    }
    return Array.from(assignments);
  });

  /** Remembered locations live in the *server's* KV store, so each remote
   *  keeps its own set (docs/design/kv.md). `workspace.kv*` is per-connection,
   *  the same route the tab registry takes. */
  const webKv = (connectionId: string) => ({
    kvFetch: (key: string) => workspace.kvFetch(connectionId, key),
    kvPut: (key: string, value: Uint8Array) =>
      workspace.kvPut(connectionId, key, value),
  });

  async function refreshWebLocations() {
    const id = webDestId();
    if (!id) return;
    try {
      setWebLocations(await loadLocations(webKv(id)));
    } catch {
      // If native KV is unavailable, locations remain in memory for this
      // client session only.
    }
  }

  function persistWebLocations(next: WebLocation[]) {
    setWebLocations(next);
    const id = webDestId();
    if (id) void saveLocations(webKv(id), next).catch(() => {});
  }

  /** Open a location as a pane, and remember it. */
  function openWebPane(url: string, connectionId?: string, paneId?: string) {
    const assignment = webAssignment(connectionId ?? activeConnectionId(), url);
    // Floating mode is one window per assignment. A target pane only names
    // where the picker was opened; it must not turn creation into replace.
    if (!showAsFloatingWindow(assignment)) {
      if (paneId) dropTileIntoPane(assignment, paneId);
      else openTile(assignment);
    }
    persistWebLocations(withLocation(webLocations(), url, Date.now()));
  }

  /** How many panes currently hold content of one kind — counted the same way
   *  for panes and the single single-view slot, so the status bar's tally does
   *  not depend on which mode you are in. */
  const paneKindCount = (
    matches: (value: string | null) => boolean,
  ): number => {
    if (inLayout()) {
      const assignments = layoutAssignments()?.assignments ?? {};
      return Object.values(assignments).filter((v) => matches(v)).length;
    }
    return matches(activeTile()) ? 1 : 0;
  };

  /** The focused pane's web handle, or null — what the status bar drives. */
  const focusedWebPane = (): {
    handle: WebPaneHandle;
    url: string;
    retarget: (url: string) => void;
  } | null => {
    const assign = inLayout()
      ? (layoutAssignments()?.assignments[layoutFocusedPaneId() ?? ""] ?? null)
      : activeTile();
    const parsed = parseWebAssignment(assign);
    if (!parsed) return null;
    if (!assign) return null;
    // Only read when in a layout (retarget passes undefined otherwise), so the
    // single-view slot needs no name here.
    const paneId = layoutFocusedPaneId() ?? "";
    const handle = webHandles()[assign];
    if (!handle) return null;
    return {
      handle,
      url: parsed.url,
      // A new origin is a new relayed target, so the pane is re-assigned
      // rather than navigated — and remembered, like any other open.
      retarget: (url: string) =>
        openWebPane(url, parsed.connectionId, inLayout() ? paneId : undefined),
    };
  };

  onMount(() => {
    if (!previewSupported()) {
      setWebUnavailable(
        "previews need a secure context (https, or http on localhost)",
      );
      return;
    }
    // Register early: a pane that renders before the worker is active fetches
    // straight past it and lands on the Edge's 503.
    void ensurePreviewWorker().then(setWebUnavailable);
  });

  createEffect(() => {
    // Locations follow the active server, and are re-read when the picker
    // opens: the first read can land before the connection is ready, and a
    // stale empty list reads as "nothing remembered".
    void webDestId();
    if (overlay() === "web" || overlay() === null) void refreshWebLocations();
  });

  function openTile(assignment: string, asNewFloatingWindow = false) {
    // Manage panels can open terminal assignments through the same tile
    // boundary editors use. Dispatch those to the generic focused-view path;
    // only IDE/web assignments belong in the tile registry below.
    if (!isTileAssignment(assignment) && !isWebAssignment(assignment)) {
      focusAssignment(assignment);
      return;
    }
    if (inLayout()) {
      if (asNewFloatingWindow && showAsFloatingWindow(assignment, true)) return;
      const paneId = preferredTilePane();
      recordNav(paneId, assignment);
      if (openAssignmentInPane(assignment, paneId)) return;
      queueTilePlacement(assignment, paneId);
      return;
    }
    recordNav(NAV_NONBSP, assignment);
    const previous = focusedAssignment();
    if (previous && previous !== assignment) {
      queueTilePlacement(previous, "0");
      queueTilePlacement(assignment, "1");
      applyLayout(t("workspace.split"), "line(_, _)");
      return;
    }
    if (activeLayout()) {
      exitLayout();
      saveActiveLayout(null); // persist, or a remount resurrects it
    }
    setActiveTile(assignment);
  }

  // Drop a dragged pane assignment into a specific pane (records nav
  // history there). Any assignment the panel can hold: an IDE/web tile, or a
  // parked terminal/surface. recordNav is a no-op for the latter — it only
  // pushes when the assignment being *replaced* is a tile, which is what makes
  // Back return to a tile a dropped terminal displaced.
  function dropTileIntoPane(
    assignment: string,
    paneId: string,
    sourcePaneId?: string,
  ) {
    if (sourcePaneId == null && showAsFloatingWindow(assignment)) return;
    recordNav(paneId, assignment);
    if (sourcePaneId == null && openTabInPaneFn?.(assignment, paneId)) return;
    if (moveToPaneFn) moveToPaneFn(assignment, paneId, sourcePaneId);
    else queueTilePlacement(assignment, paneId);
  }

  /**
   * Show an assignment of any kind, wherever "here" currently is — the focused
   * pane, or the single main view. This is the one entry point that does
   * not care what it is holding: it dispatches on the assignment kind, because
   * each has its own slot (activeTile for a tile, the focused surface for a
   * surface, the focused session for a terminal), and each of the three
   * functions below already knows how to place itself in a pane as well.
   * All three dismiss the other two slots, so the modes can't overlap.
   *
   * Used by both drags that land on the main view and Ctrl+B [ / ].
   */
  function focusAssignment(assignment: string) {
    const surface = parseSurfaceAssignment(assignment);
    if (surface) {
      focusSurface(surface.surfaceId, surface.connectionId);
      return;
    }
    if (isTileAssignment(assignment) || isWebAssignment(assignment)) {
      openTile(assignment);
      return;
    }
    // Everything else in the assignment namespace is a bare session id.
    switchSession(assignment as SessionId);
  }
  /**
   * What the slot those keys act on is showing right now: the focused
   * pane's occupant, or — with no layout — whichever of the three single-view
   * slots is in use. Null when it holds nothing (a parked view), which makes
   * the next cycle step enter the ring at its near end instead of skipping one.
   */
  function focusedAssignment(): string | null {
    const paneId = layoutFocusedPaneId();
    if (activeLayout() && paneId) {
      return layoutAssignments()?.assignments[paneId] ?? null;
    }
    const tile = activeTile();
    if (tile) return tile;
    const surfaceId = focusedSurfaceId();
    if (surfaceId != null) {
      const connId =
        focusedSurfaceConnId() ??
        surfaces().find((s) => s.surfaceId === surfaceId)?.connectionId;
      if (connId) return surfaceAssignment(connId, surfaceId);
    }
    // Not wsState().focusedSessionId: the core always keeps *some* session
    // focused, so only the main view's own slot can say "nothing here".
    return mainViewSessionId();
  }

  /** A switcher mode prefix typed for you, consumed when it opens. */
  const [switcherSeed, setSwitcherSeed] = createSignal("");

  /** Ctrl+B Ctrl+B: the way past the one chord YAS reserves. */
  function forwardPrefixToFocusedPane() {
    const assignment = focusedAssignment();
    if (!assignment) return;
    const surface = parseSurfaceAssignment(assignment);
    if (surface) {
      const conn = workspace.getConnection(surface.connectionId);
      if (!conn) return;
      // Linux evdev: KEY_LEFTCTRL=29, KEY_B=48. YasSurfaceCanvas uses the
      // same codes for physical keyboard input.
      conn.sendSurfaceInput(surface.surfaceId, 29, true);
      conn.sendSurfaceInput(surface.surfaceId, 48, true);
      conn.sendSurfaceInput(surface.surfaceId, 48, false);
      conn.sendSurfaceInput(surface.surfaceId, 29, false);
      return;
    }
    // Editors and web panes own no PTY input channel. Do not send the chord to
    // a stale focused terminal behind one of those tiles.
    if (isTileAssignment(assignment) || isWebAssignment(assignment)) return;
    workspace.sendInput(assignment as SessionId, new Uint8Array([0x02]));
  }
  /** Highlight shown while a drag hovers the single-view main view (panes draw
   *  their own, per pane). */
  const [mainViewDragOver, setMainViewDragOver] = createSignal(false);
  const [layoutPaneActions, setLayoutPaneActions] =
    createSignal<PaneToolActions | null>(null);

  /** True while a pane's own content is being dragged (its grip). The dock
   *  reveals itself as a drop-to-park target for exactly this window — it is
   *  hidden when nothing is parked, which is precisely when a drag most needs
   *  it. Depth-counted: dragenter/dragleave fire per element crossed. */
  const [paneDragActive, setPaneDragActive] = createSignal(false);

  /**
   * Whether the preview panel is on screen — and so whether its thumbnails
   * exist to be watched.
   *
   * Both the panel's own `<Show>` and the parked sessions' stream
   * subscriptions read this, because they have to agree: a parked pty whose
   * thumbnail is not rendered would otherwise keep receiving Terminal frames
   * for a panel nobody can see. A grip drag reveals the panel even when
   * it is empty or toggled off — it is the drop-to-park target, and "nothing
   * parked yet" is exactly when a drag needs somewhere to park.
   */
  const previewPanelHasItems = () =>
    offScreenSessions().length > 0 ||
    offScreenSurfaces().length > 0 ||
    backgroundTiles().length > 0;
  // The status-bar toggle reflects the persistent preference even while the
  // empty shelf is suppressed. A drag remains the sole temporary visibility
  // exception because it needs a drop-to-park target.
  const previewPanelState = createMemo(() =>
    derivePreviewPanelState(
      previewPanelOpen(),
      previewPanelHasItems(),
      paneDragActive(),
    ),
  );
  const previewPanelVisible = () => previewPanelState().visible;
  let paneDragDepth = 0;
  const paneDragEnter = (e: DragEvent) => {
    if (!isPaneDrag(e)) return;
    paneDragDepth++;
    setPaneDragActive(true);
  };
  const paneDragLeave = (e: DragEvent) => {
    if (!isPaneDrag(e)) return;
    if (--paneDragDepth <= 0) {
      paneDragDepth = 0;
      setPaneDragActive(false);
    }
  };
  // `dragend` fires on the source — always ours here — and `drop` anywhere;
  // either way the window is over, whatever the enter/leave count says.
  const paneDragDone = () => {
    paneDragDepth = 0;
    setPaneDragActive(false);
  };
  // Pane drop targets stop propagation after they mutate the tree. Listening
  // for DROP only in the bubble phase therefore leaves paneDragActive stuck
  // when the dragged source unmounts before DRAGEND. Capture the event, but
  // defer cleanup until dispatch finishes so the revealed shelf remains
  // mounted long enough to receive its own drop.
  const paneDropDone = () => queueMicrotask(paneDragDone);
  window.addEventListener("dragenter", paneDragEnter);
  window.addEventListener("dragleave", paneDragLeave);
  window.addEventListener("drop", paneDropDone, true);
  window.addEventListener("dragend", paneDragDone, true);
  window.addEventListener("blur", paneDragDone);
  onCleanup(() => {
    window.removeEventListener("dragenter", paneDragEnter);
    window.removeEventListener("dragleave", paneDragLeave);
    window.removeEventListener("drop", paneDropDone, true);
    window.removeEventListener("dragend", paneDragDone, true);
    window.removeEventListener("blur", paneDragDone);
  });

  /** What the status bar's move handle drags from the main view: its tile,
   * surface, or terminal. Parking the focused session is focusing nothing —
   * the view falls back to EmptyPane and the session joins the dock, which
   * derives "parked" from "not displayed". */
  const mainViewDragAssignment = (): string | null => {
    const tile = activeTile();
    if (tile) return tile;
    const sid = focusedSurfaceId();
    const connId = focusedSurfaceConnId();
    if (sid != null && connId != null) return surfaceAssignment(connId, sid);
    return mainViewSessionId() ?? null;
  };

  /** Keep the core's focused session in the dock when the standalone view's
   * foreground assignment is removed. Surface and tile focus sit in UI-only
   * slots above that session, so merely clearing either slot would otherwise
   * expose the terminal as an unwanted second backgrounding step. */
  function parkMainViewSession() {
    const fid = wsState().focusedSessionId;
    if (fid != null) setParkedSessionId(fid);
  }

  /** A grip drag landed on the dock: park the content by taking it off
   *  screen — the dock lists exactly what is open but not displayed. */
  function parkDraggedAssignment(assignment: string, source: string) {
    if (source === MAIN_PANE_SOURCE) {
      if (assignment === activeTile()) {
        parkMainViewSession();
        setActiveTile(null);
        return;
      }
      const surface = parseSurfaceAssignment(assignment);
      if (surface && surface.surfaceId === focusedSurfaceId()) {
        parkMainViewSession();
        focusSurfaceById(null);
        return;
      }
      if (assignment === wsState().focusedSessionId) {
        setParkedSessionId(assignment as SessionId);
      }
      return;
    }
    // A pane: empty it, if it still holds what the drag carried — a
    // layout change mid-drag must not evict a bystander.
    if (layoutAssignments()?.assignments[source] === assignment) {
      clearPaneAssignmentFn?.(source);
    }
  }

  // Send the currently-focused IDE or web tile to the dock (Ctrl+B q).
  // Handles both the single-view focused tile and a tile occupying the focused
  // pane. Returns true if a tile was backgrounded (so the keyboard handler
  // knows it consumed the key). Stopping displaying it IS backgrounding it —
  // the tab stays registered, and the derived dock picks it up.
  function backgroundFocusedTile(): boolean {
    if (activeTile()) {
      parkMainViewSession();
      setActiveTile(null);
      return true;
    }
    const paneId = layoutFocusedPaneId();
    if (activeLayout() && paneId) {
      const assign = layoutAssignments()?.assignments[paneId] ?? null;
      if (assign && (isTileAssignment(assign) || isWebAssignment(assign))) {
        clearPaneAssignmentFn?.(paneId);
        return true;
      }
    }
    return false;
  }

  /**
   * Close the focused tile outright — the Ctrl+Alt+Shift+Q counterpart to
   * {@link backgroundFocusedTile}'s Ctrl+B q. Same targets (a single-view
   * active tile, or an IDE/web tile in the focused pane), but the tab is
   * closed rather than merely stopped being displayed, matching what the same
   * chord does to a terminal or a surface. Closing is host-wide now: the
   * registry record goes, so the tab leaves every frontend's dock.
   */
  function closeFocusedTile(): boolean {
    const tile = activeTile();
    if (tile) {
      setActiveTile(null);
      closeTab(tile);
      return true;
    }
    const paneId = layoutFocusedPaneId();
    if (activeLayout() && paneId) {
      const assign = layoutAssignments()?.assignments[paneId] ?? null;
      if (assign && (isTileAssignment(assign) || isWebAssignment(assign))) {
        clearPaneAssignmentFn?.(paneId);
        closeTab(assign);
        return true;
      }
    }
    return false;
  }

  function closeSurfaceFromUi(
    connectionId: ConnectionId,
    surfaceId: SurfaceId,
  ): void {
    if (
      focusedSurfaceId() === surfaceId &&
      focusedSurfaceConnId() === connectionId
    ) {
      setPendingMainRef(null);
    }
    const assignment = surfaceAssignment(connectionId, surfaceId);
    const operation = closingSurfaces.begin(assignment);
    try {
      workspace.closeSurface(connectionId, surfaceId);
      closingSurfaces.settle(
        assignment,
        operation,
        true,
        surfaces().some(
          (surface) =>
            surface.connectionId === connectionId &&
            surface.surfaceId === surfaceId,
        ),
      );
    } catch (error) {
      closingSurfaces.settle(assignment, operation, false, false);
      throw error;
    }
  }

  createEffect(() => {
    closingSurfaces.reconcile(
      new Set(
        surfaces().map((surface) =>
          surfaceAssignment(surface.connectionId, surface.surfaceId),
        ),
      ),
    );
  });

  function closeSessionFromUi(sessionId: SessionId): Promise<void> {
    if (wsState().focusedSessionId === sessionId) setPendingMainRef(null);
    return workspace.closeSession(sessionId);
  }

  /**
   * The single-view counterpart to LayoutContainer's per-pane ✕: close whatever the
   * single main view is showing. Same cascade as Ctrl+Alt+Shift+Q, minus the
   * layout-pane surface branch that can't apply here.
   */
  function closeFocusedPane() {
    if (closeFocusedTile()) return;
    const sid = focusedSurfaceId();
    const sConnId = focusedSurfaceConnId();
    if (sid != null && sConnId != null) {
      closeSurfaceFromUi(sConnId, sid);
      return;
    }
    const fid = wsState().focusedSessionId;
    if (fid) void closeSessionFromUi(fid);
  }

  /** True when the main view is holding something the ✕ can close. A parked
   *  view holds nothing, so it gets no toolbar. */
  const mainViewClosable = () =>
    !!activeTile() || focusedSurfaceId() != null || !!mainViewSessionId();

  const statusPaneActions = createMemo<PaneToolActions | null>(() => {
    if (inLayout() && activeLayout()) return layoutPaneActions();
    if (!mainViewClosable()) return null;
    const assignment = mainViewDragAssignment();
    return {
      drag: assignment ? { assignment, paneId: MAIN_PANE_SOURCE } : undefined,
      onClose: closeFocusedPane,
    };
  });

  // Restore a backgrounded tile: showing it removes it from the dock, which is
  // derived as "open minus displayed".
  function restoreTile(assignment: string, asNewFloatingWindow = false) {
    openTile(assignment, asNewFloatingWindow);
  }
  // The ✕ on a background-editor card. Closes the tab host-wide (it is an
  // explicit close, the same as Ctrl+Alt+Shift+Q on a displayed one), so its
  // live dock tile unmounts here — fs-sync/LSP torn down — and it leaves the
  // other frontends' docks too.
  function closeBackgroundTile(assignment: string) {
    closeTab(assignment);
  }

  // The signal updates per pointermove (live layout); the localStorage write
  // lands once, on a trailing debounce, instead of per move.
  const persistTimers = new Map<string, ReturnType<typeof setTimeout>>();
  function writeStorageDebounced(key: string, value: string) {
    const prev = persistTimers.get(key);
    if (prev !== undefined) clearTimeout(prev);
    persistTimers.set(
      key,
      setTimeout(() => {
        persistTimers.delete(key);
        writeStorage(key, value);
      }, 250),
    );
  }
  onCleanup(() => {
    for (const t of persistTimers.values()) clearTimeout(t);
  });
  function persistLeftDockWidth(w: number) {
    setLeftDockWidth(w);
    writeStorageDebounced(LEFT_DOCK_WIDTH_KEY, String(w));
  }
  function persistPreviewPanelWidth(w: number) {
    setPreviewPanelWidth(w);
    writeStorageDebounced(PREVIEW_PANEL_WIDTH_KEY, String(w));
  }

  let paletteOverlayOrigin: TerminalPalette | null = null;
  let fontOverlayOrigin: {
    family: string;
    size: number;
    gamma: number;
  } | null = null;

  const storedPaletteId = useStoredValue(PALETTE_KEY);
  const storedFont = useStoredValue(FONT_KEY);
  const storedFontSize = useStoredValue(FONT_SIZE_KEY);
  const storedTextGamma = useStoredValue(TEXT_GAMMA_KEY);
  // No media settings here on purpose — bitrate, mute, encoder effort,
  // streaming, frame rate and zoom are device-local (see storage.ts), so they
  // are read from localStorage once at startup and never taken from another
  // device.

  /** Track a device-local preference. */
  const followStored = (
    stored: () => string | null,
    apply: (raw: string) => void,
  ) => {
    createEffect(() => {
      const raw = stored();
      if (raw) apply(raw);
    });
  };

  followStored(storedPaletteId, (id) => {
    const p = PALETTES.find((x) => x.id === id);
    if (p) setPalette(p);
  });

  followStored(storedFont, (f) => {
    if (f.trim()) setFont(f.trim());
  });

  followStored(storedFontSize, (s) => {
    const n = parseInt(s, 10);
    if (n > 0) setFontSize(n);
  });

  followStored(storedTextGamma, (s) => {
    const n = Number(s);
    if (Number.isFinite(n) && n >= 0.5 && n <= 2.5) setTextGamma(n);
  });

  // Sync media preferences to all connections so new subscribes use them.
  createEffect(() => {
    const bandwidth = videoBandwidth();
    const speed = videoSpeed();
    const b = audioBitrate();
    const streaming = surfaceStreaming();
    const smoothing = surfaceSmoothing();
    const maxFps = surfaceMaxFps();
    for (const snap of allConnections()) {
      const conn = workspace.getConnection(snap.id);
      if (conn) {
        conn.defaultSurfaceBandwidth = bandwidth;
        conn.defaultSurfaceSpeed = speed;
        conn.defaultAudioBitrateKbps = b;
        conn.surfaceStreamingEnabled = streaming;
        conn.surfaceStore.setPresentationSmoothingEnabled(smoothing);
        conn.setSurfaceMaxFpsCap(maxFps);
      }
    }
  });

  // Reactively sync audio subscriptions to all connections.
  // Subscribes when unmuted and surfaces exist, unsubscribes when muted or
  // surfaces disappear. Also applies mute state to the AudioPlayer so newly
  // added connections pick up the current setting.
  //
  // AudioPlayer state changes, including reconnect resets, are wired into the
  // native connection's emit chain,
  // so this effect re-runs whenever the subscription is invalidated and can
  // re-subscribe automatically.
  createEffect(() => {
    const muted = audioMuted();
    const bitrate = audioBitrate();
    // Read surfaces() to re-run when surfaces appear/disappear.
    surfaces();
    for (const snap of allConnections()) {
      if (!snap.supportsAudio) continue;
      const conn = workspace.getConnection(snap.id);
      if (!conn) continue;
      conn.audioPlayer.setMuted(muted);
      const surfs = conn.surfaceStore.getSurfaces();
      if (surfs.size === 0) {
        // No surfaces — unsubscribe if subscribed.
        if (conn.audioPlayer.subscribed) {
          conn.sendAudioUnsubscribe();
        }
        continue;
      }
      if (!muted && !conn.audioPlayer.subscribed) {
        conn.sendAudioSubscribe(bitrate);
      } else if (muted && conn.audioPlayer.subscribed) {
        conn.sendAudioUnsubscribe();
      }
    }
  });

  const resolvedFontWithFallback = () => {
    const rf = resolvedFont();
    const base = defaultFont();
    return rf === base ? rf : `${rf}, ${base}`;
  };

  // Overlays portal to <body> (Overlay.tsx) to escape <main>'s keyboard-pin
  // transform; give them the font they used to inherit from <main>.
  createEffect(() => {
    document.body.style.fontFamily = resolvedFontWithFallback();
  });

  onMount(loadServerFonts);

  let lru: SessionId[] = [];

  createEffect(() => {
    const fid = wsState().focusedSessionId;
    if (!fid) return;
    lru = [fid, ...lru.filter((id) => id !== fid)];
  });

  createEffect(() => {
    if (activeLayout()) return;
    setLayoutAssignments(null);
    setAssignmentsResolved(true);
  });

  // Visibility management
  createEffect(() => {
    const al = activeLayout();
    const ov = overlay();
    if (al && ov !== "expose") return;
    const desired = new Set<SessionId>();
    const fid = wsState().focusedSessionId;
    // A focused terminal can be displaced by a surface/tile while remaining
    // the core's focus fallback. Do not let that special case keep a folded
    // off-screen Muster terminal subscribed.
    const focusedMusterIsFolded =
      offScreenSessions().some(
        (session) => session.id === fid && isMusterSession(session),
      ) && !watchedPreviewSessions().some((session) => session.id === fid);
    if (fid && !focusedMusterIsFolded) desired.add(fid);
    // Parked terminals are watched only while their thumbnails are rendered.
    if (previewPanelVisible()) {
      for (const s of watchedPreviewSessions()) desired.add(s.id);
    }
    if (ov === "expose") {
      for (const session of sessions()) {
        if (session.state !== "closed") desired.add(session.id);
      }
    }
    workspace.setVisibleSessions(desired);
  });

  // Auth error — trigger if any connection has an auth error.
  createEffect(() => {
    const conns = allConnections();
    if (conns.some((c) => c.error === "auth")) props.onAuthError();
  });

  // Worst status across all connections.
  const connectionStatus = () => {
    const conns = allConnections();
    if (conns.length === 0) return "disconnected" as const;
    for (const s of [
      "error",
      "disconnected",
      "closed",
      "connecting",
      "authenticating",
    ] as const) {
      if (conns.some((c) => c.status === s)) return s;
    }
    return "connected" as const;
  };

  // Auto-open the remotes overlay while connections are being established
  // on initial page load, and auto-close once everything is connected.
  // Once dismissed (by auto-close or user action), never auto-open again.
  const [remotesAutoOpen, setRemotesAutoOpen] = createSignal<
    "pending" | "open" | "done"
  >("pending");
  createEffect(() => {
    const status = connectionStatus();
    const phase = remotesAutoOpen();
    if (status === "connected") {
      if (phase === "open") {
        // All connected — auto-close if still showing.
        setRemotesAutoOpen("done");
        if (overlay() === "remotes") setOverlay(null);
      } else if (phase === "pending") {
        // Connected before we ever opened — skip entirely.
        setRemotesAutoOpen("done");
      }
      return;
    }
    // Only auto-open when there are configured remotes — a single local
    // connection is near-instant and doesn't need a status dialog.
    if (
      phase === "pending" &&
      overlay() === null &&
      remotes().length > 0 &&
      shellCapabilities().remotes
    ) {
      setRemotesAutoOpen("open");
      setOverlay("remotes");
    }
  });

  // Theme on document
  createEffect(() => {
    document.documentElement.setAttribute(
      "data-theme",
      palette().dark ? "dark" : "light",
    );
  });

  // Uniform themed scrollbars: a single global rule covering every scrollable
  // element, so nothing falls back to the chunky native bar (containers that
  // forget to spread scrollbarStyle, CodeMirror, xterm, …). Recoloured with the
  // palette.
  createEffect(() => {
    const t = theme();
    const id = "yas-scrollbars";
    let el = document.getElementById(id) as HTMLStyleElement | null;
    if (!el) {
      el = document.createElement("style");
      el.id = id;
      document.head.appendChild(el);
    }
    el.textContent = `
      * { scrollbar-width: thin; scrollbar-color: ${t.border} transparent; }
      *::-webkit-scrollbar { width: 10px; height: 10px; }
      *::-webkit-scrollbar-track { background: transparent; }
      *::-webkit-scrollbar-thumb {
        background: ${t.border};
        border-radius: 6px;
        border: 2px solid transparent;
        background-clip: padding-box;
      }
      *::-webkit-scrollbar-thumb:hover {
        background: ${t.dimFg};
        background-clip: padding-box;
      }
      *::-webkit-scrollbar-corner { background: transparent; }
    `;
  });
  onCleanup(() => document.getElementById("yas-scrollbars")?.remove());

  onMount(() => {
    document.documentElement.style.fontFamily = "system-ui, sans-serif";
  });

  // Title
  createEffect(() => {
    const host = yasHost();
    const parts: string[] = [];
    // In a layout, the workspace's focusedSessionId can be resurrected by
    // resolveFocusedSessionId's per-connection fallback on any connection
    // event (e.g. a terminal title update), even after the layout explicitly
    // cleared it to focus a surface or empty pane.  Gate on the layout's focused
    // pane actually holding a session so a background terminal's title
    // can't leak into the browser title bar.
    //
    // Outside a layout the same leak happens when a surface is focused:
    // focusedSessionId still points at the terminal that was showing
    // before the surface took over, so terminal title updates would
    // bleed into document.title.  Suppress the session branch when a
    // surface is focused.
    const al = activeLayout();
    const layoutHasSession =
      al != null &&
      (() => {
        const pid = layoutFocusedPaneId();
        if (!pid) return false;
        const assignment = layoutAssignments()?.assignments[pid] ?? null;
        return assignment != null && !isSurfaceAssignment(assignment);
      })();
    const sessionFocused = al
      ? layoutHasSession
      : focusedSurfaceId() == null && !mainTerminalParked();
    const fs = sessionFocused ? focusedSession() : null;
    if (fs) {
      if (fs.title) parts.push(truncateDocumentEntityTitle(fs.title));
      const label = connectionLabels().get(fs.connectionId);
      if (label) parts.push(label);
    } else {
      const surf =
        focusedSurfaceId() != null
          ? (surfaces().find(
              (s) =>
                s.surfaceId === focusedSurfaceId() &&
                (focusedSurfaceConnId() == null ||
                  s.connectionId === focusedSurfaceConnId()),
            ) ?? null)
          : layoutFocusedSurface();
      if (surf) {
        const name = surf.title || surf.appId;
        if (name) parts.push(truncateDocumentEntityTitle(name));
        const label = connectionLabels().get(surf.connectionId);
        if (label) parts.push(label);
      }
    }
    if (host && host !== "localhost" && host !== "127.0.0.1") parts.push(host);
    // Don't append "YAS" — installed PWA windows and most browsers already
    // prefix the tab with the app/manifest name, producing redundant
    // "YAS - … — YAS" titles.  Falling back to an empty document.title
    // when nothing is focused lets the OS/browser show just the app name.
    //
    // Assign only on a real change. This effect wakes on any connection event
    // — a terminal title update, a surface list edit, a label arriving — and
    // almost every one of those recomputes the same string. Writing it back
    // anyway makes the browser re-run its own title machinery, which is what
    // "the title updates constantly" was showing.
    const title = parts.join(" \u2014 ");
    if (document.title !== title) document.title = title;
  });

  let previousFocus: Element | null = null;

  // Auto-focus the terminal or surface canvas when the overlay closes.
  // Skip when a layout is active — LayoutContainer manages its own DOM
  // focus per-pane. Running here would always focus the first canvas in DOM
  // order (pane 1) because document.querySelector returns the first match.
  createEffect(() => {
    if (overlay()) return; // overlay is open, skip
    if (activeLayout()) return; // the layout manages its own focus
    const sid = mainViewSessionId();
    const surfId = focusedSurfaceId();
    if (!sid && surfId == null) return; // nothing to focus
    // Defer until Solid commits the DOM update. Workspace snapshots can also
    // rerun this effect while the same terminal remains selected; the shared
    // ownership guard keeps those updates from stealing focus from status
    // controls, overlays, or dock chrome.
    setTimeout(() => {
      autoFocusPaneTarget(
        () =>
          !overlay() &&
          !activeLayout() &&
          (mainViewSessionId() != null || focusedSurfaceId() != null),
        () =>
          document.querySelector<HTMLElement>(
            '[data-yas-workspace-focus-owner="main"] textarea[tabindex], [data-yas-workspace-focus-owner="main"] canvas[tabindex]',
          ),
      );
    }, 16);
  });

  function closeOverlay() {
    setSwitcherSeed("");
    // If the user manually dismisses the auto-opened remotes overlay,
    // mark it done so it never re-opens or auto-closes a later overlay.
    if (overlay() === "remotes" && remotesAutoOpen() === "open") {
      setRemotesAutoOpen("done");
    }
    paletteOverlayOrigin = null;
    fontOverlayOrigin = null;
    setOpenInNewTerminalMode(false);
    setNewTerminalTargetPaneId(null);
    // Dismissing the link dialog by any route — button, backdrop, Escape —
    // means "do not open". Clearing here keeps that true for all of them.
    setPendingLink(null);
    setOverlay(null);
    const el = previousFocus;
    previousFocus = null;
    if (el instanceof HTMLElement) setTimeout(() => el.focus(), 0);
  }

  /**
   * Bind hyperlink hover and activation to a terminal surface as it mounts.
   *
   * Applied to *every* surface, not just the focused one: hovering follows the
   * pointer, so a link in an unfocused split must still preview and open. The
   * WeakSet guards against re-binding a surface that a re-render hands back,
   * and unbinding is left to surface disposal — the listeners live on the
   * surface itself, so they die with it.
   */
  const linkBoundSurfaces = new WeakSet<YasTerminalSurface>();
  function bindTerminalLinks(surface: YasTerminalSurface | null) {
    if (!surface || linkBoundSurfaces.has(surface)) return;
    linkBoundSurfaces.add(surface);

    surface.onLinkHover(setHoveredLink);
    // Replaces core's blocking window.confirm with the in-app dialog. The
    // verdict still decides: `allow` opens, anything else asks, and the
    // overlay offers no way to proceed on `deny`.
    surface.setLinkActivateHandler((assessment) => {
      if (assessment.verdict === "allow") {
        window.open(assessment.raw, "_blank", "noopener,noreferrer");
        return;
      }
      setPendingLink({ assessment, text: hoveredLink()?.text ?? "" });
      previousFocus = document.activeElement as HTMLElement | null;
      setOverlay("link");
    });
  }

  function restoreOverlayPreview(target: Overlay) {
    if (target === "palette" && paletteOverlayOrigin) {
      setPalette(paletteOverlayOrigin);
      paletteOverlayOrigin = null;
    } else if (target === "font" && fontOverlayOrigin) {
      setFont(fontOverlayOrigin.family);
      setFontSize(fontOverlayOrigin.size);
      setTextGamma(fontOverlayOrigin.gamma);
      fontOverlayOrigin = null;
    }
  }

  function cancelOverlay() {
    restoreOverlayPreview(overlay());
    closeOverlay();
  }

  function openNewTerminalPicker(paneId?: string) {
    if (!previousFocus) previousFocus = document.activeElement;
    setNewTerminalTargetPaneId(paneId ?? null);
    setOpenInNewTerminalMode(true);
    setOverlay("expose");
  }

  function toggleOverlay(target: Overlay) {
    const current = overlay();
    if (current === target) {
      cancelOverlay();
      return;
    }
    restoreOverlayPreview(current);
    if (!current) previousFocus = document.activeElement;
    if (target === "remotes" && remotesAutoOpen() === "open") {
      // User explicitly opened remotes — stop auto-close from dismissing it.
      setRemotesAutoOpen("done");
    } else if (target === "palette") {
      paletteOverlayOrigin = palette();
    } else if (target === "font") {
      fontOverlayOrigin = {
        family: font(),
        size: fontSize(),
        gamma: textGamma(),
      };
      loadServerFonts();
    }
    setOverlay(target);
  }

  function changePalette(nextPalette: TerminalPalette) {
    setPalette(nextPalette);
    paletteOverlayOrigin = null;
    writeStorage(PALETTE_KEY, nextPalette.id);
    closeOverlay();
  }

  function changeFont(family: string, size: number, gamma: number) {
    const value = family.trim() || defaultFont();
    setFont(value);
    setFontSize(size);
    setTextGamma(gamma);
    fontOverlayOrigin = null;
    writeStorage(FONT_KEY, value);
    writeStorage(FONT_SIZE_KEY, String(size));
    writeStorage(TEXT_GAMMA_KEY, String(gamma));
    closeOverlay();
  }

  function changeAudioBitrate(kbps: number) {
    setAudioBitrate(kbps);
    writeStorage(AUDIO_BITRATE_KEY, String(kbps));
    // Re-subscribe all active audio connections with the new bitrate.
    for (const snap of allConnections()) {
      if (!snap.supportsAudio) continue;
      const conn = workspace.getConnection(snap.id);
      if (!conn || !conn.audioPlayer.subscribed) continue;
      conn.sendAudioSubscribe(kbps);
    }
  }

  function toggleAudio() {
    const newMuted = !audioMuted();
    setAudioMuted(newMuted);
    writeStorage(AUDIO_MUTED_KEY, newMuted ? "1" : "0");
    // The reactive effect (syncAudioSubscriptions) will handle
    // subscribing/unsubscribing and applying mute to all connections.
  }

  function changeVideoBandwidth(bandwidth: number) {
    setVideoBandwidth(bandwidth);
    writeStorage(VIDEO_BANDWIDTH_KEY, String(bandwidth));
    applyVideoEncoding();
  }

  function changeVideoSpeed(speed: number) {
    setVideoSpeed(speed);
    writeStorage(VIDEO_SPEED_KEY, String(speed));
    applyVideoEncoding();
  }

  /** Push the current bandwidth/speed pair to every live subscription. */
  function applyVideoEncoding() {
    const bandwidth = videoBandwidth();
    const speed = videoSpeed();
    for (const snap of allConnections()) {
      const conn = workspace.getConnection(snap.id);
      if (!conn) continue;
      conn.defaultSurfaceBandwidth = bandwidth;
      conn.defaultSurfaceSpeed = speed;
      for (const surface of conn.surfaceStore.getSurfaces().values()) {
        conn.sendSurfaceResubscribe(surface.surfaceId, bandwidth, speed);
      }
    }
  }

  function changeSurfaceStreaming(enabled: boolean) {
    setSurfaceStreaming(enabled);
    writeStorage(SURFACE_STREAMING_KEY, enabled ? "1" : "0");
    for (const snap of allConnections()) {
      const conn = workspace.getConnection(snap.id);
      if (!conn) continue;
      conn.setSurfaceStreamingEnabled(enabled);
    }
  }

  function changeSurfaceSmoothing(enabled: boolean) {
    setSurfaceSmoothing(enabled);
    writeStorage(SURFACE_SMOOTHING_KEY, enabled ? "1" : "0");
    for (const snap of allConnections()) {
      workspace
        .getConnection(snap.id)
        ?.surfaceStore.setPresentationSmoothingEnabled(enabled);
    }
  }

  function changeSurfaceMaxFps(maxFps: number) {
    setSurfaceMaxFps(maxFps);
    writeStorage(SURFACE_MAX_FPS_KEY, String(maxFps));
  }

  /** Narrow which codecs this device accepts for native Surface video, then
   * refresh every live view so its format offer is reconsidered. */
  function changeSurfaceCodecs(mask: number) {
    setSurfaceCodecs(mask);
    writeStorage(SURFACE_CODECS_KEY, String(mask));
    setAllowedCodecSupport(mask);
    for (const snap of allConnections()) {
      workspace.getConnection(snap.id)?.refreshCodecSupport();
    }
  }

  /** Every resizable surface view re-derives the scale it asks the compositor
   *  for, so there is nothing to push to the connections here. */
  function changeSurfaceZoom(percent: number) {
    const clamped = Math.min(
      MAX_SURFACE_ZOOM,
      Math.max(MIN_SURFACE_ZOOM, Math.round(percent)),
    );
    setSurfaceZoom(clamped);
    writeStorage(SURFACE_ZOOM_KEY, String(clamped));
  }

  function changeSurfaceZoomMode(mode: SurfaceZoomMode) {
    setSurfaceZoomMode(mode);
    writeStorage(SURFACE_ZOOM_MODE_KEY, mode);
  }

  function changeSurfaceTouchMode(mode: SurfaceTouchMode) {
    setSurfaceTouchMode(mode);
    writeStorage(SURFACE_TOUCH_MODE_KEY, mode);
  }

  function changeWaylandKeyboardRequests(enabled: boolean) {
    setWaylandKeyboardRequests(enabled);
    writeStorage(WAYLAND_KEYBOARD_REQUESTS_KEY, enabled ? "1" : "0");
    if (enabled || keyboardManualOverride) return;
    const input = automaticKeyboardInput;
    automaticKeyboardInput = null;
    if (!input) return;
    setKeyboardWanted(false);
    if (document.activeElement === input) input.blur();
  }

  let focusBySessionFn: ((sessionId: SessionId) => void) | null = null;
  let moveSessionToPaneFn:
    | ((sessionId: SessionId, targetPaneId: string) => void)
    | null = null;
  let moveToPaneFn:
    | ((value: string, targetPaneId: string, fromPaneId?: string) => void)
    | null = null;
  let tabIntoPaneFn: ((value: string, sourcePaneId: string) => boolean) | null =
    null;
  let openTabInPaneFn:
    | ((value: string, sourcePaneId: string) => boolean)
    | null = null;
  let openInContainerFn:
    | ((value: string, targetPaneId: string) => boolean)
    | null = null;
  let splitPaneFn:
    | ((
        value: string,
        targetPaneId: string,
        direction?: "horizontal" | "vertical",
      ) => void)
    | null = null;
  // A tile to drop into a freshly-created layout, flushed when LayoutContainer
  // wires moveToPane on mount (no-layout file open).
  // Layout controls disappear briefly while a tree remounts. Keep every
  // placement by assignment rather than one last-writer-wins slot: browsers
  // can create their main window and another toplevel in the same catalogue
  // update.
  const pendingTilePlacements = new Map<
    string,
    { assignment: string; paneId: string }
  >();
  const queueTilePlacement = (assignment: string, paneId: string) => {
    pendingTilePlacements.set(assignment, { assignment, paneId });
  };
  let clearPaneAssignmentFn: ((paneId: string) => void) | null = null;
  let focusPaneFn: ((paneId: string) => void) | null = null;
  let addFloatingWindowFn: ((assignment: string) => boolean) | null = null;
  let addManagedWindowFn: ((assignment: string) => boolean) | null = null;
  // Surfaces can arrive during the brief interval in which a floating
  // LayoutContainer is remounting and has not published its controls yet.
  // Retain those placements until the replacement container can accept them.
  const pendingFloatingPlacements = new Set<string>();
  // New compositor toplevels are not ordinary focused-pane replacements.
  // Retain them until LayoutContainer can append/split a managed window.
  const pendingManagedWindowPlacements = new Set<string>();
  // Drop every LayoutContainer control-fn reference. These close over a specific
  // LayoutContainer instance; when the container unmounts that instance is
  // disposed, so the stale fns must be cleared or a later call would write into
  // a dead instance (the tile lands nowhere and never renders). The next
  // container re-wires them via its onMoveToPane/etc. effects on mount.
  function clearLayoutControlFns() {
    focusBySessionFn = null;
    moveSessionToPaneFn = null;
    moveToPaneFn = null;
    tabIntoPaneFn = null;
    openTabInPaneFn = null;
    openInContainerFn = null;
    splitPaneFn = null;
    clearPaneAssignmentFn = null;
    focusPaneFn = null;
    addFloatingWindowFn = null;
    addManagedWindowFn = null;
  }

  /** Ordinary opens inherit the focused pane's current container layout.
   * Explicit drag/move paths keep using moveToPane because their intent is to
   * relocate an existing view. */
  function openAssignmentInPane(assignment: string, paneId: string): boolean {
    const occupant = layoutAssignments()?.assignments[paneId] ?? null;
    if (occupant != null && openInContainerFn?.(assignment, paneId))
      return true;
    if (!moveToPaneFn) return false;
    moveToPaneFn(assignment, paneId);
    return true;
  }

  /** A pane grip landed on a tab-capable parked card. Keep the dragged pane
   * as the visible tab and add that card beside it, instead of letting the
   * sidebar's generic drop handler park the source. Parked surface previews
   * are not hosts; a surface must be live in a pane to accept a drop. */
  function tabDraggedPaneWithParked(
    parkedAssignment: string,
    draggedAssignment: string,
    sourcePaneId: string,
  ): void {
    if (
      !isParkedTabDropTarget(parkedAssignment) ||
      parkedAssignment === draggedAssignment
    ) {
      return;
    }
    if (sourcePaneId === MAIN_PANE_SOURCE) {
      if (inLayout() || mainViewDragAssignment() !== draggedAssignment) return;
      // Queue before mounting LayoutContainer. It flushes in insertion order,
      // so put the parked tab down first and the current view last: focus ends
      // on the view the user dragged, with no visible content swap.
      queueTilePlacement(parkedAssignment, "1");
      queueTilePlacement(draggedAssignment, "0");
      applyLayout(t("workspace.tabs"), "tabs(_, _)");
      return;
    }
    if (layoutAssignments()?.assignments[sourcePaneId] !== draggedAssignment) {
      return;
    }
    tabIntoPaneFn?.(parkedAssignment, sourcePaneId);
  }

  /** Show a parked item without evicting a floating window already on screen. */
  function showAsFloatingWindow(assignment: string, explicit = false): boolean {
    const layout = activeLayout();
    if (
      !inLayout() ||
      !layout ||
      !shouldOpenAsFloatingWindow(layout.root, explicit)
    ) {
      return false;
    }
    const shown = Object.entries(layoutAssignments()?.assignments ?? {}).find(
      ([, value]) => value === assignment,
    )?.[0];
    if (shown) {
      pendingFloatingPlacements.delete(assignment);
      focusPaneFn?.(shown);
      return true;
    }
    // Falling back here means "replace the current pane", which is never the
    // right semantics in floating mode. Queue across the remount instead.
    if (!addFloatingWindowFn) {
      pendingFloatingPlacements.add(assignment);
      return true;
    }
    if (!addFloatingWindowFn(assignment)) {
      pendingFloatingPlacements.add(assignment);
      return true;
    }
    pendingFloatingPlacements.delete(assignment);
    return true;
  }

  /** Add a compositor toplevel without evicting an existing window. */
  function showAsManagedWindow(assignment: string): boolean {
    pendingManagedWindowPlacements.add(assignment);
    const layout = activeLayout();
    if (!layout) return true;
    const shown = Object.entries(layoutAssignments()?.assignments ?? {}).find(
      ([, value]) => value === assignment,
    )?.[0];
    if (shown) {
      pendingManagedWindowPlacements.delete(assignment);
      focusPaneFn?.(shown);
      return true;
    }
    if (addManagedWindowFn?.(assignment)) {
      // Keep the request pending until LayoutContainer publishes the
      // assignment. Accepting a callback is not delivery: its tree can be
      // replaced in the same reactive turn during a layout/HMR transition.
      const confirmed = Object.values(
        layoutAssignments()?.assignments ?? {},
      ).includes(assignment);
      if (confirmed) pendingManagedWindowPlacements.delete(assignment);
    }
    return true;
  }

  const surfaceBootGeneration = (connectionId: ConnectionId) =>
    wsState().connections.find((connection) => connection.id === connectionId)
      ?.bootGeneration ?? null;

  // Visibility must not depend on which launcher happened to create a
  // toplevel. The launcher correlation above is still useful for focus, but a
  // surface started from a terminal, an extension, or a delayed process
  // follows the same placement path. Identities include the server boot: a
  // transient catalogue reset does not place existing windows again, while a
  // restarted compositor is free to reuse its numeric handles.
  const initiallyRestoredSurfaces = restoredSurfaceAssignments(
    initialPaneAssignments,
  );
  const knownTopLevelSurfacePlacementKeys = new Set<string>();
  const deferredRestoredSurfacePlacements = new Set<string>();
  createEffect(() => {
    const current = surfaces();
    const restoredAssignmentsResolved = assignmentsResolved();
    const readyConnections = readyConnIds();
    const added: string[] = [];
    const liveTopLevels = new Set<string>();
    for (const surface of current) {
      const assignment = surfaceAssignment(
        surface.connectionId,
        surface.surfaceId,
      );
      const identity = surfacePlacementIdentity(
        surface,
        surfaceBootGeneration(surface.connectionId),
      );
      const topLevel = surface.parentId === 0n;
      if (topLevel) liveTopLevels.add(assignment);
      if (
        observeTopLevelSurface(
          knownTopLevelSurfacePlacementKeys,
          identity,
          topLevel,
        )
      ) {
        added.push(assignment);
      }
    }
    for (const assignment of added) {
      // LayoutContainer resolves stable references from the restored workspace.
      // Everything else is a new live window and must be placed whether the
      // catalogue populated before or after this effect's first run.
      if (
        initiallyRestoredSurfaces.has(assignment) &&
        !restoredAssignmentsResolved
      ) {
        deferredRestoredSurfacePlacements.add(assignment);
        continue;
      }
      pendingManagedWindowPlacements.add(assignment);
    }
    if (restoredAssignmentsResolved) {
      const shown = new Set(
        Object.values(layoutAssignments()?.assignments ?? {}).filter(
          (assignment): assignment is string => assignment != null,
        ),
      );
      for (const assignment of [...deferredRestoredSurfacePlacements]) {
        deferredRestoredSurfacePlacements.delete(assignment);
        if (liveTopLevels.has(assignment) && !shown.has(assignment)) {
          pendingManagedWindowPlacements.add(assignment);
        }
      }
    }
    // Reconcile desired visibility against observed layout state. This effect
    // also tracks layoutAssignments, so an accepted-but-not-yet-published
    // insertion is retried or confirmed instead of disappearing from the
    // arrival stream forever.
    for (const assignment of [...pendingManagedWindowPlacements]) {
      if (
        pendingSurfacePlacementIsRetired(
          assignment,
          liveTopLevels,
          readyConnections,
        )
      ) {
        pendingManagedWindowPlacements.delete(assignment);
        continue;
      }
      if (!liveTopLevels.has(assignment)) continue;
      showAsManagedWindow(assignment);
    }
  });

  function applyLayout(name: string, dsl: string) {
    const layout: WorkspaceLayout = { ...layoutFromDSL(dsl), name };
    setActiveTile(null);
    setActiveLayout(layout);
    saveActiveLayout(layout);
  }
  // Invariant: the control fns are valid exactly while a LayoutContainer is
  // mounted, which is exactly while inLayout() is true (it mounts under
  // `inLayout() && activeLayout()`). Whenever we're not in a layout, clear them so a
  // dangling reference to a disposed container can never be called — covers
  // every teardown path (open-tile, clear-layout, multi→single collapse).
  createEffect(() => {
    if (!inLayout()) clearLayoutControlFns();
  });
  const [layoutFocusedPaneId, setLayoutFocusedPaneId] = createSignal<
    string | null
  >(null);
  const activePaneId = createMemo(() =>
    activeLayout() ? layoutFocusedPaneId() : null,
  );

  /** Resolve the surface occupying the layout's focused pane (if any). */
  const layoutFocusedSurface = createMemo(() => {
    const paneId = activePaneId();
    if (!paneId) return null;
    const la = layoutAssignments();
    if (!la) return null;
    const value = la.assignments[paneId] ?? null;
    const parsed = parseSurfaceAssignment(value);
    if (!parsed) return null;
    return (
      surfaces().find(
        (s) =>
          s.surfaceId === parsed.surfaceId &&
          s.connectionId === parsed.connectionId,
      ) ?? null
    );
  });

  /** Surface identity used by the status/debug bar.
   *
   * Pane focus and its assignment snapshot are published by separate owners.
   * A pointer press can therefore expose one reactive turn with neither, even
   * though the same Surface view and decoder remain live. Preserve the prior
   * object only through that unresolved turn (or a transient catalogue gap),
   * while still clearing immediately for an explicitly focused terminal,
   * tile, web view, or empty pane. */
  const statusFocusedSurface = createMemo<YasSurface | null>((previous) => {
    const live = surfaces();
    if (inLayout()) {
      const paneId = activePaneId();
      const assignments = layoutAssignments();
      if (!paneId || !assignments) return previous;
      const parsed = parseSurfaceAssignment(
        assignments.assignments[paneId] ?? null,
      );
      if (!parsed) return null;
      return (
        live.find(
          (surface) =>
            surface.surfaceId === parsed.surfaceId &&
            surface.connectionId === parsed.connectionId,
        ) ??
        (previous?.surfaceId === parsed.surfaceId &&
        previous.connectionId === parsed.connectionId
          ? previous
          : null)
      );
    }

    const surfaceId = focusedSurfaceId();
    if (surfaceId == null) return null;
    const connectionId = focusedSurfaceConnId();
    return (
      live.find(
        (surface) =>
          surface.surfaceId === surfaceId &&
          (connectionId == null || surface.connectionId === connectionId),
      ) ??
      (previous?.surfaceId === surfaceId &&
      (connectionId == null || previous.connectionId === connectionId)
        ? previous
        : null)
    );
  }, null);

  /**
   * Retire the badge's entries: the surface the viewer is now looking at has
   * answered its own request, and one that has gone away can no longer make it.
   *
   * Lives down here rather than beside `pendingAttention` because it reads
   * `inLayout` and `layoutFocusedSurface`, and a memo declared above them runs its
   * body inside their temporal dead zone at setup — an eager
   * `Cannot access 'inLayout' before initialization` that takes the whole workspace
   * down to the error screen, with nothing in tsc or the unit tests to catch it.
   * The same hazard is called out on `parkedSessionId` above.
   */
  const frontSurfaceAssignment = createMemo(() => {
    if (inLayout()) {
      const f = layoutFocusedSurface();
      return f ? surfaceAssignment(f.connectionId, f.surfaceId) : null;
    }
    const sid = focusedSurfaceId();
    const connId = focusedSurfaceConnId();
    return sid != null && connId != null
      ? surfaceAssignment(connId, sid)
      : null;
  });
  createEffect(() => {
    const front = frontSurfaceAssignment();
    const live = new Set(
      surfaces().map((s) => surfaceAssignment(s.connectionId, s.surfaceId)),
    );
    setPendingAttention((prev) =>
      settleAttention(prev, front, (a) => live.has(a)),
    );
  });

  /** Reset a custom tree to one managed tiling pane. */
  function exitLayout() {
    setLayoutAssignments(null);
    setUnresolvedLayoutAssignments({});
    setAssignmentsResolved(true);
    const layout = freshTilingLayout();
    setActiveLayout(layout);
    saveActiveLayout(layout);
  }

  /** A manager with no remaining windows falls back to one empty managed pane. */
  function collapseLayoutToSingle(_assignment: string | null) {
    setLayoutAssignments(null);
    setUnresolvedLayoutAssignments({});
    setAssignmentsResolved(true);
    const layout = freshTilingLayout();
    setActiveLayout(layout);
    saveActiveLayout(layout);
    setActiveTile(null);
    focusSurfaceById(null);
    parkMainViewSession();
  }

  /** The mirror of exitLayout. Under a layout the panes own what is on
   *  screen, so the single-view surface slot describes nothing — and entering
   *  entering a layout never placed the focused surface in a pane, it simply stopped
   *  rendering it. Leaving the slot set handed every consumer of "is a surface
   *  focused" the wrong answer. */
  createEffect(() => {
    if (inLayout()) focusSurfaceById(null);
  });

  function switchSession(sessionId: SessionId) {
    focusSessionFromUi(sessionId);
    previousFocus = null;
    closeOverlay();
  }

  function focusSessionFromUi(
    sessionId: SessionId,
    asNewFloatingWindow = false,
  ) {
    retainMainTerminalRef(sessionId);
    // Re-showing the parked session itself: focus does not change, so only an
    // explicit clear can un-park it.
    if (sessionId === parkedSessionId()) setParkedSessionId(null);
    focusSurfaceById(null);
    // Stops DISPLAYING the single-view tile; the tab stays open and drops into
    // the dock (and stays listed in every other frontend).
    setActiveTile(null);
    if (
      activeLayout() &&
      !(asNewFloatingWindow && showAsFloatingWindow(sessionId, true))
    ) {
      focusBySessionFn?.(sessionId);
    }
    workspace.focusSession(sessionId);
  }

  function focusSurface(
    surfaceId: SurfaceId,
    connectionId?: ConnectionId,
    asNewWindow = false,
  ) {
    setActiveTile(null); // stops displaying the single-view tile; tab stays open
    const layout = activeLayout();
    const focusedPaneId = layoutFocusedPaneId();
    // Resolve the assignment before consulting focus: a freshly remounted
    // floating layout can have no focused pane for one reactive turn, but a
    // new application must still append its own window during that turn.
    if (layout) {
      const connId =
        connectionId ??
        surfaces().find((x) => x.surfaceId === surfaceId)?.connectionId ??
        activeConnectionId();
      const assignment = surfaceAssignment(connId, surfaceId);
      if (asNewWindow && showAsManagedWindow(assignment)) {
        focusSurfaceById(null);
        previousFocus = null;
        closeOverlay();
        return;
      }
      // During a LayoutContainer remount the focused pane and its move
      // callback disappear for one reactive turn. Never consume the surface
      // arrival in that gap: queue it against pane 0, which moveToPane validates
      // against the replacement tree and redirects to its focused/first pane.
      const targetPaneId = focusedPaneId ?? preferredTilePane();
      // Already displayed in some pane? Focus that pane instead of moving it.
      // moveToPane would *swap*: assignmentsAfterDrop recovers a surface's
      // source pane from the current assignments (surfaces are unique views),
      // so the focused pane's occupant would take the vacated one. That is
      // right for a drag, and wrong for every caller here — none of them is a
      // drag. xdg_activation is the loud case: dropping a link from Slack onto
      // Brave makes Brave raise itself while Slack's pane still holds focus,
      // and the two panes traded places. Terminals never had the bug because
      // focusBySession checks this first; surfaces now match.
      const shown = Object.entries(layoutAssignments()?.assignments ?? {}).find(
        ([, value]) => value === assignment,
      )?.[0];
      if (shown) focusPaneFn?.(shown);
      else if (openAssignmentInPane(assignment, targetPaneId)) {
        // Placement completed as a new active tab.
      } else queueTilePlacement(assignment, targetPaneId);
      focusSurfaceById(null);
    } else {
      focusSurfaceById(surfaceId, connectionId);
    }
    // Null first: closeOverlay restores previousFocus on a timeout, which
    // would steal focus back from the surface — see selectPane.
    previousFocus = null;
    closeOverlay();
  }

  function raiseMediaPlayer(player: {
    connectionId: string;
    desktopEntry: string;
    identity: string;
  }) {
    const target = surfaces()
      .filter((surface) => surface.connectionId === player.connectionId)
      .map((surface) => ({
        surface,
        score:
          mprisSurfaceMatchScore(player, surface) +
          (surface.parentId === 0n ? 1 : 0),
      }))
      .filter((candidate) => candidate.score > 1)
      .sort((left, right) => right.score - left.score)[0]?.surface;
    if (target) focusSurface(target.surfaceId, target.connectionId, true);
  }

  /**
   * A Wayland client asked for its own toplevel (xdg_activation_v1 — an
   * Electron app reacting to a notification click). It is answered with a
   * highlight where the surface already is, and nothing else: the view is the
   * user's, and an app that wants it can only ask to be looked at.
   *
   * Raising instead is what made the dock unusable next to a talkative client.
   * Tokens are cheap and their delivery unacknowledged, so a client repeats the
   * request several times a second, and each repeat landed after whatever the
   * user had just picked — their choice appearing for an instant and being
   * dragged back off, with repeated clicking working only when one fell in a
   * gap. Under a layout it was worse: each repeat re-focused a pane out from
   * under them. See ./surfaceAttention.ts.
   */
  function activateSurface(surfaceId: SurfaceId, connectionId: ConnectionId) {
    // Already on top: the user is looking straight at it, so lighting it up
    // would be noise rather than news.
    //
    // "On top" is a different slot in each mode: focusedSurfaceId is the
    // single-view main view, which is left null under a layout, so testing only
    // that would leave this dead in a layout. There the equivalent question is
    // whether the surface already occupies the focused pane.
    if (inLayout()) {
      const focused = layoutFocusedSurface();
      if (
        focused?.surfaceId === surfaceId &&
        focused?.connectionId === connectionId
      ) {
        return;
      }
    } else if (
      focusedSurfaceId() === surfaceId &&
      focusedSurfaceConnId() === connectionId
    ) {
      return;
    }
    // Already marked: a repeat is the same request arriving again, and adding it
    // twice would say nothing new. Returning `prev` keeps it out of the render.
    const target = surfaceAssignment(connectionId, surfaceId);
    setPendingAttention((prev) =>
      prev.has(target) ? prev : new Set(prev).add(target),
    );
  }

  let termHandle: { rows: number; cols: number; focus: () => void } | null =
    null;

  async function createAndFocus(command?: string, connectionId?: string) {
    // `[remote>][command]` doubles as a location bar: an entry with a scheme
    // or a port is a web pane, not a program (see looksLikeWebLocation).
    if (command && looksLikeWebLocation(command)) {
      openWebPane(command, connectionId);
      closeOverlay();
      return;
    }
    try {
      const previous = focusedAssignment();
      const fid = wsState().focusedSessionId;
      const connId = connectionId ?? activeConnectionId();
      const session = await workspace.createSession({
        connectionId: connId,
        rows: termHandle?.rows ?? 24,
        cols: termHandle?.cols ?? 80,
        ...(command ? { command } : {}),
        ...(!command && fid && !connectionId ? { cwdFromSessionId: fid } : {}),
      });
      if (!showAsFloatingWindow(session.id)) {
        if (inLayout()) {
          openAssignmentInPane(session.id, preferredTilePane());
        } else if (previous) {
          queueTilePlacement(previous, "0");
          queueTilePlacement(session.id, "1");
          applyLayout(t("workspace.split"), "line(_, _)");
        } else {
          focusSurfaceById(null);
          setActiveTile(null);
        }
      }
      retainMainTerminalRef(session.id);
      workspace.focusSession(session.id);
      previousFocus = null;
      closeOverlay();
    } catch {}
  }

  /** Open a terminal in an absolute directory on the session's own
   *  connection — the Branches panel's secondary action on a worktree. Takes
   *  the focused pane like any other new terminal, so it lands where you are
   *  looking rather than somewhere you have to go find. */
  async function openTerminalIn(path: string) {
    const connectionId = activeSession()?.connectionId;
    if (!connectionId) return;
    try {
      const previous = focusedAssignment();
      const session = await workspace.createSession({
        connectionId,
        rows: termHandle?.rows ?? 24,
        cols: termHandle?.cols ?? 80,
        cwd: path,
      });
      focusSurfaceById(null);
      setActiveTile(null);
      if (!showAsFloatingWindow(session.id)) {
        if (inLayout()) {
          openAssignmentInPane(session.id, preferredTilePane());
        } else if (previous) {
          queueTilePlacement(previous, "0");
          queueTilePlacement(session.id, "1");
          applyLayout(t("workspace.split"), "line(_, _)");
        }
      }
      retainMainTerminalRef(session.id);
      workspace.focusSession(session.id);
    } catch {}
  }

  async function createInPane(
    paneId: string,
    command?: string,
    connectionId?: string,
  ) {
    if (command && looksLikeWebLocation(command)) {
      openWebPane(command, connectionId, paneId);
      return;
    }
    try {
      const fid = wsState().focusedSessionId;
      const connId = connectionId ?? activeConnectionId();
      const session = await workspace.createSession({
        connectionId: connId,
        rows: termHandle?.rows ?? 24,
        cols: termHandle?.cols ?? 80,
        ...(command ? { command } : {}),
        ...(!command && fid && !connectionId ? { cwdFromSessionId: fid } : {}),
      });
      if (!showAsFloatingWindow(session.id)) {
        openAssignmentInPane(session.id, paneId);
      }
      retainMainTerminalRef(session.id);
      workspace.focusSession(session.id);
    } catch {}
  }

  /** Create first, then change geometry, so a failed terminal creation cannot
   * leave an empty stack behind. */
  async function createBesideFocused() {
    try {
      const previous = focusedAssignment();
      const fid = wsState().focusedSessionId;
      const session = await workspace.createSession({
        connectionId: activeConnectionId(),
        rows: termHandle?.rows ?? 24,
        cols: termHandle?.cols ?? 80,
        ...(fid ? { cwdFromSessionId: fid } : {}),
      });
      const paneId = inLayout() ? layoutFocusedPaneId() : null;
      if (paneId && splitPaneFn) {
        splitPaneFn(session.id, paneId);
      } else {
        if (previous) {
          queueTilePlacement(previous, "0");
          queueTilePlacement(session.id, "1");
          applyLayout(t("workspace.split"), "line(_, _)");
        } else {
          focusSurfaceById(null);
          setActiveTile(null);
        }
      }
      retainMainTerminalRef(session.id);
      workspace.focusSession(session.id);
      previousFocus = null;
      closeOverlay();
    } catch {}
  }

  function selectPane(
    paneId: string,
    sessionId: SessionId | null,
    command?: string,
    connectionId?: string,
  ) {
    if (sessionId && !command) {
      focusSurfaceById(null);
      focusPaneFn?.(paneId);
      retainMainTerminalRef(sessionId);
      workspace.focusSession(sessionId);
    } else if (command || connectionId) {
      void createInPane(paneId, command, connectionId);
    } else {
      // Empty pane, no command — just move focus.
      focusPaneFn?.(paneId);
    }
    // Null first: closeOverlay restores previousFocus on a timeout, which
    // would steal focus back from the chosen pane — on touch devices that
    // drops the virtual keyboard and clears keyboardWanted.
    previousFocus = null;
    closeOverlay();
  }

  function handleRestartOrClose() {
    const fs = focusedSession();
    if (!fs) {
      const paneId = layoutFocusedPaneId();
      if (paneId) {
        void createInPane(paneId);
      } else {
        void createAndFocus();
      }
      return;
    }
    if (fs.state !== "exited") return;
    if (connection()?.supportsRestart) {
      workspace.restartSession(fs.id);
    } else {
      void closeSessionFromUi(fs.id);
    }
  }

  createKeyboardShortcuts({
    workspace,
    overlay,
    activeLayout,
    inLayout,
    layoutFocusedPaneId,
    layoutAssignments,
    focusedSession,
    sessions,
    focusedSessionId: () => wsState().focusedSessionId,
    supportsRestart: () => connection()?.supportsRestart ?? false,
    focusedSurfaceId,
    focusedSurfaceConnId,
    closeSurface: (connectionId: ConnectionId, surfaceId: SurfaceId) => {
      closeSurfaceFromUi(connectionId, surfaceId);
    },
    unfocusSurface: () => {
      parkMainViewSession();
      focusSurfaceById(null);
    },
    backgroundFocusedSession: parkMainViewSession,
    toggleOverlay,
    cycleWorkspaceTab: (delta) => {
      const controller = props.workspaceSessions;
      const currentId = props.workspaceSession?.id;
      if (!controller || !currentId) return false;
      const ids = controller.attachedSessionIds();
      if (ids.length < 2) return false;
      const index = ids.indexOf(currentId);
      if (index < 0) return false;
      void controller.select(ids[(index + delta + ids.length) % ids.length]);
      return true;
    },
    createWorkspaceTab: () => {
      void props.workspaceSessions?.create();
    },
    openWorkspaceManager: () => props.workspaceSessions?.openManager(),
    detachWorkspaceTab: () => {
      if (props.workspaceSession) {
        void props.workspaceSessions?.detach(props.workspaceSession.id);
      }
    },
    forwardPrefix: forwardPrefixToFocusedPane,
    seedSwitcher: setSwitcherSeed,
    cancelOverlay,
    toggleDebug,
    togglePreviewPanel,
    toggleLeftPanel: focusSection,
    toggleSearch: () => {
      // Three-way, not a plain toggle. Closed: open and focus. Open but
      // unfocused: just focus — you were looking at the results and asked
      // to get back to the query, not to lose them. Only when the input
      // already has focus does the chord dismiss. The query and results
      // survive a close (see ide/searchStore), so reopening resumes.
      if (!searchOpen()) {
        setSearchOpen(true);
        setSearchFocus((n) => n + 1);
      } else if (!searchInputFocused()) {
        setSearchFocus((n) => n + 1);
      } else {
        closeSearch();
      }
    },
    createAndFocus,
    createInPane,
    createBesideFocused,
    openNewTerminalPicker,
    handleRestartOrClose,
    connectionCount: () => allConnections().length,
    cycleRing,
    focusedAssignment,
    focusAssignment,
    clearFocusedPaneAssignment: () => {
      const paneId = layoutFocusedPaneId();
      if (paneId) clearPaneAssignmentFn?.(paneId);
    },
    backgroundFocusedTile,
    closeFocusedTile,
    navigateBack: () => navigateHistory("back"),
    navigateForward: () => navigateHistory("forward"),
  });

  // Follow the focused terminal's cwd: poll it and expand the Explorer tree so
  // a `cd` reveals the directory. Server reads the pty cwd (no OSC-7 needed).
  // The same poll feeds the root-picker label (conn:cwd), so it runs whenever a
  // terminal is focused — not only when an IDE root is active.
  let lastFollowedCwd = "";
  /**
   * The worktree top of the repository enclosing `dir`, or null when there
   * is none. One bare `GIT_OPEN` (no watch, no status — nothing to compute
   * server-side) closed as soon as it has answered; asked once per `cd`,
   * since the poll below only reaches it when the cwd has changed.
   */
  const repoTopOf = async (
    connectionId: ConnectionId,
    dir: string,
  ): Promise<string | null> => {
    try {
      const handle = await workspace.openRepo(connectionId, dir, {});
      const top = handle.workdir;
      handle.close();
      return top || null;
    } catch {
      // Not a repository, or git is unavailable on that server: either way
      // there is no boundary here to re-root on.
      return null;
    }
  };
  const pollFocusedCwd = () => {
    const fid = wsState().focusedSessionId;
    if (!fid) {
      setFocusedTerm(null);
      return;
    }
    const focused = wsState().sessions.find((x) => x.id === fid);
    if (!focused) {
      setFocusedTerm(null);
      return;
    }
    const connId = focused.connectionId;
    workspace
      .sessionCwd(connId, fid)
      .then((cwd) => {
        if (!cwd) {
          // No answer for this pty. Drop a reading that belongs to the
          // terminal we just switched away from rather than leaving it
          // on screen attributed to this one.
          setFocusedTerm((prev) =>
            prev && prev.sessionId !== fid ? null : prev,
          );
          return;
        }
        setBoundedStringLru(
          lastTerminalCwds,
          terminalCwdKey(connId, focused.ptyId),
          cwd,
        );
        setFocusedTerm({
          sessionId: fid,
          conn: connId,
          ptyId: focused.ptyId,
          cwd,
        });
        // A stale override for another terminal never outlives its focus.
        const ov = termCwdOverride();
        if (ov && ov.sessionId !== fid) setTermCwdOverride(null);
        const s = activeSession();
        const root = s?.root();
        if (!s || !root || cwd === lastFollowedCwd) return;
        lastFollowedCwd = cwd;
        if (cwd === root || cwd.startsWith(`${root}/`)) {
          // Inside the current root: reveal, don't re-root.
          s.expandTo(cwd === root ? "" : cwd.slice(root.length + 1));
          // Unless the cd crossed into a *different repository*. `cd linux`
          // from a plain `/src` is not a subdirectory to expand — it is a
          // project to show, and Files and Log belong to that repo rather
          // than to the directory above it. Only a repo boundary re-roots,
          // so cd-ing deeper inside one repo still just expands.
          if (cwd !== root && rootSel().kind === "focused") {
            void repoTopOf(connId, cwd).then((top) => {
              if (!top || top === root || top === s.repoWorkdir()) return;
              // The repo must enclose the cwd and sit inside the current
              // root: a repo *above* the root is the outer project the user
              // narrowed away from on purpose.
              if (top !== cwd && !cwd.startsWith(`${top}/`)) return;
              if (!top.startsWith(`${root}/`)) return;
              // Root at the repo's top, not at the cwd, so `cd linux/mm`
              // still shows the whole kernel.
              setTermCwdOverride({
                sessionId: fid,
                connectionId: connId,
                cwd: top,
              });
            });
          }
        } else if (rootSel().kind === "focused") {
          // Outside it (and the dock follows the terminal, not a pinned
          // root): re-root Files + Log at the new cwd.
          setTermCwdOverride({ sessionId: fid, connectionId: connId, cwd });
        }
      })
      .catch(() => {});
  };
  onMount(() => {
    // Paused while the document is hidden — a background tab has nothing to
    // reveal; becoming visible polls immediately and resumes the interval.
    let timer: ReturnType<typeof setInterval> | null = null;
    const stop = () => {
      if (timer != null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const start = () => {
      pollFocusedCwd();
      if (timer == null) timer = setInterval(pollFocusedCwd, 1500);
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") stop();
      else start();
    };
    if (document.visibilityState !== "hidden") start();
    document.addEventListener("visibilitychange", onVisibility);
    onCleanup(() => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    });
  });

  // Set font defaults on connection
  createEffect(() => {
    const conn = workspace.getConnection(activeConnectionId());
    if (!conn) return;
    const dpr = window.devicePixelRatio || 1;
    conn.setFontSize(fontSize() * dpr);
    conn.setFontFamily(resolvedFontWithFallback());
  });

  // Durable ephemeral-session → PTY identities. They let a pane remain
  // serializable across the short connection-removal window before its
  // replacement browser SessionId appears.
  const durableSessionRefs = new Map<string, string>();

  function stableAssignmentRef(
    assignment: string,
    bySessionId: ReadonlyMap<string, YasSession>,
  ): string | null {
    if (isTileAssignment(assignment) || isWebAssignment(assignment)) {
      const tab = stripConn(assignment);
      return tab ? tabWorkspaceRef(tab.connectionId, tabId(tab.bare)) : null;
    }
    const surface = parseSurfaceAssignment(assignment);
    if (surface) {
      return surfaceWorkspaceRefForId(surface.connectionId, surface.surfaceId);
    }
    const session = bySessionId.get(assignment);
    if (session?.ptyId != null) {
      return terminalWorkspaceRefForPtyId(session.connectionId, session.ptyId);
    }
    return durableSessionRefs.get(assignment) ?? null;
  }

  function currentStoredWorkspace(): WorkspaceSessionWorkspace {
    const allSessions = sessions();
    const bySessionId = new Map(
      allSessions.map((session) => [session.id, session]),
    );
    for (const session of allSessions) {
      if (session.ptyId != null) {
        setBoundedStringLru(
          durableSessionRefs,
          session.id,
          terminalWorkspaceRefForPtyId(session.connectionId, session.ptyId),
        );
      }
    }

    const assignments: Record<string, string> = {
      ...unresolvedLayoutAssignments(),
    };
    for (const [paneId, assignment] of Object.entries(
      layoutAssignments()?.assignments ?? {},
    )) {
      if (assignment == null) {
        if (!(paneId in unresolvedLayoutAssignments())) {
          delete assignments[paneId];
        }
        continue;
      }
      // LayoutContainer retains the authoritative native identity after it has
      // resolved to a browser-local assignment. It must win during a remote
      // reconnect, when the old alias cannot be translated and may soon be
      // reused for a different native object.
      if (paneId in unresolvedLayoutAssignments()) continue;
      const stable = stableAssignmentRef(assignment, bySessionId);
      if (stable) assignments[paneId] = stable;
    }

    let main = pendingMainRef();
    if (inLayout()) {
      main = null;
    } else if (main == null) {
      const tile = activeTile();
      if (tile) {
        const tab = stripConn(tile);
        if (tab) main = tabWorkspaceRef(tab.connectionId, tabId(tab.bare));
      } else {
        const surfaceId = focusedSurfaceId();
        const surfaceConnectionId = focusedSurfaceConnId();
        if (surfaceId != null && surfaceConnectionId) {
          main = surfaceWorkspaceRefForId(surfaceConnectionId, surfaceId);
        } else {
          const focused = wsState().focusedSessionId;
          if (focused) main = stableAssignmentRef(focused, bySessionId);
        }
      }
    }

    const active = activeLayout();
    const collapsed = collapsedSections();
    return {
      layout: active ? { name: active.name, dsl: active.dsl } : null,
      assignments,
      focusedPaneId: layoutFocusedPaneId(),
      main,
      panels: {
        leftOpen: leftDockOpen(),
        previewOpen: previewPanelOpen(),
        expandedSections: [
          ...unknownStoredExpandedSections,
          ...LEFT_PANELS.filter((panel) => !collapsed.has(panel)),
        ],
        project: rootSel(),
        musterExpanded: musterPreviewExpanded(),
        debugOpen: debugPanel(),
      },
    };
  }

  // The controller keeps `restoring` asserted after attach. Release it only
  // once the layout and the single-pane main ref have completed their first remote
  // resolution pass; unresolved refs are already retained in the snapshot.
  createEffect(() => {
    const binding = props.workspaceSession;
    if (
      binding?.restoring() &&
      assignmentsResolved() &&
      mainRestoreResolved()
    ) {
      binding.finishRestoring();
    }
  });

  // Persist one debounced, field-semantic workspace patch after the UI settles.
  // Name and activeRemotes are deliberately absent, so concurrent CRUD and
  // remote management are not overwritten by a layout/panel update.
  let workspacePatchTimer: ReturnType<typeof setTimeout> | undefined;
  const workspacePatchSequencer = new WorkspaceSessionPatchSequencer();
  let latestWorkspacePatchTarget: WorkspaceSessionBinding | null = null;
  let latestUiWorkspace = currentStoredWorkspace();
  createEffect(() => {
    const binding = props.workspaceSession;
    const restoring = binding?.restoring() ?? true;
    const next = currentStoredWorkspace();
    latestWorkspacePatchTarget = binding ?? null;
    latestUiWorkspace = next;
    clearTimeout(workspacePatchTimer);
    workspacePatchTimer = undefined;
    if (!binding) {
      const layout = activeLayout();
      if (layout) {
        saveActiveLayoutState(layout, next.assignments, next.focusedPaneId);
      }
      workspacePatchSequencer.reset(null, next);
      return;
    }
    if (restoring) {
      workspacePatchSequencer.stage(binding, binding.current().workspace, next);
      return;
    }
    workspacePatchTimer = setTimeout(() => {
      workspacePatchTimer = undefined;
      workspacePatchSequencer.submit(binding, next);
    }, WORKSPACE_SESSION_PATCH_DEBOUNCE_MS);
  });
  onCleanup(() => {
    clearTimeout(workspacePatchTimer);
    if (latestWorkspacePatchTarget) {
      workspacePatchSequencer.submit(
        latestWorkspacePatchTarget,
        latestUiWorkspace,
      );
    }
    workspacePatchSequencer.finishAfterDrain();
  });

  const { countFrame, timeline, net, metrics } = createMetrics(
    () =>
      props
        .connectionSpecs()
        .map((spec) => spec.connection?.transport ?? spec.transport)
        .filter((transport): transport is YasTransport => transport != null),
    debugPanel,
  );

  // Surface timing samples exist solely for the debug pane. Avoid creating
  // and correlating one record per video frame while it is closed.
  createEffect(() => workspace.setSurfaceDiagnosticsEnabled(debugPanel()));

  // Periodically bump a counter while the debug panel is open so that
  // debugStats (which reads from non-reactive Maps) gets re-sampled.
  const [debugTick, setDebugTick] = createSignal(0);
  createEffect(() => {
    if (!debugPanel()) return;
    const id = setInterval(() => setDebugTick((n) => n + 1), 1000);
    onCleanup(() => clearInterval(id));
  });

  const theme = () => themeFor(palette());
  const chromeScale = () => uiScale(fontSize());
  const mod = /Mac|iPhone|iPad/.test(navigator.platform) ? "Cmd" : "Ctrl";
  // Intent alone isn't enough for the key line: it must vanish the moment the
  // software keyboard is reduced, not a settling period later when intent
  // expires — and never sit over a keyboard that failed to rise (hardware
  // keyboard attached, focus lost to an overlay).  The occlusion gate tracks
  // the keyboard itself; the iPadOS shortcut bar (>32px) still counts.
  const showMobileToolbar = createMemo(
    () => isMobileTouch() && keyboardWanted() && viewportOccluded(),
  );
  const statusBarHeight = () => chromeScale().md + chromeScale().controlY * 3;

  return (
    <YasWorkspaceProvider
      workspace={workspace}
      palette={palette()}
      fontFamily={resolvedFontWithFallback()}
      fontSize={fontSize()}
      advanceRatio={advanceRatio()}
      textGamma={textGamma()}
    >
      <main
        style={{
          ...layout.workspace,
          "background-color": theme().bg,
          color: theme().fg,
          "font-family": resolvedFontWithFallback(),
          // While anything is parked over the viewport, pin to it so content
          // is not hidden.  Otherwise let the 100dvh root size the app
          // natively to avoid double-counting keyboard/browser-chrome space.
          ...(isMobileTouch() && viewportOccluded() && vpHeight()
            ? {
                position: "fixed",
                "inset-inline": "0",
                top: "0",
                height: `${vpHeight()}px`,
                transform: `translateY(${vpOffset()}px)`,
              }
            : {}),
        }}
      >
        <Show when={props.workspaceSessions}>
          {(controller) => (
            <>
              <WorkspaceSessionTabs
                controller={controller()}
                palette={palette()}
                fontFamily={resolvedFontWithFallback()}
                fontSize={fontSize()}
              />
              <WorkspaceSessionOverlay
                controller={controller()}
                palette={palette()}
                fontFamily={resolvedFontWithFallback()}
                fontSize={fontSize()}
              />
            </>
          )}
        </Show>
        <PrefixMap
          palette={palette()}
          fontFamily={resolvedFontWithFallback()}
          fontSize={fontSize()}
        />
        <PersistentWebPanes
          assignments={persistentWebAssignments()}
          registry={webPaneHosts}
          onHandle={(assignment, handle) =>
            setWebHandles((previous) => ({
              ...previous,
              [assignment]: handle,
            }))
          }
        />
        <section
          style={{
            ...layout.termContainer,
            display: "flex",
            "flex-direction": "row",
          }}
        >
          <Show when={leftDockOpen()}>
            <LeftDock
              collapsed={collapsedForDock()}
              weights={sectionWeights()}
              header={rootPickerHeader()}
              theme={theme()}
              scale={chromeScale()}
              isMobileTouch={isMobileTouch()}
              width={leftDockWidth()}
              onResizeWidth={persistLeftDockWidth}
              onResizeWeight={resizeSectionWeight}
              onToggleCollapse={toggleSectionCollapse}
              renderBody={panelBody}
            />
          </Show>
          {/* The middle column, with the docks flanking it. Project search is
              absolutely overlaid inside this box so opening it cannot resize
              the workspace, while both docks retain their full height. */}
          <div
            ref={(element) => (middleWorkspaceColumn = element)}
            style={{
              flex: 1,
              "min-width": 0,
              display: "flex",
              "flex-direction": "column",
              overflow: "hidden",
              position: "relative",
            }}
          >
            <Show when={searchOpen()}>
              <div
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  right: 0,
                  "z-index": z.exitedBanner,
                  display: "flex",
                  "flex-direction": "column",
                  ...(searchHeight() == null
                    ? { height: "auto", "max-height": "50%" }
                    : { height: `${(searchHeight()! * 100).toFixed(1)}%` }),
                }}
              >
                <section
                  data-yas-search-pane
                  style={{
                    // This is visual chrome over the workspace, not a flex
                    // sibling of it. Opening search must never renegotiate a
                    // terminal grid or a native surface size.
                    flex: searchHeight() == null ? "0 1 auto" : 1,
                    "min-height": 0,
                    display: "flex",
                    "flex-direction": "column",
                    overflow: "hidden",
                    background: theme().bg,
                  }}
                >
                  <SearchPanel
                    {...leftPanelProps}
                    focusNonce={searchFocus()}
                    onClose={closeSearch}
                  />
                </section>
                <ResizeHandle
                  direction="vertical"
                  measureElement={() => middleWorkspaceColumn}
                  onDrag={(fraction) =>
                    setSearchHeight((cur) =>
                      Math.min(
                        0.9,
                        Math.max(
                          0.08,
                          (cur ?? autoSearchFraction()) + fraction,
                        ),
                      ),
                    )
                  }
                />
              </div>
            </Show>
            <div
              data-yas-workspace-focus-owner={
                !(inLayout() && activeLayout()) ? "main" : undefined
              }
              style={{ flex: 1, overflow: "hidden", position: "relative" }}
              // Drop target for the single-pane main view. Every handler bails
              // in a layout: panes are the precise targets there and they sit
              // inside this div, so without the guard their drops would bubble
              // up and be handled twice.
              onDragOver={(e) => {
                if (inLayout() && activeLayout()) return;
                if (!isTileDrag(e)) return;
                e.preventDefault(); // allow the drop
                e.dataTransfer!.dropEffect = "copy";
                if (!mainViewDragOver()) setMainViewDragOver(true);
              }}
              onDragLeave={(e) => {
                // Ignore leaves into child elements; only clear when truly
                // leaving (same rule as a pane).
                if (!e.currentTarget.contains(e.relatedTarget as Node | null))
                  setMainViewDragOver(false);
              }}
              onDrop={(e) => {
                setMainViewDragOver(false);
                if (inLayout() && activeLayout()) return;
                const assignment = tileDragAssignment(e);
                if (!assignment) return;
                e.preventDefault();
                focusAssignment(assignment);
              }}
            >
              <Show when={mainViewDragOver()}>
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
              <Show
                when={inLayout() && activeLayout()}
                fallback={
                  <Show
                    when={parseWebAssignment(activeTile())}
                    fallback={
                      <Show
                        when={activeTile()}
                        fallback={
                          <Show
                            when={focusedSurfaceId()}
                            fallback={
                              <Show
                                when={mainViewSessionId()}
                                fallback={
                                  <EmptyPane
                                    paneId="__workspace_empty__"
                                    isFocused={true}
                                    theme={theme()}
                                    palette={palette()}
                                    fontSize={fontSize()}
                                    connectionId={activeConnectionId()}
                                    connectionLabels={connectionLabels()}
                                    onCreateInPane={(
                                      _paneId,
                                      command,
                                      connectionId,
                                    ) => {
                                      // In single-view mode, paneId is irrelevant — we just
                                      // create a terminal and focus it.  When the user
                                      // didn't type a remote prefix or command and there
                                      // are multiple connections, fall back to the
                                      // remote picker so they can choose.
                                      if (
                                        !command &&
                                        !connectionId &&
                                        allConnections().length > 1
                                      ) {
                                        openNewTerminalPicker();
                                      } else {
                                        void createAndFocus(
                                          command,
                                          connectionId,
                                        );
                                      }
                                    }}
                                    onSwitcher={() => toggleOverlay("expose")}
                                    onHelp={() => toggleOverlay("help")}
                                  />
                                }
                              >
                                {(fid) => (
                                  <>
                                    <TerminalDropTarget
                                      workspace={workspace}
                                      sessionId={fid()}
                                      connectionId={
                                        focusedSession()?.connectionId ??
                                        activeConnectionId()
                                      }
                                      surface={terminalSurface}
                                      theme={theme()}
                                      scale={chromeScale()}
                                    >
                                      <YasTerminal
                                        sessionId={fid()}
                                        readOnly={isSessionReadOnly(fid())}
                                        onRender={countFrame}
                                        style={{
                                          width: "100%",
                                          height: "100%",
                                        }}
                                        fontFamily={resolvedFontWithFallback()}
                                        fontSize={fontSize()}
                                        palette={palette()}
                                        surfaceRef={(s) => {
                                          setTerminalSurface(s);
                                          bindTerminalLinks(s);
                                        }}
                                      />
                                    </TerminalDropTarget>
                                    <Show
                                      when={
                                        focusedSession()?.state === "exited"
                                      }
                                    >
                                      <div
                                        style={{
                                          position: "absolute",
                                          bottom: "32px",
                                          left: "50%",
                                          transform: "translateX(-50%)",
                                          "background-color":
                                            theme().solidPanelBg,
                                          border: `1px solid ${theme().border}`,
                                          padding: `${chromeScale().controlY}px ${chromeScale().controlX}px`,
                                          "font-size": `${chromeScale().sm}px`,
                                          "z-index": z.exitedBanner,
                                          display: "flex",
                                          "align-items": "center",
                                          gap: `${chromeScale().gap}px`,
                                        }}
                                      >
                                        <mark
                                          style={{
                                            ...ui.badge,
                                            "background-color":
                                              "rgba(255,100,100,0.3)",
                                          }}
                                        >
                                          {t("workspace.exited")}
                                        </mark>
                                        <Show
                                          when={connection()?.supportsRestart}
                                        >
                                          <button
                                            onClick={() =>
                                              handleRestartOrClose()
                                            }
                                            style={{
                                              ...ui.btn,
                                              "font-size": `${chromeScale().md}px`,
                                            }}
                                          >
                                            {t("workspace.restart")}{" "}
                                            <kbd style={ui.kbd}>
                                              {t("keyboard.enter")}
                                            </kbd>
                                          </button>
                                        </Show>
                                        <button
                                          onClick={() => {
                                            const fs = focusedSession();
                                            if (fs)
                                              void closeSessionFromUi(fs.id);
                                          }}
                                          style={mergeStyle(ui.btn, {
                                            "font-size": `${chromeScale().md}px`,
                                            opacity: 0.5,
                                          })}
                                        >
                                          {t("workspace.close")}{" "}
                                          <kbd style={ui.kbd}>
                                            {t("keyboard.esc")}
                                          </kbd>
                                        </button>
                                      </div>
                                    </Show>
                                  </>
                                )}
                              </Show>
                            }
                          >
                            {(sid) => (
                              <YasSurfaceView
                                connectionId={
                                  focusedSurfaceConnId() ?? activeConnectionId()
                                }
                                surfaceId={sid()}
                                focus
                                resizable
                                zoom={surfaceZoom() / 100}
                                zoomMode={surfaceZoomMode()}
                                touchMode={surfaceTouchMode()}
                                style={{
                                  width: "100%",
                                  height: "100%",
                                }}
                              />
                            )}
                          </Show>
                        }
                      >
                        {/* Pane actions are common status-bar chrome, not
                            content overlaid on this editor. */}
                        {(tile) => (
                          <div
                            style={{
                              width: "100%",
                              height: "100%",
                              position: "relative",
                            }}
                          >
                            <YasTile
                              workspace={workspace}
                              assignment={tile()}
                              focused
                              theme={theme()}
                              palette={palette()}
                              scale={chromeScale()}
                              fontFamily={resolvedFontWithFallback()}
                              fontSize={fontSize()}
                              onOpenTile={openTile}
                              isConnectionReadOnly={isConnectionReadOnly}
                            />
                          </div>
                        )}
                      </Show>
                    }
                  >
                    {/* Close is provided by the status bar. */}
                    <div
                      style={{
                        width: "100%",
                        height: "100%",
                        position: "relative",
                      }}
                    >
                      <WebPaneHost
                        assignment={activeTile()!}
                        hostId={NAV_NONBSP}
                        register={webPaneHosts.register}
                        focused
                      />
                    </div>
                  </Show>
                }
              >
                {(al) => (
                  <LayoutContainer
                    layout={al()}
                    onLayoutChange={setWorkspaceLayout}
                    connectionId={activeConnectionId()}
                    isSessionReadOnly={isSessionReadOnly}
                    isConnectionReadOnly={isConnectionReadOnly}
                    connectionLabels={connectionLabels()}
                    palette={palette()}
                    fontFamily={resolvedFontWithFallback()}
                    fontSize={fontSize()}
                    surfaceZoom={surfaceZoom() / 100}
                    surfaceZoomMode={surfaceZoomMode()}
                    surfaceTouchMode={surfaceTouchMode()}
                    focusedSessionId={wsState().focusedSessionId}
                    lruSessionIds={lru}
                    liveSurfaceKeys={surfaces().map(
                      (s) => `${s.connectionId}:${s.surfaceId}`,
                    )}
                    hasAttention={hasAttention}
                    manageVisibility={overlay() !== "expose"}
                    extraVisibleSessions={
                      previewPanelVisible()
                        ? watchedPreviewSessions().map((s) => s.id)
                        : []
                    }
                    onAssignmentsChange={setLayoutAssignments}
                    storedAssignments={initialPaneAssignments}
                    storedFocusedPaneId={
                      clientFocusedPaneKey
                        ? readStorage(clientFocusedPaneKey)
                        : (localLayoutState?.focusedPaneId ?? null)
                    }
                    restoreKey={props.workspaceSession?.id}
                    onUnresolvedAssignmentsChange={
                      setUnresolvedLayoutAssignments
                    }
                    onAssignmentsResolved={setAssignmentsResolved}
                    onFocusSession={(id) => workspace.focusSession(id)}
                    onFocusBySession={(fn) => {
                      focusBySessionFn = fn;
                    }}
                    onFocusPane={(fn) => {
                      focusPaneFn = fn;
                    }}
                    onAddFloatingWindow={(fn) => {
                      addFloatingWindowFn = fn;
                      const layout = activeLayout();
                      if (layout) {
                        for (const assignment of [
                          ...pendingFloatingPlacements,
                        ]) {
                          if (fn(assignment))
                            pendingFloatingPlacements.delete(assignment);
                        }
                      }
                      return () => {
                        if (addFloatingWindowFn === fn)
                          addFloatingWindowFn = null;
                      };
                    }}
                    onAddManagedWindow={(fn) => {
                      addManagedWindowFn = fn;
                      for (const assignment of [
                        ...pendingManagedWindowPlacements,
                      ]) {
                        fn(assignment);
                      }
                      return () => {
                        if (addManagedWindowFn === fn)
                          addManagedWindowFn = null;
                      };
                    }}
                    onMoveSessionToPane={(fn) => {
                      moveSessionToPaneFn = fn;
                    }}
                    onMoveToPane={(fn) => {
                      moveToPaneFn = fn;
                      // Flush every placement queued while the layout controls
                      // were absent. Deleting before dispatch lets a nested
                      // reactive update safely queue it again if needed.
                      for (const p of [...pendingTilePlacements.values()]) {
                        pendingTilePlacements.delete(p.assignment);
                        fn(p.assignment, p.paneId);
                      }
                      return () => {
                        if (moveToPaneFn === fn) moveToPaneFn = null;
                      };
                    }}
                    onTabIntoPane={(fn) => {
                      tabIntoPaneFn = fn;
                      return () => {
                        if (tabIntoPaneFn === fn) tabIntoPaneFn = null;
                      };
                    }}
                    onOpenTabInPane={(fn) => {
                      openTabInPaneFn = fn;
                      return () => {
                        if (openTabInPaneFn === fn) openTabInPaneFn = null;
                      };
                    }}
                    onOpenInContainer={(fn) => {
                      openInContainerFn = fn;
                      return () => {
                        if (openInContainerFn === fn) openInContainerFn = null;
                      };
                    }}
                    onSplitPane={(fn) => {
                      splitPaneFn = fn;
                      return () => {
                        if (splitPaneFn === fn) splitPaneFn = null;
                      };
                    }}
                    onClearPaneAssignment={(fn) => {
                      clearPaneAssignmentFn = fn;
                    }}
                    onCollapseToSingle={collapseLayoutToSingle}
                    onFocusedPaneChange={(paneId) => {
                      setLayoutFocusedPaneId(paneId);
                      if (clientFocusedPaneKey && paneId) {
                        writeStorage(clientFocusedPaneKey, paneId);
                      }
                    }}
                    onFocusedPaneActionsChange={setLayoutPaneActions}
                    onOpenTile={openTile}
                    registerWebPaneHost={webPaneHosts.register}
                    onDropTile={dropTileIntoPane}
                    isMobileTouch={isMobileTouch()}
                    onCloseTab={closeTab}
                    onCloseSurface={closeSurfaceFromUi}
                    onCreateInPane={(paneId, command, connectionId) => {
                      if (
                        !command &&
                        !connectionId &&
                        allConnections().length > 1
                      ) {
                        openNewTerminalPicker(paneId);
                      } else {
                        void createInPane(paneId, command, connectionId);
                      }
                    }}
                    onSwitcher={() => toggleOverlay("expose")}
                    onHelp={() => toggleOverlay("help")}
                    onRender={countFrame}
                    onTerminalSurface={bindTerminalLinks}
                  />
                )}
              </Show>
            </div>
          </div>
          <Show when={previewPanelVisible()}>
            <PreviewPanel
              parkDropActive={paneDragActive()}
              onParkDrop={parkDraggedAssignment}
              onTabDrop={tabDraggedPaneWithParked}
              offScreenSessions={offScreenSessions()}
              allSessions={sessions()}
              surfaces={offScreenSurfaces()}
              focusedSurfaceId={focusedSurfaceId()}
              focusedSurfaceConnId={focusedSurfaceConnId()}
              hasAttention={hasAttention}
              connectionId={activeConnectionId()}
              connectionLabels={connectionLabels()}
              theme={theme()}
              scale={chromeScale()}
              palette={palette()}
              fontFamily={resolvedFontWithFallback()}
              fontSize={fontSize()}
              isMobileTouch={isMobileTouch()}
              onFocusSession={(sessionId) =>
                focusSessionFromUi(sessionId, true)
              }
              onFocusSurface={(connectionId, surfaceId) =>
                focusSurface(surfaceId, connectionId, true)
              }
              onCloseSession={(id) => void closeSessionFromUi(id)}
              onCloseSurface={(connectionId, surfaceId) =>
                closeSurfaceFromUi(connectionId, surfaceId)
              }
              width={previewPanelWidth()}
              onResize={persistPreviewPanelWidth}
              onClose={togglePreviewPanel}
              musterExpanded={musterPreviewExpanded()}
              expandedMusterStacks={expandedMusterStacks()}
              onToggleMuster={() =>
                setMusterPreviewExpanded((expanded) => !expanded)
              }
              onToggleMusterStack={toggleMusterStack}
              backgroundEditors={
                <For each={backgroundTiles()}>
                  {(assignment, index) => {
                    // Re-read, not read once: a manage tile's title carries the
                    // tab its panels are on, which changes under the card.
                    const d = () => tileDisplay(assignment);
                    const web = parseWebAssignment(assignment);
                    return (
                      // The same card parked terminals and surfaces get:
                      // swipe right dismisses, swipe left (or a hold)
                      // starts the drag, a click restores to the main view.
                      <Thumbnail
                        theme={theme()}
                        scale={chromeScale()}
                        isMobileTouch={isMobileTouch()}
                        assignment={assignment}
                        onFocus={() => restoreTile(assignment, true)}
                        onClose={() => closeBackgroundTile(assignment)}
                        closeTitle={t("common.close")}
                        header={() => (
                          <span
                            style={{
                              flex: 1,
                              "min-width": 0,
                              "text-align": "left",
                              display: "flex",
                              "flex-direction": "column",
                              overflow: "hidden",
                            }}
                          >
                            <span
                              style={{
                                "white-space": "nowrap",
                                overflow: "hidden",
                                "text-overflow": "ellipsis",
                                "max-width": "100%",
                                "font-size": `${chromeScale().sm}px`,
                              }}
                            >
                              {/* Address dim, then the name — the same shape
                                  the terminal and surface cards below use, so
                                  a column of parked things reads as one list
                                  rather than three conventions. */}
                              <Show when={d().prefix}>
                                <span style={{ opacity: 0.5 }}>
                                  {d().prefix}
                                </span>
                                <Show when={d().title}>{" \u203A "}</Show>
                              </Show>
                              {d().title}
                            </span>
                            <Show when={d().subtitle}>
                              <span
                                style={{
                                  "white-space": "nowrap",
                                  overflow: "hidden",
                                  "text-overflow": "ellipsis",
                                  "max-width": "100%",
                                  "font-size": `${chromeScale().xs}px`,
                                  opacity: 0.6,
                                }}
                              >
                                {d().subtitle}
                              </span>
                            </Show>
                          </span>
                        )}
                        body={() => (
                          // Read-only zoomed-out preview, terminal-thumbnail
                          // semantics: click to bring it back to the main
                          // view. Only the most recent cards are live — a
                          // mounted preview editor holds an fs sync and a web
                          // preview holds an iframe, so both are budgeted
                          // (LIVE_DOCK_PREVIEWS).
                          //
                          // A manage tile has no picture worth taking: its
                          // panels are lists of text at a size nobody can read,
                          // and mounting them to draw that would run a client
                          // catalog every second behind the card. Its title
                          // says which server and which tab, which is the whole
                          // of what the card is picked by.
                          <Show
                            when={
                              index() < LIVE_DOCK_PREVIEWS &&
                              d().kind !== "manage"
                            }
                          >
                            <div
                              style={{
                                position: "relative",
                                width: "100%",
                                height: `${Math.min(240, Math.max(120, Math.round(fontSize() * 12)))}px`,
                                overflow: "hidden",
                                "background-color": theme().bg,
                              }}
                            >
                              <Show
                                when={web}
                                fallback={
                                  <YasTile
                                    workspace={workspace}
                                    assignment={assignment}
                                    theme={theme()}
                                    palette={palette()}
                                    scale={chromeScale()}
                                    fontFamily={resolvedFontWithFallback()}
                                    fontSize={Math.max(
                                      7,
                                      Math.round(fontSize() * 0.6),
                                    )}
                                    onOpenTile={openTile}
                                    isConnectionReadOnly={isConnectionReadOnly}
                                    preview
                                  />
                                }
                              >
                                {(_) => (
                                  <WebPaneHost
                                    assignment={assignment}
                                    hostId={`dock:${assignment}`}
                                    register={webPaneHosts.register}
                                    interactive={false}
                                  />
                                )}
                              </Show>
                            </div>
                          </Show>
                        )}
                      />
                    );
                  }}
                </For>
              }
            />
          </Show>
        </section>
        <Show when={overlay() === "expose"}>
          {(_) => (
            <SwitcherOverlay
              initialQuery={switcherSeed()}
              sessions={sessions()}
              focusedSessionId={
                focusedSurfaceId() != null || mainTerminalParked()
                  ? null
                  : wsState().focusedSessionId
              }
              lru={lru}
              palette={palette()}
              fontFamily={resolvedFontWithFallback()}
              fontSize={fontSize()}
              onSelect={switchSession}
              onClose={closeOverlay}
              onCreate={(command, connectionId) => {
                const paneId = newTerminalTargetPaneId();
                if (paneId) {
                  void createInPane(paneId, command, connectionId);
                } else {
                  void createAndFocus(command, connectionId);
                }
              }}
              initialNewTerminalMode={openInNewTerminalMode()}
              activeLayout={activeLayout()}
              layoutAssignments={layoutAssignments()}
              onSelectPane={selectPane}
              focusedPaneId={activePaneId()}
              onMoveToPane={(sessionId, targetPaneId) => {
                moveSessionToPaneFn?.(sessionId, targetPaneId);
                workspace.focusSession(sessionId);
                // Null first: closeOverlay restores previousFocus on a
                // timeout — see selectPane.
                previousFocus = null;
                closeOverlay();
              }}
              onApplyLayout={(l) => {
                // Re-applying the already-active layout object (e.g. the
                // current preset from the switcher) is a no-op: the signal
                // setter below would not notify on the same reference, so
                // clearing layoutAssignments here would leave it null
                // forever — tile counts vanish from the status bar and the
                // side panel goes empty until reload.
                if (l === activeLayout()) {
                  closeOverlay();
                  return;
                }
                // Clear stale assignments immediately so session persistence
                // cannot record old pane IDs before LayoutContainer re-computes.
                setLayoutAssignments(null);
                // Clear any focused surface — a layout takes over the main
                // area so the surface overlay won't render, and leaving
                // focusedSurfaceId set would hide the surface from the
                // side panel as well (offScreenSurfaces filters it out).
                focusSurfaceById(null);
                // Same for a single-view tile: entering a layout hides the fullscreen
                // slot, so leaving activeTile set would count as "displayed"
                // and keep it out of the dock while nothing renders it.
                // Clearing hands it to the dock; the tab stays open.
                setActiveTile(null);
                setActiveLayout(l);
                saveActiveLayout(l);
                saveToHistory(l);
                setRecentLayouts(loadRecentLayouts());
                closeOverlay();
              }}
              onRemoveLayout={(dsl) => {
                removeFromHistory(dsl);
                setRecentLayouts(loadRecentLayouts());
              }}
              onClearLayout={() => {
                exitLayout();
                closeOverlay();
              }}
              recentLayouts={recentLayouts()}
              onOpenWeb={() => toggleOverlay("web")}
              onOpenSearch={() => {
                // Null first: closeOverlay restores previousFocus (the
                // terminal) on a timeout, which would steal the search
                // input's focus right back.
                previousFocus = null;
                closeOverlay();
                if (!searchOpen()) setSearchOpen(true);
                setSearchFocus((n) => n + 1);
              }}
              remotes={remotes()}
              remoteStatuses={remoteStatuses()}
              surfaces={surfaces()}
              connectionId={activeConnectionId()}
              connectionLabels={connectionLabels()}
              multiConnection={multiConnection()}
              focusedSurfaceId={focusedSurfaceId()}
              focusedSurfaceConnId={focusedSurfaceConnId()}
              hasAttention={hasAttention}
              onFocusSurface={focusSurface}
              onMoveSurfaceToPane={(sid, connId, targetPaneId) => {
                moveToPaneFn?.(surfaceAssignment(connId, sid), targetPaneId);
                focusSurfaceById(null);
                // Null first: closeOverlay restores previousFocus on a
                // timeout — see selectPane.
                previousFocus = null;
                closeOverlay();
              }}
              backgroundTiles={backgroundTiles()}
              onRestoreTile={(assignment) => {
                restoreTile(assignment);
                closeOverlay();
              }}
              onStartApplication={startAppFromSwitcher}
              workspaceSessionId={props.workspaceSession?.id}
              fileSearchLocal={(q) => {
                const s = activeSession();
                const root = s?.root() ?? "";
                if (!s || !root) return null;
                // A truncated list (giant tree) is still served — a best-
                // effort prefix beats nothing, and the budgets make it rare.
                const index = localFileIndex(workspace, s.connectionId, root);
                if (!index) return null;
                const recency = editorRecencySnapshot(s.connectionId);
                return searchFileIndex(index, q, 100, (rel) => {
                  return recency.get(`${root}/${rel}`) ?? null;
                });
              }}
              fileSearchWarm={() => {
                const s = activeSession();
                const root = s?.root() ?? "";
                if (s && root) localFileIndex(workspace, s.connectionId, root);
              }}
              onOpenFile={(relPath) => {
                const s = activeSession();
                if (!s) return;
                // "" when the session has no synced root yet.
                const a = s.fileAssignment(relPath);
                if (a) openTile(a);
              }}
              symbolSearchWarm={() => activeSession()?.ensureLsp()}
              symbolSearch={async (q) => {
                const s = activeSession();
                const h = s?.lspHandle();
                // An empty query asks most backends for everything; skip
                // it rather than pull the whole index over the wire.
                if (!h || !q) return [];
                const res = await h.workspaceSymbols(q);
                if (res.status !== LSP_STATUS_OK) return [];
                return res.records
                  .filter((r) => r.kind === "symbol")
                  .map((r) => ({
                    name: r.name,
                    symKind: r.symKind,
                    path: r.path,
                    line: r.line,
                    col: r.col,
                  }));
              }}
              onOpenSymbol={(hit) => {
                const s = activeSession();
                if (!s) return;
                // Symbol paths are relative to the LSP root, which is not
                // always the fs root — resolve against the attachment's
                // own root rather than through fileAssignment().
                const root = (s.lspHandle()?.root ?? s.root() ?? "").replace(
                  /\/+$/,
                  "",
                );
                const abs = hit.path.startsWith("/")
                  ? hit.path
                  : `${root}/${hit.path.replace(/^\/+/, "")}`;
                setReveal(s.connectionId, abs, {
                  text: "",
                  line: hit.line + 1, // LSP is 0-based, reveal is 1-based
                  col: hit.col,
                });
                openTile(editorAssignment(s.connectionId, abs));
              }}
            />
          )}
        </Show>
        <Show when={overlay() === "palette"}>
          {(_) => (
            <PaletteOverlay
              current={palette()}
              fontSize={fontSize()}
              onSelect={changePalette}
              onPreview={setPalette}
              onClose={closeOverlay}
            />
          )}
        </Show>
        <Show when={overlay() === "font"}>
          {(_) => (
            <FontOverlay
              currentFamily={font()}
              currentSize={fontSize()}
              currentGamma={textGamma()}
              serverFonts={serverFonts()}
              fontChoices={fontCatalog()}
              palette={palette()}
              fontSize={fontSize()}
              onSelect={changeFont}
              onPreview={(family, size, gamma) => {
                setFont(family);
                setFontSize(size);
                setTextGamma(gamma);
              }}
              onClose={closeOverlay}
            />
          )}
        </Show>
        <Show when={overlay() === "help"}>
          {(_) => (
            <HelpOverlay
              onClose={closeOverlay}
              palette={palette()}
              fontSize={fontSize()}
            />
          )}
        </Show>
        <Show when={overlay() === "link" && pendingLink()}>
          {(pending) => (
            <LinkOverlay
              palette={palette()}
              fontSize={fontSize()}
              assessment={pending().assessment}
              linkText={pending().text}
              onOpen={() => {
                const url = pending().assessment.raw;
                closeOverlay();
                window.open(url, "_blank", "noopener,noreferrer");
              }}
              onClose={closeOverlay}
            />
          )}
        </Show>
        <Show when={overlay() === "remotes" && shellCapabilities().remotes}>
          {(_) => (
            <RemotesOverlay
              remotes={remotes()}
              statuses={remoteStatuses()}
              palette={palette()}
              fontSize={fontSize()}
              activeRemotes={activeRemoteNames()}
              onSetSessionActive={setSessionRemoteActive}
              onReconnect={(name) => workspace.reconnectConnection(name)}
              stored={homeStoredRemotes()}
              onAddRemote={(name, uri) => {
                const connectionId = homeConnectionId();
                if (!connectionId) return;
                return storeAndActivateWorkspaceSessionRemote(
                  () => addStoredRemote(workspace, connectionId, name, uri),
                  setSessionRemoteActive,
                  name,
                );
              }}
              onRemoveRemote={(name) =>
                homeConnectionId()
                  ? removeStoredRemote(workspace, homeConnectionId()!, name)
                  : undefined
              }
              onToggleRemote={(name) =>
                homeConnectionId()
                  ? toggleStoredRemote(workspace, homeConnectionId()!, name)
                  : undefined
              }
              onClose={closeOverlay}
              connections={allConnections()}
              onManage={(name) => {
                // The panels are a tile, so the dialog that asked for them is
                // in the way once they exist.
                closeOverlay();
                openTile(manageAssignment(name));
              }}
            />
          )}
        </Show>
        <Show when={overlay() === "web"}>
          <WebOverlay
            locations={webLocations()}
            remotes={allConnections().map((c) => ({
              id: c.id,
              label: connectionLabels().get(c.id) ?? c.id,
            }))}
            dest={webDestId()}
            onDest={setWebDest}
            palette={{
              bg: theme().bg,
              fg: theme().fg,
              accent: theme().accent,
              dim: theme().border,
              selectedBg: theme().selectedBg,
              subtleBorder: theme().subtleBorder,
            }}
            fontSize={chromeScale().md}
            unavailable={webUnavailable()}
            onOpen={(url, dest) => openWebPane(url, dest)}
            onForget={persistWebLocations}
            onClose={() => setOverlay(null)}
          />
        </Show>
        <Show when={overlay() === "roots"}>
          {(_) => (
            <RootsOverlay
              roots={roots()}
              remotes={remotes()}
              palette={palette()}
              fontSize={fontSize()}
              workspace={workspace}
              connectionForRemote={connectionForRemote}
              defaultRemote={
                activeConnectionId() === defaultConnectionId()
                  ? ""
                  : activeConnectionId()
              }
              defaultPath={activeSession()?.root() ?? ""}
              onAdd={(name, remote, path) => {
                const connId = connectionForRemote(remote);
                addServerRoot(workspace, connId, name, path);
              }}
              onRemove={(name) => {
                const r = roots().find((x) => x.name === name);
                const connId = r && connectionForRemote(r.remote);
                if (connId) removeServerRoot(workspace, connId, name);
              }}
              onToggle={(name) => {
                const r = roots().find((x) => x.name === name);
                const connId = r && connectionForRemote(r.remote);
                if (connId) toggleServerRoot(workspace, connId, name);
              }}
              onReorder={(names) => {
                // A global drag-order splits into each server's subset,
                // preserving relative order within each KV document.
                const byConn = new Map<string, string[]>();
                for (const name of names) {
                  const r = roots().find((x) => x.name === name);
                  if (!r) continue;
                  const connId = connectionForRemote(r.remote);
                  const list = byConn.get(connId) ?? [];
                  list.push(name);
                  byConn.set(connId, list);
                }
                for (const [connId, subset] of byConn) {
                  reorderServerRoots(workspace, connId as ConnectionId, subset);
                }
              }}
              onClose={closeOverlay}
            />
          )}
        </Show>
        <Show when={overlay() === "media"}>
          {(_) => (
            <MediaOverlay
              palette={palette()}
              fontSize={fontSize()}
              audioBitrate={audioBitrate()}
              videoBandwidth={videoBandwidth()}
              videoSpeed={videoSpeed()}
              audioMuted={audioMuted()}
              audioAvailable={allConnections().some((c) => c.supportsAudio)}
              surfaceStreaming={surfaceStreaming()}
              surfaceSmoothing={surfaceSmoothing()}
              surfaceMaxFps={surfaceMaxFps()}
              surfaceZoom={surfaceZoom()}
              surfaceZoomMode={surfaceZoomMode()}
              surfaceTouchMode={surfaceTouchMode()}
              surfaceTouchAvailable={allConnections().some(
                (connection) => connection.supportsSurfaceTouch,
              )}
              waylandKeyboardRequests={waylandKeyboardRequests()}
              devices={mediaDevices}
              surfaceCodecs={surfaceCodecs()}
              probedSurfaceCodecs={probedSurfaceCodecs()}
              onSurfaceCodecsChange={changeSurfaceCodecs}
              onAudioBitrateChange={changeAudioBitrate}
              onVideoBandwidthChange={changeVideoBandwidth}
              onVideoSpeedChange={changeVideoSpeed}
              onSurfaceStreamingChange={changeSurfaceStreaming}
              onSurfaceSmoothingChange={changeSurfaceSmoothing}
              onSurfaceMaxFpsChange={changeSurfaceMaxFps}
              onSurfaceZoomChange={changeSurfaceZoom}
              onSurfaceZoomModeChange={changeSurfaceZoomMode}
              onSurfaceTouchModeChange={changeSurfaceTouchMode}
              onWaylandKeyboardRequestsChange={changeWaylandKeyboardRequests}
              onToggleAudio={toggleAudio}
              onClose={closeOverlay}
            />
          )}
        </Show>
        <footer
          style={{
            ...layout.statusBar,
            padding: showMobileToolbar()
              ? "0 1em"
              : "0 1em env(safe-area-inset-bottom)",
            "background-color": theme().bg,
            color: theme().fg,
            "border-top-color": theme().border,
            height: showMobileToolbar()
              ? `${statusBarHeight()}px`
              : `calc(${statusBarHeight()}px + env(safe-area-inset-bottom))`,
            "font-size": `${chromeScale().md}px`,
          }}
        >
          <StatusBar
            activities={activities()}
            paneActions={statusPaneActions()}
            sessions={sessions()}
            surfaceCount={surfaces().length}
            attentionCount={pendingAttention().size}
            // Displayed panes plus docked tabs: backgroundTiles already
            // excludes whatever a pane (or the single-view slot) displays, so
            // the two never double count — and a parked editor still shows
            // up in the tally, like off-screen terminals do.
            tileCount={
              paneKindCount(isTileAssignment) +
              backgroundTiles().filter(isTileAssignment).length
            }
            webCount={
              paneKindCount(isWebAssignment) +
              backgroundTiles().filter(isWebAssignment).length
            }
            hoveredLink={hoveredLink()}
            focusedSession={
              statusFocusedSurface() != null || mainTerminalParked()
                ? null
                : focusedSession()
            }
            focusedSurface={statusFocusedSurface()}
            focusedCwd={(() => {
              // Only when the reading is about the session the bar is
              // naming — the poll keeps its last value when a pty can't
              // answer, and a cwd from the previous terminal is worse
              // than none.
              const f = focusedTerm();
              const fid = focusedSessionId();
              return f && fid && f.sessionId === fid ? f.cwd : null;
            })()}
            connectionLabels={connectionLabels()}
            connections={allConnections()}
            status={connectionStatus()}
            onRemotes={
              shellCapabilities().remotes
                ? () => toggleOverlay("remotes")
                : undefined
            }
            onManageConnection={
              shellCapabilities().remotes
                ? undefined
                : (connectionId) => openTile(manageAssignment(connectionId))
            }
            onReconnectConnection={
              shellCapabilities().remotes
                ? undefined
                : (connectionId) => workspace.reconnectConnection(connectionId)
            }
            metrics={metrics()}
            palette={palette()}
            fontSize={fontSize()}
            fontFamily={resolvedFontWithFallback()}
            fontLoading={fontLoading()}
            debug={debugPanel()}
            toggleDebug={toggleDebug}
            previewPanelOpen={previewPanelState().enabled}
            onPreviewPanel={togglePreviewPanel}
            leftDockOpen={leftDockOpen()}
            onToggleLeftDock={toggleLeftDock}
            webPane={focusedWebPane()}
            debugStats={
              (debugTick(),
              workspace.getConnectionDebugStats(
                activeConnectionId(),
                wsState().focusedSessionId,
              ))
            }
            timeline={timeline}
            net={net}
            onSwitcher={() => toggleOverlay("expose")}
            onPalette={() => toggleOverlay("palette")}
            onFont={() => toggleOverlay("font")}
            audioMuted={audioMuted()}
            audioAvailable={allConnections().some((c) => c.supportsAudio)}
            isMobileTouch={isMobileTouch()}
            // The icon and the toggle agree: lit means a keyboard input panel
            // is genuinely up, including the iPadOS shortcut bar.
            keyboardOpen={keyboardWanted() && viewportOccluded()}
            onToggleKeyboard={toggleMobileKeyboard}
            onMedia={() => toggleOverlay("media")}
            desktopChrome={(compact) => (
              <DesktopChrome
                workspace={workspace}
                connections={allConnections()}
                connectionLabels={connectionLabels()}
                readOnlyConnections={readOnlyConnections()}
                theme={theme()}
                scale={chromeScale()}
                compact={compact}
                focusedConnectionId={
                  focusedSurfaceConnId() ?? activeConnectionId()
                }
                onRaisePlayer={raiseMediaPlayer}
              />
            )}
          />
        </footer>
        <Show when={showMobileToolbar()}>
          <MobileToolbar
            keyboardTarget={() => {
              // Subscribe the lookup to pane/session focus. The target itself
              // is DOM-owned, but the toolbar must re-bind its modifier state
              // when focus moves while the software keyboard stays open.
              wsState().focusedSessionId;
              focusedSurfaceId();
              layoutFocusedSurface();
              return focusedKeyboardInput();
            }}
            theme={theme()}
            scale={chromeScale()}
          />
        </Show>
      </main>
    </YasWorkspaceProvider>
  );
}

function PreviewPanel(props: {
  offScreenSessions: readonly YasSession[];
  /** Includes displayed terminals so their parked surfaces still have an
   *  ownership parent in the Muster hierarchy. */
  allSessions: readonly YasSession[];
  surfaces: readonly YasSurface[];
  focusedSurfaceId: SurfaceId | null;
  focusedSurfaceConnId: ConnectionId | null;
  /** Is this pane assignment currently lit by an activation? */
  hasAttention: (assignment: string) => boolean;
  connectionId: string;
  connectionLabels?: Map<string, string>;
  theme: Theme;
  scale: UIScale;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
  isMobileTouch: boolean;
  onFocusSession: (id: SessionId) => void;
  onFocusSurface: (connectionId: ConnectionId, surfaceId: SurfaceId) => void;
  onCloseSession: (id: SessionId) => void;
  onCloseSurface: (connectionId: ConnectionId, surfaceId: SurfaceId) => void;
  width: number;
  onResize: (width: number) => void;
  onClose: () => void;
  musterExpanded: boolean;
  expandedMusterStacks: ReadonlySet<string>;
  onToggleMuster: () => void;
  onToggleMusterStack: (key: string) => void;
  /** Live background-editor cards (rendered by WorkspaceScreen, which owns the
   *  tile assignments), shown above the terminal/surface thumbnails. */
  backgroundEditors?: JSX.Element;
  /** A grip drag is in flight: this panel is its drop-to-park target. */
  parkDropActive?: boolean;
  /** A grip drag landed here; park `assignment`, emptying `source`. */
  onParkDrop?: (assignment: string, source: string) => void;
  /** A grip drag landed on a non-surface parked card; group both as tabs. */
  onTabDrop?: (
    parkedAssignment: string,
    draggedAssignment: string,
    source: string,
  ) => void;
}) {
  const [expandedId, setExpandedId] = createSignal<number | null>(null);
  const [resizeHover, setResizeHover] = createSignal(false);
  const [resizeActive, setResizeActive] = createSignal(false);
  /** The grip drag is hovering the panel (parallel to a pane's highlight). */
  const [parkOver, setParkOver] = createSignal(false);
  const [tabDropActive, setTabDropActive] = createSignal(false);
  let markedTabTarget: HTMLElement | null = null;
  const cardAt = (event: DragEvent): HTMLElement | null => {
    const origin = event.target;
    if (!(origin instanceof Element)) return null;
    const card = origin.closest<HTMLElement>("[data-yas-preview-assignment]");
    const panel = event.currentTarget;
    return card && panel instanceof HTMLElement && panel.contains(card)
      ? card
      : null;
  };
  const tabTargetAt = (event: DragEvent): HTMLElement | null => {
    const card = cardAt(event);
    const assignment = card?.dataset.yasPreviewAssignment;
    return assignment && isParkedTabDropTarget(assignment) ? card : null;
  };
  const markTabTarget = (target: HTMLElement | null) => {
    if (markedTabTarget === target) return;
    if (markedTabTarget) {
      markedTabTarget.style.removeProperty("outline");
      markedTabTarget.style.removeProperty("outline-offset");
    }
    markedTabTarget = target;
    if (target) {
      target.style.setProperty("outline", `2px solid ${props.theme.accent}`);
      target.style.setProperty("outline-offset", "-2px");
    }
    setTabDropActive(target != null);
  };
  onCleanup(() => markTabTarget(null));
  const resources = createMemo(() =>
    groupMusterPreviewResources(
      props.offScreenSessions,
      props.allSessions,
      props.surfaces,
    ),
  );
  const musterStackExpanded = (connectionId: string, instance: string | null) =>
    props.expandedMusterStacks.has(musterStackKey(connectionId, instance));

  function handleResizePointerDown(e: PointerEvent) {
    e.preventDefault();
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    setResizeActive(true);
    const startX = e.clientX;
    const startWidth = props.width;
    // Cap the panel at a fraction of the viewport so a touch drag can't
    // push the terminal off-screen.
    const maxWidth = Math.max(
      MIN_PREVIEW_PANEL_WIDTH,
      Math.floor(window.innerWidth * 0.85),
    );

    const onMove = (me: PointerEvent) => {
      const delta = startX - me.clientX;
      props.onResize(
        Math.min(
          maxWidth,
          Math.max(MIN_PREVIEW_PANEL_WIDTH, startWidth + delta),
        ),
      );
    };

    const onUp = () => {
      setResizeActive(false);
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
    };

    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }

  // Touch targets need a fatter hit area than the 3px desktop bar to be
  // reliably grabbable with a finger.
  const handleWidth = () => (props.isMobileTouch ? 14 : 3);

  const resizeBg = () =>
    resizeActive()
      ? "rgba(128,128,128,0.5)"
      : resizeHover()
        ? "rgba(128,128,128,0.3)"
        : "transparent";

  return (
    <div
      // Named the way a pane is (`data-yas-pane-id`): a parked card is
      // draggable, and so is every explorer row and commit, so "the parked
      // cards" is only expressible as a subtree.
      data-yas-preview-panel=""
      style={{
        width: `${props.width}px`,
        "flex-shrink": 0,
        display: "flex",
        "flex-direction": "row",
        overflow: "hidden",
        position: "relative",
      }}
      onDragOver={(e) => {
        if (!props.onParkDrop || !isPaneDrag(e)) return;
        const card = props.onTabDrop ? tabTargetAt(e) : null;
        if (card?.dataset.yasPreviewAssignment) {
          e.preventDefault();
          e.dataTransfer!.dropEffect = "move";
          setParkOver(false);
          markTabTarget(card);
          return;
        }
        markTabTarget(null);
        e.preventDefault(); // allow the drop
        e.dataTransfer!.dropEffect = "move";
        if (!parkOver()) setParkOver(true);
      }}
      onDragLeave={(e) => {
        // Ignore leaves into child elements; only clear when truly leaving.
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
          setParkOver(false);
          markTabTarget(null);
        }
      }}
      onDrop={(e) => {
        setParkOver(false);
        const card = props.onTabDrop ? tabTargetAt(e) : null;
        const parkedAssignment = card?.dataset.yasPreviewAssignment;
        const assignment = tileDragAssignment(e);
        const source = paneDragSource(e);
        markTabTarget(null);
        if (parkedAssignment && assignment && source && props.onTabDrop) {
          e.preventDefault();
          e.stopPropagation();
          props.onTabDrop(parkedAssignment, assignment, source);
          return;
        }
        if (assignment && source && props.onParkDrop) {
          e.preventDefault();
          props.onParkDrop(assignment, source);
        }
      }}
    >
      <Show when={props.parkDropActive && !tabDropActive()}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            "z-index": 5,
            "pointer-events": "none",
            "box-sizing": "border-box",
            border: parkOver()
              ? `2px solid ${props.theme.accent}`
              : `2px dashed ${props.theme.subtleBorder}`,
            background: parkOver()
              ? `color-mix(in srgb, ${props.theme.accent} 14%, transparent)`
              : "transparent",
          }}
        />
      </Show>
      <div
        onPointerDown={handleResizePointerDown}
        onPointerEnter={() => setResizeHover(true)}
        onPointerLeave={() => setResizeHover(false)}
        role="separator"
        aria-orientation="vertical"
        aria-label={t("workspace.resizePanel")}
        style={{
          width: `${handleWidth()}px`,
          "flex-shrink": 0,
          cursor: "col-resize",
          background: resizeBg(),
          "border-left": `1px solid ${props.theme.subtleBorder}`,
          transition: "background 0.1s",
          "touch-action": "none",
          display: "flex",
          "align-items": "center",
          "justify-content": "center",
        }}
      >
        <Show when={props.isMobileTouch}>
          <div
            style={{
              width: "3px",
              height: "32px",
              "border-radius": "2px",
              "background-color": props.theme.dimFg,
              opacity: resizeActive() ? 0.8 : 0.4,
              "pointer-events": "none",
            }}
          />
        </Show>
      </div>
      <div
        style={{
          flex: 1,
          // A surface canvas has a large intrinsic pixel width. Let this flex
          // item shrink below that min-content size so thumbnails use the
          // panel's width instead of being clipped at their native width.
          "min-width": 0,
          "background-color": props.theme.bg,
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            "align-items": "center",
            "justify-content": "flex-end",
            padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
            "border-bottom": `1px solid ${props.theme.subtleBorder}`,
          }}
        >
          <button
            onClick={props.onClose}
            title={tp("workspace.closePanelShortcut", {
              shortcut: "Ctrl+B r",
            })}
            style={mergeStyle(ui.btn, {
              "font-size": `${props.scale.xs}px`,
              padding: `0 ${props.scale.tightGap}px`,
              opacity: 0.5,
            })}
          >
            {"\u00D7"}
          </button>
        </div>
        <div
          style={{
            flex: "1 1 0",
            "min-height": 0,
            "overflow-y": "auto",
            display: "flex",
            "flex-direction": "column",
            ...scrollbarStyle(props.theme),
          }}
        >
          {props.backgroundEditors}
          <Index each={resources().sessions}>
            {(s) => (
              <SessionThumbnail
                session={s()}
                connectionLabel={props.connectionLabels?.get(s().connectionId)}
                theme={props.theme}
                scale={props.scale}
                palette={props.palette}
                fontFamily={props.fontFamily}
                fontSize={props.fontSize}
                isMobileTouch={props.isMobileTouch}
                onFocus={() => props.onFocusSession(s().id)}
                onClose={() => props.onCloseSession(s().id)}
              />
            )}
          </Index>
          <Index each={resources().surfaces}>
            {(s) => (
              <SurfaceThumbnail
                surface={s()}
                connectionId={s().connectionId}
                connectionLabel={props.connectionLabels?.get(s().connectionId)}
                theme={props.theme}
                scale={props.scale}
                focused={
                  s().surfaceId === props.focusedSurfaceId &&
                  s().connectionId === props.focusedSurfaceConnId
                }
                attention={props.hasAttention(
                  surfaceAssignment(s().connectionId, s().surfaceId),
                )}
                isMobileTouch={props.isMobileTouch}
                onFocus={() =>
                  props.onFocusSurface(s().connectionId, s().surfaceId)
                }
                onClose={() =>
                  props.onCloseSurface(s().connectionId, s().surfaceId)
                }
              />
            )}
          </Index>
          <Show when={resources().muster.length > 0}>
            <div
              data-yas-muster-preview=""
              style={{
                "margin-top": "auto",
                "border-top": `1px solid ${props.theme.subtleBorder}`,
              }}
            >
              <button
                type="button"
                data-yas-muster-toggle=""
                aria-expanded={props.musterExpanded}
                aria-controls="yas-muster-preview-body"
                onClick={props.onToggleMuster}
                style={mergeStyle(ui.btn, {
                  width: "100%",
                  display: "flex",
                  "align-items": "center",
                  gap: `${props.scale.tightGap}px`,
                  padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
                  color: props.theme.dimFg,
                  "font-size": `${props.scale.xs}px`,
                  "font-weight": "bold",
                  "letter-spacing": "0.08em",
                  "text-transform": "uppercase",
                  "text-align": "left",
                  opacity: 1,
                })}
              >
                <span aria-hidden="true">
                  {props.musterExpanded ? "▾" : "▸"}
                </span>
                {t("muster.title")}
              </button>
              <div
                id="yas-muster-preview-body"
                data-yas-muster-body=""
                hidden={!props.musterExpanded}
              >
                <Show when={props.musterExpanded}>
                  <Index each={resources().muster}>
                    {(instance) => (
                      <section
                        data-yas-muster-instance={
                          instance().instance ?? "standalone"
                        }
                        data-yas-muster-connection={instance().connectionId}
                      >
                        <button
                          type="button"
                          data-yas-muster-stack-toggle=""
                          aria-expanded={musterStackExpanded(
                            instance().connectionId,
                            instance().instance,
                          )}
                          onClick={() =>
                            props.onToggleMusterStack(
                              musterStackKey(
                                instance().connectionId,
                                instance().instance,
                              ),
                            )
                          }
                          style={mergeStyle(ui.btn, {
                            width: "100%",
                            display: "flex",
                            "align-items": "center",
                            gap: `${props.scale.tightGap}px`,
                            padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
                            "border-top": `1px solid ${props.theme.subtleBorder}`,
                            color: props.theme.fg,
                            "font-size": `${props.scale.sm}px`,
                            "font-weight": 600,
                            "text-align": "left",
                            opacity: 1,
                          })}
                        >
                          <span aria-hidden="true">
                            {musterStackExpanded(
                              instance().connectionId,
                              instance().instance,
                            )
                              ? "▾"
                              : "▸"}
                          </span>
                          <span
                            style={{
                              flex: 1,
                              overflow: "hidden",
                              "text-overflow": "ellipsis",
                              "white-space": "nowrap",
                            }}
                          >
                            {instance().instance ?? t("muster.standalone")}
                          </span>
                        </button>
                        <div
                          data-yas-muster-stack-body=""
                          hidden={
                            !musterStackExpanded(
                              instance().connectionId,
                              instance().instance,
                            )
                          }
                        >
                          <Show
                            when={musterStackExpanded(
                              instance().connectionId,
                              instance().instance,
                            )}
                          >
                            <div
                              style={{
                                "margin-left": `${props.scale.gap}px`,
                                "border-left": `1px solid ${props.theme.subtleBorder}`,
                              }}
                            >
                              <Index each={instance().units}>
                                {(unit) => (
                                  <section data-yas-muster-unit={unit().name}>
                                    <div
                                      style={{
                                        padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
                                        "border-top": `1px solid ${props.theme.subtleBorder}`,
                                        color: props.theme.dimFg,
                                        "font-size": `${props.scale.xs}px`,
                                        "font-weight": 600,
                                        overflow: "hidden",
                                        "text-overflow": "ellipsis",
                                        "white-space": "nowrap",
                                      }}
                                    >
                                      {unit().name}
                                    </div>
                                    <div
                                      style={{
                                        "margin-left": `${props.scale.gap}px`,
                                        "border-left": `1px solid ${props.theme.subtleBorder}`,
                                      }}
                                    >
                                      <Index each={unit().runs}>
                                        {(run) => (
                                          <div
                                            data-yas-muster-terminal={
                                              run().session.id
                                            }
                                            data-yas-muster-tag={
                                              run().session.tag
                                            }
                                          >
                                            <Show
                                              when={
                                                !run().isSequence ||
                                                !run().showTerminal
                                              }
                                            >
                                              <div
                                                style={{
                                                  display: "flex",
                                                  "align-items": "center",
                                                  gap: `${props.scale.tightGap}px`,
                                                  padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
                                                  "border-top": `1px solid ${props.theme.subtleBorder}`,
                                                  "font-size": `${props.scale.xs}px`,
                                                }}
                                              >
                                                <span
                                                  style={{
                                                    flex: 1,
                                                    overflow: "hidden",
                                                    "text-overflow": "ellipsis",
                                                    "white-space": "nowrap",
                                                  }}
                                                >
                                                  {run().isSequence
                                                    ? `#${run().label}`
                                                    : run().label}
                                                </span>
                                                <Show
                                                  when={!run().showTerminal}
                                                >
                                                  <span
                                                    style={{
                                                      color: props.theme.dimFg,
                                                    }}
                                                  >
                                                    on screen
                                                  </span>
                                                </Show>
                                              </div>
                                            </Show>
                                            <Show when={run().showTerminal}>
                                              <SessionThumbnail
                                                session={run().session}
                                                titlePrefix={
                                                  run().isSequence
                                                    ? `#${run().label}`
                                                    : undefined
                                                }
                                                connectionLabel={props.connectionLabels?.get(
                                                  run().session.connectionId,
                                                )}
                                                theme={props.theme}
                                                scale={props.scale}
                                                palette={props.palette}
                                                fontFamily={props.fontFamily}
                                                fontSize={props.fontSize}
                                                isMobileTouch={
                                                  props.isMobileTouch
                                                }
                                                onFocus={() =>
                                                  props.onFocusSession(
                                                    run().session.id,
                                                  )
                                                }
                                                onClose={() =>
                                                  props.onCloseSession(
                                                    run().session.id,
                                                  )
                                                }
                                              />
                                            </Show>
                                            <Show
                                              when={run().surfaces.length > 0}
                                            >
                                              <div
                                                data-yas-muster-surfaces=""
                                                style={{
                                                  "margin-left": `${props.scale.gap}px`,
                                                  "border-left": `1px solid ${props.theme.subtleBorder}`,
                                                }}
                                              >
                                                <Index each={run().surfaces}>
                                                  {(s) => (
                                                    <SurfaceThumbnail
                                                      surface={s()}
                                                      displayHandle={displayHandle(
                                                        s().surfaceId,
                                                      )}
                                                      connectionId={
                                                        s().connectionId
                                                      }
                                                      connectionLabel={props.connectionLabels?.get(
                                                        s().connectionId,
                                                      )}
                                                      theme={props.theme}
                                                      scale={props.scale}
                                                      focused={
                                                        s().surfaceId ===
                                                          props.focusedSurfaceId &&
                                                        s().connectionId ===
                                                          props.focusedSurfaceConnId
                                                      }
                                                      attention={props.hasAttention(
                                                        surfaceAssignment(
                                                          s().connectionId,
                                                          s().surfaceId,
                                                        ),
                                                      )}
                                                      isMobileTouch={
                                                        props.isMobileTouch
                                                      }
                                                      onFocus={() =>
                                                        props.onFocusSurface(
                                                          s().connectionId,
                                                          s().surfaceId,
                                                        )
                                                      }
                                                      onClose={() =>
                                                        props.onCloseSurface(
                                                          s().connectionId,
                                                          s().surfaceId,
                                                        )
                                                      }
                                                    />
                                                  )}
                                                </Index>
                                              </div>
                                            </Show>
                                          </div>
                                        )}
                                      </Index>
                                    </div>
                                  </section>
                                )}
                              </Index>
                            </div>
                          </Show>
                        </div>
                      </section>
                    )}
                  </Index>
                </Show>
              </div>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}

/** Default horizontal swipe distance (px) to trigger dismiss. */
const SWIPE_THRESHOLD = 60;
/** Never ask for more than this fraction of a narrow card's width, so a
 *  small side panel is still swipeable. */
const SWIPE_THRESHOLD_CARD_FRACTION = 0.5;
/** Floor for the dynamic threshold; below this a tap would be indistinguishable. */
const MIN_SWIPE_THRESHOLD = 24;
/** Minimum ratio of horizontal to vertical movement for a swipe. */
const SWIPE_RATIO = 1.5;

/** Shared wrapper for preview-panel thumbnails.  Handles swipe-right-to-
 *  dismiss (swipe-left starts a drag, via the touch-drag bridge), hover
 *  state, dismiss animation, header bar with close button. */
function Thumbnail(props: {
  theme: Theme;
  scale: UIScale;
  isMobileTouch: boolean;
  /** The pane assignment this card carries when dragged onto a pane —
   *  a session id for a terminal, `surfaceAssignment(...)` for a surface,
   *  a tile assignment (`editor:`/…) for a background editor. */
  assignment: string;
  onFocus: () => void;
  onClose: () => void;
  closeTitle: string;
  /** Extra header-bar background (e.g. for focused highlight). */
  headerBg?: string;
  /** Pulse the header: this card's content asked to come forward. */
  attention?: boolean;
  /** Inline elements rendered inside the header button. */
  header: () => any;
  /** Body content (terminal preview, surface view, etc.). */
  body: () => any;
}) {
  // Session/surface catalogue refreshes replace the containing record even
  // when this card still represents the same assignment. Collapse those
  // updates to the stable string so they cannot reset a gesture in progress.
  const assignment = createMemo(() => props.assignment);
  const [hover, setHover] = createSignal(false);
  const [swipeX, setSwipeX] = createSignal(0);
  const [swiping, setSwiping] = createSignal(false);
  const [dismissed, setDismissed] = createSignal(false);
  let touchStartX = 0;
  let touchStartY = 0;
  let locked = false;
  let swipeThreshold = SWIPE_THRESHOLD;

  // <Index> reuses this component instance when the list shifts, so stale
  // dismiss state from the previous occupant would hide the new card.
  createEffect(() => {
    assignment(); // track the occupant, not same-occupant record refreshes
    setDismissed(false);
    setSwipeX(0);
    setSwiping(false);
    locked = false;
    swipeThreshold = SWIPE_THRESHOLD;
  });

  function onTouchStart(e: TouchEvent) {
    const t = e.touches[0];
    touchStartX = t.clientX;
    touchStartY = t.clientY;
    locked = false;
    setSwiping(false);
    setSwipeX(0);
    // Narrow cards (small side panel) need a smaller threshold so the swipe
    // does not have to cross the whole panel width.
    const cardWidth = (e.currentTarget as HTMLElement).getBoundingClientRect()
      .width;
    swipeThreshold = Math.min(
      SWIPE_THRESHOLD,
      Math.max(MIN_SWIPE_THRESHOLD, cardWidth * SWIPE_THRESHOLD_CARD_FRACTION),
    );
  }

  function onTouchMove(e: TouchEvent) {
    const t = e.touches[0];
    const dx = t.clientX - touchStartX;
    const dy = t.clientY - touchStartY;
    if (!locked) {
      if (Math.abs(dx) < 8 && Math.abs(dy) < 8) return;
      locked = true;
      // Only a rightward, horizontal-dominant swipe dismisses. A leftward
      // one is the touch-drag bridge's gesture (see onPointerDown), and a
      // vertical one is the list's scroll: neither is claimed here.
      if (dx <= 0 || dx < Math.abs(dy) * SWIPE_RATIO) return;
      setSwiping(true);
    }
    if (!swiping()) return;
    e.preventDefault();
    if (dx <= 0) {
      // Reversed through the origin: the gesture is now a leftward drag,
      // which the touch-drag bridge claims. Hand the card back to rest and
      // stay out of the way for the rest of the gesture.
      setSwiping(false);
      setSwipeX(0);
      return;
    }
    setSwipeX(dx);
  }

  function finishSwipe(e: TouchEvent) {
    const completed = swiping() && swipeX() >= swipeThreshold;
    setSwiping(false);
    if (completed) {
      // Cancel the synthetic click that follows a swipe so it cannot also
      // trigger the card's own close/focus buttons (or another card's).
      e.preventDefault();
      setDismissed(true);
      setSwipeX(400);
      // Own the close at the gesture boundary. A connection/catalogue refresh
      // may transiently unmount this card; deferring ownership to a component
      // timer lets cleanup cancel a completed swipe before it reaches the
      // session/surface owner. The hidden state still supplies the animation
      // while the close settles.
      props.onClose();
    } else {
      setSwipeX(0);
    }
  }

  function cancelSwipe() {
    setSwipeX(0);
    setSwiping(false);
  }

  return (
    // Draggable onto a pane, like the background-tile cards above: the
    // card is inert (see the body wrapper), so the whole thing is the handle
    // and a drag can't be swallowed by the terminal or surface inside.
    // Touch is unaffected — mobile browsers don't synthesize dragstart, so
    // swipe-to-dismiss below keeps working.
    <div
      data-yas-preview-assignment={props.assignment}
      draggable={true}
      onDragStart={(e) => startTileDrag(e, props.assignment)}
      // Touch never reaches onDragStart. A leftward swipe starts the drag —
      // the one horizontal gesture the swipe-to-dismiss below does not claim
      // (it claims rightward) — and a hold works as a fallback. Either way
      // the card can still be flicked away to the right.
      onPointerDown={(e) =>
        startTouchDrag(
          e,
          (dt) => fillTileDrag(dt, props.assignment),
          "swipe-left",
        )
      }
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onTouchStart={onTouchStart}
      onTouchMove={onTouchMove}
      onTouchEnd={finishSwipe}
      onTouchCancel={cancelSwipe}
      style={{
        "border-bottom": `1px solid ${props.theme.subtleBorder}`,
        display: dismissed() ? "none" : "flex",
        "flex-direction": "column",
        "flex-shrink": 0,
        width: "100%",
        "box-sizing": "border-box",
        overflow: "hidden",
        position: "relative",
        transform: `translateX(${swipeX()}px)`,
        opacity: swiping()
          ? Math.max(0, 1 - Math.abs(swipeX()) / 200)
          : dismissed()
            ? 0
            : 1,
        transition: swiping() ? "none" : "transform 0.2s, opacity 0.2s",
        "touch-action": "pan-y",
      }}
    >
      <button
        onClick={props.onFocus}
        style={mergeStyle(ui.btn, {
          display: "flex",
          "align-items": "center",
          gap: `${props.scale.tightGap}px`,
          padding: `${props.scale.controlY}px ${props.scale.tightGap}px`,
          "font-size": `${props.scale.sm}px`,
          width: "100%",
          "text-align": "left",
          opacity: 1,
          "flex-shrink": 0,
          "background-color": props.headerBg ?? "transparent",
          // Still asking: the title goes red and stays red until the window is
          // looked at. Ink rather than fill, matching the surface count and the
          // switcher's mark — red is what a mark is written in here, never what
          // it is painted on.
          ...(props.attention
            ? { color: props.theme.errorText, "font-weight": "bold" }
            : {}),
        })}
      >
        {props.header()}
        <Show when={!props.isMobileTouch && hover()}>
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onClose();
            }}
            title={props.closeTitle}
            style={mergeStyle(ui.btn, {
              "font-size": `${props.scale.sm}px`,
              padding: `0 ${props.scale.tightGap}px`,
              opacity: 0.6,
              "flex-shrink": 0,
            })}
          >
            {"\u00D7"}
          </button>
        </Show>
      </button>
      <div
        style={{ overflow: "hidden", cursor: "pointer" }}
        onClick={props.onFocus}
      >
        {/* Parked content is inert, matching the background-tile cards.
            `inert` takes the subtree out of hit-testing *and* the tab order:
            a read-only YasTerminal still attaches a keydown listener on a
            tabindex=0 input (scroll keys work), and a preview
            YasSurfaceView's canvas is tabindex=0 too — so without this a
            parked card can take focus away from the live view. The explicit
            pointer-events keeps the click landing on the parent (restore),
            rather than relying on how each engine hit-tests inert. */}
        <div inert style={{ "pointer-events": "none" }}>
          {props.body()}
        </div>
      </div>
    </div>
  );
}

function SessionThumbnail(props: {
  session: YasSession;
  titlePrefix?: string;
  connectionLabel?: string;
  theme: Theme;
  scale: UIScale;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
  isMobileTouch: boolean;
  onFocus: () => void;
  onClose: () => void;
}) {
  return (
    <Thumbnail
      theme={props.theme}
      scale={props.scale}
      isMobileTouch={props.isMobileTouch}
      // A terminal's pane assignment is its bare session id.
      assignment={props.session.id}
      onFocus={props.onFocus}
      onClose={props.onClose}
      closeTitle={t("workspace.closeTerminal")}
      header={() => (
        <>
          <span
            style={{
              flex: 1,
              overflow: "hidden",
              "text-overflow": "ellipsis",
              "white-space": "nowrap",
            }}
          >
            <Show when={props.titlePrefix}>
              <span style={{ "font-weight": 600 }}>{props.titlePrefix}</span>
              {" \u203A "}
            </Show>
            <span style={{ opacity: 0.5 }}>
              {sessionPrefix(props.session, props.connectionLabel)}
            </span>
            {" \u203A "}
            {sessionName(props.session)}
          </span>
          <Show when={props.session.state === "exited"}>
            <mark
              style={{
                ...ui.badge,
                "background-color": "rgba(255,100,100,0.3)",
                "font-size": `${props.scale.xs}px`,
              }}
            >
              exited
            </mark>
          </Show>
        </>
      )}
      body={() => (
        <YasTerminal
          sessionId={props.session.id}
          readOnly
          resizable={false}
          fitWidth
          showCursor={false}
          style={{ width: "100%", height: "auto" }}
          fontFamily={props.fontFamily}
          fontSize={props.fontSize}
          palette={props.palette}
        />
      )}
    />
  );
}

function SurfaceThumbnail(props: {
  surface: YasSurface;
  /** Muster uses its canonical fixed-width, prefix-free handle here. */
  displayHandle?: string;
  connectionId: string;
  connectionLabel?: string;
  theme: Theme;
  scale: UIScale;
  focused: boolean;
  attention?: boolean;
  isMobileTouch: boolean;
  onFocus: () => void;
  onClose: () => void;
}) {
  return (
    <Thumbnail
      theme={props.theme}
      scale={props.scale}
      isMobileTouch={props.isMobileTouch}
      assignment={surfaceAssignment(
        props.surface.connectionId,
        props.surface.surfaceId,
      )}
      onFocus={props.onFocus}
      onClose={props.onClose}
      closeTitle={t("workspace.closeSurface")}
      headerBg={props.focused ? props.theme.selectedBg : undefined}
      attention={props.attention}
      header={() => (
        <>
          <span
            style={{
              flex: 1,
              overflow: "hidden",
              "text-overflow": "ellipsis",
              "white-space": "nowrap",
            }}
          >
            {/* `dev:S3 \u203A Slack`. The id is the only thing that names a
                window unambiguously \u2014 titles repeat across an app's windows
                and change under you \u2014 and it is what `yas surface` takes,
                so the card doubles as the lookup for driving that window
                from a terminal. */}
            <span style={{ opacity: 0.5 }}>
              {props.connectionLabel ? `${props.connectionLabel}:` : ""}
              {props.displayHandle ?? `S${props.surface.surfaceId}`}
            </span>
            {" \u203A "}
            {props.surface.title ||
              props.surface.appId ||
              tp("workspace.surfaceFallback", {
                id: String(props.displayHandle ?? props.surface.surfaceId),
              })}
          </span>
        </>
      )}
      body={() => (
        <YasSurfaceView
          connectionId={props.surface.connectionId}
          surfaceId={props.surface.surfaceId}
          // A parked surface has no foreground view to populate the shared
          // frame cache. Keep a thumbnail subscription of its own; passive
          // views advertise a scaled target capped at thumbnail cadence and
          // are excluded from surface size mediation, so this cannot resize
          // Brave or pin a later foreground handoff to the card's dimensions.
          resizable={false}
          style={{
            display: "block",
            width: "100%",
            // The window's own aspect, *not* the canvas's. A card whose height
            // came from the canvas closed a loop: the height is what
            // YasSurfaceCanvas measures into `_presentBox` to pick an encode
            // size, and the encode size is what sizes the canvas. So a card
            // whose height landed on an octave boundary (≈2^k/aspect: 113,
            // 227, 455 px wide at 16:9) asked for 256x128, got a 128-tall
            // stream, grew to 128.6, asked for 256x256, got a 144-tall
            // stream, shrank to 127.7, and repeated at ~16Hz. Each turn
            // retires and rebuilds a hardware encoder; a few hundred of those
            // segfaults the NVIDIA encode library and takes the server down.
            //
            // And the window's *logical* aspect, not the composited one. The
            // composite is the logical size times whatever scale the
            // highest-DPI viewer asked for, floored onto the even 4:2:0 grid,
            // so its ratio is off by up to a pixel per axis — and it moves
            // when another viewer's DPI does, for a window that never
            // changed. `logicalWidth`/`logicalHeight` move only when the app
            // resizes, which is the only thing this card should follow.
            //
            // Before the first surface info every dimension is 0; leave the
            // ratio off rather than emit a degenerate one, and the card is
            // laid out by the 640x480 placeholder canvas for that one frame.
            // A server too old to report a logical size falls back to the
            // composite, which is what it used to use throughout.
            ...cardAspectRatio(props.surface),
            height: "auto",
            "object-fit": "contain",
          }}
        />
      )}
    />
  );
}
