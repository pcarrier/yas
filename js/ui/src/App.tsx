import { TapButton } from "./TapButton";
import {
  createSignal,
  createEffect,
  createMemo,
  ErrorBoundary,
  onCleanup,
  Show,
} from "solid-js";
import {
  YAS_WEBSOCKET_SUBPROTOCOL,
  YasConnection,
  YasEdgeWebSocketTransport,
  YasNativeRelayTransport,
  YasNativeWorkspaceConnection,
  YasRelayClient,
  WorkspaceSessionDeviceStore,
  WorkspaceSessionStore,
  yasBrowserConnectionOptions,
} from "@yas-run/core";
import { YasWebTransportTransport } from "@yas-run/core/transports";
import type {
  YasTransport,
  YasWasmModule,
  YasWorkspace,
  YasWorkspaceConnection,
  YasRelayRoute,
} from "@yas-run/core";
import { YasMark } from "./Logo";
import { themeFor } from "./theme";
import { t } from "./i18n";
import { Workspace } from "./Workspace";
import { PASSPHRASE_KEY } from "./passphrase-storage";
import { preferredPalette } from "./storage";
import {
  cancelHmrRelease,
  claimHmrLease,
  createHmrConnectionSlot,
  deferHmrRelease,
  type HmrLeaseState,
} from "./hmrLease";
import {
  boundedRelayRoutes,
  RelayConnectionCache,
} from "./relayTransportCache";
import { installPreviewNetBroker } from "./previewNetProtocol";
import { consumePassphraseFromHash } from "./workspaceSessionUrl";
import { createWorkspaceSessionController } from "./workspaceSession";
import { reconcileWorkspaceSessionRelayConnections } from "./workspaceSessionRemotes";
import { getOrCreateWorkspaceSessionDeviceId } from "./workspaceSessionDevice";
import {
  discoverEdgeWebTransport,
  fetchEdgeCertificateHash,
  type EdgeWebTransportConfig,
} from "./edgeWebTransport";

function readPassphrase(): string | null {
  let stored: string | null = null;
  try {
    stored = localStorage.getItem(PASSPHRASE_KEY);
  } catch {}

  const consumed = consumePassphraseFromHash(location.hash);
  if (!consumed.found) return stored;

  // First contact — secret is being delivered via the URL fragment. Move it
  // to localStorage and strip it from the URL so it does not end up in
  // browser history or get re-shared accidentally.
  const newHash = consumed.hash;
  const newUrl =
    location.pathname + location.search + (newHash ? `#${newHash}` : "");
  history.replaceState(null, "", newUrl);
  if (consumed.passphrase) {
    try {
      localStorage.setItem(PASSPHRASE_KEY, consumed.passphrase);
    } catch {}
    return consumed.passphrase;
  }
  return stored;
}

readPassphrase();

export interface ConnectionSpec {
  id: string;
  label: string;
  /** Prebuilt typed YAS product connection for production browser paths. */
  connection?: YasWorkspaceConnection;
  /** Custom/embed transport. Omitted by native product connections. */
  transport?: YasTransport;
  /** Called when Workspace has materialized (or removed) the connection. */
  onConnection?: (connection: WorkspaceConnection | null) => void;
  /** The connection is read-only (an `.ro` share): the server refuses
   *  writes, so its terminals render without input affordances rather
   *  than swallowing keystrokes silently. */
  readOnly?: boolean;
}

type WorkspaceConnection = NonNullable<
  ReturnType<YasWorkspace["getConnection"]>
>;

const RELAY_RECONNECT_MIN_MS = 500;
const RELAY_RECONNECT_MAX_MS = 10_000;

/** The protocol-transparent edge endpoint connected only to the home server. */
function edgeWsUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return proto + "//" + location.host + "/edge";
}

