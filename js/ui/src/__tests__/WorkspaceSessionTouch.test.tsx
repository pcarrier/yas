import { PALETTES } from "@yas-run/core";
import { render } from "solid-js/web";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceSessionTabs } from "../WorkspaceSessionTabs";
import { WorkspaceSessionOverlay } from "../WorkspaceSessionOverlay";
import { t } from "../i18n";
import type { WorkspaceSessionController } from "../workspaceSession";

let dispose: (() => void) | undefined;
const visualViewportDescriptor = Object.getOwnPropertyDescriptor(
  window,
  "visualViewport",
);
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  if (visualViewportDescriptor) {
    Object.defineProperty(window, "visualViewport", visualViewportDescriptor);
  } else {
    Reflect.deleteProperty(window, "visualViewport");
  }
});

function mount() {
  const input = document.createElement("textarea");
  const host = document.createElement("div");
  document.body.append(input, host);
  input.focus();
  const openManager = vi.fn();
  const controller = {
    attachedSessions: () => [],
    current: () => null,
    error: () => null,
    warnings: () => [],
    openManager,
  } as unknown as WorkspaceSessionController;
  dispose = render(
    () => (
      <WorkspaceSessionTabs
        controller={controller}
        palette={PALETTES[0]}
        fontFamily="monospace"
        fontSize={14}
        isMobileTouch
      />
    ),
    host,
  );
  const button = host.querySelector("button")!;
  vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 40, 40),
  );
  return { button, input, openManager };
}

function touch(button: HTMLButtonElement, type: string, x = 20, count = 1) {
  const points = Array.from({ length: count }, (_, i) => ({
    identifier: -2147483648 + i,
    clientX: x,
    clientY: 20,
  }));
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: { value: type === "touchstart" ? points : [] },
    changedTouches: { value: points },
  });
  button.dispatchEvent(event);
  return event;
}

