import { createMemo, createSignal, For, Show } from "solid-js";
import type { JSX } from "solid-js";
import type {
  YasConnectionSnapshot,
  ConnectionStatus,
  TerminalPalette,
} from "@yas-run/core";
import { OverlayBackdrop, OverlayHeader, OverlayPanel } from "./Overlay";
import { scrollbarStyle, themeFor, ui, uiScale } from "./theme";
import { t } from "./i18n";
import { validRemoteName, type StoredRemote } from "./remotesStore";
import {
  setWorkspaceSessionRemoteMembership,
  type Remote,
} from "./workspaceSessionRemotes";

const STATUS_COLORS: Record<string, string> = {
  connected: "#4caf50",
  connecting: "#ff9800",
  authenticating: "#ff9800",
  disconnected: "#888",
  closed: "#888",
  error: "#f44336",
};

/** Relay catalogue and per-session membership. Connector configuration stays
 * on the server and is never copied into browser storage. */
export function RemotesOverlay(props: {
  remotes: Remote[];
  statuses?: ReadonlyMap<string, ConnectionStatus>;
  palette: TerminalPalette;
  fontSize: number;
  /** Remote names materialized by the selected workspace. */
  activeRemotes?: readonly string[];
  onSetSessionActive?: (name: string, active: boolean) => void | Promise<void>;
  onReconnect?: (name: string) => void;
  onClose: () => void;
  connections?: readonly YasConnectionSnapshot[];
  onManage?: (name: string) => void;
  /**
   * The catalogue as stored on the home server, and how to edit it. Absent
   * while no home connection is up, which is what disables the editor rather
   * than hiding it: "you cannot do this right now" is information.
   */
  stored?: readonly StoredRemote[];
  onAddRemote?: (name: string, uri: string) => void | Promise<void>;
  onRemoveRemote?: (name: string) => void | Promise<void>;
  onToggleRemote?: (name: string) => void | Promise<void>;
}) {
  const [newName, setNewName] = createSignal("");
  const [newUri, setNewUri] = createSignal("");
  const [editError, setEditError] = createSignal<string | null>(null);
  const [hovered, setHovered] = createSignal<string | null>(null);
  const storedFor = (name: string) =>
    props.stored?.find((remote) => remote.name === name);
  const canEdit = () => !!props.onAddRemote;
  const submit = () => {
    const name = newName().trim();
    const uri = newUri().trim();
    if (!validRemoteName(name)) {
      setEditError(t("remotes.invalidName"));
      return;
    }
    if (!uri) {
      setEditError(t("remotes.invalidUri"));
      return;
    }
    setEditError(null);
    void Promise.resolve(props.onAddRemote?.(name, uri)).then(() => {
      setNewName("");
      setNewUri("");
    });
  };
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  const connectionFor = (name: string) =>
    props.connections?.find((connection) => connection.id === name);
  const canManage = (name: string) =>
    !!props.onManage && connectionFor(name)?.status === "connected";
  const anyManageable = () =>
    props.remotes.some((remote) => canManage(remote.name));
  const sessionActive = (name: string) =>
    name === "local" || (props.activeRemotes ?? []).includes(name);
  const missingSessionRemotes = createMemo(() => {
    const available = new Set(props.remotes.map((remote) => remote.name));
    available.add("local");
    return (props.activeRemotes ?? []).filter(
      (name, index, all) =>
        !!name && !available.has(name) && all.indexOf(name) === index,
    );
  });
  const buttonStyle = () => ({
    ...ui.btn,
    "font-size": `${scale().sm}px`,
    "border-radius": "0",
    border: "none",
    "background-color": "transparent",
    color: "inherit",
    padding: `${scale().controlY}px ${scale().controlX + 2}px`,
    cursor: "pointer",
    "white-space": "nowrap",
    opacity: 0.7,
  });
  // Every action cell is the same box whether it is a button or the dimmed
  // stand-in for one, so a row of them shares a baseline.
  const actionStyle = () => ({
    ...buttonStyle(),
    display: "flex",
    "align-items": "center",
    "align-self": "stretch",
    "border-left": `1px solid ${theme().subtleBorder}`,
  });
  const inputStyle = () => ({
    ...ui.input,
    "background-color": theme().inputBg,
    color: "inherit",
    border: `1px solid ${theme().border}`,
    "border-radius": "0",
    "font-size": `${scale().sm}px`,
    padding: `${scale().controlY}px ${scale().controlX}px`,
    "min-width": "0",
  });

  // Session membership, identity, then the action strip: manage, enable,
  // remove, reconnect. Declared once on the list and inherited by every row
  // through `subgrid`, so the columns line up down the panel instead of each
  // row sizing its own.
  const columns = "auto minmax(12em, 1fr) auto auto auto auto";
  // A cell that keeps a column's place on a row that has no action there.
  const filler = (): JSX.Element => <div />;

  return (
    <OverlayBackdrop
      palette={props.palette}
      label={t("remotes.label")}
      onClose={props.onClose}
    >
      <OverlayPanel
        palette={props.palette}
        fontSize={props.fontSize}
        style={{
          display: "flex",
          "flex-direction": "column",
          gap: `${scale().gap}px`,
          width: "fit-content",
          "min-width": "min(94vw, 32em)",
          "max-width": "min(860px, 94vw)",
        }}
      >
        <OverlayHeader
          palette={props.palette}
          fontSize={props.fontSize}
          title={t("remotes.title")}
          onClose={props.onClose}
        />

        <Show
          when={props.remotes.length > 0}
          fallback={
            <div
              style={{
                padding: `${scale().panelPadding}px`,
                border: `1px dashed ${theme().border}`,
                "text-align": "center",
                color: theme().dimFg,
                "font-size": `${scale().sm}px`,
              }}
            >
              {t("remotes.empty")}
            </div>
          }
        >
          <div
            role="list"
            style={{
              display: "grid",
              "grid-template-columns": columns,
              border: `1px solid ${theme().border}`,
              "max-height": "60vh",
              "overflow-y": "auto",
              ...scrollbarStyle(theme()),
            }}
          >
            <For each={props.remotes}>
              {(remote, index) => {
                const status = () => props.statuses?.get(remote.name) ?? null;
                const rtt = () => connectionFor(remote.name)?.rttMs ?? null;
                const statusColor = () => {
                  const current = status();
                  return current
                    ? (STATUS_COLORS[current] ?? theme().dimFg)
                    : theme().dimFg;
                };
                // Connected is what the dot already says; anything else is
                // worth spelling out next to the URI.
                const statusText = () => {
                  const current = status();
                  return current && current !== "connected"
                    ? t(`remotes.status.${current}`)
                    : null;
                };
                const entry = () => storedFor(remote.name);
                const disabled = () => !!entry()?.disabled;
                return (
                  <div
                    role="listitem"
                    onMouseEnter={() => setHovered(remote.name)}
                    onMouseLeave={() =>
                      setHovered((current) =>
                        current === remote.name ? null : current,
                      )
                    }
                    style={{
                      display: "grid",
                      "grid-template-columns": "subgrid",
                      "grid-column": "1 / -1",
                      "align-items": "center",
                      "border-top":
                        index() === 0
                          ? undefined
                          : `1px solid ${theme().border}`,
                      "background-color":
                        hovered() === remote.name
                          ? theme().hoverBg
                          : theme().solidPanelBg,
                      // A disabled entry stays legible but visibly out of
                      // service, the way a disabled root does.
                      opacity: disabled() ? 0.55 : 1,
                    }}
                  >
                    {/* Membership is a state, not a command, and it leads the
                        row: a column of checkboxes says which remotes this
                        session materializes at a glance, where a column of
                        "Add to session" buttons only said what could be
                        pressed. */}
                    <Show when={props.onSetSessionActive} fallback={filler()}>
                      <label
                        title={
                          remote.name === "local"
                            ? t("remotes.localAlwaysActive")
                            : sessionActive(remote.name)
                              ? t("remotes.removeFromSession")
                              : t("remotes.addToSession")
                        }
                        style={{
                          display: "flex",
                          "align-items": "center",
                          "justify-content": "center",
                          "align-self": "stretch",
                          padding: `0 ${scale().controlX}px`,
                          cursor:
                            remote.name === "local" ? "default" : "pointer",
                        }}
                      >
                        <input
                          type="checkbox"
                          checked={sessionActive(remote.name)}
                          disabled={remote.name === "local"}
                          aria-label={
                            remote.name === "local"
                              ? t("remotes.localAlwaysActive")
                              : sessionActive(remote.name)
                                ? t("remotes.removeFromSession")
                                : t("remotes.addToSession")
                          }
                          onChange={(event) =>
                            void setWorkspaceSessionRemoteMembership(
                              props.onSetSessionActive,
                              remote.name,
                              event.currentTarget.checked,
                            )
                          }
                          style={{
                            margin: "0",
                            width: `${scale().md}px`,
                            height: `${scale().md}px`,
                            "accent-color": theme().accent,
                            cursor: "inherit",
                          }}
                        />
                      </label>
                    </Show>

                    <div
                      style={{
                        padding: `${scale().controlY}px ${scale().controlX}px`,
                        display: "grid",
                        "grid-template-columns": "auto minmax(0, 1fr)",
                        gap: `0 ${scale().tightGap}px`,
                        "align-items": "center",
                        "min-width": "0",
                      }}
                    >
                      <span
                        title={status() ? t(`remotes.status.${status()}`) : ""}
                        style={{
                          width: "8px",
                          height: "8px",
                          "border-radius": "50%",
                          "background-color": statusColor(),
                          "grid-row": "1 / span 2",
                        }}
                      />
                      <strong
                        style={{
                          "font-size": `${scale().md}px`,
                          overflow: "hidden",
                          "text-overflow": "ellipsis",
                          "white-space": "nowrap",
                        }}
                      >
                        {remote.name}
                      </strong>
                      <span
                        title={remote.uri}
                        style={{
                          color: theme().dimFg,
                          "font-size": `${scale().xs}px`,
                          "font-family": "monospace, inherit",
                          overflow: "hidden",
                          "text-overflow": "ellipsis",
                          "white-space": "nowrap",
                        }}
                      >
                        {remote.uri}
                        <Show when={statusText()}>
                          {(text) => <> · {text()}</>}
                        </Show>
                        <Show when={status() === "connected" && rtt() !== null}>
                          {` · ${Math.round(rtt()!)} ms`}
                        </Show>
                      </span>
                    </div>

                    <Show when={anyManageable()} fallback={filler()}>
                      <Show
                        when={canManage(remote.name)}
                        fallback={
                          <div
                            title={t("remotes.controlDisconnected")}
                            style={{ ...actionStyle(), opacity: 0.25 }}
                          >
                            {t("remotes.control")}
                          </div>
                        }
                      >
                        <button
                          type="button"
                          title={t("remotes.openControl")}
                          onClick={() => props.onManage?.(remote.name)}
                          style={actionStyle()}
                        >
                          {t("remotes.control")}
                        </button>
                      </Show>
                    </Show>

                    {/* Only what the home server actually stores can be
                        edited: `local` is this process, and a route materialized
                        by a session is not a catalogue entry. */}
                    <Show when={entry()} fallback={filler()}>
                      {(stored) => (
                        <button
                          type="button"
                          onClick={() =>
                            void props.onToggleRemote?.(remote.name)
                          }
                          style={actionStyle()}
                        >
                          {stored().disabled
                            ? t("remotes.enable")
                            : t("remotes.disable")}
                        </button>
                      )}
                    </Show>
                    <Show when={entry()} fallback={filler()}>
                      <button
                        type="button"
                        onClick={() => void props.onRemoveRemote?.(remote.name)}
                        style={actionStyle()}
                      >
                        {t("remotes.remove")}
                      </button>
                    </Show>

                    {/* The word, not a circular-arrow glyph: the terminal font
                        draws those as a hairline scratch at this size. */}
                    <button
                      type="button"
                      title={t("disconnected.reconnectNow")}
                      disabled={!props.onReconnect}
                      onClick={() => props.onReconnect?.(remote.name)}
                      style={{
                        ...actionStyle(),
                        opacity: props.onReconnect ? 0.7 : 0.25,
                        cursor: props.onReconnect ? "pointer" : "default",
                      }}
                    >
                      {t("disconnected.reconnectNow")}
                    </button>
                  </div>
                );
              }}
            </For>
          </div>
        </Show>

        <Show when={missingSessionRemotes().length > 0}>
          <div
            role="list"
            aria-label={t("remotes.unavailableSessionRemotes")}
            style={{
              display: "grid",
              border: `1px solid ${theme().border}`,
            }}
          >
            <For each={missingSessionRemotes()}>
              {(name, index) => (
                <div
                  role="listitem"
                  style={{
                    display: "grid",
                    "grid-template-columns": "minmax(0, 1fr) auto",
                    "align-items": "center",
                    "border-top":
                      index() === 0
                        ? undefined
                        : `1px solid ${theme().subtleBorder}`,
                    "background-color": theme().solidPanelBg,
                    opacity: 0.7,
                  }}
                >
                  <div
                    style={{
                      padding: `${scale().controlY}px ${scale().controlX}px`,
                      "font-size": `${scale().md}px`,
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                      "white-space": "nowrap",
                    }}
                  >
                    <strong>{name}</strong>{" "}
                    <span
                      style={{
                        color: theme().dimFg,
                        "font-size": `${scale().xs}px`,
                      }}
                    >
                      {t("sessions.unavailable")}
                    </span>
                  </div>
                  <button
                    type="button"
                    title={t("remotes.removeFromSession")}
                    aria-label={t("remotes.removeFromSession")}
                    onClick={() =>
                      void setWorkspaceSessionRemoteMembership(
                        props.onSetSessionActive,
                        name,
                        false,
                      )
                    }
                    style={{
                      ...actionStyle(),
                      "font-size": `${scale().md}px`,
                      "line-height": 1,
                    }}
                  >
                    ✕
                  </button>
                </div>
              )}
            </For>
          </div>
        </Show>

        <Show when={canEdit()}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
            style={{
              display: "flex",
              gap: `${scale().tightGap}px`,
              "align-items": "stretch",
              "flex-wrap": "wrap",
              "border-top": `1px solid ${theme().subtleBorder}`,
              "padding-top": `${scale().gap}px`,
            }}
          >
            <input
              value={newName()}
              onInput={(event) => setNewName(event.currentTarget.value)}
              placeholder={t("remotes.namePlaceholder")}
              aria-label={t("remotes.namePlaceholder")}
              autocomplete="off"
              autocorrect="off"
              autocapitalize="off"
              spellcheck={false}
              style={{ ...inputStyle(), flex: "0 0 8em", "font-weight": 600 }}
            />
            <input
              value={newUri()}
              onInput={(event) => setNewUri(event.currentTarget.value)}
              placeholder={t("remotes.uriPlaceholder")}
              aria-label={t("remotes.uriPlaceholder")}
              autocomplete="off"
              autocorrect="off"
              autocapitalize="off"
              spellcheck={false}
              style={{
                ...inputStyle(),
                flex: "1 1 12em",
                "font-family": "monospace, inherit",
              }}
            />
            <button
              type="submit"
              style={{
                ...buttonStyle(),
                border: `1px solid ${theme().border}`,
                "background-color": theme().inputBg,
                opacity: 1,
                "flex-shrink": 0,
              }}
            >
              {t("remotes.add")}
            </button>
          </form>
          <Show when={editError()}>
            <div
              role="alert"
              style={{
                color: theme().errorText,
                "font-size": `${scale().sm}px`,
              }}
            >
              {editError()}
            </div>
          </Show>
          <div style={{ color: theme().dimFg, "font-size": `${scale().xs}px` }}>
            {t("remotes.storedWarning")}
          </div>
        </Show>
      </OverlayPanel>
    </OverlayBackdrop>
  );
}
