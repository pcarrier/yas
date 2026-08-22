import { TEXT_EXPLANATIONS } from "./text-explanations";

export type ExplanationKind = "title" | "text" | "code" | "diagram";

interface ExplanationContext {
  slideId: string;
  slideTitle: string;
  kind: ExplanationKind;
  text: string;
  lineIndex?: number;
}

const SPECIAL: Record<string, string> = {
  "01-intro/01-yas::YAS":
    "YAS is a protocol and workspace server for operating a whole remote environment, not just a shell. The rest of the deck unpacks that promise.",
  "01-intro/01-yas::The shell stayed. The workspace grew around it.":
    "YAS keeps terminal work intact while making applications, files, language tools, media, routes, and durable workspace state available through the same server. It expands the remote session without replacing the shell.",
  "01-intro/01-yas::pcarrier.com for indent.com":
    "Pierre Carrier presents this work for Indent. The links open the corresponding sites in a new tab.",
  "01-intro/03-live::Retry safely, recover explicitly, and know exactly what gets cleaned up.":
    "Operation IDs settle ambiguous retries, state cursors expose whether replay is possible, and every session-owned resource has an explicit teardown rule. A reconnect never has to guess whether a mutation happened or a resource survived.",
  "02-protocol/01-architecture::diagram":
    "The server owns the durable resources on the left. Every browser, CLI, agent, or extension negotiates an independent YAS session and opens only the views and watches it needs.",
  "02-protocol/04-state::diagram":
    "A watch begins with a bounded snapshot and continues with ordered deltas; acknowledgements report the applied revision and grant more byte credit. After reconnect, the boot ID and revision decide whether retained deltas can replay or an explicit reset and new snapshot are required.",
  "04-wayland/05-video::diagram":
    "Application buffers are composed into a surface tree, adapted for each viewer, encoded, decoded with WebCodecs, and drawn to a canvas. Each stage can change without changing the Surface resource model.",
  "04-wayland/03-input::Coordinates: signed 32.32 fixed-point positions in the app's native-resolution frame; remote pointer/touch marks expire automatically.":
    "Signed 32.32 fixed point stores a coordinate in 64 bits: a signed 32-bit whole-number part and a 32-bit fraction, with a range of roughly ±2.1 billion pixels and subpixel precision of 2⁻³². Coordinates refer to the app's native composited frame even when a viewer receives downscaled video; remote-user marks are best-effort overlays that disappear on release, disconnect, or timeout.",
  "08-delivery/02-topologies::diagram":
    "The local, relayed, and peer-to-peer paths all end in the same YAS session model. Moving between them changes routing, not how clients address resources.",
  "08-delivery/04-interfaces::yas open":
    "Opens the browser workspace for the current server or route. It is the quickest way for a person to enter the session.",
  "08-delivery/04-interfaces::yas terminal start htop":
    "Starts htop as a persistent terminal resource. The terminal can outlive this CLI invocation and be viewed elsewhere.",
  "08-delivery/04-interfaces::yas terminal show 1":
    "Prints the current state of terminal 1 without attaching an interactive TTY. Scripts and agents can inspect the same screen a person sees.",
  "08-delivery/04-interfaces::yas run --in /src/yas -- cargo test":
    "Runs cargo test in the selected remote directory as a Process resource. Output, exit status, and lifecycle remain structured and observable.",
  "08-delivery/04-interfaces::yas surface capture 1":
    "Captures the current frame of surface 1. This works on an individual remote application window rather than a whole desktop image.",
  "08-delivery/04-interfaces::yas --on prod git status":
    "Runs the typed Git status operation through the saved prod route. The command changes destination without changing the interface.",
  "08-delivery/05-start::Unix:":
    "These commands install YAS with a POSIX shell and then open the workspace. The installer is intentionally usable without curl's silent or fail-fast flags.",
  "08-delivery/05-start::curl https://yas.run | sh":
    "Downloads the Unix installer and passes it to the shell. Inspect the URL first when your environment requires reviewing remote install scripts.",
  "08-delivery/05-start::yas open":
    "Starts or discovers the local YAS service and opens its browser workspace. The same command works after the Unix or Windows install.",
  "08-delivery/05-start::Windows PowerShell:":
    "These are the native PowerShell equivalents for Windows. Invoke-RestMethod is shortened to its standard irm alias.",
  "08-delivery/05-start::irm https://yas.run/install.ps1 | iex":
    "Downloads the PowerShell installer and executes it in the current session. The full cmdlet names are Invoke-RestMethod and Invoke-Expression.",
  "08-delivery/05-start::Share over WebRTC—read-write passphrase, with a derived read-only token when needed:":
    "A normal share lets the guest interact with the selected resources. Deriving the read-only token asks the server to advertise observation-only operations instead.",
  "08-delivery/05-start::yas share":
    "Creates a browser-accessible WebRTC share and prints its credentials. The server remains the authority for what the guest may see or control.",
  "08-delivery/05-start::Nothing to configure. No required external dependencies.":
    "The first useful workspace does not depend on a database, reverse proxy, display server, or GPU. Optional integrations can still improve particular workloads later.",
  "08-delivery/06-close::Start a job, close the laptop, and pick it up from a phone.":
    "The job and its parsed state remain on the server when the first browser disconnects. A phone opens its own appropriately sized view instead of inheriting the laptop's viewport or frame backlog.",
};

