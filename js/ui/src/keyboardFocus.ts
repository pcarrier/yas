/** One keyboard-show attempt. WebKit needs a different editable element when
 * the pane's textarea already holds hardware focus with inputmode="none". */
export function createKeyboardFocus(options: {
  ios: () => boolean;
  visible: () => boolean;
  wanted: () => boolean;
  canFocus: (target: HTMLElement) => boolean;
  label: () => string;
}) {
  let host: HTMLTextAreaElement | null = null;
  let target: HTMLElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let frame: number | undefined;

  function clearPending() {
    clearTimeout(timer);
    if (frame !== undefined) cancelAnimationFrame(frame);
    timer = undefined;
    frame = undefined;
  }

  function cancel() {
    clearPending();
    target = null;
    const previous = host;
    host = null;
    previous?.remove();
  }

  function land() {
    const destination = target;
    const ownsFocus = host && document.activeElement === host;
    // Retire the attempt before focusing: focus handlers may start another.
    const previous = host;
    clearPending();
    target = null;
    host = null;
    if (
      ownsFocus &&
      destination?.isConnected &&
      options.wanted() &&
      options.canFocus(destination)
    )
      destination.focus({ preventScroll: true });
    previous?.remove();
  }

  return {
    cancel,
    land,
    ownsFocus: () => !!host && document.activeElement === host,
    show(input: HTMLElement, beforeTap: Element | null = null, retry = false) {
      // Repeated taps while the keyboard animates must not restart its assist
      // or let an older timeout consume a newer attempt.
      if (!retry && target === input && host && document.activeElement === host)
        return;
      const alreadyFocused =
        document.activeElement === input || beforeTap === input;
      cancel();
      const desired = input.dataset.yasInputmode;
      if (desired) input.setAttribute("inputmode", desired);
      else input.removeAttribute("inputmode");
      if (!options.visible() && alreadyFocused && options.ios()) {
        target = input;
        host = document.createElement("textarea");
        host.setAttribute("aria-label", options.label());
        host.tabIndex = -1;
        Object.assign(host.style, {
          position: "fixed",
          top: `${window.visualViewport?.offsetTop ?? 0}px`,
          left: "0",
          width: "1px",
          height: "1px",
          opacity: "0",
          fontSize: "16px",
          padding: "0",
          border: "none",
          outline: "none",
          resize: "none",
          overflow: "hidden",
          pointerEvents: "none",
        });
        document.body.appendChild(host);
        host.focus({ preventScroll: true });
        // A busy video stream can hold the main thread beyond the deadline.
        // Give WebKit layout/viewport delivery a rendering opportunity before
        // interpreting that expired timer as a failed keyboard show.
        timer = setTimeout(() => {
          frame = requestAnimationFrame(() => {
            frame = requestAnimationFrame(land);
          });
        }, 600);
      } else {
        if (!options.visible() && alreadyFocused) input.blur();
        input.focus({ preventScroll: true });
      }
      try {
        (
          navigator as { virtualKeyboard?: { show?: () => void } }
        ).virtualKeyboard?.show?.();
      } catch {
        // A remote request may arrive outside browser user activation.
      }
    },
  };
}
