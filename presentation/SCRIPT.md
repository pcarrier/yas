# YAS — presentation script

Written for roughly 30 minutes at a conversational pace. The slide text is deliberately dense; this is the spoken path through it, not a second copy of every bullet.

## 01 · YAS

This talk is about one fairly simple preference: when I connect to another machine, I want the useful state to still be there.

The shell is part of that. A very important part. Then there are terminals I already started, processes still running, files, Git, diagnostics, application windows, clipboard, audio, cameras, networking, and a few other details I will get to.

YAS puts those things behind one server and one protocol model. The scope is broad. The idea is not: the connection may disappear; the workspace should not.

## 02 · A shell stopped being enough

I live in terminals. I am not here to explain that shells are obsolete. A shell is still where I start.

The problem begins roughly thirty seconds later. I want to edit a file, inspect a diff, follow diagnostics, open a browser, hear audio, copy an image, or reach a service that only the remote machine can see.

We have an answer for every one of those things: another tunnel, another daemon, another protocol, another piece of state to recover. They work, separately. The resulting workspace does not feel like one place, because technically it is not one place.

## 03 · The server stays. Clients come and go.

The first useful decision in YAS is that the connection is not the workspace.

Terminals, processes, windows, routes, and watched state live on the server. A browser can vanish. A phone can appear. A CLI can inspect the same terminal while an agent waits for a command to finish. None of them needs to own the underlying resource.

On reconnect, the client gets a bounded snapshot and then ordered changes. If history is missing, the protocol says so. I wanted disconnection to be boring: an expected state transition, not an invitation to guess what survived.

## 04 · I kept finding one more thing to remote

This is the point where calling YAS a terminal multiplexer stops being useful.

There are twenty protocol families. Some establish the connection and recover state. Some expose things people see and control: terminals, application windows, clipboard, notifications, media, fonts. Some work with code. Some run programs or reach networks. A few exist because the server itself needs to be observable.

The grouping here is by what the user is trying to do. The wire has numeric family IDs, naturally, but nobody wakes up wanting to use family 0x0032. They want a definition or a diagnostic.

## 05 · Local should be instant. Remote should survive.

I wanted one protocol to work over a local socket and over a bad WAN without making either case miserable.

That means every viewer is paced independently. The workstation can have a large, fast surface while the phone gets fewer, smaller frames. The reliable stream remains authoritative; loss-tolerant delivery is used only where the owning feature knows how to recover.

Current status, without decorative optimism: Linux on x86_64 and arm64 has the full feature set. macOS arm64 and Windows x86_64 currently have PTY multiplexing. Every v1 family is implemented, but the complete cross-platform, load, latency, and fuzz qualification matrix is not finished.

## 06 · The server owns the useful state

This diagram is the architecture in one slide.

PTYs, files, processes, networking, Wayland, desktop services, and media belong to the server. Each browser, CLI, agent, or extension sends its own HELLO and gets an independent session.

That distinction matters. Clients may share resources, but they do not share a connection or presentation state. The phone is not a tiny mirror of the laptop. The agent does not inherit the browser's viewport. An edge can authenticate and adapt WebSocket framing, but it does not secretly become a second server.

## 07 · Negotiate once, then be boring

Core exists to make the rest of the protocol unsurprising.

The handshake selects versions, operations, codecs, limits, and platform facts. An admitted request receives exactly one matching result. Message-kind IDs stay stable so a packet capture is readable six months later. One receive budget prevents a peer from hiding unbounded data across several frame types.

PING measures round-trip time and clock offset. CANCEL still settles the request. GOAWAY drains admitted work. SHUTDOWN returns its result before the server announces closure. None of this is exciting, which is precisely what I want from the foundation.

## 08 · Large payloads get flow control, not special cases

The control protocol should not contain a dozen almost-identical ways to send a large body.

Transfer provides byte streams and message streams with explicit offsets, boundaries, credit, close, and reset. Files, query pages, process output, relay links, fonts, clipboard data, and channels all reuse it.

