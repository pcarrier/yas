import { onCleanup, splitProps, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

/** A swipe starting on a button must still scroll its menu or tab strip. */
function canScrollFrom(button: HTMLElement): boolean {
  for (let el = button.parentElement; el; el = el.parentElement) {
    const style = getComputedStyle(el);
    if (
      (/auto|scroll/.test(style.overflowX) &&
        el.scrollWidth > el.clientWidth) ||
      (/auto|scroll/.test(style.overflowY) && el.scrollHeight > el.clientHeight)
    )
      return true;
  }
  return false;
}

/** Complete taps inside the native touch gesture instead of waiting for
 * iPadOS's compatibility mouse events, which can blur/remount the control.
 * Use the element's activation method so the browser owns click dispatch and
 * default actions such as form submission. */
export function TapButton(
  props: Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "style"> & {
    onActivate?: () => void;
    /** Overrides onClick for activation completed by the touch handler. */
    onTouchClick?: JSX.EventHandler<HTMLButtonElement, MouseEvent>;
    style?: JSX.CSSProperties;
  },
) {
  const [local, rest] = splitProps(props, [
    "onActivate",
    "onClick",
    "onTouchClick",
    "style",
    "onContextMenu",
  ]);
  return (
    <button
      {...rest}
      {...createTapHandlers<HTMLButtonElement>(local)}
      onContextMenu={local.onContextMenu}
      style={{ "touch-action": "manipulation", ...local.style }}
    />
  );
}

/** Touch activation for existing rows/headers, without changing their layout
 * or nesting buttons around secondary controls. Keyboard semantics stay with
 * the caller, just like an ordinary div/span. */
export function TapArea(
  props: Omit<JSX.HTMLAttributes<HTMLElement>, "style"> & {
    as?: "div" | "span";
    style?: JSX.CSSProperties;
  },
) {
  const [local, rest] = splitProps(props, [
    "as",
    "onClick",
    "style",
    "onContextMenu",
  ]);
  return (
    <Dynamic
      component={local.as ?? "div"}
      {...rest}
      {...createTapHandlers<HTMLElement>(local)}
      onContextMenu={local.onContextMenu}
      style={{ "touch-action": "manipulation", ...local.style }}
    />
  );
}

function createTapHandlers<T extends HTMLElement>(props: {
  onClick?: JSX.HTMLAttributes<T>["onClick"];
  onContextMenu?: JSX.HTMLAttributes<T>["onContextMenu"];
  onActivate?: () => void;
  onTouchClick?: JSX.EventHandler<T, MouseEvent>;
}) {
  let press: { id: number; x: number; y: number } | null = null;
  let suppressTouchClick = false;
  let dispatchingTouchClick = false;
  let touchDocument: Document | null = null;
  let compatibilityClickCleanup: (() => void) | null = null;
  const cancelCompatibilityClick = () => {
    compatibilityClickCleanup?.();
    compatibilityClickCleanup = null;
  };
  const suppressCompatibilityClickAt = (document: Document, touch: Touch) => {
    cancelCompatibilityClick();
    const { clientX, clientY } = touch;
    const suppress = (event: MouseEvent) => {
      // Programmatic and keyboard activation use detail=0. The delayed touch
      // compatibility click is a mouse click at the lifted finger's position,
      // possibly retargeted to UI mounted by the immediate activation.
      if (
        event.detail === 0 ||
        Math.hypot(event.clientX - clientX, event.clientY - clientY) > 32
      )
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      cancelCompatibilityClick();
    };
    document.addEventListener("click", suppress, true);
    const timeout = setTimeout(cancelCompatibilityClick, 800);
    compatibilityClickCleanup = () => {
      clearTimeout(timeout);
      document.removeEventListener("click", suppress, true);
    };
  };
  const cancelPress = () => {
    press = null;
    touchDocument?.removeEventListener("touchstart", cancelMultitouch);
    touchDocument = null;
  };
  const cancelMultitouch = (event: TouchEvent) => {
    if (event.touches.length !== 1) cancelPress();
  };
  onCleanup(() => {
    cancelPress();
    cancelCompatibilityClick();
  });
  const moved = (touch: Touch) =>
    !press || Math.hypot(touch.clientX - press.x, touch.clientY - press.y) > 10;
  return {
    "data-yas-tap": "",
    "on:pointerdown": ((event) => {
      if (event.pointerType !== "touch") {
        suppressTouchClick = false;
        // A real mouse/pen press supersedes the delayed compatibility-click
        // window from the preceding touch.
        cancelCompatibilityClick();
      }
    }) satisfies JSX.EventHandler<T, PointerEvent>,
    "on:pointercancel": cancelPress,
    "on:contextmenu": cancelPress,
    // The drag bridge can claim a stationary hold before any touchmove
    // reaches the row. That gesture must never finish as a tap.
    "on:dragstart": cancelPress,
    "on:touchstart": ((event) => {
      // Nested actions own their taps. In particular, a TapButton inside a
      // row can stop its click without the row later clicking itself.
      const target = event.target as Element;
      if (
        target.closest(
          "[data-yas-tap], button, a, input, select, textarea, [contenteditable]",
        ) !== event.currentTarget
      )
        return;
      suppressTouchClick = true;
      cancelCompatibilityClick();
      cancelPress();
      if (
        event.touches.length !== 1 ||
        event.currentTarget.matches(":disabled")
      )
        return;
      const touch = event.touches[0];
      press = { id: touch.identifier, x: touch.clientX, y: touch.clientY };
      // The second finger may land outside this button and lift first.
      touchDocument = event.currentTarget.ownerDocument;
      touchDocument.addEventListener("touchstart", cancelMultitouch, {
        passive: true,
      });
      // Keep the keyboard stable, except where native scrolling or a long
      // press context menu must remain available.
      if (!canScrollFrom(event.currentTarget) && !props.onContextMenu)
        event.preventDefault();
    }) satisfies JSX.EventHandler<T, TouchEvent>,
    "on:touchmove": ((event) => {
      const touch = Array.from(event.touches).find(
        (t) => t.identifier === press?.id,
      );
      if (event.touches.length !== 1 || !touch || moved(touch)) cancelPress();
    }) satisfies JSX.EventHandler<T, TouchEvent>,
    "on:touchcancel": cancelPress,
    "on:touchend": ((event) => {
      const touch = Array.from(event.changedTouches).find(
        (t) => t.identifier === press?.id,
      );
      const isTap = touch && !moved(touch);
      cancelPress();
      const button = event.currentTarget;
      if (!isTap || event.touches.length || button.matches(":disabled")) return;
      const box = button.getBoundingClientRect();
      if (
        touch.clientX < box.left ||
        touch.clientX > box.right ||
        touch.clientY < box.top ||
        touch.clientY > box.bottom
      )
        return;
      event.preventDefault();
      dispatchingTouchClick = true;
      try {
        button.click();
      } finally {
        dispatchingTouchClick = false;
      }
      suppressCompatibilityClickAt(button.ownerDocument, touch);
    }) satisfies JSX.EventHandler<T, TouchEvent>,
    onClick: ((event) => {
      if (suppressTouchClick && !dispatchingTouchClick && event.detail !== 0) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      const handler =
        (dispatchingTouchClick && props.onTouchClick) || props.onClick;
      if (typeof handler === "function") handler(event);
      else handler?.[0](handler[1], event);
      props.onActivate?.();
    }) satisfies JSX.EventHandler<T, MouseEvent>,
  };
}
