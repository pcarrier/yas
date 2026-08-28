/**
 * What this server is running, and what it could run.
 *
 * One list over one identity: an extension is its BLAKE3 digest, so a row that
 * is both installed and offered shows the digest the definition is pinned to
 * next to the one the registry offers, and "outdated" is that comparison. There
 * is no version to trust. Installed and offered used to be two tables, which
 * named the same extension twice and made an update look like a fresh install.
 */

import {
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import type {
  YasExtensionRecord,
  YasWorkspace,
  ConnectionId,
  TerminalPalette,
} from "@yas-run/core";
import {
  YAS_EXTENSION_CONTROL_DISABLE,
  YAS_EXTENSION_CONTROL_ENABLE,
  YAS_EXTENSION_CONTROL_RESTART,
  YAS_EXTENSION_CONTROL_START,
  YAS_EXTENSION_CONTROL_STOP,
  YAS_EXTENSION_DEFINITION_ENABLED,
  YAS_EXTENSION_DEFINITION_PERSISTENT,
  YAS_EXTENSION_PHASE_BACKOFF,
  YAS_EXTENSION_PHASE_BLOCKED,
  YAS_EXTENSION_PHASE_NEED_OBJECT,
  YAS_EXTENSION_PHASE_QUEUED,
  YAS_EXTENSION_PHASE_RUNNING,
  YAS_EXTENSION_PHASE_STOPPED,
  YAS_EXTENSION_PHASE_STOPPING,
  YAS_EXTENSION_PHASE_VALIDATING,
  yasExtensionHashHex,
} from "@yas-run/core";
import {
  defaultRegistry,
  disableAndRemoveExtension,
  fetchRegistry,
  formatNativeExtensionHandle,
  installFromRegistry,
  isOutdated,
  mergeExtensionInventory,
  mergeExtensions,
  nativeExtensionHost,
  upsertExtensionRecord,
  type ExtensionRow,
  type Registry,
} from "./extensionRegistry";
import { mergeStyle, scrollbarStyle, themeFor, ui, uiScale } from "./theme";
import { t, tp } from "./i18n";

export function ExtensionsPanel(props: {
  workspace: YasWorkspace;
  connectionId: ConnectionId;
  palette: TerminalPalette;
  fontSize: number;
}) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);

  // `null` means that the server inventory is not authoritative yet. Treating
  // it as an empty list made every registry entry look installable during a
  // slow or failed list request.
  const [installed, setInstalled] = createSignal<
    readonly YasExtensionRecord[] | null
  >(null);
  const [registry, setRegistry] = createSignal<Registry | null>(null);
  const [registryUrl, setRegistryUrl] = createSignal(defaultRegistry());
  const [inventoryError, setInventoryError] = createSignal<string | null>(null);
  const [registryError, setRegistryError] = createSignal<string | null>(null);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<string | null>(null);
  const [inventoryLoading, setInventoryLoading] = createSignal(false);
  const [registryLoading, setRegistryLoading] = createSignal(false);
  const [actionBusy, setActionBusy] = createSignal<string | null>(null);

  let inventoryRequest = 0;
  let registryRequest = 0;

  const host = () =>
    nativeExtensionHost(props.workspace.getConnection(props.connectionId));

  const refresh = async () => {
    const connection = host();
    const request = ++inventoryRequest;
    setInventoryLoading(true);
    if (!connection) {
      setInstalled(null);
      setInventoryError("Connection is unavailable");
      setInventoryLoading(false);
      return;
    }
    try {
      const records = await connection.listExtensions();
      if (request !== inventoryRequest) return;
      setInstalled((previous) => mergeExtensionInventory(previous, records));
      setInventoryError(null);
    } catch (failure) {
      if (request !== inventoryRequest) return;
      setInstalled(null);
      setInventoryError(
        failure instanceof Error ? failure.message : String(failure),
      );
    } finally {
      if (request === inventoryRequest) setInventoryLoading(false);
    }
  };

  const loadRegistry = async () => {
    const request = ++registryRequest;
    setRegistryLoading(true);
    try {
      const loaded = await fetchRegistry(registryUrl());
      if (request !== registryRequest) return;
      setRegistry(loaded);
      setRegistryError(null);
    } catch (failure) {
      if (request !== registryRequest) return;
      setRegistry(null);
      setRegistryError(
        failure instanceof Error ? failure.message : String(failure),
      );
    } finally {
      if (request === registryRequest) setRegistryLoading(false);
    }
  };

  onMount(() => {
    void refresh();
    void loadRegistry();
  });
  onCleanup(() => {
    inventoryRequest++;
    registryRequest++;
  });

  const rows = createMemo<ExtensionRow[]>(() => {
    const inventory = installed();
    return inventory === null
      ? []
      : mergeExtensions(inventory, registry()?.extensions ?? []);
  });
  const errors = createMemo(() =>
    [actionError(), inventoryError(), registryError()].filter(
      (error, index, all): error is string =>
        error !== null && all.indexOf(error) === index,
    ),
  );
  const controlsBusy = () => actionBusy() !== null || inventoryLoading();
  const installsBusy = () => controlsBusy() || registryLoading();

  const act = async (label: string, action: () => Promise<unknown>) => {
    setActionBusy(label);
    setActionError(null);
    setNote(null);
    try {
      await action();
    } catch (failure) {
      setActionError(
        failure instanceof Error ? failure.message : String(failure),
      );
    } finally {
      await refresh();
      setActionBusy(null);
    }
  };

  /**
   * Install, or replace the definition of the same name in place.
   *
   * The install helper re-lists at click time, so a render-time snapshot can
   * never turn an existing durable name into a duplicate create.
   */
  const install = (row: ExtensionRow) => {
    const connection = host();
    const source = registry();
    if (!connection || !source || !row.offered) return;
    void act(row.label, async () => {
      const updated = await installFromRegistry(
        connection,
        source,
        row.offered!,
      );
      setInstalled((records) => upsertExtensionRecord(records, updated));
      setNote(tp("extensions.installed", { name: row.label }));
    });
  };

  /** Removal is a two-step verb: a definition must be quiescent first. */
  const remove = (record: YasExtensionRecord) => {
    const connection = host();
    if (!connection) return;
    void act(record.name, async () => {
      await disableAndRemoveExtension(connection, record);
      setNote(tp("extensions.removed", { name: record.name }));
    });
  };

  const control = (
    record: YasExtensionRecord,
    action: number,
    noteKey: string,
  ) => {
    const connection = host();
    if (!connection) return;
    void act(record.name, async () => {
      await connection.controlExtension(record.extensionHandle, action);
      setNote(tp(noteKey, { name: record.name }));
    });
  };

  const short = (digest: string) => digest.slice(0, 12);
  const isStopped = (record: YasExtensionRecord) =>
    record.phase === YAS_EXTENSION_PHASE_STOPPED ||
    record.phase === YAS_EXTENSION_PHASE_BLOCKED;
  const isPersistent = (record: YasExtensionRecord) =>
    (record.flags & YAS_EXTENSION_DEFINITION_PERSISTENT) !== 0;
  const isEnabled = (record: YasExtensionRecord) =>
    (record.flags & YAS_EXTENSION_DEFINITION_ENABLED) !== 0;
  const phaseName = (phase: number): string =>
    ({
      [YAS_EXTENSION_PHASE_NEED_OBJECT]: "need-object",
      [YAS_EXTENSION_PHASE_VALIDATING]: "validating",
      [YAS_EXTENSION_PHASE_QUEUED]: "queued",
      [YAS_EXTENSION_PHASE_RUNNING]: "running",
      [YAS_EXTENSION_PHASE_BACKOFF]: "backoff",
      [YAS_EXTENSION_PHASE_STOPPED]: "stopped",
      [YAS_EXTENSION_PHASE_BLOCKED]: "blocked",
      [YAS_EXTENSION_PHASE_STOPPING]: "stopping",
    })[phase] ?? String(phase);

  return (
    <>
      <For each={errors()}>
        {(error) => (
          <div
            style={{
              color: theme().error,
              "font-size": `${scale().sm}px`,
              "margin-bottom": `${scale().xs}px`,
            }}
          >
            {error}
          </div>
        )}
      </For>
      <Show when={note()}>
        <div
          style={{
            color: theme().dimFg,
            "font-size": `${scale().sm}px`,
            "margin-bottom": `${scale().xs}px`,
          }}
        >
          {note()}
        </div>
      </Show>

      <div
        style={{
          display: "flex",
          gap: `${scale().xs}px`,
          "align-items": "center",
          "margin-bottom": `${scale().xs}px`,
        }}
      >
        <span style={{ color: theme().dimFg, "font-size": `${scale().sm}px` }}>
          {t("extensions.registryTitle")}
        </span>
        <input
          data-registry-url
          value={registryUrl()}
          onInput={(event) => setRegistryUrl(event.currentTarget.value)}
          onChange={() => void loadRegistry()}
          style={mergeStyle(ui.input, {
            flex: "1 1 auto",
            "font-size": `${scale().sm}px`,
          })}
        />
        <button
          type="button"
          disabled={
            actionBusy() !== null || inventoryLoading() || registryLoading()
          }
          style={mergeStyle(ui.btn, { "font-size": `${scale().sm}px` })}
          onClick={() => {
            void refresh();
            void loadRegistry();
          }}
        >
          {t("extensions.reload")}
        </button>
      </div>

      <div
        style={mergeStyle(scrollbarStyle(theme()), {
          "overflow-y": "auto",
          // Bounded by the pane rather than the viewport — see SystemdPanel's
          // table for why a `vh` cap inside a pane scrolls twice.
          flex: "1 1 0",
          "min-height": "6em",
          "font-size": `${scale().sm}px`,
        })}
      >
        <For
          each={rows()}
          fallback={
            <div style={{ color: theme().dimFg }}>
              {inventoryLoading() || registryLoading()
                ? t("extensions.loading")
                : inventoryError() || registryError()
                  ? ""
                  : t("extensions.none")}
            </div>
          }
        >
          {(row) => (
            <div
              data-extension={row.label}
              style={{
                display: "flex",
                "flex-direction": "column",
                gap: `${scale().xs}px`,
                padding: `${scale().xs}px 0`,
                "border-bottom": `1px solid ${theme().border}`,
              }}
            >
              <div
                style={{
                  display: "grid",
                  // The info line keeps fixed columns so phase/digest/flags line
                  // up across rows. Actions live on their own line below.
                  "grid-template-columns": "minmax(0, 1fr) 6em 13em 7em",
                  gap: `${scale().sm}px`,
                  "align-items": "center",
                }}
              >
                <span style={{ "min-width": 0 }}>
                  <span
                    title={
                      row.installed
                        ? `id:${formatNativeExtensionHandle(row.installed.extensionHandle)}`
                        : undefined
                    }
                  >
                    {row.label}
                  </span>
                  <Show when={row.description}>
                    <div
                      style={{
                        color: theme().dimFg,
                        "font-size": `${scale().xs}px`,
                      }}
                    >
                      {row.description}
                    </div>
                  </Show>
                </span>

                <span
                  style={{
                    color: !row.installed
                      ? theme().dimFg
                      : row.installed.phase === YAS_EXTENSION_PHASE_RUNNING
                        ? theme().success
                        : theme().dimFg,
                  }}
                >
                  {row.installed
                    ? phaseName(row.installed.phase)
                    : t("extensions.available")}
                </span>

                {/* The digest is the identity, so an update is shown as one. */}
                <span
                  style={{
                    color: isOutdated(row) ? theme().warning : theme().dimFg,
                  }}
                  title={
                    row.installed && row.offered
                      ? `${yasExtensionHashHex(row.installed.contentHash)}\n${row.offered.blake3}`
                      : row.installed
                        ? yasExtensionHashHex(row.installed.contentHash)
                        : row.offered?.blake3
                  }
                >
                  {short(
                    row.installed
                      ? yasExtensionHashHex(row.installed.contentHash)
                      : (row.offered?.blake3 ?? ""),
                  )}
                  <Show when={isOutdated(row)}>
                    {" → "}
                    {short(row.offered!.blake3)}
                  </Show>
                </span>

                <span style={{ color: theme().dimFg }}>
                  <Show
                    when={row.installed}
                    fallback={
                      row.offered?.brotliBytes
                        ? `${Math.round(row.offered.brotliBytes / 1024)} KiB`
                        : ""
                    }
                  >
                    {row.installed!.flags & YAS_EXTENSION_DEFINITION_PERSISTENT
                      ? t("extensions.persistent")
                      : t("extensions.transient")}
                    {row.installed!.flags & YAS_EXTENSION_DEFINITION_ENABLED
                      ? ""
                      : ` ${t("extensions.disabled")}`}
                  </Show>
                </span>
              </div>

              <div
                style={{
                  display: "flex",
                  gap: `${scale().xs}px`,
                  "align-items": "center",
                  "justify-content": "flex-end",
                  "flex-wrap": "wrap",
                }}
              >
                <Show when={row.offered && !row.installed}>
                  <button
                    type="button"
                    disabled={installsBusy()}
                    style={mergeStyle(ui.btn, {
                      "font-size": `${scale().sm}px`,
                    })}
                    onClick={() => install(row)}
                  >
                    {t("extensions.install")}
                  </button>
                </Show>
                <Show when={isOutdated(row)}>
                  <button
                    type="button"
                    data-extension-update
                    disabled={installsBusy()}
                    style={mergeStyle(ui.btn, {
                      "font-size": `${scale().sm}px`,
                    })}
                    onClick={() => install(row)}
                  >
                    {t("extensions.update")}
                  </button>
                </Show>
                <Show when={row.installed && row.offered && !isOutdated(row)}>
                  <span style={{ color: theme().dimFg }}>
                    {t("extensions.current")}
                  </span>
                </Show>
                <Show when={row.installed}>
                  <Show when={isEnabled(row.installed!)}>
                    <button
                      type="button"
                      disabled={controlsBusy()}
                      style={mergeStyle(ui.btn, {
                        "font-size": `${scale().sm}px`,
                      })}
                      onClick={() =>
                        control(
                          row.installed!,
                          isStopped(row.installed!)
                            ? YAS_EXTENSION_CONTROL_START
                            : YAS_EXTENSION_CONTROL_RESTART,
                          isStopped(row.installed!)
                            ? "extensions.started"
                            : "extensions.restarted",
                        )
                      }
                    >
                      {isStopped(row.installed!)
                        ? t("extensions.start")
                        : t("extensions.restart")}
                    </button>
                  </Show>
                  <Show when={!isStopped(row.installed!)}>
                    <button
                      type="button"
                      disabled={controlsBusy()}
                      style={mergeStyle(ui.btn, {
                        "font-size": `${scale().sm}px`,
                      })}
                      onClick={() =>
                        control(
                          row.installed!,
                          YAS_EXTENSION_CONTROL_STOP,
                          "extensions.stopped",
                        )
                      }
                    >
                      {t("extensions.stop")}
                    </button>
                  </Show>
                  <Show when={isPersistent(row.installed!)}>
                    <Show
                      when={isEnabled(row.installed!)}
                      fallback={
                        <button
                          type="button"
                          disabled={controlsBusy()}
                          style={mergeStyle(ui.btn, {
                            "font-size": `${scale().sm}px`,
                          })}
                          onClick={() =>
                            control(
                              row.installed!,
                              YAS_EXTENSION_CONTROL_ENABLE,
                              "extensions.enabledNote",
                            )
                          }
                        >
                          {t("extensions.enable")}
                        </button>
                      }
                    >
                      <button
                        type="button"
                        disabled={controlsBusy()}
                        style={mergeStyle(ui.btn, {
                          "font-size": `${scale().sm}px`,
                        })}
                        onClick={() =>
                          control(
                            row.installed!,
                            YAS_EXTENSION_CONTROL_DISABLE,
                            "extensions.disabledNote",
                          )
                        }
                      >
                        {t("extensions.disable")}
                      </button>
                    </Show>
                    <button
                      type="button"
                      disabled={controlsBusy()}
                      style={mergeStyle(ui.btn, {
                        "font-size": `${scale().sm}px`,
                      })}
                      onClick={() => remove(row.installed!)}
                    >
                      {t("extensions.remove")}
                    </button>
                  </Show>
                </Show>
              </div>
            </div>
          )}
        </For>
      </div>
    </>
  );
}