Transfer does not decide what the bytes mean. Closing a file upload does not install the file; Filesystem still validates and commits it. The SENSITIVE flag only keeps routine diagnostics from logging payloads. It is not encryption. That comes from the transport.

## 09 · Reconnect is a state transition

Most interesting resources are watched state.

A watch begins with a bounded snapshot, continues with ordered deltas, and acknowledges both the applied revision and more byte credit. On reconnect, the client presents the server boot ID and its last revision.

If the history is retained, deltas continue. If it is gone, the server sends an explicit reset and a new snapshot. There is no clever attempt to hide the gap. Mutations carry operation IDs for the same reason: if the result was lost, retrying should recover the outcome, not perform the mutation twice.

## 10 · A route changes the machine, not the tool

I have several machines and very little patience for remembering how each one is reached.

YAS saves names such as `rabbit`, `prod`, or `local:work`. SSH routes can bootstrap YAS and use the normal agent, key files, selected SSH config fields, and known-host checks. The Relay catalogue exposes names and availability, not connector secrets.

CONNECT then opens a complete nested YAS session. The home server forwards bytes and half-closes; it does not parse the inner frames or translate handles. I can change the destination with `--on prod` and keep using the same terminal, Git, process, and surface commands.

## 11 · Also: find the forgotten tabs

Once clients are independent sessions, it becomes useful to know which ones still exist.

The Client family reports the build, label, origin, connection age, idle time, traffic totals, current sampled bandwidth, active terminal and surface views, and state watches. The browser has a Connected clients panel; the CLI has `yas client list`.

Disconnect is orderly and carries a reason. It removes one session. Stopping the entire server is a different Core operation. This slide exists because I had forgotten tabs using bandwidth and decided the protocol should admit that this happens.

## 12 · A terminal is a resource, not a connection

Now the terminal part, which remains important even after all that scope expansion.

A terminal has a stable ID, tag, title, command, working directory, dimensions, generation, journal cursor, optional deadline, application endpoint, and eventual exit state. The launch record is retained, so restart can replay it or replace it explicitly.

Exited terminals remain listable and inspectable. Signals target the process group. A deadline can act as a dead-man switch. Native attach still works, and sessions can be recorded as timestamped YASREC1 data. Leaving the browser does not define the terminal lifecycle.

## 13 · One PTY, many views

The PTY may be shared. The viewport is not.

Each mounted view chooses rows, columns, scroll, focus, frame rate, display metrics, and queue targets. A phone and workstation can look at one terminal without resizing each other or fighting over scroll position.

Views report what was actually presented, including paint backlog, available slots, RTT, goodput, and jitter. Focused work gets priority; previews use spare capacity. YAS can predict local echo, but only when PTY echo and canonical mode make that prediction correct. Password prompts are a poor place for optimism.

## 14 · Screenshots would have been much easier

I did not want terminal video. I wanted the terminal the application intended.

The server keeps a semantic grid: palette, true color, styles, wide cells, grapheme clusters, combining sequences, emoji, cursor, title, modes, scrollback, line flags, links, and OSC 7 working directory.

Font faces and metrics can come from the server, subject to export policy. The browser renders through WASM into WebGPU, WebGL2, or Canvas 2D, with selection, links, emoji, predicted echo, and the scrollbar as separate layers. Screenshots would have been much easier. They would also have thrown away nearly everything useful.

## 15 · Browser input has more edge cases than expected

Sending keys sounds simple until a browser, a terminal mode, an input method, and a remote application all disagree about what a key means.

YAS keeps physical keys, control bytes, committed text, and terminal mouse events distinct. Application-cursor mode changes arrow sequences. IME sends the final multi-codepoint text rather than guessed keystrokes. Mouse support covers the usual X10, VT200, motion, SGR, and pixel variants.

Selection remains available even when the application owns terminal mouse mode. Clipboard reads the current Wayland owner when one exists, otherwise the host clipboard. Agents use the same send, click, resize, focus, paste, and pointer operations.

