import type { WebPaneHostRegistrar } from "../WebPaneHost";
import type { SurfaceTouchMode, SurfaceZoomMode } from "../storage";
/**
 * The layout tree's shared context, deliberately in its own module.
 *
 * `createContext` mints an identity: a Provider and a consumer only match
 * when both hold the *same* object. Defining it alongside the components
 * meant every hot reload of `LayoutContainer.tsx` created a fresh context, so
 * consumers re-rendered against it while ancestor Providers still carried
 * the previous one — `useContext` then returned undefined and the first
 * field read threw. Keeping it here means the identity survives reloads of
 * the components that use it.
 */

import { createContext, useContext } from "solid-js";
import type { YasTerminalSurface, TerminalPalette } from "@yas-run/core";
import type { LayoutRect, LayoutSplit } from "@yas-run/core/layout";

const PANE_FOCUS_OWNER_SELECTOR =
  "[data-yas-pane-id], [data-yas-workspace-focus-owner]";

/**
 * Whether a focused pane may move DOM focus to its own keyboard target.
 *
 * Focus on the document body is unowned, and focus in another pane is a pane
 * handoff. Persistent web panes are marked as workspace focus owners because
 * their iframe is portaled outside the logical pane. A control outside those
 * content roots (status bar, overlay, dock chrome, …) owns focus explicitly
 * and must not have it stolen by a reactive pane update.
 */
export function canAutoFocusPane(
  active: Element | null,
  body: HTMLElement,
): boolean {
  return (
    active === null ||
    active === body ||
    active.closest(PANE_FOCUS_OWNER_SELECTOR) !== null
  );
}

/**
 * Focus a pane's keyboard target without stealing focus from app chrome.
 *
 * Some targets are attached by a child onMount after the owning pane's effect
 * runs. Retry that case once, rechecking both reactive pane ownership and DOM
 * focus ownership so an intervening overlay/control focus always wins.
 */
export function autoFocusPaneTarget(
  isFocused: () => boolean,
  findTarget: () => HTMLElement | null,
  ownerDocument: Document = document,
): void {
  const canFocus = () =>
    isFocused() &&
    canAutoFocusPane(ownerDocument.activeElement, ownerDocument.body);
  if (!canFocus()) return;

  const focus = (): boolean => {
    const target = findTarget();
    if (!target) return false;
    if (ownerDocument.activeElement !== target) target.focus();
    return true;
  };

  if (focus()) return;
  queueMicrotask(() => {
    if (canFocus()) focus();
  });
}

/** Props that stay constant through the LayoutPane recursion tree.  Hoisted
 *  into context so each level only passes the values that actually change. */
export interface LayoutTreeCtx {
  connectionId: string;
  connectionLabels?: Map<string, string>;
  multiPane: boolean;
  /** Coarse pointer: the pane's ✕ has no hover to reveal it, so it stays up. */
  isMobileTouch?: boolean;
  /** Did this pane's occupant ask to come forward (xdg_activation_v1)?  It is
   *  answered with a ring around the pane, never by taking focus. */
  hasAttention?: (assignment: string) => boolean;
  onFocusPane: (paneId: string) => void;
  /** Close whatever the pane holds — terminal, surface, IDE tile or web pane.
   *  Same targets, and the same order, as Ctrl+Alt+Shift+Q. */
  onClosePane: (paneId: string) => void;
  onCreateInPane?: (
    paneId: string,
    command?: string,
    connectionId?: string,
  ) => void;
  onSwitcher?: () => void;
  onHelp?: () => void;
  onResize: (
    split: LayoutSplit,
    indexA: number,
    indexB: number,
    fraction: number,
  ) => void;
  /** Move or resize one floating window, in percent of the viewport. */
  onRectChange: (split: LayoutSplit, index: number, rect: LayoutRect) => void;
  /** Set one scrolling column's width, as a fraction of the viewport. */
  onColumnWidth: (split: LayoutSplit, index: number, weight: number) => void;
  /**
   * Stacking order for floating windows: higher is nearer the viewer.
   *
   * Recency, not tree order — raising by reordering children would renumber
   * every pane below the moved one, and a pane id is what an assignment is
   * keyed by. So the tree stays put and only the paint order moves, which is
   * also why it is not persisted: a reload starts the stack from the layout's
   * own order, which is the only order the DSL records.
   */
  floatingDepth: (paneId: string) => number;
  /** Bring a floating window forward. */
  onRaisePane: (paneId: string) => void;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
  /** Surface zoom value and whether it is relative to DPI or absolute. */
  surfaceZoom: number;
  surfaceZoomMode: SurfaceZoomMode;
  surfaceTouchMode: SurfaceTouchMode;
  tabMemory: Record<string, number>;
  onRender?: (renderMs?: number) => void;
  /** Called with each terminal pane's surface as it mounts (and null as it
   *  unmounts), so the workspace can attach hyperlink hover and activation to
   *  every split rather than only the focused one — hovering follows the
   *  pointer, not focus. */
  onTerminalSurface?: (surface: YasTerminalSurface | null) => void;
  /** Whether a session's connection is read-only (an `.ro` share): its
   *  terminals render without input affordances instead of silently
   *  swallowing keystrokes the server will refuse. */
  isSessionReadOnly?: (sessionId: string) => boolean;
  /** The same question about a whole connection, which is what a manage tile
   *  asks: its clients panel talks a family a read-only share drops. */
  isConnectionReadOnly?: (connectionId: string) => boolean;
  /** Open an IDE tile from within a tile (commit view → editor). */
  onOpenTile?: (assignment: string) => void;
  /** Drop a dragged IDE tile assignment into a specific pane. */
  onDropTile?: (
    assignment: string,
    paneId: string,
    sourcePaneId?: string,
  ) => void;
  /** Register the visual host for a Workspace-owned persistent web pane. */
  registerWebPaneHost?: WebPaneHostRegistrar;
  /** The pane currently soloed to fill the workspace, if any. Its siblings
   *  are hidden rather than unmounted, so nothing is torn down and unsolo is
   *  free (see `LayoutContainer`'s `soloedPaneId`). */
  soloedPaneId: string | null;
  /** Solo `paneId`, or unsolo if it already is. */
  onToggleSolo: (paneId: string) => void;
}

export const LayoutTreeContext = createContext<LayoutTreeCtx>();

/** Read the tree context. Callers are always rendered under the Provider in
 *  `LayoutContainer`; an undefined here means that invariant broke, and failing
 *  loudly beats every consumer throwing on its first field access. */
export function useLayoutTree(): LayoutTreeCtx {
  const ctx = useContext(LayoutTreeContext);
  if (!ctx) throw new Error("layout tree context used outside its Provider");
  return ctx;
}
