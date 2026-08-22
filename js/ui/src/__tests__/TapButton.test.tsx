import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TapArea, TapButton } from "../TapButton";

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

function touch(button: HTMLElement, type: string, x = 20) {
  const point = { identifier: -2, clientX: x, clientY: 20 };
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: {
      value: type === "touchend" || type === "touchcancel" ? [] : [point],
    },
    changedTouches: { value: [point] },
    shiftKey: { value: true },
  });
  button.dispatchEvent(event);
  return event;
}

function geometry(button: HTMLElement) {
  vi.spyOn(button, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 100, 40),
  );
}

describe("shared touch buttons", () => {
  it("runs the existing click handler, propagates its event, and suppresses a duplicate", () => {
    const action = vi.fn();
    const parent = vi.fn();
    dispose = render(
      () => (
        <div onClick={parent}>
          <TapButton
            onClick={(event) => {
              expect(event.currentTarget).toBe(button);
              // HTMLElement.click(), unlike a hand-built MouseEvent, follows
              // the browser's button activation path and has keyboard-style
              // click metadata.
              expect(event.detail).toBe(0);
              event.stopPropagation();
              action();
            }}
          >
            Park
          </TapButton>
        </div>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    // No focused input: this must work with the software keyboard hidden too.
    for (let i = 1; i <= 3; i++) {
      touch(button, "touchstart");
      touch(button, "touchend");
      button.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }),
      );
      expect(action).toHaveBeenCalledTimes(i);
      expect(parent).not.toHaveBeenCalled();
    }
  });

  it("activates an onActivate-only control through the native click path", () => {
    const action = vi.fn();
    const bubbled = vi.fn();
    dispose = render(
      () => (
        <div onClick={bubbled}>
          <TapButton onActivate={action}>Open workspace manager</TapButton>
        </div>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);

    touch(button, "touchstart");
    touch(button, "touchend");
    button.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }),
    );

    expect(action).toHaveBeenCalledOnce();
    expect(bubbled).toHaveBeenCalledOnce();
  });

  it("submits a form once, including when the submit button has no click handler", () => {
    const submit = vi.fn();
    dispose = render(
      () => (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <TapButton type="submit">Save</TapButton>
        </form>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    touch(button, "touchstart");
    touch(button, "touchend");
    button.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }),
    );
    expect(submit).toHaveBeenCalledOnce();
  });

  it("allows native menu scrolling and ignores a swipe that ends back inside the button", () => {
    const action = vi.fn();
    dispose = render(
      () => (
        <div style={{ "overflow-y": "auto" }}>
          <TapButton onClick={action}>Select session</TapButton>
        </div>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    Object.defineProperties(button.parentElement!, {
      clientHeight: { value: 100 },
      scrollHeight: { value: 400 },
    });
    expect(touch(button, "touchstart").defaultPrevented).toBe(false);
    expect(touch(button, "touchmove", 60).defaultPrevented).toBe(false);
    touch(button, "touchend");
    expect(action).not.toHaveBeenCalled();
    touch(button, "touchstart");
    expect(touch(button, "touchend").defaultPrevented).toBe(true);
    expect(action).toHaveBeenCalledOnce();
  });

  it("respects a disabled fieldset", () => {
    const action = vi.fn();
    dispose = render(
      () => (
        <fieldset disabled>
          <TapButton onClick={action}>Save</TapButton>
        </fieldset>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    touch(button, "touchstart");
    touch(button, "touchend");
    expect(action).not.toHaveBeenCalled();
  });

  it("cancels when a second finger lands outside the button and lifts first", () => {
    const action = vi.fn();
    dispose = render(
      () => <TapButton onClick={action}>Park</TapButton>,
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    touch(button, "touchstart");
    const second = new Event("touchstart", { bubbles: true });
    Object.defineProperty(second, "touches", {
      value: [
        { identifier: -2, clientX: 20, clientY: 20 },
        { identifier: -1, clientX: 200, clientY: 20 },
      ],
    });
    document.body.dispatchEvent(second);
    touch(button, "touchend");
    expect(action).not.toHaveBeenCalled();
    touch(button, "touchstart");
    touch(button, "touchend");
    expect(action).toHaveBeenCalledOnce();
  });

  it("preserves pointer and context-menu handlers without activating a long press", () => {
    const pointer = vi.fn();
    const menu = vi.fn();
    const action = vi.fn();
    dispose = render(
      () => (
        <TapButton
          onPointerDown={pointer}
          onContextMenu={menu}
          onClick={action}
        >
          Tray
        </TapButton>
      ),
      document.body,
    );
    const button = document.querySelector("button")!;
    geometry(button);
    button.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(pointer).toHaveBeenCalledOnce();
    expect(touch(button, "touchstart").defaultPrevented).toBe(false);
    button.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    touch(button, "touchend");
    expect(menu).toHaveBeenCalledOnce();
    expect(action).not.toHaveBeenCalled();
  });
});

describe("shared touch areas", () => {
  it.each(["touchcancel", "pointercancel", "contextmenu", "dragstart"])(
    "does not activate a row after %s",
    (type) => {
      const action = vi.fn();
      dispose = render(
        () => <TapArea onClick={action}>File</TapArea>,
        document.body,
      );
      const row = document.querySelector<HTMLElement>("[data-yas-tap]")!;
      geometry(row);
      touch(row, "touchstart");
      row.dispatchEvent(new Event(type, { bubbles: true }));
      touch(row, "touchend");
      row.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
      expect(action).not.toHaveBeenCalled();
      // A subsequent real mouse press is unaffected by the cancelled touch.
      row.dispatchEvent(new Event("pointerdown", { bubbles: true }));
      row.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
      expect(action).toHaveBeenCalledOnce();
    },
  );

  it("preserves scrolling and ignores swipes ending back on the row", () => {
    const action = vi.fn();
    dispose = render(
      () => (
        <div style={{ "overflow-y": "auto" }}>
          <TapArea onClick={action}>File</TapArea>
        </div>
      ),
      document.body,
    );
    const row = document.querySelector<HTMLElement>("[data-yas-tap]")!;
    geometry(row);
    Object.defineProperties(row.parentElement!, {
      clientHeight: { value: 100 },
      scrollHeight: { value: 400 },
    });
    expect(touch(row, "touchstart").defaultPrevented).toBe(false);
    expect(touch(row, "touchmove", 60).defaultPrevented).toBe(false);
    touch(row, "touchend");
    expect(action).not.toHaveBeenCalled();
    touch(row, "touchstart");
    touch(row, "touchend");
    expect(action).toHaveBeenCalledOnce();
  });

  it("does not steal taps from nested native inputs", () => {
    const action = vi.fn();
    dispose = render(
      () => (
        <TapArea onClick={action}>
          <input />
        </TapArea>
      ),
      document.body,
    );
    const row = document.querySelector<HTMLElement>("[data-yas-tap]")!;
    geometry(row);
    const input = row.querySelector("input")!;
    expect(touch(input, "touchstart").defaultPrevented).toBe(false);
    expect(touch(input, "touchend").defaultPrevented).toBe(false);
    expect(action).not.toHaveBeenCalled();
  });
});
