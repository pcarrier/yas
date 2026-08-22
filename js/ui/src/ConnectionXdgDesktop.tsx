/**
 * ConnectionXdgDesktop — the applications ONE connection starts and keeps running:
 * what is enabled, what is actually up, how many windows each has, and controls
 * to run one.
 *
 * Two pairs of verbs, because they answer different questions. Enable/Disable
 * is intent: what this session should be running the next time it starts.
 * Start/Stop is now: try an application without adopting it, or stop one
 * without forgetting it. Collapsing them into one button — which is what this
 * had — makes "stop this for a minute" indistinguishable from "I never want
 * this again".
 *
 * State comes from the `yas.xdg-desktop.v1` native channel served by the XDG
 * desktop supervisor extension (`extensions/xdg-desktop`), not from a built-in
 * YAS family.
 * A connection whose server runs no supervisor simply shows nothing — the
 * channel connect fails and the section stays out of the way, which is why this
 * renders nothing at all rather than an error when it cannot attach.
 *
 * The channel itself is not this panel's. It belongs to {@link
 * ./xdgDesktopCatalogs.ts}, which holds one per connected server for the life of
 * the page so the switcher can search applications without waiting for a
 * catalog. This panel used to open its own and close it on the way out; two
 * mirrors of one supervisor also meant two icon caches filling with the same
 * artwork.
 */

import { TapButton } from "./TapButton";
import { createEffect, createSignal, For, Show } from "solid-js";
import type {
  YasWorkspace,
  ConnectionId,
  TerminalPalette,
} from "@yas-run/core";
import { scrollbarStyle, themeFor, ui, uiScale } from "./theme";
import {
  AppIcon,
  PanelEmpty,
  PanelRow,
  panelButton,
  SectionHeading,
  StatusPill,
  type PanelTone,
} from "./panelKit";
import type { XdgDesktopApp } from "./xdgDesktop";
import {
  applicationIcon,
  requestApplicationIcons,
  xdgDesktopHandle,
} from "./xdgDesktopCatalogs";
import { t, tp } from "./i18n";

/** Phase → the tone and word the row shows. Backoff is a warning rather than
 *  an error: it is a supervisor working, not a supervisor stuck. */
function phaseTone(app: XdgDesktopApp): { tone: PanelTone; label: string } {
  if (!app.enabled) return { tone: "idle", label: t("xdgDesktop.disabled") };
  switch (app.phase) {
    case "running":
      return { tone: "ok", label: t("xdgDesktop.running") };
    case "backoff":
      return { tone: "warn", label: t("xdgDesktop.restarting") };
    case "starting":
      return { tone: "warn", label: t("xdgDesktop.starting") };
    case "stopped":
      return { tone: "idle", label: t("xdgDesktop.stopped") };
  }
}

