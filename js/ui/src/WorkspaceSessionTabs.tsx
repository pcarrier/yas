import { For, onCleanup } from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import type { WorkspaceSessionController } from "./workspaceSession";
import { mergeStyle, themeFor, ui, uiScale } from "./theme";
import { t } from "./i18n";
import {
  detachWorkspaceSessionTab,
  openWorkspaceSessionManager,
  orderedWorkspaceSessionTabs,
  selectWorkspaceSessionTab,
  workspaceSessionTabKeyboardTarget,
} from "./workspaceSessionTabActions";

export function WorkspaceSessionTabs(props: {
  controller: WorkspaceSessionController;
  palette: TerminalPalette;
  /** The workspace's font family and size, so the bar is chrome of the same
   *  workspace rather than a strip of browser default sitting above it. */
  fontFamily: string;
  fontSize: number;
}) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  const tabRefs = new Map<string, HTMLButtonElement>();

  const selectAt = (index: number) => {
    const sessions = orderedWorkspaceSessionTabs(props.controller);
    const session = sessions[index];
    if (!session) return;
    void selectWorkspaceSessionTab(props.controller, session.id)
      .then(() => tabRefs.get(session.id)?.focus())
      .catch(() => {});
  };

  const moveFrom = (id: string, event: KeyboardEvent) => {
    const sessions = orderedWorkspaceSessionTabs(props.controller);
    const targetId = workspaceSessionTabKeyboardTarget(sessions, id, event.key);
    if (!targetId) return;
    event.preventDefault();
    const target = sessions.findIndex((session) => session.id === targetId);
    if (target >= 0) selectAt(target);
  };

  return (
    <nav
      aria-label={t("sessions.tabs")}
      style={{
        display: "flex",
        "align-items": "stretch",
        "flex-shrink": 0,
        "min-width": 0,
        height: `${Math.max(16, scale().sm + 4)}px`,
        color: theme().fg,
        "background-color": theme().solidPanelBg,
        "border-bottom": `1px solid ${theme().bg}`,
        "font-family": props.fontFamily,
        "font-size": `${scale().sm}px`,
      }}
    >
      <div
        role="tablist"
        aria-label={t("sessions.tabs")}
        style={{
          display: "flex",
          "align-items": "stretch",
          flex: 1,
          "min-width": 0,
          overflow: "auto hidden",
          "scrollbar-width": "thin",
          "scrollbar-color": `${theme().accent} ${theme().solidPanelBg}`,
        }}
      >
        <For each={orderedWorkspaceSessionTabs(props.controller)}>
          {(session) => {
            onCleanup(() => tabRefs.delete(session.id));
            const selected = () =>
              props.controller.current()?.id === session.id;
            return (
              <div
                style={{
                  display: "flex",
                  "align-items": "stretch",
                  "flex-shrink": 0,
                  width: "max-content",
                  "max-width": "180px",
                  "background-color": selected()
                    ? theme().bg
                    : theme().solidPanelBg,
                  "border-right": `1px solid ${theme().bg}`,
                  "box-shadow": selected()
                    ? `inset 0 -2px ${theme().accent}`
                    : "none",
                }}
              >
                <button
                  ref={(element) => tabRefs.set(session.id, element)}
                  type="button"
                  role="tab"
                  aria-selected={selected()}
                  tabindex={selected() ? 0 : -1}
                  title={session.name}
                  style={mergeStyle(ui.btn, {
                    color: theme().fg,
                    opacity: 1,
                    border: "none",
                    "background-color": selected()
                      ? theme().bg
                      : theme().solidPanelBg,
                    padding: "0 2px 0 4px",
                    "line-height": 1,
                    overflow: "hidden",
                    "text-overflow": "ellipsis",
                    "white-space": "nowrap",
                    cursor: "pointer",
                    "max-width": "164px",
                    "outline-color": theme().accent,
                  })}
                  onClick={() =>
                    void selectWorkspaceSessionTab(
                      props.controller,
                      session.id,
                    ).catch(() => {})
                  }
                  onKeyDown={(event) => moveFrom(session.id, event)}
                >
                  {session.name}
                </button>
                <button
                  type="button"
                  aria-label={`${t("sessions.detach")} ${session.name}`}
                  title={t("sessions.detachTab")}
                  style={mergeStyle(ui.btn, {
                    color: theme().fg,
                    opacity: 1,
                    border: "none",
                    "background-color": selected()
                      ? theme().bg
                      : theme().solidPanelBg,
                    padding: "0 2px 1px 0",
                    width: "12px",
                    cursor: "pointer",
                    "font-size": `${scale().sm + 2}px`,
                    "line-height": 1,
                    "outline-color": theme().accent,
                  })}
                  onClick={() =>
                    void detachWorkspaceSessionTab(
                      props.controller,
                      session.id,
                    ).catch(() => {})
                  }
                >
                  ×
                </button>
              </div>
            );
          }}
        </For>
      </div>
      <button
        type="button"
        aria-label={t("sessions.openManager")}
        title={t("sessions.openManager")}
        style={mergeStyle(ui.btn, {
          color: theme().fg,
          opacity: 1,
          border: "none",
          "border-left": `1px solid ${theme().accent}`,
          "background-color": theme().solidInputBg,
          padding: "0 5px",
          cursor: "pointer",
          "flex-shrink": 0,
          display: "grid",
          "place-items": "center",
          "outline-color": theme().accent,
        })}
        onClick={() => openWorkspaceSessionManager(props.controller)}
      >
        <svg
          viewBox="0 0 16 16"
          width={scale().sm + 3}
          height={scale().sm + 3}
          fill="none"
          stroke="currentColor"
          stroke-width="1.25"
          aria-hidden="true"
        >
          <rect x="2" y="2" width="12" height="3" />
          <rect x="2" y="6.5" width="12" height="3" />
          <rect x="2" y="11" width="12" height="3" />
        </svg>
      </button>
    </nav>
  );
}
