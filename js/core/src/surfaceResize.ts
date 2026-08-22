/**
 * The resizable-pane half of a surface view, shared by every framework
 * binding.
 *
 * A surface view comes in two shapes.  A *passive* one — a dock card, a
 * switcher preview — is handed a box and asks the server for a fixed
 * downscale of whatever the surface happens to be; `YasSurfaceCanvas`
 * measures that itself.  A *resizable* one owns its surface's size: it
 * reports its box as a display size, which is what puts the view into the
 * server's size mediation and, incidentally, what every input path is gated
 * on (`_displaySize`).  Without it a view is an inert preview: no pointer, no
 * wheel, no keyboard, no IME, and a 15 fps thumbnail-grade stream.
 *
 * That half used to live in each binding, which is how the React one ended up
 * never calling {@link SurfaceResizeTarget.setDisplaySize} at all — a
 * documented, full-window React pane that could not be clicked. One
 * implementation, two thin call sites.
 */

/** The part of `YasSurfaceCanvas` a resize driver needs. */
export interface SurfaceResizeTarget {
  /** Report the box, in device pixels, this view is rendering into. */
  setDisplaySize(
    width: number | null,
    height?: number,
    scale120?: number,
    cssScale120?: number,
  ): void;
  /** Ask the server to resize the surface to this view's box. */
  requestResize(width: number, height: number, scale120: number): void;
}

export interface SurfaceZoom {
  /** Zoom factor; see {@link SurfaceZoom.mode}. */
  zoom?: number;
  /** `relative` multiplies the display's DPI by `zoom`; `exact` uses `zoom`
   *  as the absolute surface scale, independent of display DPI. */
  mode?: "relative" | "exact";
}

/**
 * Clamp to a range that stays useful at both ends: below 0.25 an app is handed
 * a logical size most toolkits refuse to lay out, and above 4 one pane's
 * demand for scale would dominate every co-viewer's stream.
 */
export function clampZoom(zoom: number | undefined): number {
  if (typeof zoom !== "number" || !Number.isFinite(zoom) || zoom <= 0) return 1;
  return Math.min(4, Math.max(0.25, zoom));
}

/**
 * Short, because the server coalesces on its own: a configure opens a settle
 * window there and every size that lands inside it is folded into one
 * configure at the end. Keep forwarding the latest box during a sustained
 * drag so the server has something current to dispatch when that window
 * closes; a pure trailing-edge debounce froze the surface at the drag's first
 * size until the user let go. The same interval bounds how late the final size
 * can land after a drag ends.
 */
const RESIZE_THROTTLE_MS = 30;

/**
 * If no resize event for this long, the next one is treated as the start of a
 * fresh drag and fires immediately — so each user-visible drag gets a
 * leading-edge dispatch and the perceived reaction is bounded by RTT rather
 * than the trailing edge of the throttle.
 */
const DRAG_GAP_MS = 250;

function fallbackScale120(): number {
  return Math.round((globalThis.devicePixelRatio || 1) * 120);
}

/**
 * Drive `target`'s display size and server-side resizes from `container`'s box.
 *
 * `getZoom` is read on every measurement rather than captured, so a binding can
 * change the zoom without tearing the driver down — rebuilding it would
 * unsubscribe the view and cost a keyframe.  Call {@link reapply} when the zoom
 * changes: the box has not moved, so the observer will never fire on its own.
 * Call `remeasure` after explicit layout changes to read the final box even
 * when ResizeObserver delivery is delayed or missed.
 */
