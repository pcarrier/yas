import { For, Show } from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import { themeFor, ui, uiScale, z } from "./theme";
import { prefixArmed, prefixBindings } from "./keyPrefix";
import { t } from "./i18n";

/**
 * What Ctrl+B accepts, shown while it is waiting.
 *
 * A prefix is one chord standing in for twenty, which is only an improvement
 * if you can find out what the twenty are without leaving what you are doing.
 * So arming it draws the map: every key bound right now, including the ones a
 * layout brings with it and takes away again.
 *
 * Deliberately not a modal — no backdrop, no focus trap, nothing to dismiss.
 * The next keystroke resolves the prefix either way, and a panel that had to
 * be closed would make the fast path slower.
 */
export function PrefixMap(props: {
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
}) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  // Registration order is the menu order. Workspace actions deliberately put
  // the literal prefix first, help second, and the launcher third; layout-only
  // actions are registered afterwards and follow them.
  const bindings = prefixBindings;

  return (
    <Show when={prefixArmed() && bindings().length > 0}>
      <div
        role="status"
        aria-label={t("prefix.map")}
        style={{
          position: "fixed",
          left: "50%",
          bottom: `${scale().gap * 3}px`,
          transform: "translateX(-50%)",
          "max-width": "min(92vw, 900px)",
          // Wrapping row of content-sized entries rather than a column grid:
          // a grid column is as wide as the column, and a label that did not
          // fit one was cut. Here an entry is as wide as it needs to be, and
          // the row wraps.
          display: "flex",
          "flex-wrap": "wrap",
          "justify-content": "center",
          gap: `${scale().tightGap}px ${scale().gap}px`,
          padding: `${scale().panelPadding}px`,
          "border-radius": `${scale().tightGap}px`,
          border: `1px solid ${theme().border}`,
          background: theme().solidPanelBg,
          color: theme().fg,
          "box-shadow": "0 12px 40px rgba(0,0,0,.45)",
          "font-family": props.fontFamily,
          "font-size": `${scale().sm}px`,
          "z-index": z.overlay,
          "pointer-events": "none",
        }}
      >
        <For each={bindings()}>
          {(binding) => (
            <div
              style={{
                display: "flex",
                gap: `${scale().tightGap}px`,
                "align-items": "baseline",
                flex: "0 0 auto",
                "max-width": "100%",
              }}
            >
              <kbd
                style={{
                  ...ui.kbd,
                  "font-size": `${scale().sm}px`,
                  "flex-shrink": 0,
                }}
              >
                {binding.token === "prefix" ? "Ctrl+B" : binding.token}
              </kbd>
              {/* A label is never shortened: a map of the keys that abbreviates
                  what a key does is not a map. One long enough to need the
                  whole panel wraps inside its own entry. */}
              <span
                style={{
                  color: theme().dimFg,
                  "overflow-wrap": "anywhere",
                }}
              >
                {binding.label}
              </span>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
}
