const SWITCHER_FOCUSABLE_CONTROL =
  'button:not([disabled]), a[href], select:not([disabled]), input:not([disabled])';

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
  const onFocusIn = (event: FocusEvent) => {
    if (!isAllowed(event.target)) restore();
  };
  const onFocusOut = () => queueMicrotask(restore);

  ownerDocument.addEventListener("focusin", onFocusIn, true);
  ownerDocument.addEventListener("focusout", onFocusOut, true);
  restore();

  return () => {
    ownerDocument.removeEventListener("focusin", onFocusIn, true);
    ownerDocument.removeEventListener("focusout", onFocusOut, true);
  };
}
