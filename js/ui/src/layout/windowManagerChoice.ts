import type { WindowManager } from "@yas-run/core/layout";

// Scrolling layouts remain readable for stored workspace compatibility, but
// are not selectable while that window manager is parked.
export const WINDOW_MANAGERS = [
  "tiling",
  "floating",
] as const satisfies readonly WindowManager[];

export function nextManagerChoice(current: number, key: string): number | null {
  if (key === "Home") return 0;
  if (key === "End") return WINDOW_MANAGERS.length - 1;
  if (key === "ArrowDown" || key === "ArrowRight")
    return (current + 1) % WINDOW_MANAGERS.length;
  if (key === "ArrowUp" || key === "ArrowLeft")
    return (current - 1 + WINDOW_MANAGERS.length) % WINDOW_MANAGERS.length;
  return null;
}
