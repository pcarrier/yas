/**
 * Browser clipboard authority is page-global, not connection-local.
 *
 * A monotonically increasing epoch lets every YAS connection invalidate a
 * cached Wayland owner after any host copy, including a copy performed by a
 * different connection or by ordinary page UI.
 */
let browserClipboardEpoch = 0n;
let observerCount = 0;
let removeObservers: (() => void) | null = null;

export function currentBrowserClipboardEpoch(): bigint {
  return browserClipboardEpoch;
}

export function noteBrowserClipboardMayHaveChanged(): bigint {
  browserClipboardEpoch += 1n;
  return browserClipboardEpoch;
}

/** Install one shared observer for all workspaces in this page. */
export function retainBrowserClipboardObserver(): () => void {
  observerCount += 1;
  if (
    removeObservers === null &&
    typeof document !== "undefined" &&
    typeof window !== "undefined"
  ) {
    const note = () => noteBrowserClipboardMayHaveChanged();
    document.addEventListener("copy", note, true);
    document.addEventListener("cut", note, true);
    // Native copies performed while another application has focus do not
    // produce a DOM event. Treat the next blur as a conservative boundary.
    window.addEventListener("blur", note);
    removeObservers = () => {
      document.removeEventListener("copy", note, true);
      document.removeEventListener("cut", note, true);
      window.removeEventListener("blur", note);
    };
  }
  let retained = true;
  return () => {
    if (!retained) return;
    retained = false;
    observerCount -= 1;
    if (observerCount === 0) {
      removeObservers?.();
      removeObservers = null;
    }
  };
}
