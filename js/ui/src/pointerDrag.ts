import { onCleanup } from "solid-js";

/** Keep a UI drag owned by its initiating pointer on mixed-input devices. */
export function createPointerDrag() {
  let finish: ((completed?: boolean) => void) | null = null;
  onCleanup(() => finish?.());

  return (
    event: PointerEvent,
    onMove: (event: PointerEvent) => void,
    onEnd: (completed: boolean) => void = () => {},
  ): boolean => {
    if (finish) return false;
    const target = event.currentTarget as HTMLElement;
    const document = target.ownerDocument;
    const window = document.defaultView;
    const pointerId = event.pointerId;
    event.preventDefault();
    target.setPointerCapture(pointerId);

    const move = (event: PointerEvent) => {
      if (event.pointerId === pointerId) onMove(event);
    };
    const end = (event: PointerEvent) => {
      if (event.pointerId === pointerId) stop(event.type === "pointerup");
    };
    const cancel = () => stop();
    const stop = (completed = false) => {
      if (finish !== stop) return;
      finish = null;
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", end);
      document.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      window?.removeEventListener("blur", cancel);
      if (target.hasPointerCapture?.(pointerId))
        target.releasePointerCapture(pointerId);
      onEnd(completed);
    };
    finish = stop;
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", end);
    document.addEventListener("pointercancel", end);
    target.addEventListener("lostpointercapture", end);
    window?.addEventListener("blur", cancel);
    return true;
  };
}
