import { describe, expect, it, vi } from "vitest";
import type { YasWorkspace } from "@yas-run/core";
import {
  removeOwnedWorkspaceConnection,
  removeOwnedWorkspaceConnections,
} from "../workspaceConnectionOwnership";

function fakeWorkspace(ids: readonly string[]) {
  const remaining = [...ids];
  const removeConnection = vi.fn(
    (id: string, options?: { closeTransport?: boolean }) => {
      const index = remaining.indexOf(id);
      if (index >= 0) remaining.splice(index, 1);
      return options;
    },
  );
  const workspace = {
    getSnapshot: () => ({
      connections: remaining.map((id) => ({ id })),
    }),
    removeConnection,
  } as unknown as YasWorkspace;
  return { workspace, remaining, removeConnection };
}

describe("workspace transport ownership", () => {
  it("disposes session protocol consumers without closing App-owned transports", () => {
    const { workspace, remaining, removeConnection } = fakeWorkspace([
      "local",
      "remote",
    ]);

    removeOwnedWorkspaceConnections(workspace, "external");

    expect(remaining).toEqual([]);
    expect(removeConnection.mock.calls).toEqual([
      ["local", { closeTransport: false }],
      ["remote", { closeTransport: false }],
    ]);
  });

  it("retains closing behavior for standalone Workspace owners", () => {
    const { workspace, removeConnection } = fakeWorkspace(["local"]);

    removeOwnedWorkspaceConnection(workspace, "local", "workspace");

    expect(removeConnection).toHaveBeenCalledWith("local", {
      closeTransport: true,
    });
  });
});
