# Embedding

There are two distinct dimensions: embedding the frontend into your app, and embedding `yas server` into your own service.

## Your app, our components: `@yas-run/react` / `@yas-run/solid`

`@yas-run/react` and `@yas-run/solid` are workspace-first. Both are thin wrappers over `@yas-run/core`'s `YasTerminalSurface`. A `YasWorkspace` owns connections, each connection owns terminals, and each `YasTerminal` renders a terminal by ID.

```tsx
import {
  YasTerminal,
  YasWorkspaceProvider,
  useYasFocusedSession,
  useYasSessions,
  useYasWorkspace,
} from "@yas-run/react";
import { YasWorkspace } from "@yas-run/core";
import { useEffect, useMemo } from "react";

function EmbeddedYas({ wasm, passphrase }: { wasm: any; passphrase: string }) {
  const workspace = useMemo(
    () =>
      new YasWorkspace({
        wasm,
        connections: [
          {
            id: "default",
            transport: {
              type: "websocket",
              url: "wss://example.com/edge",
              passphrase,
            },
          },
        ],
      }),
    [passphrase, wasm],
  );

  useEffect(() => () => workspace.dispose(), [workspace]);

  return (
    <YasWorkspaceProvider workspace={workspace}>
      <TerminalScreen />
    </YasWorkspaceProvider>
  );
}

function TerminalScreen() {
  const workspace = useYasWorkspace();
  const sessions = useYasSessions();
  const focusedSession = useYasFocusedSession();

  useEffect(() => {
    if (sessions.length > 0) return;
    void workspace.createSession({
      connectionId: "default",
      rows: 24,
      cols: 80,
    });
  }, [sessions.length, workspace]);

  return (
    <YasTerminal
      sessionId={focusedSession?.id ?? null}
      style={{ width: "100%", height: "100vh" }}
    />
  );
}
```

Read-only terminals use the same sizing behavior as writable terminals; `readOnly` only disables mutating input. Terminals resize the remote session to their available grid by default. Pass `resizable={false}` for passive previews or read-only transports that must preserve the host dimensions; the canvas is then contained and centered within the embedding element. Add `fitWidth` when a passive preview should expand a smaller grid to the container's full width.

### React API

| API                                                   | Purpose                                                  |
| ----------------------------------------------------- | -------------------------------------------------------- |
| `new YasWorkspace({ wasm, connections })`             | Create a workspace with one or more transports           |
| `YasWorkspaceProvider`                                | Put the workspace, palette, and font settings in context |
| `useYasWorkspace()`                                   | Get the imperative workspace object                      |
| `useYasWorkspaceState()`                              | Read the full reactive workspace snapshot                |
| `useYasConnection(connectionId?)`                     | Read one connection snapshot                             |
| `useYasSessions()`                                    | Read all terminals                                       |
| `useYasFocusedSession()`                              | Read the currently focused terminal                      |
| `useYasWorkspaceConnection(workspace, id, transport)` | Manage a connection lifecycle with cleanup               |
| `YasTerminal`                                         | Render one terminal by `sessionId`                       |

### Solid API

```tsx
import {
  YasTerminal,
  YasWorkspaceProvider,
  createYasWorkspace,
  createYasWorkspaceState,
  createYasSessions,
  useYasFocusedSession,
} from "@yas-run/solid";
import { YasWorkspace } from "@yas-run/core";
import { createSignal, onCleanup, createEffect } from "solid-js";

function EmbeddedYas(props: { wasm: any; passphrase: string }) {
  const workspace = new YasWorkspace({
    wasm: props.wasm,
    connections: [
      {
        id: "default",
        transport: {
          type: "websocket",
          url: "wss://example.com/yas",
          passphrase: props.passphrase,
        },
      },
    ],
  });
  onCleanup(() => workspace.dispose());

  return (
    <YasWorkspaceProvider workspace={workspace}>
      <TerminalScreen />
    </YasWorkspaceProvider>
  );
}

function TerminalScreen() {
  const workspace = createYasWorkspace();
  const sessions = createYasSessions();
  const focusedSession = () => useYasFocusedSession(workspace);

  createEffect(() => {
    if (sessions().length > 0) return;
    workspace.createSession({ connectionId: "default", rows: 24, cols: 80 });
  });

  return (
    <YasTerminal
      sessionId={focusedSession()?.id ?? null}
      style={{ width: "100%", height: "100vh" }}
    />
  );
}
```

