import { For } from "solid-js";
import type { TerminalPalette } from "@yas-run/core";
import { themeFor, ui, uiScale } from "./theme";
import { OverlayBackdrop, OverlayHeader, OverlayPanel } from "./Overlay";
import { t } from "./i18n";

type Shortcut = [string, string];
type Section = { title: string; items: Shortcut[] };

export function HelpOverlay(props: {
  onClose: () => void;
  palette: TerminalPalette;
  fontSize: number;
}) {
  const theme = themeFor(props.palette);
  const scale = uiScale(props.fontSize);
  const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
  const mod = isMac ? "Cmd" : "Ctrl";
  // CodeMirror binds different chords per platform for these two.
  const fold = isMac ? "Cmd+Alt+[ / ]" : "Ctrl+Shift+[ / ]";
  const undoRedo = isMac
    ? "Cmd+Z / Cmd+Shift+Z"
    : "Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z";
  // Sections are hand-dealt between the two columns to keep their
  // heights close; re-deal when a section grows.
  const left: Section[] = [
    {
      title: t("help.prefix"),
      items: [
        ["k", t("help.menu")],
        ["[ / ]", t("help.prevNextWorkspace")],
        ["n / a / d", t("help.manageWorkspaceTabs")],
        ["Arrow", t("help.focusStack")],
        ["Shift+Arrow", t("help.moveView")],
        ["Alt+Arrow", t("help.resizeStack")],
        ["h / v", `${t("help.splitHorizontal")} / ${t("help.splitVertical")}`],
        ["b / t / s / l", t("help.cycleContainerLayout")],
        ["Space", t("help.focusModeToggle")],
        ["Shift+Space", t("help.toggleFloating")],
        ["Enter / Shift+Enter", t("help.openTabOrBeside")],
        ["Tab / Shift+Tab", t("help.prevNextWindow")],
        ["z", t("help.soloPane")],
        ["=", t("help.balanceWorkspace")],
        ["q / x", t("help.removeOrClose")],
        ["w", t("help.viewOverview")],
        ["/", t("help.commandSearch")],
        ["e f y l p", t("help.docks")],
        ["r", t("help.panels")],
        ["> / @ / #", t("help.searchModes")],
        ["?", t("help.title")],
        ["Ctrl+B", t("help.sendPrefix")],
      ],
    },
    {
      // What a prefix cannot express: these belong to whatever already has
      // the keyboard.
      title: t("help.keyboard"),
      items: [
        ["Escape", t("help.closeOverlay")],
        ["Enter", t("help.restartExited")],
        ["Ctrl+Shift+V", t("help.paste")],
        [t("help.floatingPointerKeys"), t("help.floatingPointer")],
      ],
    },
    {
      // The Cmd+K field is a mode switcher, not just a filter — the
      // prefixes are invisible unless something says so.
      title: t("help.searchModes"),
      items: [
        ["name", t("help.modePlain")],
        [">command", t("help.modeCommand")],
        ["target>command", t("help.modeTargetCommand")],
        ["@file", t("help.modeFile")],
        ["#symbol", t("help.modeSymbol")],
      ],
    },
    {
      title: t("help.scrollback"),
      items: [
        ["Shift+Wheel", t("help.scroll")],
        ["Shift+PageUp / PageDown", t("help.pageUpDown")],
        ["Shift+Home / End", t("help.topBottom")],
        ["Any key", t("help.exitScrollback")],
      ],
    },
  ];
  const right: Section[] = [
    {
      title: t("help.editor"),
      items: [
        [`F12 / ${mod}+Click`, t("help.goToDef")],
        ["Shift+F12", t("help.findRefs")],
        [t("help.hoverPointer"), t("help.hover")],
        ["F2", t("help.rename")],
        [`${mod}+Shift+O`, t("help.outline")],
        ["F8 / Shift+F8", t("help.nextDiagnostic")],
        [`${mod}+Shift+M`, t("help.listDiagnostics")],
        ["Ctrl+Space", t("help.completion")],
        ["Tab / Enter", t("help.acceptCompletion")],
        ["( / ,", t("help.signatureHelp")],
      ],
    },
    {
      title: t("help.editing"),
      items: [
        [`${mod}+S`, t("help.saveFile")],
        [undoRedo, t("help.undoRedo")],
        [`${mod}+/`, t("help.toggleComment")],
        ["Alt+↑ / ↓", t("help.moveLine")],
        ["Shift+Alt+↑ / ↓", t("help.copyLine")],
        ["Alt+Z", t("help.softWrap")],
        [fold, t("help.fold")],
        ["Alt+Click", t("help.addCursor")],
        ["Alt+Shift+drag", t("help.columnSelect")],
      ],
    },
    {
      title: t("help.find"),
      items: [
        [`${mod}+F`, t("help.findInFile")],
        [`F3 / Shift+F3`, t("help.findNextPrev")],
        [`${mod}+D`, t("help.selectNextOccurrence")],
        [`${mod}+Shift+L`, t("help.selectAllOccurrences")],
        [`${mod}+Alt+G`, t("help.gotoLine")],
      ],
    },
    {
      title: t("help.mouse"),
      items: [
        ["Click + drag", t("help.selectText")],
        ["Double / Triple-click", t("help.selectWordLine")],
        ["Alt+Click", t("help.openUrl")],
        ["Scrollbar", t("help.dragScroll")],
      ],
    },
    {
      title: t("help.touch"),
      items: [
        ["Swipe", t("help.touchScroll")],
        ["Long-press + drag", t("help.touchSelectCopy")],
        ["Long-press, release", t("help.touchRightClick")],
        ["Toolbar Paste", t("help.touchPaste")],
      ],
    },
  ];

  return (
    <OverlayBackdrop
      palette={props.palette}
      label={t("help.label")}
      onClose={props.onClose}
    >
      <OverlayPanel palette={props.palette} fontSize={props.fontSize}>
        <OverlayHeader
          palette={props.palette}
          fontSize={props.fontSize}
          title={t("help.title")}
          onClose={props.onClose}
        />
        <div
          style={{
            display: "flex",
            gap: `${scale.gap * 3}px`,
            padding: `${scale.tightGap}px 0`,
          }}
        >
          <Column sections={left} theme={theme} scale={scale} />
          <Column sections={right} theme={theme} scale={scale} />
        </div>
      </OverlayPanel>
    </OverlayBackdrop>
  );
}

