const SWITCHER_FOCUSABLE_CONTROL =
  "button:not([disabled]), a[href], select:not([disabled]), input:not([disabled])";

/**
 * Reveal a selected switcher row without asking the browser to move any
 * ancestor outside the results list. `scrollIntoView()` walks all scrolling
 * ancestors, including the document; on iPadOS that can pan the whole YAS
 * shell and carry the workspace tabs offscreen when the menu opens.
 */
export function revealSwitcherItem(
  scroller: HTMLElement,
  item: HTMLElement,
): void {
  const viewport = scroller.getBoundingClientRect();
  const row = item.getBoundingClientRect();
  if (row.top < viewport.top) {
    scroller.scrollTop -= viewport.top - row.top;
  } else if (row.bottom > viewport.bottom) {
    scroller.scrollTop += row.bottom - viewport.bottom;
  }
}

/**
 * Keep delayed pane focus work from taking keyboard input away from the
 * switcher. The search field is the switcher's default keyboard owner, while
 * its explicit controls remain reachable by pointer and keyboard.
 */
export function retainSwitcherFocus(
  root: HTMLElement,
  search: HTMLInputElement,
  ownerDocument: Document = root.ownerDocument,
): () => void {
  let released = false;
  let initialFocusPending = true;
  let frame: number | undefined;
  const isAllowed = (target: EventTarget | null): boolean => {
    if (target === search) return true;
    return (
      target instanceof HTMLElement &&
      root.contains(target) &&
      target.matches(SWITCHER_FOCUSABLE_CONTROL)
    );
  };

  const restore = () => {
    if (!root.isConnected || isAllowed(ownerDocument.activeElement)) return;
    search.focus({ preventScroll: true });
  };
  const claimInitialFocus = () => {
    if (released || !initialFocusPending) return;
    if (!root.isConnected || !search.isConnected) {
      if (typeof requestAnimationFrame === "function" && frame === undefined) {
        frame = requestAnimationFrame(() => {
          frame = undefined;
          claimInitialFocus();
        });
      }
      return;
    }
    initialFocusPending = false;
    search.focus({ preventScroll: true });
  };
  const onFocusIn = (event: FocusEvent) => {
    if (!isAllowed(event.target)) restore();
  };
  const onFocusOut = () => queueMicrotask(restore);

  ownerDocument.addEventListener("focusin", onFocusIn, true);
  ownerDocument.addEventListener("focusout", onFocusOut, true);
  // A Solid Portal can run the child's mount hook one microtask before its
  // root is attached to <body>. A focus() in that gap is silently ignored and
  // no later focus event necessarily occurs, so claim once synchronously and
  // once after portal insertion.
  claimInitialFocus();
  queueMicrotask(claimInitialFocus);

  return () => {
    released = true;
    if (frame !== undefined && typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(frame);
    }
    ownerDocument.removeEventListener("focusin", onFocusIn, true);
    ownerDocument.removeEventListener("focusout", onFocusOut, true);
  };
}
