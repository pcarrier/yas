import { For, onCleanup, onMount } from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import type { WindowManager } from "@yas-run/core/layout";
import { OverlayBackdrop, OverlayHeader, OverlayPanel } from "../Overlay";
import { t } from "../i18n";
import { themeFor, uiScale } from "../theme";
import { nextManagerChoice, WINDOW_MANAGERS } from "./windowManagerChoice";

function managerLabel(manager: WindowManager): string {
  if (manager === "tiling") return t("windowManager.tiling");
  if (manager === "scrolling") return t("windowManager.scrolling");
  return t("windowManager.floating");
}

function managerDescription(manager: WindowManager): string {
  if (manager === "tiling") return t("windowManager.tilingDescription");
  if (manager === "scrolling")
    return t("windowManager.scrollingDescription");
  return t("windowManager.floatingDescription");
}

export function WindowManagerChooser(props: {
  current: WindowManager;
  palette: TerminalPalette;
  fontSize: number;
  onChoose: (manager: WindowManager) => void;
  onClose: () => void;
}) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  const returnFocus =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  const buttons: HTMLButtonElement[] = [];

  onMount(() => {
    const index = WINDOW_MANAGERS.findIndex(
      (manager) => manager === props.current,
    );
    queueMicrotask(() => buttons[Math.max(0, index)]?.focus());
  });
  onCleanup(() => {
    queueMicrotask(() => {
      if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    });
  });

  const choose = (manager: WindowManager) => {
    props.onChoose(manager);
    props.onClose();
  };
  const keydown = (event: KeyboardEvent, index: number) => {
    if (event.key === "Escape") {
      event.preventDefault();
      props.onClose();
      return;
    }
    const next = nextManagerChoice(index, event.key);
    if (next === null) return;
    event.preventDefault();
    buttons[next]?.focus();
  };

  return (
    <OverlayBackdrop
      palette={props.palette}
      label={t("windowManager.title")}
      onClose={props.onClose}
    >
      <OverlayPanel
        palette={props.palette}
        fontSize={props.fontSize}
        style={{ width: "min(92vw, 34rem)" }}
      >
        <OverlayHeader
          palette={props.palette}
          fontSize={props.fontSize}
          title={t("windowManager.title")}
          subtitle={t("windowManager.subtitle")}
          onClose={props.onClose}
        />
        <div
          role="listbox"
          aria-label={t("windowManager.title")}
          style={{
            display: "grid",
            gap: `${scale().tightGap}px`,
            padding: `${scale().panelPadding}px`,
          }}
        >
          <For each={WINDOW_MANAGERS}>
            {(manager, index) => {
              const current = () => manager === props.current;
              return (
                <button
                  ref={(element) => (buttons[index()] = element)}
                  type="button"
                  role="option"
                  aria-selected={current()}
                  onClick={() => choose(manager)}
                  onKeyDown={(event) => keydown(event, index())}
                  style={{
                    display: "grid",
                    gap: `${scale().tightGap}px`,
                    padding: `${scale().panelPadding}px`,
                    border: `1px solid ${current() ? theme().accent : theme().subtleBorder}`,
                    "border-radius": "0",
                    background: current()
                      ? theme().selectedBg
                      : theme().solidInputBg,
                    color: theme().fg,
                    "font-family": "inherit",
                    "font-size": "inherit",
                    "text-align": "left",
                    cursor: "pointer",
                    "box-shadow": "none",
                  }}
                >
                  <span
                    style={{
                      display: "flex",
                      "justify-content": "space-between",
                      gap: `${scale().gap}px`,
                      "font-weight": 700,
                    }}
                  >
                    <span>{managerLabel(manager)}</span>
                    {current() ? (
                      <span style={{ color: theme().accent }}>
                        {t("windowManager.current")}
                      </span>
                    ) : null}
                  </span>
                  <span style={{ color: theme().dimFg }}>
                    {managerDescription(manager)}
                  </span>
                </button>
              );
            }}
          </For>
        </div>
      </OverlayPanel>
    </OverlayBackdrop>
  );
}