function Column(props: {
  sections: Section[];
  theme: ReturnType<typeof themeFor>;
  scale: ReturnType<typeof uiScale>;
}) {
  return (
    <div style={{ flex: 1, "min-width": 0 }}>
      <For each={props.sections}>
        {(s) => (
          <div style={{ "margin-bottom": `${props.scale.gap * 2}px` }}>
            <div
              style={{
                "font-size": `${props.scale.sm}px`,
                "font-weight": 600,
                color: props.theme.dimFg,
                "margin-bottom": `${props.scale.tightGap}px`,
                "text-transform": "uppercase",
                "letter-spacing": "0.05em",
              }}
            >
              {s.title}
            </div>
            <table
              style={{
                "border-spacing": `${props.scale.controlX}px ${props.scale.controlY}px`,
                "margin-left": `${-props.scale.controlX}px`,
              }}
            >
              <tbody>
                <For each={s.items}>
                  {([key, desc]) => (
                    <tr>
                      <td style={{ "white-space": "nowrap" }}>
                        <kbd
                          style={{
                            ...ui.kbd,
                            "font-size": `${props.scale.sm}px`,
                          }}
                        >
                          {key}
                        </kbd>
                      </td>
                      <td
                        style={{
                          "font-size": `${props.scale.md}px`,
                          color: props.theme.dimFg,
                        }}
                      >
                        {desc}
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        )}
      </For>
    </div>
  );
}
