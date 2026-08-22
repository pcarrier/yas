import {
  PALETTES,
  createDefaultStoredWorkspaceSession,
  parseStoredWorkspaceSession,
  type WorkspacePanePlacements,
} from "@yas-run/core";
import type {
  LayoutChild,
  LayoutRect,
  WorkspaceLayout,
} from "@yas-run/core/layout";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PaneToolActions } from "../PaneTools";
import { LayoutContainer } from "../layout/LayoutContainer";
import { workspaceSessionPatch } from "../workspaceSessionPersistence";
import {
  surfaceAssignment,
  surfaceWorkspaceRef,
  saveActiveLayoutState,
  loadActiveLayoutState,
} from "../layout/store";

vi.mock("@yas-run/solid", () => {
  const snapshot = {
    sessions: [],
    connections: [{ id: "dev", status: "connected", ready: true }],
    focusedSessionId: null,
  };
  const workspace = { getConnection: () => null, setVisibleSessions: () => {} };
  return {
    createYasWorkspace: () => workspace,
    createYasWorkspaceState: () => () => snapshot,
    createYasSessions: () => () => snapshot.sessions,
    YasTerminal: () => null,
    YasSurfaceView: (props: { surfaceId: bigint }) => (
      <div data-surface-id={String(props.surfaceId)} />
    ),
  };
});

let dispose: (() => void) | undefined;
beforeEach(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  dispose?.();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  document.body.replaceChildren();
});

it("restores solo from the workspace record and persists restoring all panes", async () => {
  const [layout, setLayout] = createSignal<WorkspaceLayout>({
    name: "Two panes",
    root: {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    },
  });
  let record = createDefaultStoredWorkspaceSession({
    id: "00000000-0000-4000-8000-000000000001",
  });
  let actions: PaneToolActions | null = null;
  let focus: string | null = null;
  let storedFocus = "1";
  const mount = () =>
    render(
      () => (
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
          storedFocusedPaneId={storedFocus}
          storedSoloedPaneId={record.workspace.soloedPaneId}
          onSoloedPaneChange={(soloedPaneId) => {
            const patch = workspaceSessionPatch(record.workspace, {
              ...record.workspace,
              soloedPaneId,
            });
            record = parseStoredWorkspaceSession(
              JSON.parse(
                JSON.stringify({
                  ...record,
                  workspace: { ...record.workspace, ...patch?.workspace },
                }),
              ),
            );
          }}
          onFocusedPaneActionsChange={(value) => {
            actions = value;
          }}
          onFocusedPaneChange={(value) => {
            focus = value;
          }}
          onFocusSession={() => {}}
        />
      ),
      document.body,
    );
  dispose = mount();
  await Promise.resolve();
  actions!.solo!.onToggle();
  expect(record.workspace.soloedPaneId).toBe("1");
  dispose();
  storedFocus = "0";
  dispose = mount();
  await Promise.resolve();
  expect(focus).toBe("1");
  expect(actions!.solo!.active).toBe(true);
  actions!.solo!.onToggle();
  expect(record.workspace.soloedPaneId).toBeNull();
  dispose();
  dispose = mount();
  await Promise.resolve();
  expect(actions!.solo!.active).toBe(false);
  actions!.solo!.onToggle();
  setLayout({ ...layout(), root: { type: "leaf" } });
  expect(record.workspace.soloedPaneId).toBeNull();
});

it.each([
  { reload: false, sole: false, resize: false },
  { reload: true, sole: false, resize: false },
  { reload: false, sole: true, resize: false },
  { reload: true, sole: true, resize: false },
  { reload: true, sole: false, resize: true },
])(
  "restores a parked floating surface (reload: $reload, sole window: $sole, resize: $resize)",
  async ({ reload, sole, resize }) => {
    let viewport = new DOMRect(0, 0, 1000, 800);
    if (resize)
      vi.spyOn(
        HTMLElement.prototype,
        "getBoundingClientRect",
      ).mockImplementation(() => viewport);
    const rect: LayoutRect = { x: 17, y: 23, width: 43, height: 51 };
    const [layout, setLayout] = createSignal<WorkspaceLayout>({
      name: "Floating surface",
      root: {
        type: "split",
        direction: "workspace",
        children: [
          ...(!sole
            ? [{ node: { type: "leaf" }, weight: 1 } as LayoutChild]
            : []),
          { node: { type: "leaf" }, weight: 1, rect },
        ],
      },
    });
    let actions: PaneToolActions | null = null;
    const currentActions = () => actions as PaneToolActions | null;
    let addManagedWindow: (assignment: string) => boolean;
    let assignments: Readonly<Record<string, string>> = sole
      ? {
          "0": surfaceWorkspaceRef("dev", 7n),
        }
      : {
          "0": surfaceWorkspaceRef("dev", 9n),
          "1": surfaceWorkspaceRef("dev", 7n),
        };
    let placements: WorkspacePanePlacements = {};
    const mount = () =>
      render(
        () => (
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
            storedAssignments={assignments}
            onUnresolvedAssignmentsChange={(value) => {
              assignments = value;
            }}
            storedParkedPlacements={placements}
            onParkedPlacementsChange={(value) => {
              placements = value;
            }}
            storedFocusedPaneId={sole ? "0" : "1"}
            onCollapseToSingle={() =>
              setLayout({ name: "Empty", root: { type: "leaf" } })
            }
            onFocusedPaneActionsChange={(value) => (actions = value)}
            onAddManagedWindow={(fn) => {
              addManagedWindow = fn;
            }}
            onFocusSession={() => {}}
          />
        ),
        document.body,
      );
    dispose = mount();
    await Promise.resolve();
    const reloadWorkspace = async () => {
      saveActiveLayoutState(
        layout(),
        assignments,
        sole ? "0" : "1",
        placements,
      );
      dispose!();
      const saved = loadActiveLayoutState()!;
      setLayout(saved.layout);
      assignments = saved.assignments;
      placements = saved.parkedPlacements!;
      dispose = mount();
      await Promise.resolve();
    };
    if (reload) await reloadWorkspace();
    expect(currentActions()?.floating?.active).toBe(true);
    const expectedRoot = structuredClone(layout().root);
    actions!.onPark!();
    await Promise.resolve();
    expect(document.querySelector('[data-surface-id="7"]')).toBeNull();
    expect(placements).toEqual({
      [surfaceWorkspaceRef("dev", 7n)]: {
        mode: "floating",
        rect,
        ...(resize
          ? { viewport: { left: 0, top: 0, width: 1000, height: 800 } }
          : {}),
      },
    });
    if (resize) viewport = new DOMRect(0, 0, 760, 800);
    if (reload) {
      await reloadWorkspace();
      expect(document.querySelector('[data-surface-id="7"]')).toBeNull();
    }
    expect(addManagedWindow!(surfaceAssignment("dev", 7n))).toBe(true);
    expect(currentActions()?.floating?.active).toBe(true);
    if (resize) {
      const root = layout().root;
      expect(root.type).toBe("split");
      if (root.type !== "split") throw new Error("missing floating workspace");
      const restored = root.children.find((child) => child.rect)?.rect;
      expect(restored).toBeDefined();
      expect((restored!.x * 760) / 100).toBeCloseTo(170);
      expect((restored!.y * 800) / 100).toBeCloseTo(184);
      expect((restored!.width * 760) / 100).toBeCloseTo(430);
      expect((restored!.height * 800) / 100).toBeCloseTo(408);
    } else expect(layout().root).toEqual(expectedRoot);
    expect(placements).toEqual({});
  },
);
