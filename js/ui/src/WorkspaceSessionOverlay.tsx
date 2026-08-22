import { TapButton } from "./TapButton";
import {
  For,
  Show,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import type { WorkspaceSessionController } from "./workspaceSession";
import { themeFor, ui, z } from "./theme";
import { t } from "./i18n";
import { YasMark } from "./Logo";
import { OverlayBackdrop, OverlayPanel } from "./Overlay";

export function WorkspaceSessionOverlay(props: {
  controller: WorkspaceSessionController;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
}) {
  const [editing, setEditing] = createSignal<string | null>(null);
  const [renameValue, setRenameValue] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const theme = () => themeFor(props.palette);
  // The manager is deliberately opt-in. Missing selections, catalogue
  // warnings, and connection failures remain visible when the user opens it,
  // but must never replace the workspace by themselves.
  const visible = () => props.controller.managerOpen();
  const sessionsById = createMemo(
    () =>
      new Map(
        props.controller.sessions().map((session) => [session.id, session]),
      ),
  );

  onMount(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && visible()) {
        event.preventDefault();
        props.controller.closeManager();
      }
    };
    window.addEventListener("keydown", keydown);
    onCleanup(() => {
      window.removeEventListener("keydown", keydown);
    });
  });

  const run = async (action: () => Promise<void>) => {
    if (busy()) return;
    setBusy(true);
    try {
      await action();
    } catch {
      // The controller retains and renders the actionable backend error.
    } finally {
      setBusy(false);
    }
  };

  const button = () => ({
    ...ui.btn,
    color: theme().fg,
    opacity: 1,
    border: `1px solid ${theme().accent}`,
    "background-color": theme().solidInputBg,
    "border-radius": "3px",
    cursor: busy() ? "wait" : "pointer",
  });

  const input = () => ({
    ...ui.input,
    color: theme().fg,
    "background-color": theme().solidInputBg,
    border: `1px solid ${theme().accent}`,
    "border-radius": "3px",
  });

  return (
    <Show when={visible()}>
      {/* While the catalogue is still arriving there is nothing to
          administer, so the mark stands alone on the backdrop: a panel would
          frame an empty box and name a concept nobody asked about. */}
      <Show when={props.controller.loading()}>
        <OverlayBackdrop
          palette={props.palette}
          label={t("sessions.label")}
          dismissOnBackdrop={false}
          style={{
            "z-index": z.disconnected + 1,
            background: theme().solidPanelBg,
            color: theme().dimFg,
          }}
        >
          <div aria-busy="true">
            <YasMark size={96} />
          </div>
        </OverlayBackdrop>
      </Show>
      <Show when={!props.controller.loading()}>
        <OverlayBackdrop
          palette={props.palette}
          label={t("sessions.label")}
          onClose={() => props.controller.closeManager()}
          style={{
            "z-index": z.disconnected + 1,
            // The theme's own background, opaque: a translucent wash over the
            // workspace reads as grey whatever the palette says.
            background: theme().bg,
            color: theme().fg,
            "font-family": props.fontFamily,
            "font-size": `${props.fontSize}px`,
          }}
        >
          <OverlayPanel
            palette={props.palette}
            fontSize={props.fontSize}
            style={{
              display: "grid",
              gap: "14px",
              width: "min(920px, 96%)",
              "max-height": "min(760px, var(--overlay-panel-cap, 80%))",
              overflow: "auto",
              padding: "16px",
              border: `1px solid ${theme().accent}`,
              "border-radius": "5px",
              background: theme().solidPanelBg,
              "font-family": props.fontFamily,
            }}
          >
            <header
              style={{
                display: "flex",
                "align-items": "start",
                "justify-content": "space-between",
                gap: "12px",
              }}
            >
              <div>
                <h2 style={{ margin: "0", "font-size": "16px" }}>
                  {t("sessions.title")}
                </h2>
              </div>
              <TapButton
                type="button"
                style={button()}
                onClick={() => props.controller.closeManager()}
              >
                {t("overlay.close")}
              </TapButton>
            </header>

            <Show when={props.controller.error()}>
              {(error) => (
                <div
                  role="alert"
                  style={{
                    padding: "9px",
                    border: `1px solid ${theme().error}`,
                    color: theme().errorText,
                    "white-space": "pre-wrap",
                  }}
                >
                  {error()}
                </div>
              )}
            </Show>

            <Show when={props.controller.warnings().length > 0}>
              <div
                role="status"
                style={{
                  padding: "9px",
                  border: `1px solid ${theme().warning}`,
                  color: theme().warning,
                }}
              >
                <strong>{t("sessions.invalidRecords")}</strong>
                <ul style={{ margin: "6px 0 0", "padding-left": "20px" }}>
                  <For each={props.controller.warnings()}>
                    {(warning) => <li>{warning}</li>}
                  </For>
                </ul>
              </div>
            </Show>

            <div
              style={{
                display: "grid",
                "grid-template-columns": "minmax(0, 1fr)",
                gap: "16px",
              }}
            >
              <div
                style={{
                  display: "grid",
                  gap: "8px",
                  "align-content": "start",
                }}
              >
                <h3 style={{ margin: "0", "font-size": "13px" }}>
                  {t("sessions.saved")}
                </h3>
                <Show
                  when={props.controller.sessions().length > 0}
                  fallback={
                    <div style={{ color: theme().fg, padding: "10px 0" }}>
                      {t("sessions.empty")}
                    </div>
                  }
                >
                  <For each={Array.from(sessionsById().keys())}>
                    {(id) => {
                      const session = () => sessionsById().get(id)!;
                      const selected = () =>
                        props.controller.current()?.id === id;
                      const attached = () =>
                        props.controller.attachedSessionIds().includes(id);
                      return (
                        <div
                          style={{
                            display: "grid",
                            gap: "6px",
                            padding: "9px",
                            border: `1px solid ${
                              selected() ? theme().accent : theme().bg
                            }`,
                            "background-color": selected()
                              ? theme().bg
                              : theme().solidPanelBg,
                          }}
                        >
                          <div
                            style={{
                              display: "flex",
                              "align-items": "center",
                              gap: "7px",
                            }}
                          >
                            <Show
                              when={editing() === id}
                              fallback={
                                <strong style={{ flex: "1", "min-width": "0" }}>
                                  {session().name}
                                </strong>
                              }
                            >
                              <form
                                style={{
                                  display: "flex",
                                  gap: "5px",
                                  flex: "1",
                                }}
                                onSubmit={(event) => {
                                  event.preventDefault();
                                  void run(async () => {
                                    await props.controller.rename(
                                      id,
                                      renameValue(),
                                    );
                                    setEditing(null);
                                  });
                                }}
                              >
                                <input
                                  aria-label={t("sessions.rename")}
                                  value={renameValue()}
                                  maxLength={128}
                                  style={{
                                    ...input(),
                                    flex: "1",
                                    "min-width": "0",
                                  }}
                                  onInput={(event) =>
                                    setRenameValue(event.currentTarget.value)
                                  }
                                />
                                <TapButton
                                  type="submit"
                                  style={button()}
                                  disabled={busy()}
                                >
                                  {t("sessions.save")}
                                </TapButton>
                              </form>
                            </Show>
                            <Show when={selected()}>
                              <span style={{ color: theme().success }}>
                                {t("sessions.selected")}
                              </span>
                            </Show>
                            <Show when={attached() && !selected()}>
                              <span style={{ color: theme().accent }}>
                                {t("sessions.attached")}
                              </span>
                            </Show>
                          </div>
                          <div
                            style={{
                              color: theme().fg,
                              "font-size": "11px",
                            }}
                          >
                            {new Date(
                              session().updatedAtUnixMs,
                            ).toLocaleString()}
                          </div>
                          <div
                            style={{
                              display: "flex",
                              gap: "5px",
                              "flex-wrap": "wrap",
                            }}
                          >
                            <Show when={selected()}>
                              <TapButton
                                type="button"
                                style={button()}
                                disabled={busy()}
                                onClick={() =>
                                  void run(() => props.controller.detach(id))
                                }
                              >
                                {t("sessions.detach")}
                              </TapButton>
                            </Show>
                            <Show when={attached() && !selected()}>
                              <TapButton
                                type="button"
                                style={button()}
                                disabled={busy()}
                                onClick={() =>
                                  void run(() => props.controller.select(id))
                                }
                              >
                                {t("sessions.select")}
                              </TapButton>
                              <TapButton
                                type="button"
                                style={button()}
                                disabled={busy()}
                                onClick={() =>
                                  void run(() => props.controller.detach(id))
                                }
                              >
                                {t("sessions.detach")}
                              </TapButton>
                            </Show>
                            <Show when={!attached()}>
                              <TapButton
                                type="button"
                                style={button()}
                                disabled={busy()}
                                onClick={() =>
                                  void run(() => props.controller.attach(id))
                                }
                              >
                                {t("sessions.attach")}
                              </TapButton>
                            </Show>
                            <TapButton
                              type="button"
                              style={button()}
                              disabled={busy()}
                              onClick={() => {
                                setEditing(id);
                                setRenameValue(session().name);
                              }}
                            >
                              {t("sessions.rename")}
                            </TapButton>
                            <TapButton
                              type="button"
                              style={{
                                ...button(),
                                color: theme().errorText,
                              }}
                              disabled={busy()}
                              onClick={() => {
                                if (confirmDelete() !== id) {
                                  setConfirmDelete(id);
                                  return;
                                }
                                setConfirmDelete(null);
                                void run(() => props.controller.delete(id));
                              }}
                            >
                              {confirmDelete() === id
                                ? t("sessions.confirmDelete")
                                : t("sessions.delete")}
                            </TapButton>
                          </div>
                        </div>
                      );
                    }}
                  </For>
                </Show>

                <TapButton
                  type="button"
                  style={{ ...button(), "margin-top": "4px" }}
                  disabled={busy()}
                  onClick={() => void run(() => props.controller.create())}
                >
                  {t("sessions.create")}
                </TapButton>
              </div>
            </div>

            <Show when={props.controller.error()}>
              <TapButton
                type="button"
                style={button()}
                disabled={busy()}
                onClick={() => void run(() => props.controller.retry())}
              >
                {t("sessions.retry")}
              </TapButton>
            </Show>
          </OverlayPanel>
        </OverlayBackdrop>
      </Show>
    </Show>
  );
}
