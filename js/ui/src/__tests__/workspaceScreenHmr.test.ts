import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  claimWorkspaceScreenHmr,
  type WorkspaceScreenHmrCache,
} from "../workspaceScreenHmr";
import {
  observeTopLevelSurface,
  surfacePlacementIdentity,
} from "../layout/surfacePlacement";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("workspace screen hot reload", () => {
  it("keeps parked surfaces parked while admitting new windows after remount", () => {
    const cache: WorkspaceScreenHmrCache = new WeakMap();
    const workspace = {};
    const first = claimWorkspaceScreenHmr(cache, workspace, "work");
    const surface = { connectionId: "dev", surfaceId: 7n };
    const identity = surfacePlacementIdentity(surface, 10n);
    expect(
      observeTopLevelSurface(first.state.knownTopLevels, identity, true),
    ).toBe(true);

    // Parking the last window leaves an explicitly empty layout. The backend
    // debounce may still be pending when HMR disposes this screen.
    first.state.snapshot = {
      layout: { name: "Tiling", root: { type: "leaf" } },
      assignments: {},
      focusedPaneId: "0",
      main: null,
      panels: {
        leftOpen: false,
        previewOpen: true,
        expandedSections: [],
        project: { kind: "focused" },
        musterExpanded: false,
        debugOpen: false,
      },
    };
    first.release();
    const next = claimWorkspaceScreenHmr(cache, workspace, "work");
    vi.runAllTimers();

    expect(next.state.snapshot).toEqual(first.state.snapshot);
    expect(
      observeTopLevelSurface(next.state.knownTopLevels, identity, true),
    ).toBe(false);
    expect(
      observeTopLevelSurface(next.state.knownTopLevels, "dev:10:8", true),
    ).toBe(true);
    expect(
      observeTopLevelSurface(
        next.state.knownTopLevels,
        surfacePlacementIdentity(surface, 11n),
        true,
      ),
    ).toBe(true);
    expect(cache.get(workspace)).toBe(next.state);
  });

  it("retains arrivals still waiting for restored panes or layout callbacks", () => {
    const cache: WorkspaceScreenHmrCache = new WeakMap();
    const workspace = {};
    const first = claimWorkspaceScreenHmr(cache, workspace, null);
    first.state.pendingPlacements.add("surface:dev:7");
    first.state.deferredRestoredPlacements.add("surface:dev:8");
    first.release();
    const next = claimWorkspaceScreenHmr(cache, workspace, null);
    expect([...next.state.pendingPlacements]).toEqual(["surface:dev:7"]);
    expect([...next.state.deferredRestoredPlacements]).toEqual([
      "surface:dev:8",
    ]);
    next.release();
    vi.runAllTimers();
    expect(cache.has(workspace)).toBe(false);
  });

  it("does not transfer state across workspace attachments or transports", () => {
    const cache: WorkspaceScreenHmrCache = new WeakMap();
    const workspace = {};
    const first = claimWorkspaceScreenHmr(cache, workspace, "work");
    first.state.knownTopLevels.add("dev:10:7");
    first.release();
    const other = claimWorkspaceScreenHmr(cache, workspace, "other");
    vi.runAllTimers();
    expect(other.state.knownTopLevels.size).toBe(0);
    expect(cache.get(workspace)).toBe(other.state);
    expect(
      claimWorkspaceScreenHmr(cache, {}, "work").state.knownTopLevels.size,
    ).toBe(0);
  });
});
