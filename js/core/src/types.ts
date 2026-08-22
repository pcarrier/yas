/** A terminal color palette. */
export interface TerminalPalette {
  id: string;
  name: string;
  /** true = dark background, false = light background. */
  dark: boolean;
  /** Default foreground color as [r, g, b] (0–255). */
  fg: [number, number, number];
  /** Default background color as [r, g, b] (0–255). */
  bg: [number, number, number];
  /** ANSI 16-color entries, indexed 0–15. */
  ansi: Array<[number, number, number]>;
}

export interface YasDebug {
  log(msg: string, ...args: unknown[]): void;
  warn(msg: string, ...args: unknown[]): void;
  error(msg: string, ...args: unknown[]): void;
}

/** Silent {@link YasDebug} that discards everything. */
export const noopDebug: YasDebug = { log() {}, warn() {}, error() {} };

/** Connection lifecycle states. */
export type ConnectionStatus =
  | "connecting"
  | "authenticating"
  | "connected"
  | "disconnected"
  | "closed"
  | "error";

export type ConnectionId = string;
export type SessionId = string;
/** Opaque, nonzero, boot-scoped native Terminal handle. */
export type TerminalId = bigint;

/**
 * Transport abstraction for yas server communication.
 * Implementations carry native YAS over WebSocket, WebTransport, WebRTC, or
 * a custom byte stream while consumers only deal with binary data and status
 * changes.
 */
/** Binary transport payload. Borrowed views must be consumed or copied
 * synchronously. */
export type YasTransportMessage = ArrayBuffer | Uint8Array;

/** Opaque server-native Wayland surface handle. */
export type SurfaceId = bigint;

export type YasTransportEventMap = {
  message: YasTransportMessage;
  /** One complete, unframed YAS Event received on an unreliable path. */
  datagram: YasTransportMessage;
  statuschange: ConnectionStatus;
};

export interface YasTransportOptions {
  /** Enable automatic reconnection on disconnect. Default: true. */
  reconnect?: boolean;
  /** Initial reconnect delay in ms. Default: 500. */
  reconnectDelay?: number;
  /** Maximum reconnect delay in ms. Default: 10000. */
  maxReconnectDelay?: number;
  /** Backoff multiplier for reconnect delay. Default: 1.5. */
  reconnectBackoff?: number;
  /** Timeout in ms to wait for the connection to be established. Default: none for WebSocket, 10000 for others. */
  connectTimeoutMs?: number;
}