export function ConnectionXdgDesktop(props: {
  workspace: YasWorkspace;
  connectionId: ConnectionId;
  palette: TerminalPalette;
  fontSize: number;
}) {
  const theme = () => themeFor(props.palette);
  const scale = () => uiScale(props.fontSize);
  const [filter, setFilter] = createSignal("");

  // The channel belongs to the catalog store, which holds one per connected
  // server for the whole page. This panel used to open its own and close it on
  // the way out; sharing is not only one channel instead of two, but one icon
  // cache instead of two holding the same artwork.
  //
  // Null while the server has no supervisor attached — the connect was refused,
  // or has not answered yet — and the panel then renders nothing, because "this
  // server does not run one" is not a fault to report.
  const handle = () => xdgDesktopHandle(props.connectionId);

  const apps = () => handle()?.apps ?? [];
  const catalog = () => handle()?.catalog ?? [];
  /** Whether the supervisor's first message has landed.
   *
   *  The channel opens before the greeting arrives, and the greeting is what
   *  carries both lists — so between the two, everything here is empty. Saying
   *  "nothing is managed yet" in that window is a lie, and a convincing one:
   *  the greeting waits behind a catalog read, which on a busy supervisor is
   *  long enough to read and believe. */
  const ready = () => handle()?.ready ?? false;
  /** Installed applications that are not already managed, matched against the
   *  filter box. A managed app is offered by its own row, not this list. */
  const addable = () => {
    const managed = new Set(apps().map((app) => app.id));
    const needle = filter().trim().toLowerCase();
    return catalog()
      .filter((entry) => !managed.has(entry.id))
      .filter(
        (entry) =>
          needle.length === 0 ||
          entry.name.toLowerCase().includes(needle) ||
          entry.id.toLowerCase().includes(needle),
      );
  };
  /** Artwork for one row. Goes through the store rather than the handle:
   *  the handle's getters are plain properties, so a reply landing in the
   *  mirror would change nothing a row is watching. The store's accessor is
   *  reactive, and an icon always arrives long after its row was drawn. */
  const iconOf = (id: string) => applicationIcon(props.connectionId, id);

  // Keep the entire installed shelf warm. The page-lifetime catalogue does
  // this before Manage opens; this effect also covers a standalone panel and
  // catalogue updates.
  createEffect(() => {
    if (!handle()) return;
    requestApplicationIcons(
      props.connectionId,
      catalog().map((entry) => entry.id),
    );
  });

  return (
    <Show when={handle()}>
      {(desktop) => (
        <div
          style={{
            display: "flex",
            "flex-direction": "column",
            "background-color": theme().panelBg,
            // Fills the pane region rather than growing past it, so the
            // catalog below can be bounded by this box instead of by the
            // viewport.
            flex: "1 1 auto",
            "min-height": "0",
          }}
        >
          <SectionHeading
            theme={theme()}
            scale={scale()}
            label={t("xdgDesktop.applications")}
            count={apps().length}
          />

          <Show
            when={apps().length > 0}
            fallback={
              <PanelEmpty theme={theme()} scale={scale()}>
                <Show
                  when={ready()}
                  fallback={t("xdgDesktop.askingSupervisor")}
                >
                  {t("xdgDesktop.empty")}
                </Show>
              </PanelEmpty>
            }
          >
            {/* Natural height while the managed set is small, which it almost
                always is — `flex: 0 1 auto` only starts scrolling once the
                rows would otherwise push the catalog's search box out of the
                pane. Its own list, its own scroller: what must never happen is
                two scrollbars for the *same* list. */}
            <div
              style={{
                flex: "0 1 auto",
                "min-height": "0",
                "overflow-y": "auto",
                ...scrollbarStyle(theme()),
              }}
            >
              <For each={apps()}>
                {(app) => (
                  <PanelRow theme={theme()} scale={scale()}>
                    <div
                      style={{
                        display: "flex",
                        "align-items": "center",
                        "justify-content": "space-between",
                        gap: `${scale().gap}px`,
                      }}
                    >
                      <span
                        style={{
                          display: "flex",
                          // The icon is the tallest thing in the row, so the text
                          // centres against it rather than sitting on a baseline
                          // the tile does not share.
                          "align-items": "center",
                          gap: `${scale().gap}px`,
                          "min-width": "0",
                        }}
                      >
                        <AppIcon
                          theme={theme()}
                          scale={scale()}
                          name={app.name}
                          src={iconOf(app.id)}
                        />
                        <span
                          style={{
                            display: "flex",
                            "align-items": "baseline",
                            gap: `${scale().tightGap}px`,
                            "min-width": "0",
                          }}
                        >
                          <strong
                            style={{
                              overflow: "hidden",
                              "text-overflow": "ellipsis",
                              "white-space": "nowrap",
                            }}
                          >
                            {app.name}
                          </strong>
                          <Show when={app.name !== app.id}>
                            <span
                              style={{
                                color: theme().dimFg,
                                "font-size": `${scale().sm}px`,
                              }}
                            >
                              {app.id}
                            </span>
                          </Show>
                        </span>
                      </span>

                      <span
                        style={{
                          display: "flex",
                          "align-items": "center",
                          gap: `${scale().gap}px`,
                          "flex-shrink": "0",
                        }}
                      >
                        <StatusPill
                          theme={theme()}
                          scale={scale()}
                          {...phaseTone(app)}
                          title={
                            app.socket
                              ? tp("xdgDesktop.waylandSocket", {
                                  socket: app.socket,
                                })
                              : undefined
                          }
                        />
                        {/* Counted from the identity the compositor stamped on
                          the app's own socket, not from a self-asserted
                          app_id — which is what makes it worth showing. */}
                        <span
                          title={t("xdgDesktop.windowsHelp")}
                          style={{
                            color: theme().dimFg,
                            "font-size": `${scale().sm}px`,
                            "font-variant-numeric": "tabular-nums",
                          }}
                        >
                          {tp(
                            app.windows === 1
                              ? "xdgDesktop.windowOne"
                              : "xdgDesktop.windowMany",
                            { count: app.windows },
                          )}
                        </span>
                        {/* Now. Running covers backoff too: a supervisor about
                          to retry is something a viewer wants to be able to
                          call off. */}
                        <TapButton
                          type="button"
                          title={
                            app.phase === "stopped"
                              ? t("xdgDesktop.startNowHelp")
                              : t("xdgDesktop.stopNowHelp")
                          }
                          style={panelButton(theme(), scale())}
                          onClick={() =>
                            app.phase === "stopped"
                              ? desktop().start(app.id)
                              : desktop().stop(app.id)
                          }
                        >
                          {app.phase === "stopped"
                            ? t("common.start")
                            : t("common.stop")}
                        </TapButton>
                        {/* Intent, and the way out of the list. Disabling keeps
                          the row -- an application that just failed is worth
                          looking at -- so there has to be something that
                          removes it, or a one-off experiment stays forever. */}
                        <TapButton
                          type="button"
                          title={
                            app.enabled
                              ? t("xdgDesktop.disableHelp")
                              : t("xdgDesktop.enableHelp")
                          }
                          style={panelButton(
                            theme(),
                            scale(),
                            app.enabled ? "bad" : undefined,
                          )}
                          onClick={() =>
                            app.enabled
                              ? desktop().disable(app.id)
                              : desktop().enable(app.id)
                          }
                        >
                          {app.enabled
                            ? t("common.disable")
                            : t("common.enable")}
                        </TapButton>
                        <TapButton
                          type="button"
                          title={t("xdgDesktop.discardHelp")}
                          style={panelButton(theme(), scale(), "bad")}
                          onClick={() => desktop().forget(app.id)}
                        >
                          {t("common.discard")}
                        </TapButton>
                      </span>
                    </div>

                    {/* Only worth a line when something went wrong: a healthy row
                      stays one line tall. */}
                    <Show when={app.failures > 0 || app.lastExit !== undefined}>
                      <div
                        style={{
                          color: theme().dimFg,
                          "font-size": `${scale().sm}px`,
                          "font-variant-numeric": "tabular-nums",
                        }}
                      >
                        <Show when={app.failures > 0}>
                          {tp(
                            app.failures === 1
                              ? "xdgDesktop.failedStartOne"
                              : "xdgDesktop.failedStartMany",
                            { count: app.failures },
                          )}
                        </Show>
                        <Show
                          when={app.failures > 0 && app.lastExit !== undefined}
                        >
                          {" · "}
                        </Show>
                        <Show when={app.lastExit !== undefined}>
                          {tp("xdgDesktop.lastExit", { code: app.lastExit! })}
                        </Show>
                      </div>
                    </Show>
                  </PanelRow>
                )}
              </For>
            </div>
          </Show>

          {/* Adding. The whole catalog, scrolling, with the filter narrowing it
              rather than summoning it: this list used to be hidden behind
              typing, which asked a viewer to name what they wanted before
              being shown that it existed. A launcher shows its shelf. */}
          <SectionHeading
            theme={theme()}
            scale={scale()}
            label={t("xdgDesktop.addApplication")}
          >
            <input
              type="text"
              value={filter()}
              onInput={(event) => setFilter(event.currentTarget.value)}
              placeholder={t("xdgDesktop.searchInstalled")}
              aria-label={t("xdgDesktop.searchInstalledLabel")}
              autocomplete="off"
              spellcheck={false}
              style={{
                ...ui.input,
                "background-color": theme().inputBg,
                color: "inherit",
                border: `1px solid ${theme().border}`,
                "font-size": `${scale().sm}px`,
                padding: `${scale().controlY}px ${scale().controlX}px`,
                "min-width": "0",
                flex: "1 1 12em",
              }}
            />
          </SectionHeading>

          <Show
            when={addable().length > 0}
            fallback={
              <PanelEmpty theme={theme()} scale={scale()}>
                {!ready()
                  ? t("xdgDesktop.askingSupervisor")
                  : catalog().length === 0
                    ? t("xdgDesktop.noInstalled")
                    : tp("xdgDesktop.noInstalledMatches", {
                        filter: filter().trim(),
                      })}
              </PanelEmpty>
            }
          >
            {/* The catalog is the only unbounded thing here, so it is the one
                thing that scrolls: letting it lengthen the panel instead would
                scroll the search box — the one control for a nine-hundred-row
                list — off the top.

                Bounded by the pane, not the viewport. It was `42vh`, which in
                a dialog capped at 80% of the screen was about right, and in a
                pane is a number unrelated to the box it is in: in a short pane
                it overflows and the pane scrolls too (two scrollbars for one
                list), in a tall one it stops short of the bottom. `flex: 1`
                against a parent with `min-height: 0` is the same intent
                measured against the right thing. */}
            <div
              style={{
                flex: "1 1 0",
                // Something to scroll even when the managed list above is
                // long: a scroller collapsed to nothing cannot be scrolled
                // back out of.
                "min-height": "6em",
                "overflow-y": "auto",
                "min-width": "0",
                ...scrollbarStyle(theme()),
              }}
            >
              <For each={addable()}>
                {(entry) => (
                  <PanelRow theme={theme()} scale={scale()}>
                    <div
                      style={{
                        display: "flex",
                        "align-items": "center",
                        "justify-content": "space-between",
                        gap: `${scale().gap}px`,
                      }}
                    >
                      <span
                        style={{
                          display: "flex",
                          "align-items": "center",
                          gap: `${scale().gap}px`,
                          "min-width": "0",
                        }}
                      >
                        <AppIcon
                          theme={theme()}
                          scale={scale()}
                          name={entry.name}
                          src={iconOf(entry.id)}
                        />
                        <span
                          style={{
                            "min-width": "0",
                            overflow: "hidden",
                            "text-overflow": "ellipsis",
                            "white-space": "nowrap",
                          }}
                        >
                          {entry.name}
                          <span
                            style={{
                              color: theme().dimFg,
                              "font-size": `${scale().sm}px`,
                            }}
                          >
                            {` ${entry.id}`}
                          </span>
                        </span>
                      </span>
                      <TapButton
                        type="button"
                        style={panelButton(theme(), scale())}
                        onClick={() => {
                          desktop().enable(entry.id);
                          setFilter("");
                        }}
                      >
                        {t("common.enable")}
                      </TapButton>
                    </div>
                  </PanelRow>
                )}
              </For>
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
}