## 16 · The terminal is already parsed. Use it.

Once the server has parsed the terminal, throwing that structure away would be odd.

SHOW returns the current viewport as plain text or ANSI. History can be paged by offsets or continuation cursor. Search can score titles, visible text, and scrollback, or run ripgrep-compatible matching while rejoining soft wraps. Copy preserves overflow graphemes and hyperlink context.

With OSC 133 shell integration, commands get stable IDs, status, exit code, timing, ranges, and command text. A client can wait for completion or a regex on the server instead of polling and scraping the screen. This is useful for people; it is extremely useful for agents.

## 17 · Most terminal frames are tiny

The grid codec is optimized for what terminals actually do: change a little bit, very often.

Each common cell is a fixed twelve bytes ready for the renderer. For every changed region, the encoder can choose a run, sparse list, bitmap, copy rectangle, or fill rectangle. Copy rectangles make scrolling cheap. Byte planes can be transposed, and LZ4 is used only when it saves at least eight bytes.

Rich side data stays separate. Every frame and decoded allocation is bounded. Deltas name a verified base; if the ancestry is missing, the client needs a keyframe. Again: no guessing.

## 18 · I wanted the window, not somebody else's desktop

The Wayland part began with a preference: I usually want one remote application, not an entire borrowed desktop with somebody else's panel and wallpaper around it.

Each top-level window is a Surface resource with title, app ID, origin, parent, size, scale, lifecycle, cursor, text-input state, and activation state. The server stamps launch identity, and icons are resolved lazily from session desktop entries.

Popups and subsurfaces stay composed with their top level. Activation asks for attention instead of stealing focus. Windows can be listed, searched, focused, resized, closed, captured, or recorded individually.

## 19 · Real applications expect a real compositor

Streaming individual windows only works if applications believe they are talking to a real Wayland environment.

The headless compositor implements the core compositor and xdg-shell, decorations, viewports, fractional scaling, presentation timing, activation, cursor shapes, relative pointer, constraints, text input, selection, data devices, and multitouch.

Configure and acknowledgement, maximize, fullscreen, parent relationships, popup grabs, dismissal, and hit testing all remain normal Wayland behavior. Portal dialogs get proper xdg-foreign parents. Native Wayland apps run directly; xwayland-satellite can cover X11. Chrome, Electron, and mpv are very effective tests of whether “real” is deserved.

## 20 · Input is where fake desktops fall apart

Video is visible, so it gets the attention. Input is where the illusion usually breaks.

Keyboard events preserve physical identity, repeats, modifiers, and guaranteed releases when focus disappears. Text input carries committed UTF-8 and IME preedit separately. Pointer events include smooth axes, wheel detents, relative motion, constraints, and proper stop events. Cursors may be named, custom RGBA, scaled, or hidden. Touch keeps simultaneous contact IDs, with optional pointer-like gestures.

Coordinates use signed 32.32 fixed point in the application's native composited frame. Scaling the video does not change the coordinate system sent to the application.

## 21 · Clipboard ownership is state too

Clipboard is not just a string in a global variable.

Clipboard, primary selection, and drag-and-drop have owners, revisions, lifecycles, items, and MIME types. Small values can be inline; large ones use Transfer and staged content. The browser bridge handles text and images without truncating them or quietly serving an older value.

Paste publishes the browser selection before releasing Ctrl+V, which avoids a delightful race. An empty browser clipboard does not erase a valid Wayland owner. Copy can cross surface to surface or surface to terminal even if the local operating-system clipboard refuses the write. Remote file drags get per-session staging and a live URI offer.

## 22 · Every viewer gets a different video stream

The compositor accepts real application buffers: shared memory or DMA-BUF, common channel orders, accumulated damage, Vulkan when available, CPU when not.

After composition, every view selects its own extent, pixel ratio, frame rate, latency target, decoder capacity, quality, chroma, GOP, and keyframe policy. The encoder chain can use NVENC, VA-API, Vulkan Video, or software for H.264 and AV1.

