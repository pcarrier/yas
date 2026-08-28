/**
 * PaneTools — the multitool in a pane's corner: a grip that drags the pane's
 * content out (drop on another pane to move it there, or on the dock to park
 * it), drops or clicks to send the toolbar itself to another corner, and the
 * ✕ that closes the content (and, in a layout, removes its pane).
 *
 * One component for both render paths (layout leaf panes in LayoutContainer and the
 * single-view focused view in Workspace) so the two can't drift, the same reason
 * YasTile is shared.
 *
 * It is the one piece of chrome floating above every pane kind, which is what
 * makes it the drag handle: a surface's canvas swallows the pointer, so
 * without the grip a surface pane has nothing to grab.
 *
 * Visibility is pointer-dependent. With a pointer it appears on hover, keeping
 * the corner of the grid clear the rest of the time. On a touch device there is
 * no hover to reveal it and no way to type the Ctrl+Alt+Shift+Q chord that
 * closes a pane — MobileToolbar offers Ctrl and Alt but no Shift — so there it
 * is always shown. Without that, a terminal opened on Android cannot be closed
 * from the pane at all.
 */

import { Show, type JSX } from "solid-js";
import type { Theme, UIScale } from "./theme";
import { t } from "./i18n";
import {
  paneToolDropPreviewController,
  startPaneTileDrag,
  startPaneTouchDrag,
} from "./ide/tileDrag";
import { PANE_TOOL_CORNERS, type PaneToolCorner } from "./paneToolCorner";

// The workspace inherits its terminal font. Keep toolbar symbols out of that
// font so missing or unusual Windows glyphs cannot change their shape.
function GripIcon(props: { size: number }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ display: "block", "pointer-events": "none" }}
    >
      <circle cx="5" cy="4" r="1.35" fill="currentColor" />
      <circle cx="11" cy="4" r="1.35" fill="currentColor" />
      <circle cx="5" cy="8" r="1.35" fill="currentColor" />
      <circle cx="11" cy="8" r="1.35" fill="currentColor" />
      <circle cx="5" cy="12" r="1.35" fill="currentColor" />
      <circle cx="11" cy="12" r="1.35" fill="currentColor" />
    </svg>
  );
}

function SoloIcon(props: { active: boolean; size: number }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ display: "block", "pointer-events": "none" }}
    >
      <rect
        x="2.5"
        y="2.5"
        width="11"
        height="11"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
      />
      <Show
        when={props.active}
        fallback={<rect x="5" y="5" width="6" height="6" fill="currentColor" />}
      >
        <path
          d="M8 3v10M3 8h10"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
        />
      </Show>
    </svg>
  );
}

function CloseIcon(props: { size: number }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ display: "block", "pointer-events": "none" }}
    >
      <path
        d="M4 4l8 8M12 4l-8 8"
        fill="none"
        stroke="currentColor"
        stroke-width="1.75"
        stroke-linecap="round"
      />
    </svg>
  );
}

