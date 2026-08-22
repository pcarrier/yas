import { For, Show, createMemo, onCleanup } from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import type { WorkspaceSessionController } from "./workspaceSession";
import {
  mergeStyle,
  themeFor,
  ui,
  uiScale,
  workspaceBarSizing,
  workspaceBarStyle,
} from "./theme";
import { t, tp } from "./i18n";
import { TapButton } from "./TapButton";
import { PaneToolsSlot, type PaneToolActions } from "./PaneTools";
import {
  detachWorkspaceSessionTab,
  openWorkspaceSessionManager,
  orderedWorkspaceSessionTabs,
  selectWorkspaceSessionTab,
  workspaceSessionTabKeyboardTarget,
} from "./workspaceSessionTabActions";

interface WorkspaceSessionTabsProps {
  controller?: WorkspaceSessionController;
  paneActions?: PaneToolActions | null;
  palette: TerminalPalette;
  /** The workspace's font family and size, so the bar is chrome of the same
   *  workspace rather than a strip of browser default sitting above it. */
  fontFamily: string;
  fontSize: number;
  isMobileTouch?: boolean;
}

export function WorkspaceSessionTabs(props: WorkspaceSessionTabsProps) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  return (
    <nav
      aria-label={t("sessions.tabs")}
      style={{
        ...workspaceBarStyle(scale(), props.isMobileTouch),
        display: "flex",
        "align-items": "stretch",
        "min-width": 0,
        color: theme().fg,
        "background-color": theme().solidPanelBg,
        "border-bottom": `1px solid ${theme().bg}`,
        "font-family": props.fontFamily,
      }}
    >
      <Show when={props.controller} fallback={<div style={{ flex: 1 }} />}>
        {(controller) => (
          <WorkspaceSessionTabControls {...props} controller={controller()} />
        )}
      </Show>
      <PaneToolsSlot
        actions={props.paneActions}
        theme={theme()}
        scale={scale()}
        isMobileTouch={props.isMobileTouch}
      />
    </nav>
  );
}

function WorkspaceSessionTabControls(
  props: WorkspaceSessionTabsProps & {
    controller: WorkspaceSessionController;
  },
) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  const bar = () => workspaceBarSizing(scale(), props.isMobileTouch);
  // Keep session controls proportional to the touch tab bar.
  const px = (value: number) => `${value * bar().touchScale}px`;
  const tabRefs = new Map<string, HTMLButtonElement>();
  const sessionsById = createMemo(
    () =>
      new Map(
        orderedWorkspaceSessionTabs(props.controller).map((session) => [
          session.id,
          session,
        ]),
      ),
  );
  const managerNotice = () => {
    const error = props.controller.error();
    if (error) return error;
    const count = props.controller.warnings().length;
    return count > 0
      ? tp(count === 1 ? "sessions.warningOne" : "sessions.warningMany", {
          count,
        })
      : null;
  };

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
    <>
      <TapButton
        type="button"
        aria-label={
          managerNotice()
            ? `${t("sessions.openManager")}: ${managerNotice()}`
            : t("sessions.openManager")
        }
        title={managerNotice() ?? t("sessions.openManager")}
        style={mergeStyle(ui.btn, {
          color: theme().fg,
          opacity: 1,
          border: "none",
          "border-right": `1px solid ${theme().accent}`,
          "background-color": theme().solidInputBg,
          "min-width": `${bar().buttonWidth}px`,
          padding: 0,
          cursor: "pointer",
          "flex-shrink": 0,
          display: "grid",
          "place-items": "center",
          "outline-color": theme().accent,
          position: "relative",
        })}
        onActivate={() => openWorkspaceSessionManager(props.controller)}
      >
        <svg
          viewBox="0 0 16 16"
          width={bar().iconSize}
          height={bar().iconSize}
          fill="none"
          stroke="currentColor"
          stroke-width="1.25"
          aria-hidden="true"
        >
          <rect x="2" y="2" width="12" height="3" />
          <rect x="2" y="6.5" width="12" height="3" />
          <rect x="2" y="11" width="12" height="3" />
        </svg>
        <Show when={managerNotice()}>
          <span
            aria-hidden="true"
            style={{
              position: "absolute",
              top: px(1),
              right: px(1),
              width: px(5),
              height: px(5),
              "border-radius": "50%",
              "background-color": theme().error,
            }}
          />
        </Show>
      </TapButton>
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
        <For each={Array.from(sessionsById().keys())}>
          {(id) => {
            const session = () => sessionsById().get(id)!;
            onCleanup(() => tabRefs.delete(id));
            const selected = () => props.controller.current()?.id === id;
            return (
              <div
                style={{
                  display: "flex",
                  "align-items": "stretch",
                  "flex-shrink": 0,
                  width: "max-content",
                  "max-width": px(180),
                  "background-color": selected()
                    ? theme().fg
                    : theme().solidPanelBg,
                  "border-right": `1px solid ${theme().bg}`,
                  "box-shadow": selected()
                    ? `inset 0 -2px ${theme().accent}`
                    : "none",
                }}
              >
                <TapButton
                  ref={(element) => tabRefs.set(id, element)}
                  type="button"
                  role="tab"
                  aria-selected={selected()}
                  tabindex={selected() ? 0 : -1}
                  title={session().name}
                  style={mergeStyle(ui.btn, {
                    color: selected() ? theme().bg : theme().fg,
                    opacity: 1,
                    border: "none",
                    "background-color": selected()
                      ? theme().fg
                      : theme().solidPanelBg,
                    padding: `0 ${px(2)} 0 ${px(4)}`,
                    "font-size": "inherit",
                    "line-height": 1,
                    overflow: "hidden",
                    "text-overflow": "ellipsis",
                    "white-space": "nowrap",
                    cursor: "pointer",
                    "max-width": px(164),
                    "outline-color": theme().accent,
                  })}
                  onClick={() =>
                    void selectWorkspaceSessionTab(props.controller, id).catch(
                      () => {},
                    )
                  }
                  onKeyDown={(event) => moveFrom(id, event)}
                >
                  {session().name}
                </TapButton>
                <TapButton
                  type="button"
                  aria-label={tp("sessions.detachNamed", {
                    name: session().name,
                  })}
                  title={t("sessions.detachTab")}
                  style={mergeStyle(ui.btn, {
                    color: selected() ? theme().bg : theme().fg,
                    opacity: 1,
                    border: "none",
                    "background-color": selected()
                      ? theme().fg
                      : theme().solidPanelBg,
                    padding: `0 ${px(2)} ${px(1)} 0`,
                    width: `${bar().buttonWidth}px`,
                    "flex-shrink": 0,
                    cursor: "pointer",
                    "font-size": `${bar().iconSize}px`,
                    "line-height": 1,
                    "outline-color": theme().accent,
                  })}
                  onClick={() =>
                    void detachWorkspaceSessionTab(props.controller, id).catch(
                      () => {},
                    )
                  }
                >
                  ×
                </TapButton>
              </div>
            );
          }}
        </For>
      </div>
    </>
  );
}