export function App(props: { wasm: YasWasmModule }) {
  const [passphrase, setPassphrase] = createSignal(readPassphrase());
  const [workspaceSessionDeviceId, setWorkspaceSessionDeviceId] = createSignal<
    string | null
  >(null);
  const [workspaceSessionDeviceError, setWorkspaceSessionDeviceError] =
    createSignal<unknown>(null);
  const [edgeWebTransport, setEdgeWebTransport] = createSignal<
    EdgeWebTransportConfig | null | undefined
  >(undefined);
  let disposed = false;
  void getOrCreateWorkspaceSessionDeviceId().then(
    (id) => {
      if (!disposed) setWorkspaceSessionDeviceId(id);
    },
    (error) => {
      if (!disposed) setWorkspaceSessionDeviceError(error);
    },
  );
  void discoverEdgeWebTransport().then(setEdgeWebTransport);
  onCleanup(() => {
    disposed = true;
  });

  createEffect(() => {
    const onHashChange = () => {
      setPassphrase(readPassphrase());
    };
    window.addEventListener("hashchange", onHashChange);
    onCleanup(() => window.removeEventListener("hashchange", onHashChange));
  });

  function handleAuth(pass: string) {
    try {
      localStorage.setItem(PASSPHRASE_KEY, pass);
    } catch {}
    setPassphrase(pass);
  }

  function handleAuthError() {
    try {
      localStorage.removeItem(PASSPHRASE_KEY);
    } catch {}
    setPassphrase(null);
  }

  // Last-resort boundary. Individual tiles contain their own failures
  // (see YasTile), but a throw in the shell — the dock, the status bar,
  // LayoutContainer itself — has nothing above it and would leave a blank
  // page with the reason only in the console. Show it, and offer the one
  // action that reliably helps.
  return (
    <ErrorBoundary fallback={(err: unknown) => <AppCrash err={err} />}>
      <Show
        when={workspaceSessionDeviceError()}
        fallback={
          <Show when={passphrase()} fallback={<AuthApp onAuth={handleAuth} />}>
            {(pass) => (
              <Show
                when={
                  workspaceSessionDeviceId() && edgeWebTransport() !== undefined
                    ? workspaceSessionDeviceId()
                    : null
                }
                fallback={<WorkspaceSessionDeviceLoading />}
              >
                {(deviceId) => (
                  <ConnectedApp
                    wasm={props.wasm}
                    passphrase={pass()}
                    workspaceSessionDeviceId={deviceId()}
                    edgeWebTransport={edgeWebTransport() ?? null}
                    onAuthError={handleAuthError}
                  />
                )}
              </Show>
            )}
          </Show>
        }
      >
        {(error) => <AppCrash err={error()} />}
      </Show>
    </ErrorBoundary>
  );
}

/**
 * What is on screen before the workspace is.
 *
 * The mark and nothing else. A sentence about fetching sessions from the home
 * server is a progress report for a step that is usually over before it can be
 * read, and it is the first thing anyone sees of YAS — so it is the mark, at
 * the weight of the surrounding text, and no words.
 */
function WorkspaceSessionDeviceLoading() {
  const theme = themeFor(preferredPalette());
  return (
    <main
      role="status"
      aria-label={t("app.loading")}
      style={{
        display: "grid",
        "place-items": "center",
        height: "100%",
        color: theme.dimFg,
        "background-color": theme.bg,
      }}
    >
      <YasMark size={72} />
    </main>
  );
}

/** The shell failed. Deliberately dependency-free: whatever broke may well
 *  be the theme or the workspace this would otherwise read from. */