const TITLE_EXPLANATIONS: Record<string, string> = {
  "01-intro/01-yas":
    "YAS is a protocol and workspace server for operating a whole remote environment, not just a shell. The deck follows that environment from connection and recovery through terminals, applications, code, execution, and sharing.",
  "01-intro/02-remote-workspace":
    "A shell carries terminal input and output, but real development also depends on files, repositories, language tools, processes, windows, media, and durable UI state. YAS makes those resources part of one coherent remote workspace.",
  "01-intro/03-live":
    "The useful state remains on the server when a browser or network disappears. A returning client receives a bounded current snapshot and then only ordered changes.",
  "01-intro/04-scope":
    "This is the complete YAS v1 capability map, grouped by what a user does rather than by numeric protocol IDs. Every named area is independently negotiated at connection time.",
  "01-intro/05-constraints":
    "YAS is designed to feel immediate locally without collapsing on a slow WAN. This slide also separates today's platform availability from the protocol's implemented-but-not-yet-fully-qualified release status.",
  "02-protocol/01-architecture":
    "The server—not an edge or browser—owns terminals, files, processes, routes, and application state. Each client negotiates its own session and presentation views over those shared resources.",
  "02-protocol/02-core":
    "Core makes startup and lifecycle deterministic: one negotiation establishes versions, operations, limits, timing, and shutdown behavior. Later families can rely on those rules instead of inventing their own handshakes.",
  "02-protocol/03-transfer":
    "Transfer is reusable flow control for large byte streams and message bodies. It moves bounded data, while Filesystem, Process, Relay, Selection, and the other owning families retain domain semantics.",
  "02-protocol/04-state":
    "Watched resources recover without hiding gaps. Clients either replay retained revisions or receive an explicit reset and fresh snapshot.",
  "02-protocol/05-relay-client":
    "A home server publishes named destinations and opens a complete nested YAS link for each connection. The route changes; the client protocol and tools do not.",
  "02-protocol/06-clients":
    "Client state answers who is connected, where they came from, what they are watching, and how much traffic they use. Disconnect is a correlated session operation, not a blind socket kill.",
  "03-terminal/01-lifecycle":
    "A terminal is a retained server resource with a reproducible launch record and explicit lifecycle. It can outlive the client, be inspected after exit, and restart without mixing generations.",
  "03-terminal/02-views":
    "Every mounted terminal view has its own geometry, scroll position, focus, frame budget, and feedback. A slow or hidden view therefore does not dictate another client's experience.",
  "03-terminal/03-content":
    "The protocol carries the semantic grid state needed to reproduce the intended terminal, including rich text, modes, links, title, cursor, and font metrics. Clients are not reconstructing a screen from screenshots.",
  "03-terminal/04-input":
    "Browser input is translated through negotiated terminal modes into exact PTY bytes. IME composition, mouse protocols, selection, clipboard ownership, and automation remain distinct instead of being guessed from DOM events.",
  "03-terminal/05-history":
    "Parsed terminal state becomes queryable history rather than a fleeting byte stream. People and agents can read, search, copy, journal commands, and wait for outcomes without scraping pixels.",
  "03-terminal/06-codec":
    "After a keyframe, the server chooses the smallest legal description of each grid change. Bounded frames, verified ancestry, and conditional compression keep the hot path compact without making recovery ambiguous.",
  "04-wayland/01-windows":
    "YAS streams individual application windows with identity and lifecycle, not a monolithic desktop capture. Titles, icons, parents, popups, activation, and control remain usable workspace objects.",
  "04-wayland/02-shell":
    "The headless compositor implements the protocols real Linux desktop applications expect. That lets native Wayland applications—and optional Xwayland clients—behave normally without a physical display server.",
  "04-wayland/03-input":
    "Surface input preserves physical keys, committed text, IME preedit, pointer axes, custom cursors, multitouch, and stable coordinates. Downscaled video never changes the coordinate system sent to the application.",
  "04-wayland/04-selection":
    "Clipboard, primary selection, and drag-and-drop cross the browser boundary with explicit ownership, MIME types, revisions, and bounded data transfer. Large or rich content is not truncated into a control message.",
  "04-wayland/05-video":
    "Each viewer negotiates its own surface extent, cadence, latency target, decoder capacity, codec, and quality. The server can use available hardware while retaining software fallbacks and reliable recovery points.",
  "05-desktop/01-desktop":
    "Tray items, menus, and notifications arrive as structured live state instead of pixels inside a desktop stream. The browser can render and invoke them with revision checks that reject stale actions.",
  "05-desktop/02-media":
    "Audio output, viewer microphones and cameras, portals, screencast choices, and media-player control share one timed resource model. Consent and failure stay scoped to the requested capability.",
  "05-desktop/03-fonts":
    "The server describes the exact font faces and metrics used by remote content, then exports allowed bytes by verified content hash. Browsers can render faithfully without a hidden edge-side font API.",
  "06-workspace/01-filesystem":
    "Filesystem exposes watched trees, typed queries, search, and atomic mutations without requiring a mount. Exact platform paths, hashes, revisions, and preconditions make concurrent remote edits safe.",
  "06-workspace/02-git":
    "Git data stays structured from refs and status through logs, objects, patches, blame, reflog, worktrees, and fetch progress. Clients preserve repository meaning instead of parsing human-formatted CLI output.",
  "06-workspace/03-lsp":
    "The server owns language-server JSON-RPC and projects revision-bound diagnostics and queries into stable YAS records. Clients work in UTF-8 byte coordinates and never inherit LSP's internal request IDs or UTF-16 positions.",
  "06-workspace/04-ide":
    "The browser combines the native Filesystem, Git, and LSP families into project panels and tiled content. Terminals and GUI applications remain first-class neighbors rather than being replaced by a separate IDE shell.",
  "06-workspace/05-kv":
    "KV is durable shared application state, not code storage and not a second filesystem. It backs remotes, extension intent, workspace sessions, and other small values that need watches and transactional updates.",
  "07-execution/01-process":
    "Process runs pipe-oriented programs directly, with exact argv, environment, cwd, stdio, ownership, and exit state. A pseudo-terminal is used only when the program actually needs terminal behavior.",
  "07-execution/02-network":
    "Network opens raw endpoints from the server's network position, then leaves HTTP, DNS, databases, and other application protocols to client code. The CLI builds familiar forwarding and SOCKS workflows on top.",
  "07-execution/03-channel":
    "Channel gives tools and extensions a named rendezvous with reliable message boundaries. It is deliberately smaller than pub/sub and more structured than sending commands through a terminal.",
  "07-execution/04-extensions":
    "Extensions run immutable Wasm or JavaScript modules under a supervisor and communicate through ordinary YAS sessions and Channels. There is no separate privileged plugin backdoor to secure or document.",
  "07-execution/05-events":
    "Events records recent server behavior in a bounded binary journal that observers cannot backpressure. Dumps, live streams, gaps, and file recordings make loss and retention explicit.",
  "07-execution/06-environment":
    "Environment returns the exact immutable process environment captured at server boot, including raw platform bytes and derived values. Because nothing is redacted, reduced-authority sessions do not receive it.",
  "08-delivery/01-transports":
    "The same YAS preface, handshake, families, and resources run over local IPC, remote streams, browser transports, and peer-to-peer sharing. Only framing and optional datagram availability change.",
  "08-delivery/02-topologies":
    "Local, relayed, and WebRTC paths differ in routing and process placement, not resource semantics. Handles and negotiated state remain scoped to the server session that created them.",
  "08-delivery/03-authority":
    "A normal credential grants broad server-OS authority; YAS v1 is not a multi-user role system. WebRTC adds one fixed server-enforced read-only profile for passive sharing, while separate servers isolate mutually untrusted users.",
  "08-delivery/04-interfaces":
    "The browser, CLI, framework bindings, and agent skill operate the same typed resources. Interactive work and automation therefore see the same terminal, surface, repository, and process state.",
  "08-delivery/05-start":
    "The shortest path installs YAS, opens the browser workspace, and optionally publishes a WebRTC share. Unix and Windows use native installers but converge on the same CLI.",
  "08-delivery/06-close":
    "The closing claim is concrete: durable jobs, native application behavior, named routes, and independent views all survive beyond a shell transport. Humans and software can meet in the same live state without sharing one screen.",
};