Frames are latest-biased because an obsolete perfect frame is still obsolete. Configuration and recovery points remain reliable. The browser decodes with WebCodecs. Still capture and raw recording use the same pipeline with explicit format and timing controls.

## 23 · A desktop app is more than its pixels

Once applications ran, they immediately asked for the rest of the desktop.

Tray items and notifications arrive as revisioned structured state, not pixels baked into a desktop stream. StatusNotifier covers icons, attention, overlays, tooltips, activation, alternate activation, and scrolling. DBusMenu supplies a bounded menu tree with revision-checked actions. Notifications include replacement, expiry, actions, replies, progress, and close reasons.

The browser decides whether that becomes a status item, menu, card, toast, or opt-in system notification. Applications get a private D-Bus with the services YAS provides. The host desktop bus is not handed over wholesale.

## 24 · Then came audio, cameras, portals, and MPRIS

This slide is where the scope expansion becomes particularly visible.

Server audio can play in the browser. A remote app can lease the viewer's microphone or camera, with browser and operating-system consent represented in protocol state. Denial affects that capability, not the whole application. Access and ScreenCast portals expose deadlines, choices, and surface candidates.

MPRIS carries player identity, track metadata, artwork, position, capabilities, and revision-checked controls. Media keeps capture and presentation timestamps. The browser reports audible-versus-visible latency, and YAS publishes that cost through PipeWire so applications can compensate without YAS delaying video. This actually works, which still amuses me.

## 25 · Yes, fonts are protocol state

Fonts look like presentation detail until the terminal grid shifts, a diff wraps differently, or a remote application asks for a face the browser does not have.

The Font family exposes real families, faces, style, weight, stretch, slant, metrics, variable axes, localized names, color support, and Unicode coverage. Bytes are exportable only when server policy and the font's embedding metadata both permit it.

Large faces use Transfer and BLAKE3 verification. Content hashes make the cache useful across restarts and route changes. There is no hidden font endpoint in the edge. Yes, this is a protocol family. I am comfortable with that.

## 26 · Files without a mount

For files, I wanted remote editing without pretending the remote tree is a local filesystem.

A root can be an exact platform path, a terminal or process working directory, one file, or per-session drag staging. Paths preserve platform components and raw bytes; the protocol does not helpfully normalize them into a different path.

Watches send snapshots and structural changes with metadata, hashes, lazy content, rename identity, and explicit reset. Queries cover reads, stats, hashes, links, ranked search, indexing, and grep with typed pages. Writes are staged, hashed, preconditioned, and atomically committed, individually or in a batch.

## 27 · Git without parsing Git's prose

Git has excellent human output. It is still a poor application protocol.

YAS watches HEAD, refs, remotes, operations, status, upstreams, stashes, and worktrees. Typed queries cover resolution, merge bases, logs, trees, blobs, diffs, patches, the index, discovery, blame, reflog, and worktrees.

SHA-1 and SHA-256 identity remain explicit. Large bodies use Transfer. Structured patches preserve files, rows, spans, gaps, merge bases, and text-versus-binary modes. Watched queries rerun when their dependent refs move. Fetch is retry-safe and reports phases, counters, and one final result per remote ref.

## 28 · LSP without leaking LSP

Language servers already provide the right intelligence, but I did not want every remote client to become an LSP client.

The server alone speaks JSON-RPC and owns backend IDs, progress, capabilities, memory use, diagnostics, and shared unsaved buffers. YAS exposes definition, references, hover, symbols, completion, actions, formatting, rename plans, and signatures as typed records.

Wire positions are zero-based UTF-8 byte offsets, not leaked UTF-16 LSP positions. Every answer names its document revision and hash. Buffer updates use compare-and-swap or staging. A rename returns a plan; Filesystem performs the writes.

## 29 · The browser became an IDE by accident

After Filesystem, Git, and LSP existed, leaving them as protocol demonstrations seemed wasteful.

