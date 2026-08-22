import { createRoot } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createPointerDrag } from "../pointerDrag";

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

function pointer(type: string, pointerId = 7) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    pointerId: { value: pointerId },
    pointerType: { value: pointerId === 7 ? "touch" : "mouse" },
  });
  return event;
}

function mount() {
  const handle = document.body.appendChild(document.createElement("div"));
  const captured = new Set<number>();
  handle.setPointerCapture = vi.fn((id) => captured.add(id));
  handle.hasPointerCapture = (id) => captured.has(id);
  handle.releasePointerCapture = vi.fn((id) => {
    captured.delete(id);
    handle.dispatchEvent(pointer("lostpointercapture", id));
  });
  const move = vi.fn();
  const end = vi.fn();
  createRoot((cleanup) => {
    dispose = cleanup;
    const start = createPointerDrag();
    handle.addEventListener("pointerdown", (event) => start(event, move, end));
  });
  handle.dispatchEvent(pointer("pointerdown"));
  return { handle, move, end };
}

describe("mixed-input dragging", () => {
  it("keeps a finger drag alive while a mouse moves, clicks, or loses capture", () => {
    const { handle, move, end } = mount();
    handle.dispatchEvent(pointer("pointerdown", 1));
    document.dispatchEvent(pointer("pointermove", 1));
    document.dispatchEvent(pointer("pointerup", 1));
    document.dispatchEvent(pointer("pointercancel", 1));
    handle.dispatchEvent(pointer("lostpointercapture", 1));
    expect(move).not.toHaveBeenCalled();
    expect(end).not.toHaveBeenCalled();
    expect(handle.setPointerCapture).toHaveBeenCalledExactlyOnceWith(7);

    document.dispatchEvent(pointer("pointermove"));
    document.dispatchEvent(pointer("pointerup"));
    expect(move).toHaveBeenCalledOnce();
    expect(end).toHaveBeenCalledExactlyOnceWith(true);

    handle.dispatchEvent(pointer("pointerdown", 1));
    document.dispatchEvent(pointer("pointermove", 1));
    document.dispatchEvent(pointer("pointerup", 1));
    expect(move).toHaveBeenCalledTimes(2);
    expect(end).toHaveBeenCalledTimes(2);
  });

  it.each(["pointercancel", "lostpointercapture", "blur", "unmount"])(
    "stops resizing after %s even if pointerup never arrives",
    (reason) => {
      const { handle, move, end } = mount();
      if (reason === "unmount") dispose?.();
      else if (reason === "blur") window.dispatchEvent(new Event("blur"));
      else handle.dispatchEvent(pointer(reason));

      document.dispatchEvent(pointer("pointermove"));
      document.dispatchEvent(pointer("pointerup"));
      expect(move).not.toHaveBeenCalled();
      expect(end).toHaveBeenCalledExactlyOnceWith(false);
      expect(handle.hasPointerCapture(7)).toBe(false);
    },
  );
});
