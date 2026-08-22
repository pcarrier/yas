import { describe, expect, it, vi } from "vitest";
import type { WorkspaceSessionWorkspace } from "@yas-run/core";
import {
  WorkspaceSessionPatchSequencer,
  workspaceSessionPatch,
} from "../workspaceSessionPersistence";

function state(): WorkspaceSessionWorkspace {
  return {
    layout: null,
    assignments: { "1": "pty:dev:2", "0": "pty:dev:1" },
    focusedPaneId: null,
    main: null,
    panels: {
      leftOpen: false,
      previewOpen: true,
      expandedSections: ["log", "explorer"],
      project: { kind: "focused" },
      musterExpanded: false,
      debugOpen: false,
    },
  };
}

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("workspace semantic persistence", () => {
  it("persists solo and an explicit restore-all without changing other fields", () => {
    const stored = state();
    const soloed = { ...stored, soloedPaneId: "1" };
    expect(workspaceSessionPatch(stored, soloed)).toEqual({
      workspace: { soloedPaneId: "1" },
    });
    expect(workspaceSessionPatch(soloed, stored)).toEqual({
      workspace: { soloedPaneId: null },
    });
    expect(
      workspaceSessionPatch(stored, { ...stored, soloedPaneId: null }),
    ).toBeNull();
  });

  it("persists and clears parked frames while ignoring reference order", () => {
    const stored = state();
    const current = state();
    current.parkedPlacements = {
      "surface:dev:7": {
        mode: "floating",
        rect: { x: 17, y: 23, width: 43, height: 51 },
      },
      "terminal:dev:9": { mode: "tiled" },
    };
    expect(workspaceSessionPatch(stored, current)).toEqual({
      workspace: { parkedPlacements: current.parkedPlacements },
    });
    expect(
      workspaceSessionPatch(current, {
        ...current,
        parkedPlacements: Object.fromEntries(
          Object.entries(current.parkedPlacements).reverse(),
        ),
      }),
    ).toBeNull();
    expect(workspaceSessionPatch(current, stored)).toEqual({
      workspace: { parkedPlacements: {} },
    });
  });
  it("does not patch reordered maps or set-like panel arrays", () => {
    const stored = state();
    const current = state();
    current.assignments = { "0": "pty:dev:1", "1": "pty:dev:2" };
    current.panels.expandedSections = ["explorer", "log"];
    expect(workspaceSessionPatch(stored, current)).toBeNull();
  });

  it("patches only independently changed workspace and panel fields", () => {
    const stored = state();
    const current = state();
    current.layout = {
      name: "Dev",
      root: {
        type: "split",
        direction: "horizontal",
        children: [
          { node: { type: "leaf" }, weight: 1 },
          { node: { type: "leaf" }, weight: 1 },
        ],
      },
    };
    current.panels.debugOpen = true;
    expect(workspaceSessionPatch(stored, current)).toEqual({
      workspace: {
        layout: {
          name: "Dev",
          root: {
            type: "split",
            direction: "horizontal",
            children: [
              { node: { type: "leaf" }, weight: 1 },
              { node: { type: "leaf" }, weight: 1 },
            ],
          },
        },
        panels: { debugOpen: true },
      },
    });
  });

  it("can explicitly clear nullable fields", () => {
    const stored = state();
    stored.main = "pty:dev:1";
    stored.focusedPaneId = "0";
    expect(workspaceSessionPatch(stored, state())).toEqual({
      workspace: { main: null },
    });
  });

  it("does not publish client-local focus", () => {
    const stored = state();
    const current = state();
    current.focusedPaneId = "1.0";
    expect(workspaceSessionPatch(stored, current)).toBeNull();
  });

  it("serializes same-field changes so an older CAS cannot land last", async () => {
    const baseline = state();
    const first = state();
    first.panels.debugOpen = true;
    const latest = state();
    latest.panels.debugOpen = false;
    const gates = [deferred(), deferred()];
    const calls: unknown[] = [];
    let active = 0;
    let maxActive = 0;
    const target = {
      patch: (patch: unknown) => {
        calls.push(patch);
        active++;
        maxActive = Math.max(maxActive, active);
        return gates[calls.length - 1]!.promise.finally(() => active--);
      },
    };
    const sequencer = new WorkspaceSessionPatchSequencer();
    sequencer.reset(target, baseline);

    sequencer.submit(target, first);
    sequencer.submit(target, latest);
    expect(calls).toHaveLength(1);

    gates[0]!.resolve();
    await gates[0]!.promise;
    await vi.waitFor(() =>
      expect(calls).toEqual([
        { workspace: { panels: { debugOpen: true } } },
        { workspace: { panels: { debugOpen: false } } },
      ]),
    );
    expect(maxActive).toBe(1);

    gates[1]!.resolve();
    await gates[1]!.promise;
    sequencer.dispose();
  });

  it("coalesces a newer UI state after a stale patch failure", async () => {
    const baseline = state();
    const first = state();
    first.panels.leftOpen = true;
    const latest = state();
    latest.panels.previewOpen = false;
    const gates = [deferred(), deferred()];
    const calls: unknown[] = [];
    const target = {
      patch: (patch: unknown) => {
        calls.push(patch);
        return gates[calls.length - 1]!.promise;
      },
    };
    const sequencer = new WorkspaceSessionPatchSequencer();
    sequencer.reset(target, baseline);
    sequencer.submit(target, first);
    sequencer.submit(target, latest);

    gates[0]!.reject(new Error("stale conflict"));
    await expect(gates[0]!.promise).rejects.toThrow("stale conflict");
    await vi.waitFor(() =>
      expect(calls).toEqual([
        { workspace: { panels: { leftOpen: true } } },
        { workspace: { panels: { previewOpen: false } } },
      ]),
    );

    gates[1]!.resolve();
    await gates[1]!.promise;
    sequencer.dispose();
  });

  it("retains the stored baseline while restoration stages user edits", async () => {
    const stored = state();
    const edited = state();
    edited.panels.leftOpen = true;
    const calls: unknown[] = [];
    const target = {
      patch: async (patch: unknown) => {
        calls.push(patch);
      },
    };
    const sequencer = new WorkspaceSessionPatchSequencer();

    sequencer.stage(target, stored, stored);
    sequencer.stage(target, stored, edited);
    expect(calls).toEqual([]);
    sequencer.submit(target, edited);
    await vi.waitFor(() =>
      expect(calls).toEqual([{ workspace: { panels: { leftOpen: true } } }]),
    );
    sequencer.dispose();
  });

  it("drains a newer snapshot after unmount instead of abandoning it", async () => {
    const stored = state();
    const first = state();
    first.panels.leftOpen = true;
    const latest = state();
    latest.panels.leftOpen = true;
    latest.panels.debugOpen = true;
    const gates = [deferred(), deferred()];
    const calls: unknown[] = [];
    const target = {
      patch: (patch: unknown) => {
        calls.push(patch);
        return gates[calls.length - 1]!.promise;
      },
    };
    const sequencer = new WorkspaceSessionPatchSequencer();
    sequencer.reset(target, stored);
    sequencer.submit(target, first);
    sequencer.submit(target, latest);
    sequencer.finishAfterDrain();

    gates[0]!.resolve();
    await vi.waitFor(() => expect(calls).toHaveLength(2));
    expect(calls[1]).toEqual({
      workspace: { panels: { debugOpen: true } },
    });
    gates[1]!.resolve();
    await gates[1]!.promise;
  });
});
