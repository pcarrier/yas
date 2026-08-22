import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KeyboardToggle } from "../KeyboardToggle";

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

function mount() {
  const input = document.createElement("textarea");
  const host = document.createElement("div");
  document.body.append(input, host);
  input.focus();
  const toggle = vi.fn();
  const [open, setOpen] = createSignal(false);
  dispose = render(
    () => (
      <KeyboardToggle
        open={open()}
        style={{}}
        onToggle={() => {
          toggle();
          setOpen((value) => !value);
        }}
      />
    ),
    host,
  );
  const button = host.querySelector("button")!;
  vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 40, 40),
  );
  return { input, button, toggle };
}

function touch(button: HTMLButtonElement, type: string, x = 20) {
  const point = { identifier: 1, clientX: x, clientY: 20 };
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: { value: type === "touchstart" ? [point] : [] },
    changedTouches: { value: [point] },
  });
  button.dispatchEvent(event);
  return event;
}

describe("keyboard toggle touch activation", () => {
  it("opens on touchend without waiting for a synthesized click", () => {
    const { input, button, toggle } = mount();
    expect(touch(button, "touchstart").defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input);
    expect(toggle).not.toHaveBeenCalled();
    expect(touch(button, "touchend").defaultPrevented).toBe(true);
    expect(toggle).toHaveBeenCalledOnce();
    expect(button.title).toBe("Hide keyboard");

    const click = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      detail: 1,
    });
    button.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(toggle).toHaveBeenCalledOnce();
  });

  it("handles consecutive show and hide taps exactly once each", () => {
    const { button, toggle } = mount();
    for (const title of ["Hide keyboard", "Show keyboard", "Hide keyboard"]) {
      touch(button, "touchstart");
      touch(button, "touchend");
      expect(button.title).toBe(title);
    }
    expect(toggle).toHaveBeenCalledTimes(3);
  });

  it.each(["touchcancel", "outside"])(
    "does not activate a %s gesture",
    (action) => {
      const { button, toggle } = mount();
      touch(button, "touchstart");
      if (action === "touchcancel") touch(button, "touchcancel");
      touch(button, "touchend", action === "outside" ? 100 : 20);
      expect(toggle).not.toHaveBeenCalled();
    },
  );

  it("preserves keyboard and mouse activation after a touch", () => {
    const { button, toggle } = mount();
    touch(button, "touchstart");
    touch(button, "touchend");
    button.click(); // Keyboard activation has detail=0.
    expect(toggle).toHaveBeenCalledTimes(2);

    const down = new Event("pointerdown", { bubbles: true });
    Object.defineProperty(down, "pointerType", { value: "mouse" });
    button.dispatchEvent(down);
    button.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    expect(toggle).toHaveBeenCalledTimes(3);
  });
});
