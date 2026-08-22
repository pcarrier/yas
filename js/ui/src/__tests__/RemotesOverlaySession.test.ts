import { describe, expect, it, vi } from "vitest";
import {
  setWorkspaceSessionRemoteMembership,
  storeAndActivateWorkspaceSessionRemote,
  workspaceSessionRemoteMembershipSetter,
  workspaceSessionRemoteRows,
} from "../workspaceSessionRemotes";

describe("RemotesOverlay workspace-session actions", () => {
  it("does not expose session controls without an attached session", async () => {
    expect(workspaceSessionRemoteMembershipSetter(undefined)).toBeUndefined();

    const setRemoteActive = vi.fn();
    const setActive = workspaceSessionRemoteMembershipSetter({
      setRemoteActive,
    });
    await setActive?.("prod", true);
    expect(setRemoteActive).toHaveBeenCalledWith("prod", true);
  });

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

  it("activates a newly stored remote and does not activate a failed add", async () => {
    const order: string[] = [];
    const setActive = vi.fn(async (name: string, active: boolean) => {
      order.push(`active:${name}:${active}`);
    });

    await storeAndActivateWorkspaceSessionRemote(
      async () => {
        order.push("stored");
      },
      setActive,
      "default",
    );
    expect(order).toEqual(["stored", "active:default:true"]);

    setActive.mockClear();
    await expect(
      storeAndActivateWorkspaceSessionRemote(
        async () => {
          throw new Error("catalogue write failed");
        },
        setActive,
        "broken",
      ),
    ).rejects.toThrow("catalogue write failed");
    expect(setActive).not.toHaveBeenCalled();
  });
});
