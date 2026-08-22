import { useEffect, useImperativeHandle, useRef } from "react";
import type { Ref } from "react";
import type { ConnectionStatus } from "@yas-run/core";
import type { Terminal } from "@yas-run/browser";
import { YasTerminalSurface } from "@yas-run/core";
import type { YasTerminalProps } from "./types";
import { useYasContext, useRequiredYasWorkspace } from "./YasContext";
import { useYasConnection } from "./hooks/useYasConnection";
import { useYasSession } from "./hooks/useYasSession";

// ---------------------------------------------------------------------------
// Public handle exposed via ref
// ---------------------------------------------------------------------------

export interface YasTerminalHandle {
  /** The underlying WASM Terminal instance, if initialised. */
  terminal: Terminal | null;
  /** Current grid dimensions. */
  rows: number;
  cols: number;
  /** Current connection status. */
  status: ConnectionStatus;
  /** Focus the input sink so the terminal can receive keyboard events. */
  focus(): void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/**
 * YasTerminal renders a yas terminal inside a WebGL canvas.
 *
 * This is a thin wrapper over `YasTerminalSurface` from `@yas-run/core`.
 * It renders a container `<div>`, attaches the surface to it on mount,
 * and forwards prop changes to the surface's setters.
 */
export function YasTerminal({
  ref,
  ...props
}: YasTerminalProps & { ref?: Ref<YasTerminalHandle> }) {
  const ctx = useYasContext();
  const workspace = useRequiredYasWorkspace();
  const session = useYasSession(props.sessionId);
  const connection = useYasConnection(session?.connectionId);
  const yasConn = session
    ? workspace.getConnection(session.connectionId)
    : null;

  const {
    sessionId,
    fontFamily = ctx.fontFamily,
    fontSize = ctx.fontSize,
    className,
    style,
    palette = ctx.palette,
    readOnly,
    resizable,
    fitWidth,
    showCursor,
    onRender,
    scrollbarColor,
    scrollbarWidth,
    advanceRatio = ctx.advanceRatio,
    textGamma = ctx.textGamma,
  } = props;

  const containerRef = useRef<HTMLDivElement>(null);
  const surfaceRef = useRef<YasTerminalSurface | null>(null);

  // Create the surface once on mount.
  useEffect(() => {
    const surface = new YasTerminalSurface({
      sessionId,
      fontFamily,
      fontSize,
      palette,
      readOnly,
      resizable,
      fitWidth,
      showCursor,
      onRender,
      scrollbarColor,
      scrollbarWidth,
      advanceRatio,
      textGamma,
    });
    surfaceRef.current = surface;
    props.surfaceRef?.(surface);

    return () => {
      props.surfaceRef?.(null);
      surface.dispose();
      surfaceRef.current = null;
    };
    // Only create/destroy on mount/unmount. Props are forwarded via setters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Attach/detach to the container div.
  useEffect(() => {
    const surface = surfaceRef.current;
    const container = containerRef.current;
    if (!surface || !container) return;
    surface.attach(container);
    return () => surface.detach();
  }, []);

  // Forward workspace + connection.
  useEffect(() => {
    surfaceRef.current?.setWorkspace(workspace);
  }, [workspace]);

  useEffect(() => {
    surfaceRef.current?.setConnection(yasConn);
  }, [yasConn]);

  // Forward all prop changes.
  useEffect(() => {
    surfaceRef.current?.setSessionId(sessionId);
  }, [sessionId]);

  useEffect(() => {
    surfaceRef.current?.setPalette(palette);
  }, [palette]);

  useEffect(() => {
    surfaceRef.current?.setFontFamily(fontFamily);
  }, [fontFamily]);

  useEffect(() => {
    surfaceRef.current?.setFontSize(fontSize);
  }, [fontSize]);

  useEffect(() => {
    surfaceRef.current?.setShowCursor(showCursor);
  }, [showCursor]);

  useEffect(() => {
    surfaceRef.current?.setOnRender(onRender);
  }, [onRender]);

  useEffect(() => {
    surfaceRef.current?.setAdvanceRatio(advanceRatio);
  }, [advanceRatio]);

  useEffect(() => {
    surfaceRef.current?.setTextGamma(textGamma);
  }, [textGamma]);

  useEffect(() => {
    surfaceRef.current?.setReadOnly(readOnly);
  }, [readOnly]);

  useEffect(() => {
    surfaceRef.current?.setResizable(resizable);
  }, [resizable]);

  useEffect(() => {
    surfaceRef.current?.setFitWidth(fitWidth);
  }, [fitWidth]);

  // Re-send dimensions when connection becomes ready.
  const status: ConnectionStatus = connection?.status ?? "disconnected";
  useEffect(() => {
    if (status === "connected" && sessionId !== null) {
      surfaceRef.current?.resendSize();
    }
  }, [status, sessionId]);

  // Imperative handle.
  useImperativeHandle(
    ref,
    () => ({
      get terminal() {
        return surfaceRef.current?.currentTerminal ?? null;
      },
      get rows() {
        return surfaceRef.current?.rows ?? 24;
      },
      get cols() {
        return surfaceRef.current?.cols ?? 80;
      },
      status,
      focus() {
        surfaceRef.current?.focus();
      },
    }),
    [status],
  );

  return (
    <div
      ref={containerRef}
      className={className}
      style={{
        position: "relative",
        overflow: "hidden",
        ...style,
      }}
    />
  );
}
