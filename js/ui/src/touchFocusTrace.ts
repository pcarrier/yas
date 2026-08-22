/** Temporary on-device trace. No key events or editable values are recorded. */
export function traceTouchFocus(): () => void {
  const state = () => ({
    height: window.visualViewport?.height,
    offset: window.visualViewport?.offsetTop,
    layoutHeight: window.innerHeight,
    scale: window.visualViewport?.scale,
    scroll: [window.scrollX, window.scrollY],
    main: (() => {
      const main = document.querySelector("main");
      const box = main?.getBoundingClientRect();
      return box
        ? [box.x, box.y, box.width, box.height, main?.scrollTop]
        : null;
    })(),
    waylandKeyboardRequests: localStorage.getItem(
      "yas.waylandKeyboardRequests",
    ),
    inputs: Array.from(document.querySelectorAll("input, textarea")).map(
      describe,
    ),
  });
  const ids = new WeakMap<Element, number>();
  let nextId = 0;
  const describe = (target: EventTarget | null) => {
    if (!(target instanceof Element)) return null;
    if (!ids.has(target)) ids.set(target, ++nextId);
    return {
      node: ids.get(target),
      tag: target.tagName,
      label: target.getAttribute("aria-label") ?? target.getAttribute("title"),
      inputmode: target.getAttribute("inputmode"),
      action: target
        .closest("[data-yas-pane-action]")
        ?.getAttribute("data-yas-pane-action"),
      connected: target.isConnected,
    };
  };
  const entries: unknown[] = [];
  let dirty = false;
  const append = (entry: unknown) => {
    entries.push(entry);
    if (entries.length > 70) entries.shift();
    dirty = true;
  };
  const record = (event: Event) => {
    const target = event.target;
    const at = performance.now();
    const start = describe(target);
    const focus = describe(document.activeElement);
    const point =
      event instanceof TouchEvent
        ? event.changedTouches.item(0)
        : event instanceof MouseEvent
          ? event
          : null;
    const control =
      target instanceof Element
        ? target.closest("button,input,textarea")
        : null;
    const box = control?.getBoundingClientRect();
    const stack = event.type.startsWith("focus")
      ? new Error().stack?.slice(0, 1000)
      : undefined;
    setTimeout(() => {
      append({
        at,
        type: event.type,
        trusted: event.isTrusted,
        cancelled: event.defaultPrevented,
        pointerType:
          event instanceof PointerEvent ? event.pointerType : undefined,
        pointerId: event instanceof PointerEvent ? event.pointerId : undefined,
        start,
        end: describe(target),
        focus,
        point: point ? [point.clientX, point.clientY] : null,
        box: box ? [box.x, box.y, box.width, box.height] : null,
        afterFocus: describe(document.activeElement),
        stack,
        request:
          event.type === "yas-surface-text-input"
            ? (event as CustomEvent).detail
            : undefined,
        state: state(),
      });
    }, 0);
  };
  const types = [
    "pointerdown",
    "pointerup",
    "pointercancel",
    "dragstart",
    "dragend",
    "drop",
    "touchstart",
    "touchend",
    "touchcancel",
    "mousedown",
    "mouseup",
    "click",
    "focusin",
    "focusout",
    "yas-surface-text-input",
  ];
  for (const type of types) document.addEventListener(type, record, true);
  const viewport = window.visualViewport;
  viewport?.addEventListener("resize", record);
  viewport?.addEventListener("scroll", record);
  append({ type: "start", userAgent: navigator.userAgent, state: state() });
  const timer = setInterval(() => {
    if (!dirty) return;
    dirty = false;
    (
      import.meta.hot as unknown as
        | { send(event: string, data: unknown): void }
        | undefined
    )?.send("yas:touch-focus", entries);
  }, 500);
  return () => {
    clearInterval(timer);
    for (const type of types) document.removeEventListener(type, record, true);
    viewport?.removeEventListener("resize", record);
    viewport?.removeEventListener("scroll", record);
  };
}
