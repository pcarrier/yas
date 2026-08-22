import { disarmPrefix } from "./keyPrefix";

/** Restore keyboard ownership after wake or removal of the focused control. */
export function restoreWorkspaceFocusOnWake(
  fallback: HTMLElement,
  findTarget: () => HTMLElement | null,
  canRecoverPane: () => boolean = () => true,
): () => void {
  const doc = fallback.ownerDocument;
  const win = doc.defaultView!;
  let previous: HTMLElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const usable = (el: Element | null): el is HTMLElement =>
    el instanceof HTMLElement &&
    el !== doc.body &&
    el !== doc.documentElement &&
    el.isConnected &&
    !el.closest("[inert], [hidden]") &&
    !el.matches(":disabled") &&
    el.getClientRects().length > 0;
  const ownsKeyboard = (el: Element | null): el is HTMLElement =>
    el !== fallback && usable(el);

  const remember = () => {
    if (ownsKeyboard(doc.activeElement)) previous = doc.activeElement;
  };
  const cancel = () => {
    clearTimeout(timer);
    timer = undefined;
  };
  const restore = (waking: boolean) => {
    timer = undefined;
    if (doc.visibilityState === "hidden" || !fallback.isConnected) return;
    if (!waking && (!doc.hasFocus() || !canRecoverPane())) return;
    const active = doc.activeElement;
    if (ownsKeyboard(active) && doc.hasFocus()) return;
    const paneTarget = findTarget();
    const target =
      [active === fallback ? paneTarget : active, previous, paneTarget].find(
        ownsKeyboard,
      ) ?? fallback;

    // A retained DOM activeElement can outlive native window focus. Make an
    // actual focus transition in that case; focusing it again is a no-op.
    if (target === active && !doc.hasFocus()) {
      if (target === fallback) target.blur();
      else fallback.focus({ preventScroll: true });
    }
    target.focus({ preventScroll: true });
  };
  const wake = () => {
    cancel();
    // Let foreground events and any newly mounted pane/overlay settle first.
    timer = setTimeout(() => restore(true), 0);
  };
  const recover = () => {
    // Removing a focused node need not fire blur/focusout. Watch DOM changes
    // as well, but leave explicit blur alone (e.g. dismissing the mobile IME)
    // while its old control is still usable. The fallback is never an owner:
    // a pane that mounts later must be able to take the keyboard from it.
    if (
      timer === undefined &&
      (doc.activeElement === fallback || (previous && !previous.isConnected)) &&
      doc.hasFocus()
    ) {
      timer = setTimeout(() => restore(false), 0);
    }
  };
  const leave = () => {
    cancel();
    remember();
    disarmPrefix();
  };
  const visibility = () => {
    if (doc.visibilityState === "hidden") leave();
    else wake();
  };

  remember();
  const observer = new MutationObserver(recover);
  observer.observe(doc.body, { childList: true, subtree: true });
  doc.addEventListener("focusin", remember);
  doc.addEventListener("focusout", recover);
  doc.addEventListener("visibilitychange", visibility);
  win.addEventListener("blur", leave);
  win.addEventListener("focus", wake);
  return () => {
    cancel();
    observer.disconnect();
    doc.removeEventListener("focusin", remember);
    doc.removeEventListener("focusout", recover);
    doc.removeEventListener("visibilitychange", visibility);
    win.removeEventListener("blur", leave);
    win.removeEventListener("focus", wake);
  };
}
