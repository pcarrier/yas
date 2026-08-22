import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { YasSurfaceView } from "../YasSurfaceView";
import { YasWorkspace } from "@yas-run/core";
import { YasWorkspaceProvider } from "../YasContext";
import type { YasWasmModule } from "@yas-run/core";
import { MockYasTransport } from "../../../core/src/__tests__/mock-yas-transport";

const mockAttach = vi.fn();
const mockDispose = vi.fn();
const mockSetTouchMode = vi.fn();
const mockSetDisplaySize = vi.fn();
const mockRequestResize = vi.fn();
/** `resizable` as the component passed it to the canvas constructor. */
let constructedResizable: boolean | undefined;

// Mock only YasSurfaceCanvas — `driveSurfaceResize` is the real thing, since
// whether the view reports a display size at all is exactly what is under test.
vi.mock("@yas-run/core", async () => {
  const actual =
    await vi.importActual<typeof import("@yas-run/core")>("@yas-run/core");
  return {
    ...actual,
    detectCodecSupport: vi.fn(),
    YasSurfaceCanvas: class {
      canvasElement = null;
      surfaceInfo = undefined;
      constructor(options: { resizable?: boolean }) {
        constructedResizable = options.resizable;
      }
      attach = mockAttach;
      dispose = mockDispose;
      setTouchMode = mockSetTouchMode;
      setDisplaySize = mockSetDisplaySize;
      requestResize = mockRequestResize;
    },
  };
});

const wasm = {
  Terminal: class {},
} as unknown as YasWasmModule;

function setup() {
  const transport = new MockYasTransport();
  const workspace = new YasWorkspace({
    wasm,
    connections: [{ id: "c1", transport }],
  });
  return { workspace };
}

function renderView(props: Record<string, unknown> = {}) {
  const { workspace } = setup();
  return render(
    <YasWorkspaceProvider workspace={workspace}>
      <YasSurfaceView connectionId="c1" surfaceId={1n} {...props} />
    </YasWorkspaceProvider>,
  );
}

describe("YasSurfaceView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    constructedResizable = undefined;
    // jsdom reports a 0x0 box, which the driver correctly ignores. Give every
    // container a real one so the initial measurement lands.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      width: 800,
      height: 600,
      left: 0,
      top: 0,
      right: 800,
      bottom: 600,
    } as DOMRect);
    vi.stubGlobal("devicePixelRatio", 1);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("owns its surface's size by default", () => {
    // Regression: this component never called setDisplaySize, so a documented
    // full-window React pane was an inert 15fps preview — every pointer,
    // wheel, keyboard and IME path in the canvas is gated on having one.
    renderView();
    expect(constructedResizable).toBe(true);
    expect(mockSetDisplaySize).toHaveBeenCalledWith(800, 600, 120, 120);
    expect(mockRequestResize).toHaveBeenCalledWith(800, 600, 120);
  });

  it("stays a passive preview when asked to", () => {
    renderView({ resizable: false });
    expect(constructedResizable).toBe(false);
    expect(mockSetDisplaySize).not.toHaveBeenCalled();
    expect(mockRequestResize).not.toHaveBeenCalled();
  });

  it("applies a relative zoom to the surface scale", () => {
    renderView({ zoom: 1.5 });
    // The pane still holds 800x600 device pixels; only the scale moves.
    expect(mockSetDisplaySize).toHaveBeenCalledWith(800, 600, 180, 120);
  });

  it("re-applies the box when zoom changes, without rebuilding the canvas", () => {
    const { rerender } = renderView({ zoom: 1 });
    const { workspace } = setup();
    mockSetDisplaySize.mockClear();

    rerender(
      <YasWorkspaceProvider workspace={workspace}>
        <YasSurfaceView connectionId="c1" surfaceId={1} zoom={2} />
      </YasWorkspaceProvider>,
    );
    // A zoom change must not tear the stream down: no new canvas, no dispose.
    expect(mockSetDisplaySize).toHaveBeenCalledWith(800, 600, 240, 120);
  });

  it("hands the surface back on unmount", () => {
    const { unmount } = renderView();
    mockSetDisplaySize.mockClear();
    unmount();
    expect(mockSetDisplaySize).toHaveBeenCalledWith(null);
    expect(mockDispose).toHaveBeenCalled();
  });

  it("clips a logical surface to its own view", () => {
    const { container } = renderView();
    const view = container.firstElementChild as HTMLElement;
    expect(view.style.position).toBe("relative");
    expect(view.style.overflow).toBe("hidden");
  });
});
