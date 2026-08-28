/**
 * The `Ctrl+B` prefix.
 *
 * YAS used to spend a chord on every workspace action: Ctrl+Shift+E, +F, +L,
 * +Y, +P, +B, +O, +Q, Ctrl+Alt+Shift+Q, Alt+Shift+[/], Ctrl+[/], Ctrl+Alt+←/→.
 * Each one is a key a terminal application can no longer see, and the set was
 * still growing. A multiplexer already knows the answer to that: take one
 * chord, and put everything behind it.
 *
 * So `Ctrl+B` is the only chord YAS reserves. It arms; the next keystroke
 * chooses. `Ctrl+B Ctrl+B` sends a literal Ctrl+B on to the pane, which is the
 * only way an application below can still receive the one key we took.
 *
 * Actions live in a registry rather than one switch, because half of them only
 * exist while a layout is on screen — LayoutContainer registers those and drops
 * them when it unmounts, so `Ctrl+B z` is simply unbound with no layout up instead
 * of being a no-op that has to test for a layout.
 */

import { createSignal } from "solid-js";

const [armed, setArmed] = createSignal(false);

/** True while the prefix is waiting for the key that chooses an action. */
export const prefixArmed = armed;

interface PrefixAction {
  run: () => void;
  /** What the key does, for the map shown while the prefix is armed. */
  label: string;
}

const actions = new Map<string, PrefixAction>();
const [revision, bumpRevision] = createSignal(0, { equals: false });

/**
 * Bind one key behind the prefix. Returns the unbind, for `onCleanup`.
 *
 * A second registration of the same token wins and the first is restored when
 * the winner unbinds — nothing registers a token twice today, and stacking
 * beats silently dropping one of the two.
 */
export function registerPrefixAction(
  token: string,
  run: () => void,
  label = "",
): () => void {
  const previous = actions.get(token);
  const action: PrefixAction = { run, label };
  actions.set(token, action);
  bumpRevision(0);
  return () => {
    if (actions.get(token) !== action) return;
    if (previous) actions.set(token, previous);
    else actions.delete(token);
    bumpRevision(0);
  };
}

/** Test seam: what is bound right now. */
export function prefixActionTokens(): string[] {
  return [...actions.keys()].sort();
}

/**
 * Every key behind the prefix, for the map.
 *
 * A prefix you cannot see is a prefix you have to remember, so the armed state
 * shows what it accepts. Reactive on registration, because half of these exist
 * only while a layout is on screen.
 */
export function prefixBindings(): { token: string; label: string }[] {
  revision();
  return [...actions.entries()]
    .filter(([, action]) => action.label !== "")
    .map(([token, action]) => ({ token, label: action.label }));
}

type Chord = Pick<
  KeyboardEvent,
  "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "key" | "code"
>;

function currentPlatform(): string {
  return typeof navigator === "undefined" ? "" : navigator.platform;
}

function isMacPlatform(platform: string): boolean {
  return /Mac|iPhone|iPad/.test(platform);
}

/** Human-readable prefix for compact, platform-specific UI hints. */
export function prefixChordLabel(platform = currentPlatform()): string {
  return isMacPlatform(platform) ? "⌘B" : "ctrl-b";
}

/** `Ctrl+B`, plus `Command+B` on Apple platforms, and nothing else. */
export function isPrefixChord(
  event: Chord,
  platform = currentPlatform(),
): boolean {
  const control = event.ctrlKey && !event.metaKey;
  const command = isMacPlatform(platform) && event.metaKey && !event.ctrlKey;
  if ((!control && !command) || event.altKey || event.shiftKey) {
    return false;
  }
  return event.key === "b" || event.key === "B" || event.code === "KeyB";
}

/**
 * The token an armed prefix reads from the next keystroke.
 *
 * `null` means "not a keystroke yet": a bare modifier press is how `Ctrl+B Q`
 * and `Ctrl+B Shift+Tab` start, so holding Shift must not count as the choice
 * and cancel the prefix.
 *
 * Letters keep their case — `q` backgrounds a pane and `Q` is free for
 * something louder later — and named keys keep their `event.key` spelling.
 * Bracket and backquote fall back to `event.code` because a non-US layout
 * reports something else for `event.key`.
 */
export function prefixToken(event: Chord): string | null {
  if (
    event.key === "Shift" ||
    event.key === "Control" ||
    event.key === "Alt" ||
    event.key === "Meta" ||
    event.key === "CapsLock" ||
    event.key === "Dead"
  ) {
    return null;
  }
  if (isPrefixChord(event)) return "prefix";
  if (event.key === "Tab") return event.shiftKey ? "Shift+Tab" : "Tab";
  if (event.key.length === 1) {
    if (event.code === "BracketLeft") return "[";
    if (event.code === "BracketRight") return "]";
    if (event.code === "Backquote") return "`";
    return event.key;
  }
  return event.key;
}

/**
 * Feed one keydown to the prefix state machine.
 *
 * Returns true when the prefix consumed the event, which is every keystroke
 * from the arming chord until one of them chooses: an unbound key cancels and
 * is swallowed rather than arriving somewhere as a surprise.
 */
export function handlePrefixKey(event: Chord): boolean {
  if (!armed()) {
    if (!isPrefixChord(event)) return false;
    setArmed(true);
    return true;
  }
  const token = prefixToken(event);
  if (token == null) return true;
  setArmed(false);
  if (token === "Escape") return true;
  actions.get(token)?.run();
  return true;
}

/** Drop the armed state, e.g. when the window loses the keyboard. */
export function disarmPrefix(): void {
  setArmed(false);
}
