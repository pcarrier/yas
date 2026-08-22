# yas

Terminal multiplexer and experimental Wayland compositor for browsers and AI agents. Nothing to configure, no required dependencies.

We publish a [computer agent skill](https://yas.run/SKILL.md).

Install and run locally:

```bash
curl -sf https://yas.run | sh
yas open # opens a browser
```

Share over WebRTC:

```bash
yas share # prints a URL anyone can open
```

Manage named remotes and connect to them:

```bash
yas remote add rabbit ssh:rabbit          # save a named remote
yas remote add prod ssh:alice@prod.co     # another one
yas remote add work local:work            # a named local server
yas remote list                           # show all remotes
yas remote set-default rabbit             # make rabbit the CLI default

yas open                                  # local + all configured remotes
yas terminal list                         # lists terminals on rabbit
yas --on prod terminal list               # one-off override
yas --on ssh:newhost terminal list        # full URI also works
```

The CLI default remote is stored in `~/.config/yas/yas.conf` as
`yas.target = rabbit` and can also be set via the `YAS_TARGET` environment
variable. Named remotes live in the home server's own KV store, under the
`remotes` key, which is per instance; the server watches it and publishes all
routes over Relay. Any client of that server can read the stored URIs,
credentials included.
`yas open` shows the home server and its routes; the modern browser keeps its
active route selection in the attached backend workspace session, alongside
its layout and panel state. The browser URL names only that session. SSH
remotes are auto-installed on first connection.
`local:NAME` entries work in both the CLI and the home server's Relay
catalogue.

Forward ports to whatever the server can reach — `ssh -L` over any yas
transport, plus UDP:

```bash
yas forward 8080:localhost:3000                # local 8080 → server's :3000
yas forward 8080:localhost:3000 5432:db:5432    # a list, over one connection
yas forward udp/5353:resolver.internal:53       # UDP too
yas forward add web 8080:localhost:3000         # remember it
yas forward --all                               # start every saved forward
```

Or proxy everything the server can reach through one port — `ssh -D`:

```bash
yas socks 1080                                  # SOCKS5 on 127.0.0.1:1080
curl -x socks5h://localhost:1080 http://api.internal/
```

Names are resolved on the server, so `socks5h://` (or a browser set to proxy
DNS) reaches hosts your machine cannot look up.

Listeners bind to loopback unless you name a bind address. The relay reaches
whatever the server reaches; restrict it with
`yas server --allow-forward 'host[:ports]'`. Saved forwards live in
`~/.config/yas/yas.forwards` (mode 0600). See
[docs/design/net.md](docs/design/net.md).

Control terminals programmatically:

```bash
yas terminal start htop # start a terminal, print its ID
yas terminal show 1     # dump current terminal text
yas terminal send 1 q   # send keystrokes
yas terminal journal 1  # commands the shell has run (needs OSC 133)
yas terminal output 1 --wait 60  # that command's output
```

Run a pipe-oriented process without a terminal, connecting its stdin, stdout,
and stderr and returning its exit code:

```bash
yas run --in /src/yas --env RUST_LOG=debug -- cargo test
```

`--in` selects the server-side working directory and `--env` is repeatable.
The program is executed directly; use an explicit shell when shell syntax is
needed.

Inspect and disconnect other clients attached to the same server:

```bash
yas client list
yas client disconnect "$SESSION_ID" --reason "duplicate browser tab"
```

In the browser, open the Ctrl/Cmd-K menu and choose **Connected clients** to
see a live list of every client's age, measured outbound bandwidth, audio,
filesystem, Git, LSP, KV, network, terminal, and surface subscriptions. Terminal
and surface entries include their requested view sizes. Disconnecting asks for
confirmation and lets you give the peer a reason.

Ordinary connections that can reach the private server socket receive the full
catalogue and may list or disconnect peers. Read-only `yas share` consumers use
a REQUIRED Core HELLO marker; the server itself selects a least-authority
catalogue and rejects every unadvertised operation. A share can observe the
selected Terminal, Surface, Media, and Font state, but cannot enumerate peers,
create or control resources, inject input, or shut down the server.

Run GUI apps — on Linux, every terminal includes an experimental headless Wayland compositor:

```bash
yas terminal start foot    # launch a Wayland terminal emulator
yas surface list           # list graphical windows
yas surface capture 1      # screenshot a surface
yas surface click 1 100 50 # click at (x, y)
yas surface type 1 "hello{Return}" # type into a GUI window
```

The server auto-starts when needed.

Run isolated local instances by giving the server a name, or address one as
`local:<name>` and let the client auto-start it:

```bash
yas server --name work
yas --on local:work terminal list
yas --on local:test terminal start htop # auto-starts a second instance
```

Every server has a name; omitting `--name` uses `default`. Each instance uses a
named socket and stores its KV database, installed extensions, and object cache
under `yas/instances/<name>/` in the platform state/cache directories. The
`@session` extension's intent is in that instance's KV database, while
`@muster` reads the corresponding platform configuration directory at
`yas/instances/<name>/muster/`. Explicit `YAS_SOCK`, `YAS_KV_PATH`,
`YAS_EXTENSION_PATH`, `YAS_WASM_CACHE`, and `YAS_MUSTER_DIR` overrides still
win. Add `--export-sock` if commands launched in an instance's terminals should
automatically target that instance.

## Supported platforms

| Platform | Arch          | Wayland compositor | Notes                 |
| -------- | ------------- | ------------------ | --------------------- |
| Linux    | x86_64, arm64 | Yes                | Full features         |
| macOS    | arm64         | No                 | PTY multiplexing only |
| Windows  | x86_64        | No                 | PTY multiplexing only |

SSH remotes are auto-installed on first connection. Requirements on the remote:
`curl` or `wget`, CA certificates, and a supported OS/arch.

The embedded SSH client authenticates via ssh-agent (`SSH_AUTH_SOCK`) or key files
(`~/.ssh/id_{ed25519,ecdsa,rsa}`), and resolves `~/.ssh/config` for Hostname,
User, Port, and IdentityFile.

## Install

```bash
curl -sf https://yas.run | sh
```

The default binary is MIT-licensed (software H.264 via openh264). On Linux
you can opt into a GPL build that uses x264 (GPL-2.0-or-later) for better
software H.264 instead:

```bash
curl -sf https://yas.run | YAS_GPL=1 sh
```

Every binary prints its exact terms with `yas --license`.

### Windows (PowerShell)

```powershell
irm https://yas.run/install.ps1 | iex
```

This downloads `yas.exe` to `%LOCALAPPDATA%\yas\bin` and adds it to your user `PATH`. Set `YAS_INSTALL_DIR` to override the install location on Windows.

## How it works

`yas` hosts terminals and tracks their parsed state. Through the native Terminal
family, each browser watches complete lifecycle State and opens its own grid
view. The server sends sequenced keyframes or deltas in the negotiated grid
codec; deltas can use LZ4 compression, cell patches, and copy rectangles. The
browser validates and acknowledges applied frames, then renders them with
WebGL.

On Linux, every yas server includes an experimental headless Wayland compositor shared by all terminals. GUI applications launched inside any terminal (anything that speaks the Wayland protocol — terminal emulators, browsers, editors, media players) automatically connect to it. Surfaces are captured, encoded as H.264 or AV1 video, and streamed to connected browsers in real time. No X server, no display, no GPU required — rendering uses GPU compositing (Vulkan via dlopen) when available, with a CPU software fallback. Encoding uses openh264 or x264 (a build-time choice, see Install) and rav1e, with optional NVENC or VA-API hardware acceleration on Linux. The compositor is available on Linux only.

Each client is paced independently based on render metrics it reports back: display rate, frame apply time, backlog depth. A phone on 3G doesn't stall a workstation on localhost. The focused terminal gets full frame rate; background terminals throttle down. Keystrokes go straight to the PTY — latency is bounded by link RTT.

`yas open` opens the browser with an embedded YAS edge. For persistent browser
access, `yas edge` authenticates one `yas.v1` WebSocket, serves the web app, and
adapts it to one fixed native YAS home socket. The eight-byte YAS preface is
forwarded raw; each later WebSocket message maps byte-for-byte to one
length-prefixed YAS frame on that socket. Relay routing and font service live
in `yas server`; the edge has no destination mux, configuration WebSocket,
font side channel, or alternate wire protocol. `yas server` can also run
standalone for headless/daemon use. For embedding in your own app,
[`@yas-run/react`](EMBEDDING.md) and [`@yas-run/solid`](EMBEDDING.md) provide
framework bindings.

The browser passphrase is a full-control credential, not a viewer token. A connected client can use every family the home server advertises, including opening every Relay route with credentials held by that server. Route URIs and credentials never enter route snapshots or routine errors in the default edge mode. Font metadata is visible when enabled, but font bytes require `YAS_FONT_EXPORT=1` and remain subject to each face's OS/2 embedding restrictions. Because the passphrase travels in the WebSocket handshake flow, non-loopback deployments must use WSS through a TLS reverse proxy; exposing the default plaintext listener to an untrusted network leaks full server authority. See the complete [trust model](docs/design/yas.md#trust-model).

`yas proxy-daemon` is a connection pool that makes remote connections feel local. It runs as a persistent background daemon per user session, maintaining pre-warmed connections to each upstream target so browser tabs connect instantly without paying SSH negotiation or TCP handshake cost. The proxy auto-starts transparently on Unix and Windows — set `YAS_PROXY=0` to opt out.

For the native protocol entry point, see [docs/protocol.md](docs/protocol.md);
the canonical contract is [docs/design/yas.md](docs/design/yas.md) plus the
[generated wire registry](protocol/yas/wire.md). System topology and transport
deployment are covered by [ARCHITECTURE.md](ARCHITECTURE.md) and
[docs/transports.md](docs/transports.md).

## Configuration

| Variable                | Default                            | Purpose                                                                                                                                                                      |
| ----------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `YAS_SOCK`              | owner-only runtime directory       | Exact native YAS override used by YAS clients and the fixed-home edge                                                                                                        |
| `YAS_SERVER_UID`        | edge process effective UID         | Numeric kernel peer UID required by standalone and embedded edges for the native home socket (root is also accepted); set it for explicit cross-UID deployments              |
| `YAS_PROXY_SOCK`        | owner-only per-user runtime socket | Exact proxy socket override; automatic Unix sockets use a validated mode-`0700` runtime directory                                                                            |
| `YAS_PROXY_UID`         | client process effective UID       | Numeric kernel peer UID required for the proxy (root is also accepted); cross-UID use also requires an explicitly permissioned `YAS_PROXY_SOCK`                              |
| `YAS_SERVER_NAME`       | `default`                          | Server instance name (also `yas server --name NAME`); isolates the socket, state, cache, and built-in extension settings. Address it with `--on local:NAME`                  |
| `YAS_EXPORT_SOCK`       | unset                              | Set to `1` (or pass `--export-sock` to `yas server`) to export the server's socket path as `YAS_SOCK` in spawned terminals, so `yas` commands inside them target that server |
| `YAS_INJECT_PATH`       | unset                              | Set to `1` (or pass `--inject-path` to `yas server`) to append the server binary's directory to `PATH` in spawned terminals, so `yas` itself is callable inside them         |
| `YAS_UPLINK_TOKEN`      | unset                              | Bearer token for the `yas uplink` control endpoint                                                                                                                           |
| `YAS_TARGET`            | unset                              | Default remote for non-browser CLI commands: a URI or named remote (overrides `yas.target` in `yas.conf`)                                                                    |
| `YAS_REMOTES`           | `~/.config/yas/yas.remotes`        | Only the file the one-time import reads; the live catalogue is the home server's `remotes` KV key                                                                            |
| `YAS_RELAY`             | `1`                                | Set to `0` on the home server to disable route publication and nested connections                                                                                            |
| `YAS_FONTS`             | `1`                                | Set to `0` on the server to disable font enumeration, descriptions, and fetch                                                                                                |
| `YAS_FONT_EXPORT`       | unset                              | Set to `1` to permit font byte export when the face's embedding metadata also allows it; secure default is metadata-only                                                     |
| `YAS_FONT_DIRS`         | platform defaults                  | Additional server font scan roots (`:`-separated on Unix, `;`-separated on Windows)                                                                                          |
| `YAS_SCROLLBACK`        | `10000`                            | Scrollback rows per PTY                                                                                                                                                      |
| `YAS_EVENTS_SIZE`       | `1048576`                          | Binary server event-ring capacity                                                                                                                                            |
| `YAS_EVENTS`            | low-throughput lifecycle events    | Fine-grained event names/selectors (`default,+frame.*,+pty.*`, or `all`)                                                                                                     |
| `YAS_EVENTS_FILE`       | unset                              | Stream binary events to this server-side file from startup                                                                                                                   |
| `YAS_HUB`               | `wss://yas.run`                    | Default WebRTC signaling hub for CLI and read-only share connections                                                                                                         |
| `YAS_PASSPHRASE`        | unset                              | Passphrase for `yas edge` or `yas share`; an edge also accepts an Argon2 PHC hash from `yas hash-passphrase`, while browsers still enter the plaintext                       |
| `YAS_EDGE`              | unset                              | `1` makes `yas server` serve the browser itself, instead of a separate `yas edge` (also `--edge`)                                                                            |
| `YAS_SHARE`             | unset                              | `1` makes `yas server` publish itself over WebRTC, instead of a separate `yas share` (also `--share`)                                                                        |
| `YAS_EDGE_PASSPHRASE`   | `YAS_PASSPHRASE`                   | The edge's own passphrase, for one server hosting both an edge and a share                                                                                                   |
| `YAS_SHARE_PASSPHRASE`  | `YAS_PASSPHRASE`                   | The share's own passphrase, for one server hosting both an edge and a share                                                                                                  |
| `YAS_TRUSTED_PROXY_IPS` | unset                              | Exact comma-separated reverse-proxy IPs whose bounded `X-Forwarded-For` chain may identify auth clients; all forwarding headers are ignored by default                       |
| `YAS_PREFIX`            | `/usr/local` or `~/.local` (Unix)  | Override install prefix (`bin/`, `lib/`, `share/` go under this)                                                                                                             |
| `YAS_INSTALL_DIR`       | `%LOCALAPPDATA%\yas\bin` (Windows) | Override install location (Windows PowerShell installer)                                                                                                                     |
| `YAS_SURFACE_ENCODERS`  | see below                          | Comma-separated encoder priority list (see below)                                                                                                                            |
| `YAS_SURFACE_BANDWIDTH` | `ultra`                            | Ceiling on video bandwidth: `low`, `medium`, `high`, `ultra`, or a raw AV1 quantizer `10`–`255`. Adaptation is always on and only moves cheaper than this                    |
| `YAS_SURFACE_SPEED`     | `realtime`                         | Encoder speed preset: `slow`, `medium`, `fast`, `realtime`, or a raw `10`–`255` (10 = slowest, 255 = fastest)                                                                |
| `YAS_VAAPI_DEVICE`      | `/dev/dri/renderD128`              | VA-API render node for hardware-accelerated encoding                                                                                                                         |
| `YAS_CUDA_DEVICE`       | `0`                                | CUDA device ordinal for NVENC hardware encoding                                                                                                                              |

### Surface video encoders

Set `yas server --surface-encoders` (or `YAS_SURFACE_ENCODERS`) to a
comma-separated priority list of encoders. The server tries each in order and
uses the first that works.

```bash
# Default priority (dedicated encode engines, compositor-resident, then software):
# av1-nvenc,h264-nvenc,av1-vaapi,h264-vaapi,av1-vulkan,h264-vulkan,h264-software,av1-software

# Force software AV1 only:
YAS_SURFACE_ENCODERS=av1-software

# Prefer NVENC, fall back to software:
YAS_SURFACE_ENCODERS=av1-nvenc,h264-nvenc,h264-software

# Same, as a flag — a typo here stops the server instead of being ignored:
yas server --surface-encoders av1-nvenc,h264-nvenc,h264-software
```

The other direction — what viewers may send from their camera and microphone —
is `--camera-codecs` / `--microphone-codecs` (or `YAS_MEDIA_CAMERA_CODECS` /
`YAS_MEDIA_MICROPHONE_CODECS`). Viewers choose within what both ends allow,
from the media panel. See [docs/server.md](docs/server.md).

| Value           | Codec | Backend          | Max resolution | Notes                                                                                      |
| --------------- | ----- | ---------------- | -------------- | ------------------------------------------------------------------------------------------ |
| `av1-nvenc`     | AV1   | NVIDIA NVENC     | 8192×4352      | RTX 40+ series; fastest AV1 encode                                                         |
| `h264-nvenc`    | H.264 | NVIDIA NVENC     | 3840×2160      | Requires proprietary NVIDIA driver                                                         |
| `av1-vaapi`     | AV1   | VA-API           | 8192×4352      | Intel/AMD GPU                                                                              |
| `h264-vaapi`    | H.264 | VA-API           | 3840×2160      | Intel/AMD GPU                                                                              |
| `av1-vulkan`    | AV1   | Vulkan Video     | 8192×4352      | On the compositor's GPU; per-client scaling and pacing; 4:4:4 where the driver supports it |
| `h264-vulkan`   | H.264 | Vulkan Video     | 3840×2160      | On the compositor's GPU; per-client scaling and pacing; 4:4:4 where the driver supports it |
| `h264-software` | H.264 | openh264 or x264 | 3840×2160      | Build-time choice (x264 = GPL opt-in)                                                      |
| `av1-software`  | AV1   | rav1e (software) | 3840×2160      | CPU-bound; capped to stay interactive                                                      |

The browser automatically detects the codec from each frame and configures
its WebCodecs decoder accordingly. Clients advertise which codecs they
support and the largest frame they can decode; the server skips encoders the
client can't decode.

The resolution ceiling is per viewer, not per surface. At ordinary display
scales a surface is composited at whatever its most capable subscriber can
receive — so an AV1 client on a 5K display gets a native 5120×2880 stream —
and any viewer whose encoder or decoder stops lower is served an
aspect-preserving downscale of that same surface rather than dragging it down
for everyone. At a sub-1× zoom the 1× compositor source may be larger than the
encode ceiling; the viewer still receives only its viewport-sized downscale.
Clients that don't report a decode ceiling (anything predating the field)
stay at 3840×2160.

For `yas edge` configuration, running as a systemd/launchd service, and Nix module setup, see [SERVICES.md](SERVICES.md) and [`nix/README.md`](nix/README.md).

### Optional dependencies

yas has no required dependencies — software H.264 and AV1 encoders are statically linked, and the CPU software renderer works everywhere. GPU acceleration and audio are enabled automatically when the right libraries or binaries are present. All GPU libraries are loaded at runtime via `dlopen`; missing ones are silently skipped.

**Video — GPU compositing and hardware encoding (Linux)**

| Library                                 | Used for                                         |
| --------------------------------------- | ------------------------------------------------ |
| `libvulkan.so.1`                        | GPU compositing, Vulkan Video encode             |
| `libva.so.2`, `libva-drm.so.2`          | VA-API hardware encode (Intel/AMD)               |
| `libgbm.so.1`                           | DMA-BUF allocation for zero-copy VA-API encoding |
| `libcuda.so.1`, `libnvidia-encode.so.1` | NVENC hardware encode                            |

Without any of the above, the compositor falls back to CPU rendering and software encoding. No configuration needed.

**Desktop services and audio (Linux)**

| Dependency             | Used for                                          |
| ---------------------- | ------------------------------------------------- |
| `pipewire`             | Audio daemon (private instance per compositor)    |
| `pipewire-pulse`       | PulseAudio compatibility for apps                 |
| `libpipewire-0.3.so.0` | Monitor capture (in-process, loaded via `dlopen`) |
| `dbus-daemon`          | Private desktop and PipeWire D-Bus sessions       |
| `wireplumber`          | Session manager (optional, started if available)  |
| `xwayland-satellite`   | X11 applications (optional, started if available) |

Audio is disabled automatically when PipeWire is not installed or `libpipewire-0.3.so.0` is not resolvable via `ld.so` (set `LD_LIBRARY_PATH` if you have it in a non-default location), or explicitly with `YAS_AUDIO=0`.

X11 applications run through `xwayland-satellite`, which yas starts once per session when the binary is on `PATH` and exports the resulting `DISPLAY` to every terminal. Wayland stays the first choice for anything that speaks it; X11 is the fallback behind it. Without the binary, sessions are Wayland-only and no `DISPLAY` is exported. Set `YAS_XWAYLAND=0` to opt out.

## How it compares

|                          | yas                                 | ttyd                | gotty               | Eternal Terminal      | Mosh                  | xterm.js + node-pty  |
| ------------------------ | ----------------------------------- | ------------------- | ------------------- | --------------------- | --------------------- | -------------------- |
| Architecture             | Single binary                       | Single binary       | Single binary       | Client + daemon       | Client + server       | Library (BYO server) |
| Multiple PTYs            | ✅ First-class                      | ❌ One per instance | ❌ One per instance | ❌ One per connection | ❌ One per connection | ⚠️ Manual            |
| Browser access           | ✅                                  | ✅                  | ✅                  | ❌                    | ❌                    | ✅                   |
| Delta updates            | ✅ Only changed cells               | ❌                  | ❌                  | ❌                    | ✅ State diffs        | ❌                   |
| LZ4 compression          | ✅                                  | ❌                  | ❌                  | ❌                    | ❌                    | ❌                   |
| Per-client backpressure  | ✅ Render-metric pacing             | ❌                  | ❌                  | ⚠️ SSH flow control   | ❌                    | ❌                   |
| WebGL rendering          | ✅                                  | ❌                  | ❌                  | ❌                    | ❌                    | ⚠️ Addon             |
| Transport                | YAS over WS, WebRTC, Unix, SSH, TCP | WebSocket           | WebSocket           | TCP                   | UDP                   | WebSocket            |
| Embeddable (React/Solid) | ✅                                  | ❌                  | ❌                  | ❌                    | ❌                    | ✅                   |
| Wayland compositor       | ✅ Built-in headless (experimental) | ❌                  | ❌                  | ❌                    | ❌                    | ❌                   |
| GUI app streaming        | ✅ H.264 / AV1                      | ❌                  | ❌                  | ❌                    | ❌                    | ❌                   |
| Agent / CLI subcommands  | ✅                                  | ❌                  | ❌                  | ❌                    | ❌                    | ❌                   |
| SSH tunneling built-in   | ✅                                  | ❌                  | ❌                  | ✅                    | ✅                    | ❌                   |

## Browser tips

### Disable Ctrl+W tab close (Chrome / Brave / Edge)

When using yas in the browser, `Ctrl+W` closes the browser tab instead of
reaching your terminal. Chromium-based browsers let you disable this:

1. Navigate to `chrome://settings/system/shortcuts`
   (or `brave://settings/system/shortcuts` in Brave)
2. Find the **Close Tab** shortcut and remove or reassign it

This frees `Ctrl+W` for terminal use (e.g. deleting a word in bash/zsh).

## Contributing

Building from source, running tests, dev environment setup, code conventions, and release process are all covered in [CONTRIBUTING.md](CONTRIBUTING.md). CI/CD pipelines, the install site, and the signaling hub are documented in [SERVICES.md](SERVICES.md). The crate and package map is in [ARCHITECTURE.md](ARCHITECTURE.md).
