import {
  onMount,
  onCleanup,
  createEffect,
  createMemo,
  createSignal,
  type JSX,
} from "solid-js";
import { YasTerminalSurface } from "@yas-run/core";
import type { SessionId, TerminalPalette } from "@yas-run/core";
import { useYasContext, useRequiredYasWorkspace } from "./YasContext";
import { createYasWorkspaceState } from "./hooks/createYasWorkspace";

export interface YasTerminalProps {
  sessionId: SessionId | null;
  fontFamily?: string;
  fontSize?: number;
  class?: string;
  style?: JSX.CSSProperties;
  palette?: TerminalPalette;
  readOnly?: boolean;
  /** Resize the remote session to this surface. Disable for passive previews. Default: true. */
  resizable?: boolean;
  /** Stretch a passive preview to its container width. Ignored while resizable. */
  fitWidth?: boolean;
  showCursor?: boolean;
  onRender?: (renderMs: number) => void;
  scrollbarColor?: string;
  scrollbarWidth?: number;
  advanceRatio?: number;
  /** Coverage gamma for glyph antialiasing: 1 leaves antialiasing untouched,
   *  above 1 thins light-on-dark text. Default: DEFAULT_TEXT_GAMMA. */
  textGamma?: number;
  /** Callback to receive the underlying YasTerminalSurface after mount. */
  surfaceRef?: (surface: YasTerminalSurface | null) => void;
}

/**
 * YasTerminal renders a yas terminal inside a WebGL canvas.
 *
 * This is a thin Solid wrapper over `YasTerminalSurface` from `@yas-run/core`.
 * It renders a container `<div>`, attaches the surface on mount, and uses
 * `createEffect` to forward reactive prop changes to the surface.
 */
export function YasTerminal(props: YasTerminalProps) {
  const ctx = useYasContext();
  const workspace = useRequiredYasWorkspace();
  const snapshot = createYasWorkspaceState(workspace);

  let containerRef!: HTMLDivElement;
  // Use a signal so that effects re-run when the surface is created in
  // onMount.  Without this, effects that run during component init see
  // `null` and never retry after the surface is attached.
  const [surface, setSurface] = createSignal<YasTerminalSurface | null>(null);
  // Cleanup must not write this reactive signal while Solid is disposing the
  // owner. Clear non-reactive ownership before dispose so synchronous parent
  // invalidation cannot dispose the same surface recursively.
  let ownedSurface: YasTerminalSurface | null = null;

  onMount(() => {
    const s = new YasTerminalSurface({
      sessionId: props.sessionId,
      fontFamily: props.fontFamily ?? ctx.fontFamily,
      fontSize: props.fontSize ?? ctx.fontSize,
      palette: props.palette ?? ctx.palette,
      readOnly: props.readOnly,
      resizable: props.resizable,
      fitWidth: props.fitWidth,
      showCursor: props.showCursor,
      onRender: props.onRender,
      scrollbarColor: props.scrollbarColor,
      scrollbarWidth: props.scrollbarWidth,
      advanceRatio: props.advanceRatio ?? ctx.advanceRatio,
      textGamma: props.textGamma ?? ctx.textGamma,
    });
    ownedSurface = s;
    s.setWorkspace(workspace);
    s.attach(containerRef);
    setSurface(s);
    props.surfaceRef?.(s);
  });

  onCleanup(() => {
    const s = ownedSurface;
    ownedSurface = null;
    props.surfaceRef?.(null);
    s?.dispose();
  });

  // Forward connection changes. Reading snapshot() inside createEffect makes
  // this reactive — it re-runs whenever the workspace snapshot changes
  // (connection status transitions, new sessions, etc.).
  createEffect(() => {
    const s = surface();
    const snap = snapshot();
    const session = props.sessionId
      ? (snap.sessions.find((ss) => ss.id === props.sessionId) ?? null)
      : null;
    const conn = session ? workspace.getConnection(session.connectionId) : null;
    s?.setConnection(conn);
  });

  // Forward prop changes.
  createEffect(() => surface()?.setSessionId(props.sessionId));
  createEffect(() => surface()?.setPalette(props.palette ?? ctx.palette));
  createEffect(() =>
    surface()?.setFontFamily(props.fontFamily ?? ctx.fontFamily),
  );
  createEffect(() => surface()?.setFontSize(props.fontSize ?? ctx.fontSize));
  createEffect(() => surface()?.setShowCursor(props.showCursor));
  createEffect(() => surface()?.setOnRender(props.onRender));
  createEffect(() =>
    surface()?.setAdvanceRatio(props.advanceRatio ?? ctx.advanceRatio),
  );
  createEffect(() => surface()?.setTextGamma(props.textGamma ?? ctx.textGamma));
  createEffect(() => surface()?.setReadOnly(props.readOnly));
  createEffect(() => surface()?.setResizable(props.resizable));
  createEffect(() => surface()?.setFitWidth(props.fitWidth));

  // Re-send dimensions only when this session's connection becomes ready.
  // Reading snapshot() directly in the effect made every unrelated workspace
  // update repeat CONFIGURE_VIEW, including terminal frames caused by the
  // preceding configure.
  const readySessionId = createMemo(() => {
    const snap = snapshot();
    const session = props.sessionId
      ? (snap.sessions.find((ss) => ss.id === props.sessionId) ?? null)
      : null;
    const connection = session
      ? (snap.connections.find((c) => c.id === session.connectionId) ?? null)
      : null;
    return connection?.status === "connected" ? props.sessionId : null;
  });
  createEffect(() => {
    const s = surface();
    if (readySessionId() !== null) s?.resendSize();
  });

  return (
    <div
      ref={containerRef}
      class={props.class}
      style={{
        position: "relative",
        overflow: "hidden",
        ...props.style,
      }}
    />
  );
}