export function PaneTools(props: {
  theme: Theme;
  scale: UIScale;
  /** No hover to reveal it (touch): keep it on screen. */
  alwaysVisible: boolean;
  /** The pointer is over the pane. */
  hovered: boolean;
  /** When set, the grip is shown, dragging this assignment out of this pane.
   *  Absent in the single-view single view, where there is nowhere to drop. */
  drag?: { assignment: string; paneId: string };
  /** When set, the solo segment is shown. Absent where there is nothing to
   *  solo against: the single-view single view, and a one-pane layout. */
  solo?: { active: boolean; onToggle: () => void };
  onClose: () => void;
}) {
  // Which corner the toolbar sits in. It floats over the pane's content, and
  // a surface is a real app that may have its own controls exactly under the
  // default top-right — clicking the grip cycles the toolbar to the next
  // corner, out of the way of whatever it is covering. It follows the pane
  // assignment for the page lifetime, but is not persisted across reloads.
  const activeCorner = (): PaneToolCorner =>
    paneToolDropPreviewController.rememberedCorner(props.drag?.assignment);
  const rememberCorner = (next: PaneToolCorner) => {
    const assignment = props.drag?.assignment;
    if (assignment) paneToolDropPreviewController.remember(assignment, next);
  };
  const displayedCorner = () =>
    paneToolDropPreviewController.displayedCorner(props.drag);
  const onLeft = () => displayedCorner().endsWith("-left");
  const cornerStyle = (): JSX.CSSProperties => {
    const gap = `${props.scale.tightGap}px`;
    switch (displayedCorner()) {
      case "top-right":
        return { top: gap, right: gap };
      case "bottom-right":
        return { bottom: gap, right: gap };
      case "bottom-left":
        return { bottom: gap, left: gap };
      case "top-left":
        return { top: gap, left: gap };
    }
  };
  const touchScale = () => (props.alwaysVisible ? 1.5 : 1);
  const iconSize = () => props.scale.sm * touchScale();
  const segment = (
    touchAction: "manipulation" | "none" = "manipulation",
  ): JSX.CSSProperties => ({
    display: "flex",
    "align-items": "center",
    "justify-content": "center",
    "min-width": `${props.scale.md * 2 * touchScale()}px`,
    height: `${props.scale.md * 2 * touchScale()}px`,
    padding: 0,
    "background-color": props.theme.solidPanelBg,
    border: `1px solid ${props.theme.subtleBorder}`,
    "border-radius": "0",
    color: props.theme.fg,
    opacity: 0.75,
    "touch-action": touchAction,
  });
  return (
    <Show when={props.alwaysVisible || props.hovered}>
      <div
        data-yas-pane-tools-assignment={props.drag?.assignment}
        data-yas-pane-tools-pane-id={props.drag?.paneId}
        style={{
          position: "absolute",
          ...cornerStyle(),
          // Above the pane's content and above the tile-drag highlight, which
          // sits at 5 and is pointer-events:none.
          "z-index": 6,
          display: "flex",
          // Keep the close button against the outside edge and the grip
          // toward the pane's content on both sides.
          "flex-direction": onLeft() ? "row-reverse" : "row",
        }}
      >
        <Show when={props.drag}>
          {(drag) => (
            <button
              type="button"
              title={t("pane.move")}
              aria-label={t("pane.move")}
              draggable={true}
              onDragStart={(e) => {
                const source = drag();
                startPaneTileDrag(e, source.assignment, source.paneId);
                paneToolDropPreviewController.start(source, e);
              }}
              onDragEnd={paneToolDropPreviewController.finishActive}
              // Touch never reaches `onDragStart`, so a finger could tap this
              // (cycling the corner) but never move the pane.
              onPointerDown={(e) => {
                const source = { ...drag() };
                startPaneTouchDrag(e, source.assignment, source.paneId, {
                  onDragBegin: (event) =>
                    paneToolDropPreviewController.start(source, event),
                  onDragMove: paneToolDropPreviewController.update,
                  onDragFinish: (event) =>
                    paneToolDropPreviewController.finish(source, event),
                  onDragCancel: () =>
                    paneToolDropPreviewController.cancel(source),
                });
              }}
              // A click (no drag happened — the browser suppresses click
              // after a drag) relocates the toolbar itself. Stopped like the
              // ✕'s: the content underneath must not also see it as input.
              onClick={(e) => {
                e.stopPropagation();
                const current = PANE_TOOL_CORNERS.indexOf(activeCorner());
                rememberCorner(
                  PANE_TOOL_CORNERS[(current + 1) % PANE_TOOL_CORNERS.length],
                );
              }}
              style={{
                ...segment("none"),
                cursor: "grab",
                // The ✕ brings the shared edge; doubling it reads as a gap.
                [onLeft() ? "border-left" : "border-right"]: "none",
              }}
            >
              <GripIcon size={iconSize()} />
            </button>
          )}
        </Show>
        <Show when={props.solo}>
          {(solo) => (
            <button
              type="button"
              title={solo().active ? t("pane.unsolo") : t("pane.solo")}
              aria-label={solo().active ? t("pane.unsolo") : t("pane.solo")}
              onClick={(e) => {
                e.stopPropagation();
                solo().onToggle();
              }}
              style={{
                ...segment(),
                cursor: "pointer",
                // The ✕ brings the shared edge, as with the grip.
                [onLeft() ? "border-left" : "border-right"]: "none",
              }}
            >
              {/* One filled cell versus a grid of them: what you get, not
                  what you are leaving. */}
              <SoloIcon active={solo().active} size={iconSize()} />
            </button>
          )}
        </Show>
        <button
          type="button"
          title={t("pane.close")}
          aria-label={t("pane.close")}
          // Let pointerdown reach the pane — focusing what you are about to
          // close is harmless — but keep the click: a terminal or surface
          // underneath must not also see it as input.
          onClick={(e) => {
            e.stopPropagation();
            props.onClose();
          }}
          style={{ ...segment(), cursor: "pointer" }}
        >
          <CloseIcon size={iconSize()} />
        </button>
      </div>
    </Show>
  );
}