| API                                                      | Purpose                                                  |
| -------------------------------------------------------- | -------------------------------------------------------- |
| `new YasWorkspace({ wasm, connections })`                | Create a workspace with one or more transports           |
| `YasWorkspaceProvider`                                   | Put the workspace, palette, and font settings in context |
| `createYasWorkspace()`                                   | Get the imperative workspace object from context         |
| `createYasWorkspaceState(workspace?)`                    | Reactive signal tracking the workspace snapshot          |
| `createYasSessions(workspace?)`                          | Reactive signal tracking all terminals                   |
| `useYasSession(workspace, sessionId)`                    | Look up a single terminal by ID (non-reactive)           |
| `useYasFocusedSession(workspace)`                        | Look up the focused terminal (non-reactive)              |
| `useYasConnection(workspace, sessionId)`                 | Look up a connection snapshot (non-reactive)             |
| `createYasWorkspaceConnection(workspace, id, transport)` | Manage a connection lifecycle with `onCleanup`           |
| `YasTerminal`                                            | Render one terminal by `sessionId`                       |

### Wayland surface rendering (experimental)

`YasSurfaceView` renders a single Wayland surface from a terminal's compositor. The server encodes each surface as H.264 or AV1; the component decodes via WebCodecs and draws to a canvas.

By default the view participates in sizing its surface: the largest logical
rectangle that fits all active viewers is requested, subject to the application's
minimum/maximum size hints. The view is fully interactive. Pass
`resizable={false}` for a passive preview — a dock card, a switcher thumbnail —
that shares another view's stream.
Such a view is served a fixed downscale capped at a thumbnail cadence and takes
no input at all, so it is the wrong choice for anything the user clicks in.

`zoom` scales the surface independently of the pane's pixel size: `zoomMode`
`"relative"` (the default) multiplies the display's DPI by `zoom`, while
`"exact"` uses `zoom` as the absolute surface scale. Only resizable views drive
the scale.

Resizable views display decoded pixels 1:1 when codec rounding leaves the
frame within two device pixels of its intended size on both axes. This avoids
an extra browser resize on lower-DPI viewers sharing a HiDPI surface; a small
gap at the right or bottom is preferable to filtering the text again.
Adaptive frames with substantially lower resolution still fill their logical
window size. When the window exceeds the pane (an application minimum or an
in-flight resize), it is uniformly scaled down to fit, never cropped or distorted.
Smaller windows stay at their intended scale rather than filling unused space.
Minimum-forced zoom-out also expands the logical size offered to the application:
if width forces a 360×780 pane to fit a 500px-wide window, it offers approximately
1083 logical pixels of height. These adjusted bounds are intersected across
viewers, and reset when the application releases its minimum.

```tsx
import { YasSurfaceView } from "@yas-run/react";

function AppWindow({
  connectionId,
  surfaceId,
}: {
  connectionId: string;
  surfaceId: number;
}) {
  return (
    <YasSurfaceView
      connectionId={connectionId}
      surfaceId={surfaceId}
      style={{ width: 800, height: 600 }}
    />
  );
}
```

`touchMode` chooses how touchscreen contacts reach the app. The default,
`"direct"`, forwards every contact to the app's own `wl_touch`, so pinch,
rotate, and multi-finger gestures belong to the app. Set `"pointer"` to opt out
and use YAS's compatibility gestures: tap to click, one-finger drag to scroll,
and long-press for right-click. A server without multitouch support
automatically keeps the pointer mapping. The mode is safe to change at runtime
and does not restart the video stream. Trackpads and pens are unaffected.

```tsx
<YasSurfaceView
  connectionId={connectionId}
  surfaceId={surfaceId}
  touchMode="pointer"
/>
```

Every terminal has an experimental Wayland compositor available. Any command — shell, TUI, or GUI — can open Wayland surfaces:

```tsx
workspace.createSession({
  connectionId: "default",
  rows: 24,
  cols: 80,
  command: "my-gui-app",
});
```

Surfaces created by the terminal appear in the connection's `surfaceStore`, keyed by the terminal's PTY ID. Each surface has a `surfaceId`, `parentId`, `title`, `appId`, `width`, and `height`.

### Workspace operations

- `createSession({ connectionId, rows, cols, tag?, command?, cwdFromSessionId? })`
- `closeSession(sessionId)`
- `restartSession(sessionId)`
- `focusSession(sessionId | null)`
- `search(query, { connectionId? })`
- `setVisibleSessions(sessionIds)`
- `addConnection(...)` / `removeConnection(connectionId)` / `reconnectConnection(connectionId)`