function cleaned(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function keyFor(slideId: string, text: string): string {
  return `${slideId}::${cleaned(text)}`;
}

export function explanationFor({
  slideId,
  slideTitle,
  kind,
  text,
  lineIndex,
}: ExplanationContext): string {
  const normalized = cleaned(text);
  const special =
    SPECIAL[keyFor(slideId, kind === "diagram" ? "diagram" : normalized)];
  if (special) return special;

  if (kind === "title" && TITLE_EXPLANATIONS[slideId]) {
    return TITLE_EXPLANATIONS[slideId];
  }

  if (kind === "text" && lineIndex !== undefined) {
    const explanation = TEXT_EXPLANATIONS[slideId]?.[lineIndex];
    if (explanation) return explanation;
  }

  if (kind === "diagram") {
    return `This diagram shows the data flow behind “${slideTitle}.” Follow the arrows to see which component owns state and where each client-specific view is produced.`;
  }

  if (kind === "code") {
    return `This is a directly runnable example for “${slideTitle}.” It uses the same typed resources that the browser and other clients see.`;
  }

  if (kind === "title")
    return `This slide explains “${normalized}” in terms of concrete user-visible behavior and the protocol rules that support it.`;

  const lead = normalized.match(/^([^:]{2,48}):\s/)?.[1];
  return lead
    ? `“${lead}” groups the concrete behaviors listed on this line. YAS makes each one explicit so clients can implement it consistently and recover cleanly.`
    : `This is a concrete guarantee behind “${slideTitle}.” YAS represents it as protocol state or a typed operation rather than relying on client-side inference.`;
}
