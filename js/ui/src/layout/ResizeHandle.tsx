import { createSignal } from "solid-js";
import { createPointerDrag } from "../pointerDrag";

// A divider must not consume layout space: even two pixels between every pair
// of panes reads as a large grid of gaps. Keep a generous pointer target that
// overlays both neighbours and reveal only its one-pixel centre line on hover.
const HIT_TARGET_SIZE = 10;

export function ResizeHandle(props: {
  direction: "horizontal" | "vertical";
  onDrag: (fraction: number) => void;
  /** Element whose full extent defines the reported fraction. Useful when
   * the handle lives in an absolutely-positioned overlay. */
  measureElement?: () => HTMLElement | null;
}) {
  const [active, setActive] = createSignal(false);
  const [hover, setHover] = createSignal(false);

  const startDrag = createPointerDrag();
  let host: HTMLDivElement | undefined;

  function handlePointerDown(e: PointerEvent) {
    const isHoriz = props.direction === "horizontal";
    let startPos = isHoriz ? e.clientX : e.clientY;
    const container = props.measureElement?.() ?? host?.parentElement;
    const containerSize = container
      ? isHoriz
        ? container.clientWidth
        : container.clientHeight
      : 1;

    const onMove = (me: PointerEvent) => {
      const current = isHoriz ? me.clientX : me.clientY;
      const delta = current - startPos;
      startPos = current;
      props.onDrag(delta / containerSize);
    };

    if (startDrag(e, onMove, () => setActive(false))) setActive(true);
  }

  const isHoriz = () => props.direction === "horizontal";
  const bg = () =>
    active()
      ? "rgba(128,128,128,0.5)"
      : hover()
        ? "rgba(128,128,128,0.3)"
        : "transparent";

  return (
    <div
      ref={host}
      style={{
        "flex-shrink": 0,
        width: isHoriz() ? "0" : "100%",
        height: isHoriz() ? "100%" : "0",
        position: "relative",
        "z-index": 1,
      }}
    >
      <div
        onPointerDown={handlePointerDown}
        onPointerEnter={() => setHover(true)}
        onPointerLeave={() => setHover(false)}
        style={{
          position: "absolute",
          left: isHoriz() ? `${-HIT_TARGET_SIZE / 2}px` : "0",
          top: isHoriz() ? "0" : `${-HIT_TARGET_SIZE / 2}px`,
          width: isHoriz() ? `${HIT_TARGET_SIZE}px` : "100%",
          height: isHoriz() ? "100%" : `${HIT_TARGET_SIZE}px`,
          cursor: isHoriz() ? "col-resize" : "row-resize",
          background: isHoriz()
            ? `linear-gradient(to right, transparent calc(50% - 0.5px), ${bg()} calc(50% - 0.5px), ${bg()} calc(50% + 0.5px), transparent calc(50% + 0.5px))`
            : `linear-gradient(to bottom, transparent calc(50% - 0.5px), ${bg()} calc(50% - 0.5px), ${bg()} calc(50% + 0.5px), transparent calc(50% + 0.5px))`,
          transition: "background 0.1s",
          "touch-action": "none",
        }}
      />
    </div>
  );
}
