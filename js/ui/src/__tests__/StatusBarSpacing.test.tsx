import { PALETTES } from "@yas-run/core";
import { createSignal, type ComponentProps } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NetSampleRing, RenderSampleRing } from "../createMetrics";
import { StatusBar } from "../StatusBar";
import { uiScale, workspaceBarSizing } from "../theme";
import { t } from "../i18n";

let dispose: (() => void) | undefined;
beforeEach(() => {
  // Leave enough identity space to render the full icon cluster in jsdom.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 1000, 30),
  );
});
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function mount(fontSize: number, touch: boolean) {
  const [size, setSize] = createSignal(fontSize);
  const [isMobileTouch, setTouch] = createSignal(touch);
  const props: ComponentProps<typeof StatusBar> = {
    sessions: [],
    surfaceCount: 0,
    attentionCount: 0,
    tileCount: 0,
    webCount: 0,
    focusedSession: null,
    focusedSurface: null,
    connections: [],
    status: "connected",
    metrics: { bwIn: 0, bwOut: 0, fps: 0, ups: 0, renderMs: 0, maxRenderMs: 0 },
    palette: PALETTES[0],
    get fontSize() {
      return size();
    },
    fontFamily: "monospace",
    fontLoading: false,
    debug: false,
    toggleDebug: vi.fn(),
    previewPanelOpen: false,
    onPreviewPanel: vi.fn(),
    leftDockOpen: false,
    onToggleLeftDock: vi.fn(),
    webPane: null,
    debugStats: null,
    timeline: new RenderSampleRing(1),
    net: new NetSampleRing(1),
    onSwitcher: vi.fn(),
    onPalette: vi.fn(),
    onFont: vi.fn(),
    audioMuted: false,
    audioAvailable: true,
    onMedia: vi.fn(),
    get isMobileTouch() {
      return isMobileTouch();
    },
    activities: [],
  };
  dispose = render(() => <StatusBar {...props} />, document.body);
  return { setSize, setTouch };
}

function expectSpacing(fontSize: number, touch: boolean) {
  const scale = uiScale(fontSize);
  const bar = workspaceBarSizing(scale, touch);
  const width = `${bar.iconSize + scale.tightGap * 2}px`;
  const tools = document.querySelectorAll<HTMLElement>("[data-status-tool]");
  expect(tools).toHaveLength(6);
  for (const tool of tools) {
    expect(tool.style.minWidth).toBe(width);
    expect(tool.style.padding).toBe(`0px ${scale.tightGap}px`);
    expect(tool.style.fontSize).toBe(`${bar.iconSize}px`);
    expect(tool.style.alignSelf).toBe("stretch");
  }
  // Tray entries share this width, but top-bar pane controls stay unchanged.
  expect(
    tools[0].parentElement!.style.getPropertyValue("--yas-bar-button-width"),
  ).toBe(width);
  expect(
    document.querySelector<HTMLElement>('[role="status"]')!.style.minWidth,
  ).toBe(width);
  if (touch) {
    const keyboard = Array.from(document.querySelectorAll("button")).find(
      (button) => button.title === t("statusbar.showKeyboard"),
    )!;
    expect(keyboard.style.minWidth).toBe(width);
    expect(keyboard.style.padding).toBe(`0px ${scale.tightGap}px`);
  }
  const switcher = Array.from(document.querySelectorAll("button")).find(
    (button) => button.title === t("statusbar.menuTitle"),
  )!;
  expect(switcher.style.padding).toBe(`0px ${scale.controlX}px`);
}

describe("status-bar icon spacing", () => {
  it.each([12, 14, 18, 24])("keeps icons compact at %ipx", (size) => {
    const { setTouch } = mount(size, false);
    expectSpacing(size, false);
    setTouch(true);
    expectSpacing(size, true);
  });

  it("tracks font-size changes without scaling the padding for touch", () => {
    const { setSize } = mount(14, true);
    setSize(24);
    expectSpacing(24, true);
  });
});