### Transports

All transports share a common set of options (`YasTransportOptions`):

| Option              | Default                      | Description                  |
| ------------------- | ---------------------------- | ---------------------------- |
| `reconnect`         | `true`                       | Auto-reconnect on disconnect |
| `reconnectDelay`    | `500`                        | Initial reconnect delay (ms) |
| `maxReconnectDelay` | `10000`                      | Maximum reconnect delay (ms) |
| `reconnectBackoff`  | `1.5`                        | Backoff multiplier           |
| `connectTimeoutMs`  | none (WS) / `10000` (WebRTC) | Connection timeout (ms)      |

```ts
// Authenticated native YAS edge.
const edge = { type: "websocket", url, passphrase, options };

// Native read-only WebRTC share through the signaling hub.
const share = { type: "share", hubUrl, passphrase };

// Low-level native byte-stream transport for an existing peer connection.
const dataChannel = createWebRtcDataChannelTransport(peerConnection);
// Its ordered channel selector is `yas.v1`.
```

Or implement your own:

```ts
interface YasTransport {
  connect(): void;
  send(data: Uint8Array): void;
  close(): void;
  readonly status: ConnectionStatus;
  readonly authRejected: boolean;
  readonly lastError: string | null;
  addEventListener(type: "message" | "statuschange", listener: Function): void;
  removeEventListener(
    type: "message" | "statuschange",
    listener: Function,
  ): void;
}
```

## Server-side: a Node/Bun client over a unix socket

You can also run a `@yas-run/core` client **server-side** (Node/Bun/Deno) to drive a
local `yas server` over its unix-domain socket — e.g. to script terminals or run
headless commands. The non-browser building blocks live under the
`@yas-run/core/node` subpath (kept out of the package root so `node:net` and
runtime globals never leak into browser bundles):

```ts
import { YasWorkspace, exitCodeFromStatus, nullLogger } from "@yas-run/core";
import { NodeUnixSocketTransport, loadYasWasm } from "@yas-run/core/node";

// `loadYasWasm()` initializes the @yas-run/browser WASM off-browser: it reads
// the colocated yas_browser_bg.wasm from disk and feeds it to init(), so you
// never touch raw wasm bytes. (If you depend on a self-initializing
// `@yas-run/browser/node` build it is returned as-is.)
const wasm = await loadYasWasm();

const socket = process.env.YAS_SOCK;
if (!socket) {
  throw new Error("YAS_SOCK must name the server's explicit Unix socket");
}
const transport = new NodeUnixSocketTransport(socket);
const workspace = new YasWorkspace({
  wasm,
  logger: nullLogger, // no-op logger; omit to log lifecycle events to console
  connections: [{ id: "default", transport }],
});

const session = await workspace.createSession({
  connectionId: "default",
  rows: 24,
  cols: 80,
  command: "my-command",
});
```

The unix transport speaks yas's framing protocol (4-byte little-endian
length-prefixed frames) for you — there is no need to re-implement the wire
format. `BunUnixSocketTransport` and `DenoUnixSocketTransport` are the
runtime-native equivalents.

### Exit status

When a session's process exits, its `YasSession.state` becomes `"exited"` and
`YasSession.exitStatus` carries the raw status from the server:

- `>= 0` — normal exit; the value is the exit code.
- `< 0` — terminated by a signal; the value is the negated signal number.
- `EXIT_STATUS_UNKNOWN` — not yet collected.

`exitCodeFromStatus(status)` maps that to a conventional shell exit code
(unknown → `1`, signalled → `128 + signal`), and `formatExitStatus(status)`
renders `"exited(<code>)"` / `"signal(<n>)"`. Both mirror the `yas` CLI.

```ts
import { exitCodeFromStatus } from "@yas-run/core";

workspace.subscribe(() => {
  for (const s of workspace.getSnapshot().sessions) {
    if (s.state === "exited" && s.exitStatus !== null) {
      console.log(`${s.id} exited with code`, exitCodeFromStatus(s.exitStatus));
    }
  }
});
```

## Your service, our server: `fd-channel` mode

`fd-channel` lets an external process own `yas server`'s lifecycle and control which clients connect via `SCM_RIGHTS` fd passing. See the [transport reference](docs/transports.md#fd-channel) and the working examples:

- [Python](examples/fd-channel-python.py)
- [Bun](examples/fd-channel-bun.ts)