The browser now has Explorer, Search, and Problems panels fed by native state. Editor, diff, commit, web, terminal, and Wayland application panes share one tiling and focus model. Saves use Filesystem compare-and-swap and atomic commit, so a stale buffer becomes a visible conflict.

The boundaries remain useful: Git and LSP analyze, Filesystem edits, Process or Terminal runs arbitrary commands. The IDE is mostly composition. It did not require inventing another server API, which is a pleasant consequence of having gone too far earlier.

## 30 · Small state needed a home

Not everything is a file, and not everything deserves a database schema.

KV stores persistent binary key-value namespaces, ordered by prefix and watchable by revision. Entries carry hashes, timestamps, and optional inline values. Larger values use staged Transfer. PUT, DELETE, and transactional BATCH support revision or hash preconditions and operation IDs.

Deduplication survives server restart. Saved remotes, extension intent, and workspace-session state live here: panes, focus, route choice, panel state. Source code does not. KV is deliberately boring storage for the bits of workspace intent that otherwise end up in improvised files.

## 31 · Not every program wants a PTY

A lot of remote-execution tools start a pseudo-terminal because that is the only abstraction they have. Programs often did not ask for one.

Process launches an exact argv and environment with no implicit shell. The working directory can come from a server path, terminal, process, or Filesystem root. Standard streams are bounded Transfers with lifetime offsets, and stderr merging is explicit.

Clients can watch PID, owner, lifecycle, detach policy, offsets, exit, and retention. Late attachment gets the current offset or an explicit gap. One client owns stdin; many can observe output. Signal, terminate, kill, detach, and wait remain distinct operations. Launch retries cannot spawn the program twice.

## 32 · The server can reach things the client cannot

Sometimes the useful property of the remote machine is simply its network position.

Network opens TCP, UDP, Unix stream, datagram, or seqpacket sockets, and Windows byte or message pipes. It can add TLS with SNI and ALPN. Higher-level protocols stay in client libraries: HTTP, DNS, PostgreSQL, WebSocket, whatever is actually needed.

The CLI builds familiar forwarding and SOCKS5 workflows on top, with server-side name resolution. Datagram use can be required, preferred, or replaced explicitly with a reliable tunnel. Sequence and drop information stay visible. Destination policy is enforced, and listeners bind to loopback unless asked otherwise.

## 33 · Tools needed a place to meet

Extensions and tools also need a small rendezvous mechanism. A terminal is not a message bus, however tempting that shortcut may be.

A Channel publishes a watched UTF-8 name, metadata, one current listener, and a generation. Connect and accept create a bidirectional message Transfer with exact boundaries and credit. Generation checks prevent a raced command from landing on a replacement listener.

Existing conversations survive listener closure and end through their own Transfer lifecycle. The messages can contain RPC, streaming results, actor mailboxes, or command schemas. Channel does not pretend to be pub-sub. It is one listener and point-to-point conversations, on purpose.

## 34 · Extensions use YAS too

The extension system does not get a magical privileged API.

Wasmi or QuickJS modules are addressed by BLAKE3. Objects are staged, verified, and atomically committed; a durable declaration specifies runtime, arguments, limits, restart backoff, persistence, and enabled state.

Each attempt receives a complete in-process YAS session and uses the same negotiated families as any other client. Commands arrive through Channels. Output and logs have replay and explicit gaps. Deployment and control deduplication survive restart. This does not make extensions unprivileged—their session authority still matters—but it avoids a second, undocumented capability surface.

## 35 · Observation must not become backpressure

I wanted enough diagnostics to understand the system without making diagnostics part of the failure.

Events records stable typed event IDs into a process-wide bounded binary ring. Configuration is revisioned. Clients can dump retained history, follow into live events, and receive an exact GAP when requested data has been overwritten. The server can also record selected events to a path with final counters.

The important rule is that observers never backpressure producers. A slow trace reader loses history; it does not slow the terminal or audio thread. Raw frames, PTY bytes, environment, and content are disabled by default because useful diagnostics do not require casually collecting every secret.

## 36 · Sometimes the exact environment is the bug

