import { PALETTES, type YasSession } from "@yas-run/core";
import { render } from "solid-js/web";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SwitcherOverlay } from "../SwitcherOverlay";
import { t } from "../i18n";

const workspace = vi.hoisted(() => ({
  search: vi.fn().mockResolvedValue([]),
  closeSession: vi.fn(),
  killSession: vi.fn(),
}));
vi.mock("@yas-run/solid", () => ({
  createYasWorkspace: () => workspace,
  YasTerminal: () => null,
  YasSurfaceView: () => null,
}));
vi.mock("../xdgDesktopCatalogs", () => ({
  xdgDesktopCatalogs: () => [
    {
      connectionId: "dev",
      apps: [],
      catalog: [{ id: "test.desktop", name: "Test application" }],
    },
  ],
  applicationIcon: () => undefined,
  requestApplicationIcons: () => {},
  startApplication: vi.fn(),
}));

let dispose: (() => void) | undefined;
const scrollDescriptor = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollIntoView",
);
beforeEach(() => {
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  if (scrollDescriptor) {
    Object.defineProperty(
      HTMLElement.prototype,
      "scrollIntoView",
      scrollDescriptor,
    );
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

function mount(sessions: YasSession[] = []) {
  const select = vi.fn();
  const create = vi.fn();
  const close = vi.fn();
  const start = vi.fn().mockReturnValue(true);
  dispose = render(
    () => (
      <SwitcherOverlay
        sessions={sessions}
        focusedSessionId={sessions[0]?.id ?? null}
        lru={sessions.map((session) => session.id)}
        palette={PALETTES[0]}
        onSelect={select}
        onCreate={create}
        onClose={close}
        onStartApplication={start}
      />
    ),
    document.body,
  );
  return { select, create, close, start };
}

function session(id: string): YasSession {
  return {
    id,
    connectionId: "dev",
    ptyId: 1n,
    tag: id,
    title: id,
    command: null,
    state: "active",
    usedRows: 1,
    exitStatus: null,
  };
}

function row(title: string) {
  const element = [
    ...document.querySelectorAll<HTMLElement>("section div"),
  ].find(
    (element) =>
      element.style.cursor === "pointer" &&
      element.textContent?.includes(title),
  );
  expect(element, `menu row: ${title}`).toBeDefined();
  vi.spyOn(element!, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 300, 60),
  );
  return element!;
}

function touch(element: HTMLElement, type: string, x = 20) {
  const point = { identifier: -2, clientX: x, clientY: 20 };
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: {
      value: type === "touchend" || type === "touchcancel" ? [] : [point],
    },
    changedTouches: { value: [point] },
  });
  element.dispatchEvent(event);
  return event;
}

function compatibilityClick(element: HTMLElement) {
  element.dispatchEvent(
    new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      detail: 1,
      clientX: 20,
      clientY: 20,
    }),
  );
}

describe("main menu touch activation", () => {
  it("activates on touchend, retains search focus, and ignores a later compatibility click", async () => {
    const { create, close } = mount();
    await Promise.resolve(); // The focus owner claims the inserted portal.
    const search = document.querySelector("input")!;
    expect(document.activeElement).toBe(search);
    const entry = row(t("switcher.newTerminal"));
    const label = entry.querySelector("span")!;
    touch(label, "touchstart");
    expect(touch(label, "touchend").defaultPrevented).toBe(true);
    expect(create).toHaveBeenCalledExactlyOnceWith(undefined, undefined);
    expect(document.activeElement).toBe(search);
    compatibilityClick(entry);
    expect(create).toHaveBeenCalledOnce();
    expect(close).not.toHaveBeenCalled();
  });

  it("selects the tapped terminal instead of the keyboard-selected terminal", () => {
    const { select } = mount([
      session("First terminal"),
      session("Second terminal"),
    ]);
    const entry = row("Second terminal");
    touch(entry, "touchstart");
    touch(entry, "touchend");
    expect(select).toHaveBeenCalledExactlyOnceWith("Second terminal");
    compatibilityClick(entry);
    expect(select).toHaveBeenCalledOnce();
  });

  it("launches an application from a touch-only tap", () => {
    const { start, close } = mount();
    const entry = row("Test application");
    touch(entry, "touchstart");
    touch(entry, "touchend");
    expect(start).toHaveBeenCalledExactlyOnceWith("dev", "test.desktop");
    expect(close).toHaveBeenCalledOnce();
  });

  it("lets the list scroll without activating the swiped row", () => {
    const { create } = mount();
    const entry = row(t("switcher.newTerminal"));
    const scroller = [...document.querySelectorAll<HTMLElement>("div")].find(
      (element) => element.style.overflow === "auto" && element.contains(entry),
    )!;
    // jsdom does not expand the overflow shorthand into computed longhands.
    scroller.style.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { value: 800 },
      clientHeight: { value: 300 },
    });
    expect(touch(entry, "touchstart").defaultPrevented).toBe(false);
    expect(touch(entry, "touchmove", 80).defaultPrevented).toBe(false);
    touch(entry, "touchend");
    compatibilityClick(entry);
    expect(create).not.toHaveBeenCalled();
    touch(entry, "touchstart");
    touch(entry, "touchend");
    expect(create).toHaveBeenCalledOnce();
  });

  it("keeps a nested Close tap from selecting its terminal", () => {
    const { select } = mount([session("Terminal")]);
    const entry = row("Terminal");
    const button = entry.querySelector<HTMLButtonElement>(
      `button[title="${t("switcher.close")}"]`,
    )!;
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 0, 40, 40),
    );
    touch(button, "touchstart");
    touch(button, "touchend");
    compatibilityClick(button);
    expect(workspace.closeSession).toHaveBeenCalledExactlyOnceWith("Terminal");
    expect(select).not.toHaveBeenCalled();
  });

  it("preserves mouse clicks and Enter activation", () => {
    const { create } = mount();
    row(t("switcher.newTerminal")).click();
    expect(create).toHaveBeenCalledOnce();
    document.querySelector("input")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(create).toHaveBeenCalledTimes(2);
  });

  it("opens the nested Kill picker and sends its signal without selecting the row", () => {
    const { select } = mount([session("Terminal")]);
    const entry = row("Terminal");
    for (const title of [t("switcher.kill"), "INT"]) {
      const button = entry.querySelector<HTMLButtonElement>(
        `button[title="${title}"]`,
      )!;
      vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
        new DOMRect(0, 0, 40, 40),
      );
      touch(button, "touchstart");
      touch(button, "touchend");
    }
    expect(workspace.killSession).toHaveBeenCalledExactlyOnceWith(
      "Terminal",
      2,
    );
    expect(select).not.toHaveBeenCalled();
  });
});
