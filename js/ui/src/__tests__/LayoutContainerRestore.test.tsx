import type { LayoutNode, WorkspaceLayout } from "@yas-run/core/layout";
import { PALETTES } from "@yas-run/core";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LayoutContainer } from "../layout/LayoutContainer";
import { terminalWorkspaceRef, type LayoutAssignments } from "../layout/store";

vi.mock("@yas-run/solid", () => {
  const sessions = [
    { id: "parked", connectionId: "dev", state: "running", ptyId: 7n },
  ];
  const snapshot = { sessions, connections: [], focusedSessionId: "parked" };
  const workspace = { getConnection: () => null, setVisibleSessions: () => {} };
  return {
    createYasWorkspace: () => workspace,
    createYasWorkspaceState: () => () => snapshot,
    createYasSessions: () => () => sessions,
    YasTerminal: () => null,
    YasSurfaceView: () => null,
  };
});

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

describe("layout assignment hydration", () => {
  it.each<{
    name: string;
    stored: Readonly<Record<string, string>> | undefined;
    expected: string | null;
  }>([
    {
      name: "leaves parked terminals out of an empty saved layout",
      stored: {},
      expected: null,
    },
    {
      name: "restores an explicitly assigned terminal",
      stored: { "0": terminalWorkspaceRef("dev", 7n) },
      expected: "parked",
    },
    {
      name: "populates a fresh layout without saved assignments",
      stored: undefined,
      expected: "parked",
    },
  ])("$name", ({ stored, expected }) => {
    let assignments: LayoutAssignments | undefined;
    const layout = {
      name: "Test layout",
      root: { type: "leaf" } as LayoutNode,
    } as WorkspaceLayout;
    dispose = render(
      () => (
        <LayoutContainer
          layout={layout}
          onLayoutChange={() => {}}
          connectionId="dev"
          palette={PALETTES[0]}
          fontFamily="monospace"
          fontSize={14}
          focusedSessionId="parked"
          lruSessionIds={["parked"]}
          storedAssignments={stored}
          onAssignmentsChange={(value) => {
            assignments = value;
          }}
          onFocusSession={() => {}}
        />
      ),
      document.body,
    );
    expect(assignments?.assignments).toEqual({ "0": expected });
  });
});