Environment is a tiny family with a deliberately sharp trust boundary.

It returns the exact server process environment captured at boot, plus derived values such as the effective server name. Keys and values preserve raw platform bytes in deterministic order. Small records are inline; a larger snapshot uses Transfer. It is immutable: GET only, no watch, no write.

Nothing is redacted. That is the point, because a carefully curated approximation is often useless while debugging. It also means only full-authority sessions receive it. If that sounds dangerous, good—the protocol should make the danger obvious.

## 37 · The transport is replaceable

The same YAS session can run over Unix sockets, Windows pipes, TCP, SSH, WebSocket, WebTransport, or WebRTC.

Every link uses the same preface, HELLO, families, and resource semantics. Only framing and optional datagram availability change. The reliable path remains authoritative everywhere.

WebTransport and WebRTC may carry eligible Surface, Media, and native Network events as unordered, non-retransmitted datagrams. The owning family decides how loss and ordering work and always has a reliable fallback. Terminal frames remain reliable. I did not want “which transport are you on?” to become the first question in every feature implementation.

## 38 · The path changes. The session does not.

Locally, the browser reaches the home server through the embedded edge. Remotely, the home server can keep a Relay route warm to another YAS server. For sharing, `yas share` connects the guest browser over WebRTC to the home server; the server can also integrate that forwarder directly.

These paths differ in routing and process placement, not in the client resource model. Workspace state remembers route choice, panes, focus, and panels in KV.

Handles remain scoped to the server that issued them. A home-server terminal handle does not magically become a remote-server terminal handle just because a diagram has an arrow between them.

## 39 · Full control is full control

The authority model is intentionally simple and should not be softened in the wording.

A normal session runs with the server process's operating-system identity and can invoke every operation advertised to it: terminals, files, processes, environment, networking, and configured Relay routes. The browser passphrase is a bearer credential for that authority. It is not an account and not a viewer token.

WebRTC sharing normally remains read-write. A derived `.ro` token negotiates a fixed server-enforced read-only catalogue for selected Terminal, Surface, Media, and Font observation. YAS v1 has no general user or family ACL. Mutually untrusted users need separate server processes and OS identities. SENSITIVE is still not authorization.

## 40 · Humans and agents use the same resources

There is one protocol model and several ways into it.

`yas open` gives a person the browser workspace. The CLI can start a persistent terminal, inspect it without attaching, run a pipe-oriented process, capture one application window, or direct a Git operation through the `prod` route. React and Solid bindings embed terminal and surface views in other applications.

The published agent skill exposes the same resource operations. An agent does not receive a parallel automation-only approximation of the workspace. Its actions and results appear in the same terminal, process, repository, and surface state a person can inspect.

## 41 · One command to start. One command to share.

Installation is intentionally unceremonious.

On Unix, fetch the installer and pipe it to the shell. On Windows, the equivalent uses `irm` and `iex` in PowerShell. Then `yas open` discovers or starts the local service and opens the workspace.

`yas share` publishes it over WebRTC and prints the credentials. The normal passphrase allows interaction; the derived read-only token is available when observation is enough.

The first useful workspace needs no external database, reverse proxy, display server, or GPU. There are optional accelerators and integrations, naturally. They are not prerequisites for finding out whether the thing works.

## 42 · The shell is still there. It is just not alone.

This started as remote execution and became an attempt to make the remote machine feel like one place.

Start a job, close the laptop, and pick it up from a phone. Open one application window and keep its title, icon, input, clipboard, audio, and notifications. Move from local to `prod` without changing tools. Let a browser, CLI, extension, and agent share current state without sharing one viewport.

The shell remains excellent. YAS does not replace it. It gives the shell—and everything I kept needing thirty seconds later—the same durable, inspectable, recoverable home.

That is YAS. Broad in scope, deliberately one workspace.

## Sources

- Technical source of truth: [`README.md`](../README.md), [`docs/design/yas.md`](../docs/design/yas.md), and [`protocol/yas/wire.md`](../protocol/yas/wire.md).