describe("sessions manager touch activation", () => {
  it("fits the manager into the visual viewport above a virtual keyboard", () => {
    const viewport = Object.assign(new EventTarget(), {
      height: window.innerHeight,
      width: window.innerWidth,
      offsetTop: 0,
      offsetLeft: 0,
      pageTop: 0,
      pageLeft: 0,
      scale: 1,
      onresize: null,
      onscroll: null,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: viewport,
    });
    const controller = {
      managerOpen: () => true,
      loading: () => false,
      error: () => null,
      warnings: () => [],
      sessions: () => [],
      attachedSessionIds: () => [],
      current: () => null,
      closeManager: vi.fn(),
      create: vi.fn(async () => {}),
    } as unknown as WorkspaceSessionController;

    dispose = render(
      () => (
        <WorkspaceSessionOverlay
          controller={controller}
          palette={PALETTES[0]}
          fontFamily="monospace"
          fontSize={14}
        />
      ),
      document.body,
    );

    const backdrop = document.querySelector<HTMLElement>('[role="dialog"]')!;
    const panel = backdrop.firstElementChild as HTMLElement;
    viewport.height = window.innerHeight - 300;
    viewport.offsetTop = 96;
    viewport.pageTop = 96;
    viewport.dispatchEvent(new Event("resize"));

    expect(backdrop.style.top).toBe("96px");
    expect(backdrop.style.height).toBe(`${viewport.height}px`);
    expect(backdrop.style.getPropertyValue("--overlay-panel-cap")).toBe("100%");
    expect(panel.style.maxHeight).toBe(
      "min(760px, var(--overlay-panel-cap, 80%))",
    );
  });

  it("keeps a rename input and a pressed tab mounted across catalogue updates", () => {
    const initial = { id: "session-one", name: "One", updatedAtUnixMs: 0 };
    const [sessions, setSessions] = createSignal([initial]);
    const select = vi.fn(async () => {});
    const controller = {
      managerOpen: () => true,
      loading: () => false,
      error: () => null,
      warnings: () => [],
      sessions,
      attachedSessions: sessions,
      attachedSessionIds: () => [initial.id],
      current: () => sessions()[0],
      select,
      closeManager: vi.fn(),
    } as unknown as WorkspaceSessionController;
    dispose = render(
      () => (
        <>
          <WorkspaceSessionTabs
            controller={controller}
            palette={PALETTES[0]}
            fontFamily="monospace"
            fontSize={14}
            isMobileTouch
          />
          <WorkspaceSessionOverlay
            controller={controller}
            palette={PALETTES[0]}
            fontFamily="monospace"
            fontSize={14}
          />
        </>
      ),
      document.body,
    );
    const rename = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent === t("sessions.rename"),
    )!;
    rename.click();
    const input = document.querySelector("input")!;
    input.focus();
    input.setSelectionRange(1, 2);
    const tab = document.querySelector<HTMLButtonElement>('[role="tab"]')!;
    vi.spyOn(tab, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 0, 100, 40),
    );
    touch(tab, "touchstart");
    setSessions([{ ...initial, updatedAtUnixMs: 1 }]);
    expect(document.querySelector("input")).toBe(input);
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(1);
    expect(document.querySelector('[role="tab"]')).toBe(tab);
    touch(tab, "touchend");
    expect(select).toHaveBeenCalledExactlyOnceWith(initial.id, "push");
  });

  it("runs manager actions and rename submission from single taps", async () => {
    const session = { id: "session-one", name: "One", updatedAtUnixMs: 0 };
    const rename = vi.fn(async () => {});
    const detach = vi.fn(async () => {});
    const remove = vi.fn(async () => {});
    const create = vi.fn(async () => {});
    const closeManager = vi.fn();
    const controller = {
      managerOpen: () => true,
      loading: () => false,
      error: () => null,
      warnings: () => [],
      sessions: () => [session],
      attachedSessionIds: () => [session.id],
      current: () => session,
      rename,
      detach,
      delete: remove,
      create,
      closeManager,
    } as unknown as WorkspaceSessionController;
    dispose = render(
      () => (
        <WorkspaceSessionOverlay
          controller={controller}
          palette={PALETTES[0]}
          fontFamily="monospace"
          fontSize={14}
        />
      ),
      document.body,
    );
    const tap = (label: string) => {
      const button = Array.from(document.querySelectorAll("button")).find(
        (b) => b.textContent === label,
      )!;
      expect(button).toBeDefined();
      vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
        new DOMRect(0, 0, 100, 40),
      );
      touch(button, "touchstart");
      touch(button, "touchend");
      // A late compatibility click must not double-activate an action or
      // count as the second confirmation for deletion.
      button.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }),
      );
    };
    const settle = async () => {
      await Promise.resolve();
      await Promise.resolve();
    };

    tap(t("sessions.rename"));
    const input = document.querySelector("input")!;
    input.value = "Renamed";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    tap(t("sessions.save"));
    await settle();
    expect(rename).toHaveBeenCalledExactlyOnceWith(session.id, "Renamed");

    tap(t("sessions.detach"));
    await settle();
    expect(detach).toHaveBeenCalledExactlyOnceWith(session.id);

    tap(t("sessions.delete"));
    expect(remove).not.toHaveBeenCalled();
    tap(t("sessions.confirmDelete"));
    await settle();
    expect(remove).toHaveBeenCalledExactlyOnceWith(session.id);

    tap(t("sessions.create"));
    await settle();
    expect(create).toHaveBeenCalledOnce();
    expect(closeManager).not.toHaveBeenCalled();
  });

  it("opens on the first and subsequent taps without blurring or waiting for click", () => {
    const { button, input, openManager } = mount();
    for (let i = 1; i <= 3; i++) {
      expect(touch(button, "touchstart").defaultPrevented).toBe(true);
      expect(document.activeElement).toBe(input);
      expect(touch(button, "touchend").defaultPrevented).toBe(true);
      expect(openManager).toHaveBeenCalledTimes(i);
      button.dispatchEvent(
        new MouseEvent("click", { bubbles: true, detail: 1 }),
      );
      expect(openManager).toHaveBeenCalledTimes(i);
    }
  });

  it("ignores a compatibility click retargeted to the mounted manager backdrop", () => {
    const [managerOpen, setManagerOpen] = createSignal(false);
    const controller = {
      attachedSessions: () => [],
      current: () => null,
      error: () => null,
      warnings: () => [],
      managerOpen,
      loading: () => false,
      sessions: () => [],
      openManager: () => setManagerOpen(true),
      closeManager: () => setManagerOpen(false),
    } as unknown as WorkspaceSessionController;
    dispose = render(
      () => (
        <>
          <WorkspaceSessionTabs
            controller={controller}
            palette={PALETTES[0]}
            fontFamily="monospace"
            fontSize={14}
            isMobileTouch
          />
          <WorkspaceSessionOverlay
            controller={controller}
            palette={PALETTES[0]}
            fontFamily="monospace"
            fontSize={14}
          />
        </>
      ),
      document.body,
    );
    const button = document.querySelector<HTMLButtonElement>(
      'button[aria-label="Open workspace manager"]',
    )!;
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 0, 40, 40),
    );

    touch(button, "touchstart");
    touch(button, "touchend");
    const backdrop = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(backdrop).not.toBeNull();
    backdrop.dispatchEvent(
      new MouseEvent("click", {
        bubbles: true,
        cancelable: true,
        detail: 1,
        clientX: 20,
        clientY: 20,
      }),
    );

    expect(managerOpen()).toBe(true);
    expect(document.querySelector('[role="dialog"]')).toBe(backdrop);
  });

  it.each(["cancelled", "outside", "multitouch"])(
    "ignores %s gestures",
    (kind) => {
      const { button, openManager } = mount();
      touch(button, "touchstart", 20, kind === "multitouch" ? 2 : 1);
      if (kind === "cancelled") touch(button, "touchcancel");
      touch(button, "touchend", kind === "outside" ? 100 : 20);
      expect(openManager).not.toHaveBeenCalled();
    },
  );
});