function AppCrash(props: { err: unknown }) {
  const message = () =>
    props.err instanceof Error
      ? `${props.err.name}: ${props.err.message}\n\n${props.err.stack ?? ""}`
      : String(props.err);
  return (
    <div
      style={{
        position: "fixed",
        inset: "0",
        display: "flex",
        "flex-direction": "column",
        gap: "12px",
        padding: "24px",
        overflow: "auto",
        background: "#1a1a1a",
        color: "#e0e0e0",
        "font-family": "ui-monospace, monospace",
        "font-size": "13px",
      }}
    >
      <b style={{ color: "#f66" }}>{t("app.crashTitle")}</b>
      <div>{t("app.crashRecovery")}</div>
      <div>
        <TapButton
          onClick={() => location.reload()}
          style={{
            padding: "4px 10px",
            background: "#2a2a2a",
            color: "#e0e0e0",
            border: "1px solid #808080",
            "border-radius": "3px",
            cursor: "pointer",
            font: "inherit",
          }}
        >
          {t("common.reload")}
        </TapButton>
      </div>
      <pre
        style={{
          "white-space": "pre-wrap",
          "word-break": "break-word",
          color: "#808080",
          margin: "0",
        }}
      >
        {message()}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// HMR-preserved identity: the protocol workspace reuses this key while the
// replacement app establishes a fresh native YAS session.
// ---------------------------------------------------------------------------

type HmrData = HmrLeaseState & {
  version: number;
  passphrase: string;
  workspaceKey: object;
  closed?: boolean;
};

// Bump when preserved transport instances are incompatible with hot code.
// Existing class instances keep their old prototype and receive callbacks,
// so reusing one would silently leave transport fixes inactive until reload.
const HMR_DATA_VERSION = 12;

// Connection lifetimes belong to the component and its module generation.
// Preserve workspace identity across HMR, but never an abandoned live socket.
const homeConnectionSlot = createHmrConnectionSlot();
import.meta.hot?.dispose(() => homeConnectionSlot.close());

function getHmrData(): HmrData | null {
  return (import.meta.hot?.data?.connectedApp as HmrData) ?? null;
}

function setHmrData(data: HmrData): void {
  if (import.meta.hot) {
    import.meta.hot.data.connectedApp = data;
  }
}

function clearHmrData(data: HmrData): void {
  if (import.meta.hot?.data?.connectedApp === data) {
    delete import.meta.hot.data.connectedApp;
  }
}

function closeHmrData(data: HmrData): void {
  cancelHmrRelease(data);
  if (data.closed) return;
  data.closed = true;
}

function ConnectedApp(props: {
  wasm: YasWasmModule;
  passphrase: string;
  workspaceSessionDeviceId: string;
  edgeWebTransport: EdgeWebTransportConfig | null;
  onAuthError: () => void;
}) {
  const hmrLeaseOwner = {};

  // Reuse only the Workspace identity across HMR. A native YAS handshake and
  // its catalogue belong to one physical transport and are never replayed
  // into a replacement connection object.
  const prev = getHmrData();
  const reusablePrev =
    prev?.version === HMR_DATA_VERSION &&
    prev.passphrase === props.passphrase &&
    !prev.closed;
  if (prev && !reusablePrev) {
    closeHmrData(prev);
    clearHmrData(prev);
  }
  if (reusablePrev) claimHmrLease(prev, hmrLeaseOwner);

  const workspaceKey = reusablePrev ? prev.workspaceKey : {};

  // The edge exposes exactly one native home connection. Relay creates nested
  // server transports inside that authenticated YAS session.
  const edgeTransport = props.edgeWebTransport
    ? new YasWebTransportTransport(
        props.edgeWebTransport.url,
        props.passphrase,
        { serverCertificateHash: fetchEdgeCertificateHash },
      )
    : new YasEdgeWebSocketTransport(edgeWsUrl(), props.passphrase);
  const yasOptions = yasBrowserConnectionOptions();
  const homeYas = new YasConnection(edgeTransport, yasOptions);
  const homeConnection = new YasNativeWorkspaceConnection(
    "local",
    homeYas,
    props.wasm,
    false,
  );
  // Retire the previous owner and register cleanup before opening a socket.
  // A late mount from a disposed HMR module is closed by the slot here.
  const closeHomeConnection = homeConnectionSlot.replace(homeConnection);
  onCleanup(closeHomeConnection);
  homeConnection.connect();
  const [relayCacheRevision, setRelayCacheRevision] = createSignal(0);
  const relayCache = new RelayConnectionCache(() =>
    setRelayCacheRevision((revision) => revision + 1),
  );
  const [relayRoutes, setRelayRoutes] = createSignal<readonly YasRelayRoute[]>(
    [],
  );
  const [relayClient, setRelayClient] = createSignal<YasRelayClient | null>(
    null,
  );
  const onHomeStatus = () => {
    if (edgeTransport.authRejected) props.onAuthError();
  };
  edgeTransport.addEventListener("statuschange", onHomeStatus);
  onCleanup(() =>
    edgeTransport.removeEventListener("statuschange", onHomeStatus),
  );

  createEffect(() => {
    let stopped = false;
    let stopRouteWatch: (() => void) | undefined;
    let routeWatchRetryTimer: ReturnType<typeof setTimeout> | undefined;
    let routeWatchRetryDelay = RELAY_RECONNECT_MIN_MS;
    let hasRouteSnapshot = false;

    const clearRouteWatchRetry = () => {
      if (routeWatchRetryTimer !== undefined) {
        clearTimeout(routeWatchRetryTimer);
        routeWatchRetryTimer = undefined;
      }
    };

    const stopWatching = () => {
      clearRouteWatchRetry();
      const stop = stopRouteWatch;
      stopRouteWatch = undefined;
      stop?.();
      hasRouteSnapshot = false;
    };
    const scheduleWatchRetry = () => {
      if (routeWatchRetryTimer !== undefined) return;
      const delay = routeWatchRetryDelay;
      routeWatchRetryDelay = Math.min(
        routeWatchRetryDelay * 2,
        RELAY_RECONNECT_MAX_MS,
      );
      routeWatchRetryTimer = setTimeout(() => {
        routeWatchRetryTimer = undefined;
        refresh();
      }, delay);
    };
    const refresh = async () => {
      stopWatching();
      try {
        await homeYas.connect();
        if (stopped) return;
        const relay = new YasRelayClient(homeYas);
        setRelayClient(relay);
        stopRouteWatch = relay.routes.subscribe((state) => {
          if (state.revision === 0n) {
            setRelayRoutes([]);
            if (hasRouteSnapshot) scheduleWatchRetry();
            return;
          }
          hasRouteSnapshot = true;
          clearRouteWatchRetry();
          routeWatchRetryDelay = RELAY_RECONNECT_MIN_MS;
          setRelayRoutes(boundedRelayRoutes(state.routes));
        });
        await relay.routes.watch();
      } catch {
        if (stopped) return;
        stopWatching();
        setRelayClient(null);
        setRelayRoutes([]);
        // Relay may be temporarily unavailable or administratively omitted.
        // The local home connection stays usable while bounded retries watch
        // for a later catalogue/family update; there is no second protocol.
        scheduleWatchRetry();
      }
    };

    void refresh();
    onCleanup(() => {
      stopped = true;
      stopWatching();
    });
  });

  const sessionStore = new WorkspaceSessionStore(homeYas);
  const sessionDeviceStore = new WorkspaceSessionDeviceStore(
    homeYas,
    props.workspaceSessionDeviceId,
  );
  const sessionController = createWorkspaceSessionController({
    store: sessionStore,
    deviceStore: sessionDeviceStore,
    initialHash: location.hash,
  });

  void sessionController.start();

  // Wake events bypass retry backoff only after the transport has dropped.
  // A live connection can keep playing audio in the background; reconnecting
  // it on foreground interrupts playback and every nested Relay session.
  // Leave in-progress connection attempts alone too.
  // Deliberately in the UI layer: js/core stays free of window/document so it
  // can be imported by the preview service worker (docs/design/net.md).
  createEffect(() => {
    const wake = () => {
      if (document.visibilityState === "hidden") return;
      if (
        edgeTransport.status === "disconnected" ||
        edgeTransport.status === "error"
      ) {
        homeConnection.reconnect();
      }
    };
    window.addEventListener("online", wake);
    document.addEventListener("visibilitychange", wake);
    onCleanup(() => {
      window.removeEventListener("online", wake);
      document.removeEventListener("visibilitychange", wake);
    });
  });

  const hmrData = reusablePrev
    ? prev
    : claimHmrLease<HmrData>(
        {
          version: HMR_DATA_VERSION,
          passphrase: props.passphrase,
          workspaceKey,
        },
        hmrLeaseOwner,
      );
  setHmrData(hmrData);

  // import.meta.hot exists for ordinary unmounts too. Give a replacement HMR
  // mount one task to claim the stable Workspace identity.
  onCleanup(() => {
    const current = getHmrData();
    if (import.meta.hot && current === hmrData) {
      deferHmrRelease(
        hmrData,
        hmrLeaseOwner,
        () => getHmrData() === hmrData,
        () => closeHmrData(hmrData),
        () => clearHmrData(hmrData),
      );
    } else {
      closeHmrData(hmrData);
      clearHmrData(hmrData);
    }
  });

  const connections = createMemo<ConnectionSpec[]>(() => {
    relayCacheRevision();
    const next: ConnectionSpec[] = [
      {
        id: "local",
        label: t("common.local"),
        connection: homeConnection,
      },
    ];
    const relay = relayClient();
    next.push(
      ...reconcileWorkspaceSessionRelayConnections(
        relayRoutes(),
        sessionController.current()?.activeRemotes ?? [],
        relayCache,
        relay
          ? (route) => {
              const transport = new YasNativeRelayTransport(relay, route);
              return new YasNativeWorkspaceConnection(
                route.name,
                new YasConnection(transport, yasBrowserConnectionOptions()),
                props.wasm,
              );
            }
          : null,
      ),
    );
    return next;
  });

  // The service worker is deliberately transport-blind: resolve its preview
  // sockets through the App's already-authenticated home or nested Relay YAS
  // session. No passphrase or YAS frame crosses this local MessagePort.
  const stopPreviewNetBroker = installPreviewNetBroker((dest) => {
    const spec = connections().find((candidate) => candidate.id === dest);
    return spec?.connection?.native.net ?? null;
  });

  onCleanup(() => {
    try {
      stopPreviewNetBroker();
      sessionController.dispose();
      sessionDeviceStore.dispose();
      sessionStore.dispose();
      relayCache.clear();
    } finally {
      closeHomeConnection();
      homeConnection.dispose();
    }
  });

  return (
    <>
      {/* Keep the protocol workspace mounted across keyed session screens.
          Its YasConnections have already consumed handshake/catalog frames;
          rebuilding them on the same live transports cannot replay that state. */}
      <Workspace
        connections={connections}
        wasm={props.wasm}
        hmrKey={workspaceKey}
        onAuthError={props.onAuthError}
        relayRoutes={() => relayRoutes()}
        workspaceSession={sessionController.binding}
        workspaceSessions={sessionController}
        transportOwnership="external"
      />
    </>
  );
}

function AuthApp(props: { onAuth: (pass: string) => void }) {
  const [authError, setAuthError] = createSignal<string | null>(null);

  function connect(pass: string) {
    setAuthError(null);
    const ws = new WebSocket(edgeWsUrl(), YAS_WEBSOCKET_SUBPROTOCOL);
    let authed = false;
    let throttled = false;

    ws.onopen = () => {
      ws.send(pass);
    };

    ws.onmessage = (ev) => {
      const msg = String(ev.data);
      if (msg === "ok") {
        authed = true;
        ws.close();
        props.onAuth(pass);
      } else if (msg === "busy") {
        // Throttled before the passphrase was even checked. Saying
        // "authentication failed" here sends the user hunting for a wrong
        // credential when the only thing to do is wait.
        throttled = true;
        setAuthError(t("auth.busy"));
      } else if (msg.startsWith("error:")) {
        // Authentication succeeded, but the fixed home server could not be
        // reached. Preserve the credential and report the actual boundary
        // failure instead of misdiagnosing it as a bad passphrase.
        throttled = true;
        const detail = msg.slice("error:".length).trim();
        setAuthError(detail || t("auth.homeUnavailable"));
      }
    };

    ws.onerror = () => {};

    ws.onclose = () => {
      if (!authed && !throttled) {
        setAuthError(t("auth.failed"));
      }
    };
  }

  return <AuthScreen error={authError()} onSubmit={(pass) => connect(pass)} />;
}

function AuthScreen(props: {
  error: string | null;
  onSubmit: (pass: string) => void;
}) {
  const theme = themeFor(preferredPalette());
  let inputRef!: HTMLInputElement;

  return (
    <main
      style={{
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        height: "100%",
        "background-color": theme.bg,
      }}
    >
      <form
        style={{
          display: "flex",
          "flex-direction": "column",
          "align-items": "center",
          gap: "1em",
          color: theme.dimFg,
        }}
        onSubmit={(e) => {
          e.preventDefault();
          const v = inputRef?.value;
          if (v) props.onSubmit(v);
        }}
      >
        {/* The first thing anyone sees of YAS, and the only thing on this
            screen that is not a password field. */}
        <YasMark size={72} />
        <input
          ref={inputRef}
          name="yas-passphrase"
          type="password"
          placeholder={t("auth.placeholder")}
          autofocus
          style={{
            padding: "0.5em 0.75em",
            "font-size": "1em",
            border: "1px solid #444",
            outline: "none",
            width: "20em",
            "font-family": "inherit",
            "background-color": theme.solidInputBg,
            color: theme.fg,
          }}
        />
        <Show when={props.error}>
          {(err) => (
            <output style={{ color: theme.errorText, "font-size": "0.85em" }}>
              {err()}
            </output>
          )}
        </Show>
      </form>
    </main>
  );
}
