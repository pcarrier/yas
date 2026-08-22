/**
 * Focused-pane actions rendered in the status bar. Keeping these controls out
 * of the pane avoids covering terminal, editor, web, or Wayland content while
 * retaining one universal drag handle for every content kind.
 */

import { Show, type JSX } from "solid-js";
import type { Theme, UIScale } from "./theme";
import { t } from "./i18n";
import { startPaneTileDrag, startPaneTouchDrag } from "./ide/tileDrag";

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

function FloatingIcon(props: { active: boolean; size: number }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ display: "block", "pointer-events": "none" }}
    >
      <rect
        x={props.active ? "2.5" : "2"}
        y={props.active ? "5.5" : "2"}
        width={props.active ? "8" : "12"}
        height={props.active ? "8" : "12"}
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
      />
      <Show when={props.active}>
        <path
          d="M5.5 5.5v-3h8v8h-3"
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

function ParkIcon(props: { size: number }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ display: "block", "pointer-events": "none" }}
    >
      <path
        d="M8 2.5v7M5.5 7L8 9.5 10.5 7M3 11v2h10v-2"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  );
}

export interface PaneToolActions {
  drag?: { assignment: string; paneId: string };
  floating?: { active: boolean; onToggle: () => void };
  solo?: { active: boolean; onToggle: () => void };
  onPark?: () => void;
  onClose: () => void;
}

/**
 * Keep the focused-pane toolbar mounted while its live action snapshot
 * changes. Layout mutations publish a fresh object because labels, active
 * states, pane ids, and assignments can all change together. Keying the
 * toolbar by that object replaces a pressed button before the browser can
 * deliver its click, which makes every action timing-dependent.
 */
export function PaneToolsSlot(props: {
  actions: PaneToolActions | null | undefined;
  theme: Theme;
  scale: UIScale;
}) {
  return (
    <Show when={props.actions}>
      {(actions) => (
        <PaneTools {...actions()} theme={props.theme} scale={props.scale} />
      )}
    </Show>
  );
}

export function PaneTools(
  props: PaneToolActions & {
    theme: Theme;
    scale: UIScale;
  },
) {
  const iconSize = () => props.scale.sm;
  const segment = (
    touchAction: "manipulation" | "none" = "manipulation",
    active = false,
    labelled = false,
  ): JSX.CSSProperties => ({
    display: "flex",
    "align-items": "center",
    "justify-content": "center",
    gap: labelled ? `${props.scale.tightGap}px` : 0,
    "min-width": `${props.scale.md * 1.75}px`,
    height: `${props.scale.md * 1.75}px`,
    padding: labelled ? `0 ${props.scale.tightGap}px` : 0,
    "background-color": active ? props.theme.selectedBg : "transparent",
    border: "none",
    "border-left": `1px solid ${props.theme.subtleBorder}`,
    "border-radius": "0",
    color: props.theme.fg,
    opacity: active ? 1 : 0.78,
    "font-family": "system-ui, sans-serif",
    "font-size": `${props.scale.xs}px`,
    "touch-action": touchAction,
  });
  // A status-bar click must not take focus from the pane it is about to act
  // on. Apart from disturbing terminal/editor input, that focus transition
  // can replace the focused action object between mousedown and click, leaving
  // the click attached to a disposed button. EditorActions uses the same rule.
  const keepPaneFocus = (event: MouseEvent) => event.preventDefault();
  return (
    <div
      role="toolbar"
      aria-label={t("pane.actions")}
      data-yas-pane-tools-assignment={props.drag?.assignment}
      data-yas-pane-tools-pane-id={props.drag?.paneId}
      style={{ display: "flex", "flex-shrink": 0 }}
    >
      <Show when={props.drag}>
        {(drag) => (
          <button
            type="button"
            title={t("pane.move")}
            aria-label={t("pane.move")}
            draggable={true}
            onDragStart={(event) => {
              const source = drag();
              startPaneTileDrag(event, source.assignment, source.paneId);
            }}
            onPointerDown={(event) => {
              const source = drag();
              startPaneTouchDrag(event, source.assignment, source.paneId);
            }}
            onClick={(event) => event.stopPropagation()}
            data-yas-pane-action="move"
            style={{ ...segment("none"), cursor: "grab" }}
          >
            <GripIcon size={iconSize()} />
          </button>
        )}
      </Show>
      <Show when={props.floating}>
        {(floating) => (
          <button
            type="button"
            title={floating().active ? t("pane.tile") : t("pane.float")}
            aria-label={floating().active ? t("pane.tile") : t("pane.float")}
            onMouseDown={keepPaneFocus}
            onClick={(event) => {
              event.stopPropagation();
              floating().onToggle();
            }}
            data-yas-pane-action="floating"
            aria-pressed={floating().active}
            style={{
              ...segment("manipulation", floating().active, true),
              cursor: "pointer",
            }}
          >
            <FloatingIcon active={floating().active} size={iconSize()} />
            <span>
              {floating().active ? t("pane.tileAction") : t("pane.floatAction")}
            </span>
          </button>
        )}
      </Show>
      <Show when={props.solo}>
        {(solo) => (
          <button
            type="button"
            title={solo().active ? t("pane.unsolo") : t("pane.solo")}
            aria-label={solo().active ? t("pane.unsolo") : t("pane.solo")}
            onMouseDown={keepPaneFocus}
            onClick={(event) => {
              event.stopPropagation();
              solo().onToggle();
            }}
            data-yas-pane-action="solo"
            aria-pressed={solo().active}
            style={{
              ...segment("manipulation", solo().active, true),
              cursor: "pointer",
            }}
          >
            <SoloIcon active={solo().active} size={iconSize()} />
            <span>
              {solo().active ? t("pane.unsoloAction") : t("pane.soloAction")}
            </span>
          </button>
        )}
      </Show>
      <Show when={props.onPark}>
        <button
          type="button"
          title={t("help.removeFromPane")}
          aria-label={t("help.removeFromPane")}
          onMouseDown={keepPaneFocus}
          onClick={(event) => {
            event.stopPropagation();
            props.onPark?.();
          }}
          data-yas-pane-action="park"
          style={{
            ...segment("manipulation", false, true),
            cursor: "pointer",
          }}
        >
          <ParkIcon size={iconSize()} />
          <span>{t("pane.parkAction")}</span>
        </button>
      </Show>
      <button
        type="button"
        title={t("pane.close")}
        aria-label={t("pane.close")}
        onMouseDown={keepPaneFocus}
        onClick={(event) => {
          event.stopPropagation();
          props.onClose();
        }}
        data-yas-pane-action="close"
        style={{ ...segment(), cursor: "pointer" }}
      >
        <CloseIcon size={iconSize()} />
      </button>
    </div>
  );
}
