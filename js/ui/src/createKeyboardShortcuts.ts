import { onMount, onCleanup } from "solid-js";
import type {
  YasWorkspace,
  YasSession,
  SessionId,
  ConnectionId,
  SurfaceId,
  LayoutAssignments,
} from "@yas-run/core";
import { isSurfaceAssignment, parseSurfaceAssignment } from "./layout/store";
import type { Overlay } from "./Workspace";
import { dismissTopClaim } from "./overlayStack";
import { handlePrefixKey, registerPrefixAction } from "./keyPrefix";
import { t } from "./i18n";

export interface KeyboardShortcutHandlers {
  workspace: YasWorkspace;
  /** Current overlay accessor */
  overlay: () => Overlay;
  /** Currently active layout (null = single terminal) */
  activeLayout: () => unknown | null;
  /** Whether a managed layout is on screen, including one-pane tiling. */
  inLayout: () => boolean;
  /** Currently focused pane ID */
  layoutFocusedPaneId: () => string | null;
  /** Current pane→session assignments (null when no layout active) */
  layoutAssignments: () => LayoutAssignments | null;
  /** Focused session accessor */
  focusedSession: () => YasSession | null;
  /** All sessions accessor */
  sessions: () => readonly YasSession[];
  /** Focused session ID accessor */
  focusedSessionId: () => SessionId | null;
  /** Connection supports restart */
  supportsRestart: () => boolean;
  /** Currently focused surface ID (null when a terminal is focused) */
  focusedSurfaceId: () => SurfaceId | null;
  /** Connection ID of the currently focused surface */
  focusedSurfaceConnId: () => ConnectionId | null;
  /** Close / request-close the focused surface */
  closeSurface: (connectionId: ConnectionId, surfaceId: SurfaceId) => void;
  /** Background the standalone surface and leave the main view empty. */
  unfocusSurface: () => void;
  /** Background the standalone terminal and leave the main view empty. */
  backgroundFocusedSession: () => void;

  toggleOverlay: (target: Overlay) => void;
  /** Select an adjacent client-attached workspace tab. */
  cycleWorkspaceTab: (delta: -1 | 1) => boolean;
  createWorkspaceTab: () => void;
  openWorkspaceManager: () => void;
  detachWorkspaceTab: () => void;
  /** Send a literal Ctrl-B to the terminal or Wayland surface in the focused
   *  pane — the only way past the one chord YAS reserves. */
  forwardPrefix: () => void;
  /** Open the switcher with one of its mode prefixes already typed. */
  seedSwitcher: (mode: string) => void;
  cancelOverlay: () => void;
  toggleDebug: () => void;
  togglePreviewPanel: () => void;
  toggleLeftPanel: (
    panel: "explorer" | "branches" | "log" | "problems",
  ) => void;
  /** Show/hide the project-search top pane. */
  toggleSearch: () => void;
  createAndFocus: () => Promise<void>;
  createInPane: (paneId: string) => Promise<void>;
  createBesideFocused: () => Promise<void>;
  openNewTerminalPicker: (paneId?: string) => void;
  handleRestartOrClose: () => void;
  connectionCount: () => number;
  /**
   * Everything open, as pane assignments, in a stable order: terminals, then
   * Wayland surfaces, then tabs (editors, diffs, commits, web panes). This is
   * the ring Alt+Shift+[ / ] walks.
   */
  cycleRing: () => readonly string[];
  /** What the focused slot (pane, or the single main view) is showing. */
  focusedAssignment: () => string | null;
  /** Show an assignment of any kind in the focused slot, and focus it. */
  focusAssignment: (assignment: string) => void;
  /** Clear the assignment for the focused pane (remove term without closing) */
  clearFocusedPaneAssignment: () => void;
  /**
   * Send the focused IDE tile (single-view focused tile, or a tile occupying the
   * focused pane) to the recoverable background list. Returns true if a
   * tile was backgrounded (so the caller stops handling the key).
   */
  backgroundFocusedTile: () => boolean;
  /** Close the focused IDE tile outright (no dock parking). */
  closeFocusedTile: () => boolean;
  /** Navigate the focused tile pane's history back / forward (like a browser). */
  navigateBack: () => void;
  navigateForward: () => void;
}

type SurfaceFocusHandlers = Pick<
  KeyboardShortcutHandlers,
  "inLayout" | "focusedSurfaceId" | "layoutFocusedPaneId" | "layoutAssignments"
>;

type RestartFocusHandlers = Pick<
  KeyboardShortcutHandlers,
  "focusedAssignment" | "focusedSession"