export function driveSurfaceResize(
  target: SurfaceResizeTarget,
  container: HTMLElement,
  getZoom: () => SurfaceZoom = () => ({}),
): { reapply(): void; remeasure(): void; dispose(): void } {
  const page = container.ownerDocument;
  /** The last box the observer reported, so a zoom change can be re-applied
   *  without waiting for the container to change size — it never will. */
  let lastBox: { cssW: number; cssH: number } | null = null;

  let resizeTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingResize: { w: number; h: number; scale120: number } | null = null;
  /** Negative infinity rather than 0, so the *first* box a view ever reports
   *  always counts as a drag start and dispatches at wire speed.  Anchoring at
   *  0 made that depend on when in the page's life the view happened to mount:
   *  within the first {@link DRAG_GAP_MS} of load the first size waited out the
   *  throttle interval, and after it did not. */
  let lastResizeAt = Number.NEGATIVE_INFINITY;
  let lastSentW = 0;
  let lastSentH = 0;
  let lastSentScale120 = 0;
  let disposed = false;

  const send = (w: number, h: number, scale120: number) => {
    if (w === lastSentW && h === lastSentH && scale120 === lastSentScale120)
      return;
    lastSentW = w;
    lastSentH = h;
    lastSentScale120 = scale120;
    target.requestResize(w, h, scale120);
  };

  const flushPendingResize = () => {
    resizeTimer = undefined;
    const pending = pendingResize;
    pendingResize = null;
    if (!pending || disposed) return;
    send(pending.w, pending.h, pending.scale120);
  };

  const applySize = (cssW: number, cssH: number) => {
    if (disposed) return;
    if (page.visibilityState === "hidden" || cssW <= 0 || cssH <= 0) {
      // Background tabs retain nonzero boxes, but must not constrain the page
      // the user can actually see. The same applies to a hidden pane.
      // Forget the dedup key too: showing the same box again must reclaim it.
      clearTimeout(resizeTimer);
      resizeTimer = undefined;
      pendingResize = null;
      lastSentW = lastSentH = lastSentScale120 = 0;
      lastResizeAt = Number.NEGATIVE_INFINITY;
      if (lastBox) target.setDisplaySize(null);
      lastBox = null;
      return;
    }
    // Even, because the encoder rounds each axis *down* to even on its own
    // (H.264/HEVC/AV1 NV12 sampling grids). Asking for an odd extent means the
    // frame comes back a pixel short of the pane on that axis only, so the
    // aspect no longer matches and `object-fit: contain` letterboxes the
    // difference. Giving up the odd pixel here costs nothing — it was never
    // going to carry image — and makes the server's rounding a no-op.
    const even = (n: number) => Math.max(2, n - (n % 2));
    const dpr = (globalThis.devicePixelRatio || 1) as number;
    // Recompute with the current DPR, including browser page zoom. Cached
    // physical extents belong to the old ratio. Some Chromium configurations
    // also expose CSS extents through devicePixelContentBoxSize, so use CSS
    // measurements and DPR consistently for every update.
    const w = even(Math.round(cssW * dpr));
    const h = even(Math.round(cssH * dpr));
    if (w <= 0 || h <= 0) return;
    // The container's measured device-pixel ratio, which is what converts the
    // canvas's device pixels back to a CSS box.
    const cssScale120 =
      cssW > 0 && cssH > 0
        ? Math.round(((w / cssW + h / cssH) / 2) * 120)
        : fallbackScale120();
    const { zoom, mode } = getZoom();
    const factor = clampZoom(zoom);
    // The pane always holds `w x h` device pixels. Relative zoom rides on its
    // DPI; exact zoom names the surface scale directly. A sub-1x scale is
    // meaningful: the server gives the app a larger logical window, composites
    // at Wayland's 1x floor, and downsamples the stream into this pane.
    const scale120 = Math.max(
      1,
      Math.round((mode === "exact" ? 120 : cssScale120) * factor),
    );
    target.setDisplaySize(w, h, scale120, cssScale120);
    lastBox = { cssW, cssH };
    const now = performance.now();
    const isDragStart = now - lastResizeAt > DRAG_GAP_MS;
    lastResizeAt = now;
    pendingResize = { w, h, scale120 };
    // Leading edge: first event of a new interaction dispatches at wire speed
    // so the server pipeline (configure -> repaint -> encode) starts as soon as
    // possible.
    if (isDragStart) {
      clearTimeout(resizeTimer);
      resizeTimer = undefined;
      flushPendingResize();
      return;
    }
    // Keep one timer alive instead of resetting it for every observer event.
    // Its callback forwards the newest box seen in the interval, making this a
    // latest-value throttle during a drag and its trailing-edge delivery after
    // the drag ends.
    if (resizeTimer === undefined)
      resizeTimer = setTimeout(flushPendingResize, RESIZE_THROTTLE_MS);
  };

  let observer: ResizeObserver | null = null;
  if (typeof ResizeObserver !== "undefined") {
    observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        applySize(width, height);
      }
    });
    // Browser zoom can shrink the CSS box while its physical size stays put.
    // Observing device pixels misses that change and leaves Wayland at the
    // old scale. Watch the CSS box, with a separate DPR listener below.
    observer.observe(container);
  }

  const measure = () => {
    const rect = container.getBoundingClientRect();
    applySize(rect.width, rect.height);
  };
  let lastDpr = globalThis.devicePixelRatio || 1;
  let dprQuery: MediaQueryList | null = null;
  const watchDpr = () => {
    dprQuery?.removeEventListener("change", checkDpr);
    if (
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function"
    ) {
      dprQuery = window.matchMedia(`(resolution: ${lastDpr}dppx)`);
      dprQuery.addEventListener("change", checkDpr);
    }
  };
  const checkDpr = () => {
    if (disposed) return;
    const next = globalThis.devicePixelRatio || 1;
    if (next !== lastDpr) {
      lastDpr = next;
      // A query for the old ratio only changes once when zooming further in.
      // Re-arm for each ratio so successive zoom steps and monitor moves work.
      watchDpr();
    }
    // Rotation, PWA window resizing and keyboard changes can resize the box
    // without changing DPR. These events also recover missed observer delivery.
    measure();
  };
  watchDpr();
  if (typeof window !== "undefined") {
    window.addEventListener("resize", checkDpr);
  }
  page.addEventListener("visibilitychange", measure);
  measure();

  return {
    remeasure: measure,
    /** Re-apply the last box under the current zoom. Goes through `applySize`,
     *  so it takes the same throttling and de-duplication as a drag. */
    reapply(): void {
      if (disposed || !lastBox) return;
      applySize(lastBox.cssW, lastBox.cssH);
    },
    dispose(): void {
      if (disposed) return;
      disposed = true;
      clearTimeout(resizeTimer);
      resizeTimer = undefined;
      pendingResize = null;
      observer?.disconnect();
      dprQuery?.removeEventListener("change", checkDpr);
      page.removeEventListener("visibilitychange", measure);
      if (typeof window !== "undefined") {
        window.removeEventListener("resize", checkDpr);
      }
      target.setDisplaySize(null);
    },
  };
}
