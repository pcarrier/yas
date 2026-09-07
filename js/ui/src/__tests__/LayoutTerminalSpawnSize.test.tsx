import { PALETTES } from "@yas-run/core";
import type { WorkspaceLayout } from "@yas-run/core/layout";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  LayoutContainer,
  type ReserveTerminalPane,
} from "../layout/LayoutContainer";

const snapshot = vi.hoisted(() => ({
  sessions: [] as Array<{
    id: string;
    connectionId: string;
    state: string;
    ptyId: bigint;
  }>,
  connections: [{ id: "dev", status: "connected", ready: true }],
  focusedSessionId: null as string | null,
}));

vi.mock("@yas-run/solid", () => {
  const workspace = {
    getConnection: () => null,
    setVisibleSessions: () => {},
    focusSession: () => {},
  };
  return {
    createYasWorkspace: () => workspace,
    createYasWorkspaceState: () => () => snapshot,
    createYasSessions: () => () => snapshot.sessions,
    YasTerminal: () => null,
    YasSurfaceView: () => null,
  };
});

let dispose: (() => void) | undefined;

beforeEach(() => {
  snapshot.sessions.splice(0);
  snapshot.focusedSessionId = null;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    font: "",
    measureText: (text: string) => ({
      width: text === "Mg" ? 20 : text.length * 10,
      fontBoundingBoxAscent: 15,
      fontBoundingBoxDescent: 5,
    }),
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
    function (this: HTMLElement) {
      if (!this.hasAttribute("data-yas-pane-id")) return 800;
      const count = document.querySelectorAll("[data-yas-pane-id]").length;
      return count > 1 ? 400 : 800;
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  document.body.replaceChildren();
});

function mountLayout(occupied: boolean): {
  reserve: () => ReserveTerminalPane;
  assignments: () => Readonly<Record<string, string | null>>;
} {
  if (occupied) {
    snapshot.sessions.push({
      id: "old",
      connectionId: "dev",
      state: "running",
      ptyId: 1n,
    });
    snapshot.focusedSessionId = "old";
  }
  const [layout, setLayout] = createSignal<WorkspaceLayout>({
    name: "Spawn sizing",
    root: { type: "leaf" },
  });
  let reserve: ReserveTerminalPane | undefined;
  let assignments: Readonly<Record<string, string | null>> = {};
  dispose = render(
    () => (
      <LayoutContainer
        layout={layout()}
        onLayoutChange={(next) => next && setLayout(next)}
        connectionId="dev"
        palette={PALETTES[0]}
        fontFamily="monospace"
        fontSize={14}
        focusedSessionId={occupied ? "old" : null}
        lruSessionIds={occupied ? ["old"] : []}
        onFocusSession={() => {}}
        onAssignmentsChange={(next) => {
          assignments = next.assignments;
        }}
        onReserveTerminalPane={(fn) => {
          reserve = fn;
        }}
      />
    ),
    document.body,
  );
  return {
    reserve: () => {
      if (!reserve) throw new Error("reservation callback was not registered");
      return reserve;
    },
    assignments: () => assignments,
  };
}

it("measures an existing empty pane before terminal CREATE", async () => {
  const mounted = mountLayout(false);
  const reservation = await mounted.reserve()("0");

  expect(reservation).toMatchObject({ rows: 20, cols: 80 });
  reservation?.cancel();
  expect(document.querySelectorAll("[data-yas-pane-id]")).toHaveLength(1);
});

it("measures the final split and rolls it back when CREATE fails", async () => {
  const mounted = mountLayout(true);
  const reservation = await mounted.reserve()("0", "split");

  expect(reservation).toMatchObject({ rows: 20, cols: 40 });
  expect(document.querySelectorAll("[data-yas-pane-id]")).toHaveLength(2);
  expect(Object.values(mounted.assignments())).toEqual(["old", null]);

  reservation?.cancel();
  expect(document.querySelectorAll("[data-yas-pane-id]")).toHaveLength(1);
  expect(mounted.assignments()).toEqual({ "0": "old" });
});

it("commits the created terminal into the pane that was measured", async () => {
  const mounted = mountLayout(true);
  const reservation = await mounted.reserve()("0", "split");

  expect(reservation?.commit("new")).toBe(true);
  expect(Object.values(mounted.assignments())).toEqual(["old", "new"]);
  expect(document.querySelectorAll("[data-yas-pane-id]")).toHaveLength(2);
});
