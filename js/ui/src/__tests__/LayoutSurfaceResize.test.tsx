import { PALETTES, type YasWorkspace } from "@yas-run/core";
import type { LayoutSplit, WorkspaceLayout } from "@yas-run/core/layout";
import { YasWorkspaceProvider } from "@yas-run/solid";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { LayoutContainer } from "../layout/LayoutContainer";
import { surfaceWorkspaceRef } from "../layout/store";

const { requestResize, setDisplaySize, attach, disposeCanvas, workspace } =
  vi.hoisted(() => ({
    requestResize: vi.fn(),
    setDisplaySize: vi.fn(),
    attach: vi.fn(),
    disposeCanvas: vi.fn(),
    workspace: {
      getConnection: () => null,
      setVisibleSessions: () => {},
      subscribe: () => () => {},
    },
  }));

// Keep the actual binding and resize driver; only video decoding is fake.
vi.mock("@yas-run/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@yas-run/core")>()),
  detectCodecSupport: () => {},
  YasSurfaceCanvas: class {
    canvasElement = null;
    attach = attach;
    dispose = disposeCanvas;
    constructor(private options: { surfaceId: bigint }) {}
    requestResize(width: number, height: number, scale120: number) {
      requestResize(this.options.surfaceId, width, height, scale120);
    }
    setDisplaySize = setDisplaySize;
    setConnectionId = vi.fn();
    setSurfaceId = vi.fn();
    setLive = vi.fn();
    setTouchMode = vi.fn();
  },
}));

vi.mock("@yas-run/solid", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@yas-run/solid")>()),
  createYasWorkspace: () => workspace,
  createYasWorkspaceState: () => () => ({
    sessions: [],
    connections: [{ id: "dev", status: "connected", ready: true }],
    focusedSessionId: null,
  }),
  createYasSessions: () => () => [],
}));

let dispose: (() => void) | undefined;
let box: DOMRect;
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("devicePixelRatio", 1);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  box = new DOMRect(0, 0, 800, 600);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    () => box,
  );
});
afterEach(() => {
  dispose?.();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.clearAllMocks();
  localStorage.clear();
  document.body.replaceChildren();
});

it.each(["horizontal", "vertical", "workspace"] as const)(
  "remeasures a %s layout after geometry changes without observer delivery",
  async (direction) => {
    const [layout, setLayout] = createSignal<WorkspaceLayout>({
      name: "Surface resize",
      root: {
        type: "split",
        direction,
        children: [
          { node: { type: "leaf" }, weight: 1 },
          {
            node: { type: "leaf" },
            weight: 1,
            ...(direction === "workspace"
              ? { rect: { x: 10, y: 10, width: 40, height: 40 } }
              : {}),
          },
        ],
      },
    });
    dispose = render(
      () => (
        <YasWorkspaceProvider workspace={workspace as unknown as YasWorkspace}>
          <LayoutContainer
            layout={layout()}
            onLayoutChange={(next) => next && setLayout(next)}
            connectionId="dev"
            palette={PALETTES[0]}
            fontFamily="monospace"
            fontSize={14}
            focusedSessionId={null}
            lruSessionIds={[]}
            liveSurfaceKeys={["dev:7", "dev:9"]}
            storedAssignments={{
              "0": surfaceWorkspaceRef("dev", 7n),
              "1": surfaceWorkspaceRef("dev", 9n),
            }}
            onFocusSession={() => {}}
          />
        </YasWorkspaceProvider>
      ),
      document.body,
    );
    await Promise.resolve();
    vi.advanceTimersByTime(50);
    expect(requestResize).toHaveBeenCalledWith(7n, 800, 600, 120);
    expect(requestResize).toHaveBeenCalledWith(9n, 800, 600, 120);
    expect(attach).toHaveBeenCalledTimes(2);
    requestResize.mockClear();

    for (const width of [1000, 400, 800]) {
      const root = layout().root as LayoutSplit;
      setLayout({
        ...layout(),
        root: {
          ...root,
          children: root.children.map((child, index) =>
            index === 1
              ? {
                  ...child,
                  weight: width / 800,
                  ...(child.rect
                    ? { rect: { ...child.rect, width: width / 20 } }
                    : {}),
                }
              : child,
          ),
        },
      });
      // Layout may finish after the reactive effects ran. Measure in the
      // next frame, without waiting for an observer or a window resize.
      box = new DOMRect(0, 0, width, 600);
      vi.advanceTimersByTime(50);
      expect(requestResize).toHaveBeenCalledWith(7n, width, 600, 120);
      expect(requestResize).toHaveBeenCalledWith(9n, width, 600, 120);
      expect(requestResize).toHaveBeenCalledTimes(2);
      requestResize.mockClear();
    }
    expect(attach).toHaveBeenCalledTimes(2);
    expect(disposeCanvas).not.toHaveBeenCalled();
    expect(setDisplaySize).not.toHaveBeenCalledWith(null);
  },
);