>;

/** The exited terminal must still occupy the focused slot. Core retains a
 * focused session behind editors and surfaces, so session state alone is not
 * enough to decide that bare Enter belongs to the restart action. */
export function hasFocusedExitedTerminal(h: RestartFocusHandlers): boolean {
  const session = h.focusedSession();
  return session?.state === "exited" && h.focusedAssignment() === session.id;
}

/**
 * Whether keyboard input currently belongs to a Wayland surface.
 *
 * The two slots are mutually exclusive, and this mirrors what Workspace
 * actually renders: under a real layout only the focused pane's assignment
 * says what is on screen; otherwise only `focusedSurfaceId` does. Reading both
 * at once let either one's leftovers speak for a view that is not showing —
 * and `focusedSurfaceId` can remain set when a layout takes over (no layout focus path
 * touches it).
 * That stale "a surface is focused" silently killed Cmd+Enter, Cmd+Shift+Enter,
 * every Ctrl+Shift chord and Enter-to-restart, in panes holding a terminal or
 * nothing at all.
 */
export function hasFocusedWaylandSurface(h: SurfaceFocusHandlers): boolean {
  if (h.inLayout()) {
    const paneId = h.layoutFocusedPaneId();
    if (!paneId) return false;
    const assignment = h.layoutAssignments()?.assignments[paneId] ?? null;
    return isSurfaceAssignment(assignment);
  }
  return h.focusedSurfaceId() != null;
}

export function shouldHandleNewTerminalShortcut(
  h: SurfaceFocusHandlers,
): boolean {
  return !hasFocusedWaylandSurface(h);
}

/** True when an element is CodeMirror 6's focused contenteditable. */
export function isCodeMirrorInputTarget(el: Element | null): boolean {
  return el instanceof HTMLElement && el.closest(".cm-editor") != null;
}

type TextControl = HTMLInputElement | HTMLTextAreaElement;

const MAC_DEAD_KEYS: Readonly<
  Record<string, { combining: string; spacing: string }>
> = {
  Backquote: { combining: "\u0300", spacing: "`" },
  KeyE: { combining: "\u0301", spacing: "´" },
  KeyI: { combining: "\u0302", spacing: "ˆ" },
  KeyN: { combining: "\u0303", spacing: "˜" },
  KeyU: { combining: "\u0308", spacing: "¨" },
};

function macOptionChars(): boolean {
  const nav = navigator as Navigator & {
    userAgentData?: { platform?: string };
  };
  const platform = (
    nav.userAgentData?.platform ??
    nav.platform ??
    ""
  ).toLowerCase();
  if (platform) return platform.startsWith("mac") || platform.startsWith("ip");
  return /mac|ipad|iphone/.test((nav.userAgent ?? "").toLowerCase());
}

function textControlFor(event: KeyboardEvent): TextControl | null {
  const target = event.target;
  if (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement
  ) {
    return target;
  }
  const active = document.activeElement;
  return active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement
    ? active
    : null;
}

/**
 * Chromium on macOS can drop a Latin dead-key composition while YAS's page
 * owns the keyboard: Option+E followed by E then arrives as a plain `e`, with
 * no usable composition commit.  Recreate the five US-layout Option dead keys
 * through the focused text control's normal composition/input path.  The same
 * path feeds ordinary inputs and the hidden terminal/surface textareas.
 */
