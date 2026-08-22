import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createKeyboardFocus } from "../keyboardFocus";

let frames: Map<number, FrameRequestCallback>;
let dispose: (() => void) | undefined;
beforeEach(() => {
  vi.useFakeTimers();
  frames = new Map();
  let next = 0;
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    frames.set(++next, cb);
    return next;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
});
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
function paint() {
  const callbacks = [...frames.values()];
  frames.clear();
  for (const cb of callbacks) cb(performance.now());
}
function setup() {
  const input = document.createElement("textarea");
  input.inputMode = "none";
  input.dataset.yasInputmode = "email";
  document.body.append(input);
  input.focus();
  let wanted = true;
  const focus = createKeyboardFocus({
    ios: () => true,
    visible: () => false,
    wanted: () => wanted,
    canFocus: () => true,
    label: () => "Keyboard host",
  });
  dispose = focus.cancel;
  return {
    input,
    focus,
    hide: () => {
      wanted = false;
      focus.cancel();
    },
  };
}

it("creates a real focus transition for an already-focused Wayland input", () => {
  const { input, focus } = setup();
  focus.show(input); // No pointerdown snapshot for an asynchronous request.
  expect(input.inputMode).toBe("email");
  expect(document.activeElement).not.toBe(input);
  expect(focus.ownsFocus()).toBe(true);
  const spy = vi.spyOn(input, "focus");
  focus.land();
  expect(document.activeElement).toBe(input);
  expect(spy).toHaveBeenCalledWith({ preventScroll: true });
  expect(document.querySelector('[aria-label="Keyboard host"]')).toBeNull();
});

it("keeps the assist target until rendering catches up with an expired timer", () => {
  const { input, focus } = setup();
  focus.show(input);
  vi.advanceTimersByTime(2000); // Video work delayed the next rendering opportunity.
  expect(focus.ownsFocus()).toBe(true);
  focus.land(); // Delayed visualViewport resize confirms the keyboard.
  paint();
  paint();
  expect(document.activeElement).toBe(input);
});

it("does not restart a pending show on repeated icon taps", () => {
  const { input, focus } = setup();
  focus.show(input);
  const host = document.activeElement;
  vi.advanceTimersByTime(300);
  focus.show(input);
  expect(document.activeElement).toBe(host);
  focus.land();
  expect(document.activeElement).toBe(input);
});

it("can retry a remote request in the touch-release activation", () => {
  const { input, focus } = setup();
  focus.show(input);
  expect(focus.ownsFocus()).toBe(true);
  focus.show(input, null, true);
  expect(document.activeElement).toBe(input);
  expect(document.querySelector('[aria-label="Keyboard host"]')).toBeNull();
});

it("does not let an old attempt complete a newer one", () => {
  const { input, focus } = setup();
  focus.show(input);
  vi.advanceTimersByTime(600);
  focus.land();
  focus.show(input);
  const host = document.activeElement;
  paint();
  paint();
  expect(document.activeElement).toBe(host);
  focus.land();
  expect(document.activeElement).toBe(input);
});

it.each(["hide", "unmount", "other focus"])(
  "cancels focus restoration after %s",
  (reason) => {
    const { input, focus, hide } = setup();
    focus.show(input);
    if (reason === "hide") hide();
    else if (reason === "unmount") input.remove();
    else {
      const search = document.createElement("input");
      document.body.append(search);
      search.focus();
    }
    vi.advanceTimersByTime(600);
    paint();
    paint();
    expect(document.activeElement).not.toBe(input);
    expect(document.querySelector('[aria-label="Keyboard host"]')).toBeNull();
  },
);
