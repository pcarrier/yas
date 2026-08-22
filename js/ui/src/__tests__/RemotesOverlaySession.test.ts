import { describe, expect, it, vi } from "vitest";
import {
  setWorkspaceSessionRemoteMembership,
  workspaceSessionRemoteRows,
} from "../workspaceSessionRemotes";

describe("RemotesOverlay workspace-session actions", () => {
  it("keeps missing names removable through the independent membership action", () => {
    const setActive = vi.fn();
    const rows = workspaceSessionRemoteRows(
      [{ name: "hound", label: "HOUND", available: true }],
      ["hound", "missing"],
      "local",
    );

    expect(rows.at(-1)).toEqual({
      name: "missing",
      label: "missing",
      available: false,
    });
    setWorkspaceSessionRemoteMembership(setActive, "missing", false);
    expect(setActive).toHaveBeenCalledWith("missing", false);
  });
});
