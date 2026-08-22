import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import type { TerminalPalette } from "@yas-run/core";
import type { WorkspaceSessionController } from "./workspaceSession";
import { themeFor, ui, z } from "./theme";
import { t } from "./i18n";
import { YasMark } from "./Logo";

export function WorkspaceSessionOverlay(props: {
  controller: WorkspaceSessionController;
  palette: TerminalPalette;
  fontFamily: string;
  fontSize: number;
}) {
  const [newName, setNewName] = createSignal("");
  const [editing, setEditing] = createSignal<string | null>(null);
  const [renameValue, setRenameValue] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const theme = () => themeFor(props.palette);
  const visible = () =>
    props.controller.managerOpen() || props.controller.binding() === null;

  onMount(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && visible() && props.controller.binding()) {
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
      <Portal mount={document.body}>
        {/* While the catalogue is still arriving there is nothing to
            administer, so the mark stands alone on the backdrop: a panel would
            frame an empty box and name a concept nobody asked about. */}
        <Show when={props.controller.loading()}>
          <div
            aria-busy="true"
            style={{
              position: "fixed",
              inset: "0",
              "z-index": z.disconnected + 1,
              display: "grid",
              "place-items": "center",
              background: theme().solidPanelBg,
              color: theme().accent,
            }}
          >
            <YasMark size={96} />
          </div>
        </Show>
        <Show when={!props.controller.loading()}>
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("sessions.label")}
            style={{
              position: "fixed",
              inset: "0",
              "z-index": z.disconnected + 1,
              display: "grid",
              "place-items": "center",
              padding: "16px",
              // The theme's own background, opaque: a translucent wash over the
              // workspace reads as grey whatever the palette says.
              background: theme().bg,
              color: theme().fg,
              "font-family": props.fontFamily,
              "font-size": `${props.fontSize}px`,
            }}
            onClick={() => props.controller.closeManager()}
          >
            <section
              style={{
                display: "grid",
                gap: "14px",
                width: "min(920px, 96vw)",
                "max-height": "min(760px, 92vh)",
                overflow: "auto",
                padding: "16px",
                border: `1px solid ${theme().accent}`,
                "border-radius": "5px",
                background: theme().solidPanelBg,
              }}
              onClick={(event) => event.stopPropagation()}
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
                <Show when={props.controller.binding()}>
                  <button
                    type="button"
                    style={button()}
                    onClick={() => props.controller.closeManager()}
                  >
                    {t("overlay.close")}
                  </button>
                </Show>
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
                    <For each={props.controller.sessions()}>
                      {(session) => {
                        const selected = () =>
                          props.controller.current()?.id === session.id;
                        const attached = () =>
                          props.controller
                            .attachedSessionIds()
                            .includes(session.id);
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
                                when={editing() === session.id}
                                fallback={
                                  <strong
                                    style={{ flex: "1", "min-width": "0" }}
                                  >
                                    {session.name}
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
                                        session.id,
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
                                  <button
                                    type="submit"
                                    style={button()}
                                    disabled={busy()}
                                  >
                                    {t("sessions.save")}
                                  </button>
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
                                session.updatedAtUnixMs,
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
                                <button
                                  type="button"
                                  style={button()}
                                  disabled={busy()}
                                  onClick={() =>
                                    void run(() =>
                                      props.controller.detach(session.id),
                                    )
                                  }
                                >
                                  {t("sessions.detach")}
                                </button>
                              </Show>
                              <Show when={attached() && !selected()}>
                                <button
                                  type="button"
                                  style={button()}
                                  disabled={busy()}
                                  onClick={() =>
                                    void run(() =>
                                      props.controller.select(session.id),
                                    )
                                  }
                                >
                                  {t("sessions.select")}
                                </button>
                                <button
                                  type="button"
                                  style={button()}
                                  disabled={busy()}
                                  onClick={() =>
                                    void run(() =>
                                      props.controller.detach(session.id),
                                    )
                                  }
                                >
                                  {t("sessions.detach")}
                                </button>
                              </Show>
                              <Show when={!attached()}>
                                <button
                                  type="button"
                                  style={button()}
                                  disabled={busy()}
                                  onClick={() =>
                                    void run(() =>
                                      props.controller.attach(session.id),
                                    )
                                  }
                                >
                                  {t("sessions.attach")}
                                </button>
                              </Show>
                              <button
                                type="button"
                                style={button()}
                                disabled={busy()}
                                onClick={() => {
                                  setEditing(session.id);
                                  setRenameValue(session.name);
                                }}
                              >
                                {t("sessions.rename")}
                              </button>
                              <button
                                type="button"
                                style={{
                                  ...button(),
                                  color: theme().errorText,
                                }}
                                disabled={busy()}
                                onClick={() => {
                                  if (confirmDelete() !== session.id) {
                                    setConfirmDelete(session.id);
                                    return;
                                  }
                                  setConfirmDelete(null);
                                  void run(() =>
                                    props.controller.delete(session.id),
                                  );
                                }}
                              >
                                {confirmDelete() === session.id
                                  ? t("sessions.confirmDelete")
                                  : t("sessions.delete")}
                              </button>
                            </div>
                          </div>
                        );
                      }}
                    </For>
                  </Show>

                  <form
                    style={{ display: "flex", gap: "6px", "margin-top": "4px" }}
                    onSubmit={(event) => {
                      event.preventDefault();
                      void run(async () => {
                        await props.controller.create(newName());
                        setNewName("");
                      });
                    }}
                  >
                    <input
                      aria-label={t("sessions.newName")}
                      placeholder={t("sessions.newName")}
                      value={newName()}
                      maxLength={128}
                      style={{ ...input(), flex: "1", "min-width": "0" }}
                      onInput={(event) => setNewName(event.currentTarget.value)}
                    />
                    <button type="submit" style={button()} disabled={busy()}>
                      {t("sessions.create")}
                    </button>
                  </form>
                </div>
              </div>

              <Show when={props.controller.error()}>
                <button
                  type="button"
                  style={button()}
                  disabled={busy()}
                  onClick={() => void run(() => props.controller.retry())}
                >
                  {t("sessions.retry")}
                </button>
              </Show>
            </section>
          </div>
        </Show>
      </Portal>
    </Show>
  );
}
