import { PALETTES } from "@yas-run/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneToolActions } from "../PaneTools";
import { LayoutContainer } from "../layout/LayoutContainer";
import {
  type LayoutNode,
  type WorkspaceLayout,
  surfaceAssignment,
  surfaceWorkspaceRefForId,
  type LayoutAssignments,
} from "../layout/store";

const { closeSurface } = vi.hoisted(() => ({ closeSurface: vi.fn() }));

vi.mock("@yas-run/solid", () => {
  const snapshot = {
    sessions: [],
    connections: [{ id: "dev", status: "connected", ready: true }],
    focusedSessionId: null,
  };
  const workspace = {
    getConnection: () => null,
    setVisibleSessions: () => {},
    closeSurface,
  };
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
  closeSurface.mockReset();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe("surface close lifecycle", () => {
  it.each<LayoutNode>([
    { type: "leaf" },
    {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    },
    {
      type: "split",
      direction: "tabs",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    },
    {
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 6, y: 6, width: 58, height: 58 },
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 10, y: 10, width: 58, height: 58 },
        },
      ],
    },
  ])("keeps %j intact until the surface leaves the catalogue", async (root) => {
    const hasSibling = root.type === "split";
    const [layout, setLayout] = createSignal<WorkspaceLayout>({
      name: "Test layout",
      root,
    });
    const [surfaceKeys, setSurfaceKeys] = createSignal(
      hasSibling ? ["dev:7", "dev:9"] : ["dev:7"],
    );
    const stored: Record<string, string> = {
      "0": surfaceWorkspaceRefForId("dev", 7n),
    };
    if (hasSibling) stored["1"] = surfaceWorkspaceRefForId("dev", 9n);
    let assignments: LayoutAssignments | undefined;
    let actions: PaneToolActions | null = null;
    let focusedPaneId: string | null = null;
    const collapse = vi.fn();
    dispose = render(
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
          liveSurfaceKeys={surfaceKeys()}
          storedAssignments={stored}
          storedFocusedPaneId="0"
          onAssignmentsChange={(value) => (assignments = value)}
          onFocusedPaneActionsChange={(value) => (actions = value)}
          onFocusedPaneChange={(value) => (focusedPaneId = value)}
          onCollapseToSingle={collapse}
          onFocusSession={() => {}}
        />
      ),
      document.body,
    );
    await Promise.resolve();

    const originalLayout = layout();
    const originalAssignments = { ...assignments!.assignments };
    const originalView = document.querySelector('[data-surface-id="7"]');
    expect(originalView).not.toBeNull();
    expect(originalAssignments["0"]).toBe(surfaceAssignment("dev", 7n));

    actions!.onClose!();
    expect(closeSurface).toHaveBeenCalledExactlyOnceWith("dev", 7n);
    // The app may acknowledge the request and then show a dialog, or cancel
    // closing altogether. A catalogue that still contains it keeps its view.
    setSurfaceKeys([...surfaceKeys()]);
    await Promise.resolve();
    expect(layout()).toBe(originalLayout);
    expect(assignments!.assignments).toEqual(originalAssignments);
    expect(focusedPaneId).toBe("0");
    expect(document.querySelector('[data-surface-id="7"]')).toBe(originalView);
    expect(collapse).not.toHaveBeenCalled();

    setSurfaceKeys(hasSibling ? ["dev:9"] : []);
    await Promise.resolve();
    expect(document.querySelector('[data-surface-id="7"]')).toBeNull();
    expect(Object.values(assignments!.assignments)).toEqual([
      hasSibling ? surfaceAssignment("dev", 9n) : null,
    ]);
  });
});
