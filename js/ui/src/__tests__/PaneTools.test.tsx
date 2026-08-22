import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PaneTools, PaneToolsSlot, type PaneToolActions } from "../PaneTools";
import { darkTheme, uiScale } from "../theme";

let dispose: (() => void) | null = null;

afterEach(() => {
  dispose?.();
  dispose = null;
  document.body.replaceChildren();
});

function action(name: string): HTMLButtonElement {
  const button = document.querySelector<HTMLButtonElement>(
    `[data-yas-pane-action="${name}"]`,
  );
  if (!button) throw new Error(`missing ${name} pane action`);
  return button;
}

function mouseDown(button: HTMLButtonElement): MouseEvent {
  const event = new MouseEvent("mousedown", {
    bubbles: true,
    cancelable: true,
  });
  button.dispatchEvent(event);
  return event;
}

describe("PaneTools tab-bar actions", () => {
  it("parks on each first touch release with no compatibility click", () => {
    const parked = vi.fn();
    dispose = render(
      () => (
        <PaneTools
          onPark={parked}
          onClose={() => {}}
          theme={darkTheme}
          scale={uiScale(13)}
        />
      ),
      document.body,
    );
    const button = action("park");
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 0, 100, 40),
    );
    const finger = { identifier: -2, clientX: 20, clientY: 20 };
    for (let i = 1; i <= 3; i++) {
      for (const type of ["touchstart", "touchend"]) {
        const event = new Event(type, { bubbles: true, cancelable: true });
        Object.defineProperties(event, {
          touches: { value: type === "touchstart" ? [finger] : [] },
          changedTouches: { value: [finger] },
        });
        button.dispatchEvent(event);
      }
      expect(parked).toHaveBeenCalledTimes(i);
    }
  });

  it("keeps a pressed action mounted while the layout publishes new actions", () => {
    let oldCalls = 0;
    let liveCalls = 0;
    const [actions, setActions] = createSignal<PaneToolActions>({
      floating: { active: false, onToggle: () => oldCalls++ },
      onPark: () => oldCalls++,
      onClose: () => oldCalls++,
    });
    const host = document.createElement("div");
    document.body.append(host);

    dispose = render(
      () => (
        <PaneToolsSlot
          actions={actions()}
          theme={darkTheme}
          scale={uiScale(13)}
        />
      ),
      host,
    );

    const pressed = action("floating");
    mouseDown(pressed);
    setActions({
      floating: { active: true, onToggle: () => liveCalls++ },
      onPark: () => liveCalls++,
      onClose: () => liveCalls++,
    });

    expect(action("floating")).toBe(pressed);
    expect(pressed.getAttribute("aria-pressed")).toBe("true");
    pressed.click();
    expect(oldCalls).toBe(0);
    expect(liveCalls).toBe(1);
  });

  it("keeps pane focus and toggles float/tile and zoom/restore both ways", () => {
    const [floating, setFloating] = createSignal(false);
    const [solo, setSolo] = createSignal(false);
    const host = document.createElement("div");
    document.body.append(host);

    dispose = render(
      () => (
        <PaneTools
          drag={{ assignment: "terminal-1", paneId: "0" }}
          floating={{
            active: floating(),
            onToggle: () => setFloating((active) => !active),
          }}
          solo={{
            active: solo(),
            onToggle: () => setSolo((active) => !active),
          }}
          onPark={() => {}}
          onClose={() => {}}
          theme={darkTheme}
          scale={uiScale(13)}
        />
      ),
      host,
    );

    let floatingButton = action("floating");
    expect(mouseDown(floatingButton).defaultPrevented).toBe(true);
    floatingButton.click();
    expect(floating()).toBe(true);
    floatingButton = action("floating");
    expect(floatingButton.getAttribute("aria-pressed")).toBe("true");
    expect(floatingButton.getAttribute("aria-label")).toContain(
      "Return to tiling",
    );
    floatingButton.click();
    expect(floating()).toBe(false);
    expect(action("floating").getAttribute("aria-label")).toContain(
      "Make floating",
    );

    let soloButton = action("solo");
    expect(mouseDown(soloButton).defaultPrevented).toBe(true);
    soloButton.click();
    expect(solo()).toBe(true);
    soloButton = action("solo");
    expect(soloButton.getAttribute("aria-pressed")).toBe("true");
    expect(soloButton.getAttribute("aria-label")).toContain(
      "Restore all panes",
    );
    soloButton.click();
    expect(solo()).toBe(false);
    expect(action("solo").getAttribute("aria-label")).toContain(
      "Zoom this pane",
    );
  });

  it("wires park and close independently and names the action group", () => {
    let parked = 0;
    let closed = 0;
    const host = document.createElement("div");
    document.body.append(host);

    dispose = render(
      () => (
        <PaneTools
          onPark={() => parked++}
          onClose={() => closed++}
          theme={darkTheme}
          scale={uiScale(13)}
        />
      ),
      host,
    );

    const toolbar = document.querySelector('[role="toolbar"]');
    expect(toolbar?.getAttribute("aria-label")).toBe("Focused pane actions");

    const parkButton = action("park");
    expect(parkButton.getAttribute("aria-label")).toBe("Park");
    expect(parkButton.title).toBe("Park");
    expect(mouseDown(parkButton).defaultPrevented).toBe(true);
    parkButton.click();
    expect(parked).toBe(1);

    const closeButton = action("close");
    expect(mouseDown(closeButton).defaultPrevented).toBe(true);
    closeButton.click();
    expect(closed).toBe(1);
  });
});