export function createMacDeadKeyHandler(
  enabled = macOptionChars(),
): (event: KeyboardEvent) => boolean {
  let pending: {
    target: TextControl;
    combining: string;
    spacing: string;
    start: number;
    end: number;
  } | null = null;

  /** Everything this handler dispatched, so a composition started by anyone
   *  else — the browser's own IME above all — is recognizable as foreign. */
  const ours = new WeakSet<Event>();
  const dispatch = (target: TextControl, event: Event): boolean => {
    ours.add(event);
    return target.dispatchEvent(event);
  };

  const finish = (target: TextControl, data: string) => {
    dispatch(
      target,
      new CompositionEvent("compositionend", { bubbles: true, data }),
    );
  };

  const update = (
    target: TextControl,
    start: number,
    end: number,
    data: string,
  ): boolean => {
    dispatch(
      target,
      new CompositionEvent("compositionupdate", { bubbles: true, data }),
    );
    const beforeInput = new InputEvent("beforeinput", {
      bubbles: true,
      cancelable: true,
      data,
      inputType: "insertCompositionText",
      isComposing: true,
    });
    if (!dispatch(target, beforeInput)) return false;
    target.setRangeText(data, start, end, "end");
    dispatch(
      target,
      new InputEvent("input", {
        bubbles: true,
        data,
        inputType: "insertCompositionText",
        isComposing: true,
      }),
    );
    return true;
  };

  const cancel = (active: NonNullable<typeof pending>) => {
    update(active.target, active.start, active.end, "");
    finish(active.target, "");
  };

  // `preventDefault()` on a keydown does not stop Chromium's own IME: on a
  // machine where the native dead key works, it starts a real composition of
  // its own right after this handler synthesized one, and the accent lands
  // twice.  A trusted `compositionstart` is the proof that the native path is
  // alive, so retract the synthesized preedit and leave the field to it — no
  // synthetic `compositionend`, because the native lifecycle now owns it.
  let unwatch: (() => void) | null = null;
  const watchNative = (target: TextControl) => {
    const onNative = (event: Event) => {
      if (ours.has(event)) return;
      const active = pending;
      pending = null;
      unwatchNative();
      if (!active) return;
      if (
        active.target.value.slice(active.start, active.end) === active.spacing
      )
        update(active.target, active.start, active.end, "");
    };
    target.addEventListener("compositionstart", onNative, true);
    unwatch = () =>
      target.removeEventListener("compositionstart", onNative, true);
  };
  const unwatchNative = () => {
    unwatch?.();
    unwatch = null;
  };

  return (event: KeyboardEvent): boolean => {
    if (!enabled) return false;
    const target = textControlFor(event);

    if (!pending) {
      const dead = MAC_DEAD_KEYS[event.code];
      if (
        !dead ||
        event.key !== "Dead" ||
        !event.altKey ||
        event.ctrlKey ||
        event.metaKey ||
        !target
      ) {
        return false;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      dispatch(
        target,
        new CompositionEvent("compositionstart", { bubbles: true, data: "" }),
      );
      const start = target.selectionStart ?? target.value.length;
      const end = target.selectionEnd ?? start;
      if (!update(target, start, end, dead.spacing)) {
        finish(target, "");
        return true;
      }
      pending = {
        target,
        ...dead,
        start,
        end: start + dead.spacing.length,
      };
      watchNative(target);
      return true;
    }

    const active = pending;
    pending = null;
    unwatchNative();
    if (!target || target !== active.target) {
      cancel(active);
      return false;
    }
    if (event.key === "Escape" || event.key === "Backspace") {
      event.preventDefault();
      event.stopImmediatePropagation();
      cancel(active);
      return true;
    }

    // Only a key that actually precomposes completes the accent.  NFC leaves
    // the mark bare when no precomposed form exists ("q" → "q́", and the
    // accent itself → "´́"), and a bare combining mark is never what was
    // typed: macOS commits the spacing accent followed by the character.
    const composed =
      event.key.length === 1
        ? `${event.key}${active.combining}`.normalize("NFC")
        : "";
    const text =
      event.key === " "
        ? active.spacing
        : composed.length === 0
          ? null
          : [...composed].length === 1
            ? composed
            : event.key === active.spacing
              ? active.spacing
              : `${active.spacing}${event.key}`;
    if (text == null) {
      cancel(active);
      return false;
    }
    // Anything else may have written to the field since the dead key (a native
    // composition, a click, autocorrect), which makes the recorded range point
    // at text we did not insert.  Leave it alone rather than overwriting it.
    if (target.value.slice(active.start, active.end) !== active.spacing) {
      finish(target, "");
      return false;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    if (update(target, active.start, active.end, text)) {
      finish(target, text);
    } else {
      cancel(active);
    }
    return true;
  };
}

/**
 * The next thing Alt+Shift+[ / ] should show in the focused slot, or null when
 * there is nothing to move to.
 *
 * `ring` is everything open; `displayedElsewhere` is what the OTHER panes
 * are already showing, which is excluded — the chord rotates the focused pane's
 * occupant, tiling-WM style, and pulling in a window that is already on screen
 * beside it would only shuffle the two. In single-pane mode nothing is
 * elsewhere, so the ring is walked whole.
 *
 * `current` outside the ring (nothing focused, or a parked view) enters at the
 * near end rather than skipping the first step.
 */
export function nextCycleTarget(
  ring: readonly string[],
  current: string | null,
  direction: 1 | -1,
  displayedElsewhere: ReadonlySet<string> = new Set(),
): string | null {
  const candidates = ring.filter((a) => !displayedElsewhere.has(a));
  if (candidates.length === 0) return null;
  const index = current == null ? -1 : candidates.indexOf(current);
  if (index < 0) {
    return direction === 1 ? candidates[0] : candidates[candidates.length - 1];
  }
  if (candidates.length < 2) return null;
  return candidates[
    (index + direction + candidates.length) % candidates.length
  ];
}

/**
 * Installs global keyboard shortcuts for the workspace.
 * Must be called inside a Solid component (uses onMount/onCleanup).
 */
export function createKeyboardShortcuts(h: KeyboardShortcutHandlers): void {
  onMount(() => {
    const handleMacDeadKey = createMacDeadKeyHandler();
    const eventElement = (target: EventTarget | null): Element | null => {
      if (target instanceof Element) return target;
      return document.activeElement instanceof Element
        ? document.activeElement
        : null;
    };

    // Every workspace action lives behind Ctrl+B. What is left on its own key
    // is what a prefix cannot express: Enter restarts the exited terminal you
    // are already looking at, and Escape dismisses whatever is on top of it.
    const newTerminal = (picker: boolean) => {
      const paneId = h.activeLayout() ? h.layoutFocusedPaneId() : null;
      if (picker && h.connectionCount() > 1) {
        h.openNewTerminalPicker(paneId ?? undefined);
        return;
      }
      if (paneId) void h.createInPane(paneId);
      else void h.createAndFocus();
    };

    // Take the focused thing off screen without closing it. Precedence is
    // IDE tile, then standalone surface, then pane, then the single
    // main view — a tile with focus must not have the terminal behind it
    // backgrounded out from under it.
    const backgroundFocused = () => {
      if (h.overlay()) return;
      if (h.backgroundFocusedTile()) return;
      if (h.focusedSurfaceId() != null) {
        h.unfocusSurface();
        return;
      }
      if (h.activeLayout() && h.layoutFocusedPaneId()) {
        h.clearFocusedPaneAssignment();
        return;
      }
      if (h.focusedSessionId() != null) h.backgroundFocusedSession();
    };

    const closeFocused = () => {
      if (h.overlay()) return;
      if (h.closeFocusedTile()) return;
      const surfaceId = h.focusedSurfaceId();
      const surfaceConnId = h.focusedSurfaceConnId();
      if (surfaceId != null && surfaceConnId != null) {
        h.closeSurface(surfaceConnId, surfaceId);
        return;
      }
      const paneId = h.layoutFocusedPaneId();
      if (paneId) {
        const assignment = h.layoutAssignments()?.assignments[paneId] ?? null;
        if (assignment && isSurfaceAssignment(assignment)) {
          const parsed = parseSurfaceAssignment(assignment);
          if (parsed != null) {
            h.closeSurface(parsed.connectionId, parsed.surfaceId);
            return;
          }
        }
      }
      const sessionId = h.focusedSessionId();
      if (sessionId) void h.workspace.closeSession(sessionId);
    };

    // Prev/next window: every kind the workspace holds — terminals, Wayland
    // surfaces, editors, diffs, commits, web panes — not just terminals, so
    // the key reaches whatever is open rather than stranding you on the one
    // kind it knew about. What the OTHER panes already show is excluded:
    // this rotates the focused pane's occupant, tiling-WM style, and pulling
    // in a window already on screen beside it would only shuffle the two.
    const cycleWindow = (direction: 1 | -1) => {
      const paneId = h.layoutFocusedPaneId();
      const assignments = h.layoutAssignments();
      const elsewhere = new Set<string>();
      if (assignments && paneId) {
        for (const [id, value] of Object.entries(assignments.assignments)) {
          if (id !== paneId && value != null) elsewhere.add(value);
        }
      }
      const next = nextCycleTarget(
        h.cycleRing(),
        h.focusedAssignment(),
        direction,
        elsewhere,
      );
      if (next != null) h.focusAssignment(next);
    };

    // The switcher's modes are keys of their own: reaching symbol search by
    // opening the switcher and then typing "#" is two decisions for one
    // intent, and the field's prefixes were invisible anyway.
    // Seed *before* opening. The switcher reads the seed when it is created,
    // so opening first meant it read an empty one and Ctrl+B # was just
    // Ctrl+B k with extra steps.
    const switcherMode = (mode: string) => () => {
      h.seedSwitcher(mode);
      h.toggleOverlay("expose");
    };
    const bindings: [string, () => void, string][] = [
      ["prefix", h.forwardPrefix, t("help.sendPrefix")],
      ["?", () => h.toggleOverlay("help"), t("help.title")],
      ["k", () => h.toggleOverlay("expose"), t("help.menu")],
      ["/", () => h.toggleOverlay("expose"), t("help.commandSearch")],
      [">", switcherMode(">"), t("help.modeCommand")],
      ["@", switcherMode("@"), t("help.modeFile")],
      ["#", switcherMode("#"), t("help.modeSymbol")],
      ["Enter", () => newTerminal(true), t("help.newTerminal")],
      ["Shift+Enter", () => void h.createBesideFocused(), t("help.openBeside")],
      ["e", () => h.toggleLeftPanel("explorer"), t("help.dockExplorer")],
      ["f", h.toggleSearch, t("help.projectSearch")],
      ["y", () => h.toggleLeftPanel("branches"), t("help.dockBranches")],
      ["l", () => h.toggleLeftPanel("log"), t("help.dockLog")],
      ["p", () => h.toggleLeftPanel("problems"), t("help.dockProblems")],
      ["r", h.togglePreviewPanel, t("help.previewPanel")],
      ["w", () => h.toggleOverlay("expose"), t("help.viewOverview")],
      ["q", backgroundFocused, t("pane.parkAction")],
      ["x", closeFocused, t("help.closeTerminal")],
      [
        "[",
        () => {
          if (!h.cycleWorkspaceTab(-1)) cycleWindow(-1);
        },
        t("help.previousWorkspace"),
      ],
      [
        "]",
        () => {
          if (!h.cycleWorkspaceTab(1)) cycleWindow(1);
        },
        t("help.nextWorkspace"),
      ],
      ["n", h.createWorkspaceTab, t("help.newWorkspace")],
      ["a", h.openWorkspaceManager, t("help.attachWorkspace")],
      ["d", h.detachWorkspaceTab, t("help.detachWorkspace")],
      ["ArrowLeft", h.navigateBack, t("help.navBack")],
      ["ArrowRight", h.navigateForward, t("help.navForward")],
    ];
    const unbind = bindings.map(([token, run, label]) =>
      registerPrefixAction(token, run, label),
    );
    onCleanup(() => {
      for (const drop of unbind) drop();
    });

    const handler = (e: KeyboardEvent) => {
      if (handleMacDeadKey(e)) return;
      if (e.key === "Dead" || e.isComposing || e.keyCode === 229) return;

      // Arming, choosing, and cancelling all end here: an armed prefix owns
      // the next keystroke outright, or an unbound key would arrive in the
      // pane below as a surprise.
      if (handlePrefixKey(e)) {
        e.preventDefault();
        e.stopImmediatePropagation();
        return;
      }

      // An exited terminal cannot consume input, and the action displayed in
      // its banner is deliberately immediate. Leave Enter on an actual button
      // alone so keyboard activation of Close still means Close.
      if (
        e.key === "Enter" &&
        !e.ctrlKey &&
        !e.altKey &&
        !e.metaKey &&
        !e.shiftKey &&
        !h.overlay() &&
        hasFocusedExitedTerminal(h) &&
        !eventElement(e.target)?.closest("button")
      ) {
        e.preventDefault();
        e.stopImmediatePropagation();
        h.handleRestartOrClose();
        return;
      }

      if (e.key === "Escape") {
        // An overlay stacked on top of another owns the key first, or closing
        // the one underneath would take it down with it.
        if (dismissTopClaim()) {
          e.preventDefault();
          return;
        }
        if (h.overlay()) {
          e.preventDefault();
          h.cancelOverlay();
          return;
        }
        // Do not capture Escape while a Wayland surface is focused: many
        // apps rely on it, and YasSurfaceCanvas will forward it if the
        // event is left unhandled here. Use Ctrl+Shift+Q to return to the
        // terminal view without sending input to the surface.
        if (h.focusedSurfaceId() != null) {
          return;
        }
        // When a layout is active, LayoutContainer handles Escape on
        // exited sessions itself (it needs to clear the pane assignment
        // before closing).  If we close here on the capture phase the
        // session state flips to "closed" synchronously, which
        // invalidates the LayoutContainer effect before its bubble-phase
        // handler can fire.
        if (!h.activeLayout()) {
          const fs = h.focusedSession();
          if (fs?.state === "exited") {
            e.preventDefault();
            void h.workspace.closeSession(fs.id);
          }
        }
      }
    };

    window.addEventListener("keydown", handler, true);
    onCleanup(() => {
      window.removeEventListener("keydown", handler, true);
    });
  });
}