export interface YasTransport {
  /** Message boundaries are frames unless the transport exposes a raw stream. */
  readonly yasFraming?: "message" | "stream";
  /**
   * Maximum complete YAS datagram this path can receive, or zero/undefined
   * when no unreliable path is paired with the reliable connection.
   */
  readonly maxDatagramSize?: number;
  /** Start connecting. Safe to call repeatedly. Call after registering listeners. */
  connect(): void;
  /** Send binary data to the server. */
  send(data: Uint8Array): void;
  /**
   * Send one complete, unframed YAS Event on the optional unreliable path.
   * Congestion and an unopened path may drop it; callers must not infer
   * delivery from this method returning.
   */
  sendDatagram?(data: Uint8Array): void;
  /** Close the transport connection. */
  close(): void;
  /** Stop the active connection and automatic retries without disposing it. */
  suspend?(): void;
  /** Tear down the current connection and reconnect from scratch. */
  reconnect?(): void;
  /** Current connection status. */
  readonly status: ConnectionStatus;
  /**
   * Bytes handed to `send` that have not yet reached the network, when the
   * transport can say.
   *
   * This is the only honest congestion signal a browser gets: the socket
   * drains at whatever rate the uplink allows, so a queue that keeps growing
   * *is* a link too slow for what is being sent. Realtime senders — the
   * camera above all — should drop rather than add to it, because every
   * queued byte is delay in front of the frame they are about to capture.
   *
   * `undefined` where the transport cannot report it
   * that has not sampled recently, say). Callers must treat that as "no
   * backpressure known" and fall back to their own flow control rather than
   * stalling.
   */
  readonly bufferedAmount?: number;
  /** True when the server explicitly rejected authentication. */
  readonly authRejected: boolean;
  /** Last error message, if any. Cleared on successful connection. */
  readonly lastError: string | null;
  /** Register a listener for transport events. */
  addEventListener(
    type: "message",
    listener: (data: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  /** Remove a previously registered listener. */
  removeEventListener(
    type: "message",
    listener: (data: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
}

/** A tracked terminal session. */
export type YasSession = {
  id: SessionId;
  connectionId: ConnectionId;
  ptyId: TerminalId;
  tag: string;
  title: string | null;
  /** Highest visible terminal row reached since the last terminal reset. */
  usedRows: number;
  command: string | null;
  state: "creating" | "active" | "exited" | "closed";
  /**
   * Raw exit status from the native Terminal lifecycle once the process has
   * exited, or `null` while running.
   *
   * `>= 0` is the normal exit code, `< 0` is the negated terminating
   * signal, and {@link EXIT_STATUS_UNKNOWN} means "not yet collected".
   * Use `exitCodeFromStatus` to map it to a conventional shell exit code.
   */
  exitStatus: number | null;
};

/** An active terminal subscription held by another server connection. */
export interface YasClientTerminalSubscription {
  ptyId: TerminalId;
  /** Null when the client subscribed before advertising a view size. */
  rows: number | null;
  /** Null when the client subscribed before advertising a view size. */
  cols: number | null;
}

/** An active Wayland surface subscription held by another connection. */
export interface YasClientSurfaceSubscription {
  surfaceId: SurfaceId;
  /** Encoded pixel dimensions requested by the client, if reported. */
  width: number | null;
  height: number | null;
  /** Fractional scale in 120ths (120 = 1x), if reported. */
  scale120: number | null;
}

/** A non-terminal, non-surface subscription held by a connection. */
export interface YasClientAuxSubscription {
  /** Selected static YAS family ID. Unknown values are retained. */
  kind: number;
  /**
   * What the watch is pointed at, as the owning family accounts for it: a KV
   * namespace, an FS watch root, a Git repository, an LSP workspace. Zero
   * where the family has no such resource, or where the server could not
   * resolve one. The handle belongs to the watching connection's session, so
   * it names the resource only for that peer.
   */
  id: bigint;
  /** The watch's own ID, unique per connection. Distinguishes two watches
   * that a family points at one resource. */
  subscriptionId: number;
}

/** What opened a connection, as the server accounts for it. */
export type YasClientOrigin =
  | { kind: "network" }
  | { kind: "unix"; peerPid: number; peerUid: number; peerGid: number }
  | { kind: "ssh"; remoteAddress: string; username: string }
  | { kind: "edge"; subject: string; issuer: string }
  | { kind: "relay"; routeHandle: bigint; generation: bigint; depth: number }
  | { kind: "webrtc"; peerId: string }
  | {
      kind: "extension";
      extensionId: bigint;
      definitionRevision: bigint;
      attempt: bigint;
      taskId: number;
      /** The durable name of a persistent definition, the label a transient
       *  `ext run` carried, or empty when it had neither. */
      name: string;
    }
  /** A kind this build has no name for. Still worth showing as "not an
   *  ordinary client" — it is one thing to not know what a connection is, and
   *  another to call it a browser. */
  | { kind: "unknown"; originKind: number };

export interface YasClientInfo {
  /** Canonical 32-lowercase-hex native session ID. */
  id: string;
  /** Whole seconds since the server accepted the connection. */
  ageSeconds: number;
  /** Actual framed bytes written by the server to this client per second. */
  outboundBytesPerSecond: number;
  /** The same, for framed bytes the server read from this client. Both are
   *  measured by the server, so they are comparable across client kinds. */
  inboundBytesPerSecond: number;
  /** Audio, filesystem, Git, LSP, KV and network subscriptions. */
  subscriptions: readonly YasClientAuxSubscription[];
  terminals: readonly YasClientTerminalSubscription[];
  surfaces: readonly YasClientSurfaceSubscription[];
  /** Null when the peer did not provide an origin; this is distinct from an
   *  ordinary network client. */
  origin: YasClientOrigin | null;
}

/** Snapshot returned by listClients or a live subscribeClients callback. */
export interface YasClientList {
  selfId: string;
  /** Every currently connected client, including the requester. */
  clients: readonly YasClientInfo[];
}

export interface YasConnectionSnapshot {
  id: ConnectionId;
  status: ConnectionStatus;
  ready: boolean;
  supportsRestart: boolean;
  supportsCopyRange: boolean;
  supportsCompositor: boolean;
  /** Server accepts direct touchscreen contacts for Wayland surfaces. */
  supportsSurfaceTouch: boolean;
  /** Server forwards Wayland text-input requests to surface viewers. */
  supportsSurfaceTextInput: boolean;
  supportsAudio: boolean;
  /** Server supports enumerating and kicking other connections. */
  supportsClientControl: boolean;
  supportsFsSync: boolean;
  /** The native Git family is available. */
  supportsGit: boolean;
  /** The native LSP family is available. */
  supportsLsp: boolean;
  /** Server advertises the KV store family (docs/design/kv.md). */
  supportsKv: boolean;
  /** Server bridges tray items and desktop notifications. */
  supportsDesktop: boolean;
  /** Server supports process-global named bidirectional channels. */
  supportsChannels: boolean;
  /** Server pushes which channel names have a listener, so a client can watch
   *  an extension appear and go away rather than probe for it once. */
  supportsChannelWatch: boolean;
  /** The server admits Wasmi extensions (docs/design/extensions.md). */
  supportsExtensions: boolean;
  /** Server understands viewer media, portals, and MPRIS runtime state. */
  supportsDesktopMedia: boolean;
  retryCount: number;
  /** The native HELLO's opaque 128-bit boot ID, interpreted as an unsigned
   *  big-endian integer. Null while no native session is established. */
  bootGeneration: bigint | null;
  /** Bumped on every connection reset (transport drop AND server
   *  re-establish), so views holding fs/git handles can re-open them — those
   *  don't survive a reset even when the transport stays up. */
  generation: number;
  /** Non-null when the last connection attempt failed with an explicit error message. */
  error: string | null;
  sessions: readonly YasSession[];
  focusedSessionId: SessionId | null;
}

export interface YasWorkspaceSnapshot {
  connections: readonly YasConnectionSnapshot[];
  sessions: readonly YasSession[];
  focusedSessionId: SessionId | null;
  ready: boolean;
}

export interface CopyRangeResult {
  /** Copied text.  Soft-wrapped rows are joined without a separator. */
  text: string;
  /**
   * Rows the PTY held when the copy ran (scrollback plus screen), so a caller
   * that asked for a bounded window can tell whether rows were left above it.
   */
  totalLines: number;
}

export interface YasSearchResult {
  sessionId: SessionId;
  connectionId: ConnectionId;
  score: number;
  primarySource: number;
  matchedSources: number;
  scrollOffset: number | null;
  context: string;
}

export type TransportConfig =
  | {
      type: "websocket";
      url: string;
      passphrase: string;
      options?: YasTransportOptions;
    }
  | { type: "share"; hubUrl: string; passphrase: string; debug?: YasDebug }
  | { type: "custom"; transport: YasTransport };

export const DEFAULT_FONT = "ui-monospace, monospace";
export const DEFAULT_FONT_SIZE = 13;

/**
 * Coverage gamma for glyph antialiasing (1 = untouched, higher = thinner
 * light-on-dark text).
 *
 * Glyph coverage is blended into an sRGB-encoded framebuffer, which overstates
 * partial coverage and makes light-on-dark stems read bolder than the font
 * intends. Apple platforms are where that lands hardest — the system's own
 * text rendering is the reference users compare against, and it thins stems
 * the same way — so they get a correction by default and everyone else opts
 * in. Same reasoning, and roughly the same value, as kitty's
 * `text_gamma_adjustment`.
 */
export const DEFAULT_TEXT_GAMMA = isApplePlatform() ? 1.4 : 1;

function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform);
}

/** Server-stamped identity of the socket a Wayland surface arrived on. */
export type YasSurfaceOrigin = {
  sandboxEngine: string;
  appId: string;
  instanceId: string;
};

export type YasSurface = {
  connectionId: ConnectionId;
  surfaceId: SurfaceId;
  parentId: SurfaceId;
  title: string;
  appId: string;
  /**
   * Trusted application identity supplied by the server, when the surface
   * arrived on a stamped app socket. This is distinct from `appId`, which is
   * self-reported by the Wayland client.
   *
   * Optional so callers constructing surface-shaped fixtures remain source
   * compatible; surfaces created by SurfaceStore set it explicitly.
   */
  origin?: YasSurfaceOrigin | null;
  /** Composited size in physical pixels — what the video stream carries. */
  width: number;
  height: number;
  /**
   * The same size in surface-logical pixels: the window as its Wayland
   * client measures it, before the mediated output scale.  The server
   * mediates one surface across every viewer at the *highest* DPR any of
   * them asked for, so on a 1x viewer watching a surface a 3x viewer
   * sized, `width` is three times `logicalWidth` and presenting the frame
   * to fill the pane would show the window at 3x zoom.
   *
   * 0 until a valid logical size is reported, which callers must read as
   * "unknown", not as an empty window.
   */
  logicalWidth: number;
  logicalHeight: number;
};
