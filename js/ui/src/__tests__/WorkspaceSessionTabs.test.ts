import { describe, expect, it, vi } from "vitest";
import type { StoredWorkspaceSession } from "@yas-run/core";
import type { WorkspaceSessionController } from "../workspaceSession";
import {
  detachWorkspaceSessionTab,
  openWorkspaceSessionManager,
  orderedWorkspaceSessionTabs,
  selectWorkspaceSessionTab,
  workspaceSessionTabKeyboardTarget,
} from "../workspaceSessionTabActions";

const A = "123e4567-e89b-42d3-a456-426614174000";
const B = "123e4567-e89b-42d3-a456-426614174001";

const tabs = [
  { id: A, name: "One" },
  { id: B, name: "Two" },
] as StoredWorkspaceSession[];

describe("workspace session tab strip actions", () => {
  it("keeps durable order and uses push-select, replace-detach, and manager actions", async () => {
    const select = vi.fn(async () => {});
    const detach = vi.fn(async () => {});
    const openManager = vi.fn();
    const controller = {
      attachedSessions: () => tabs,
      select,
      detach,
      openManager,
    } as unknown as WorkspaceSessionController;

    expect(
      orderedWorkspaceSessionTabs(controller).map((tab) => tab.name),
    ).toEqual(["One", "Two"]);
    expect(workspaceSessionTabKeyboardTarget(tabs, A, "ArrowRight")).toBe(B);
    expect(workspaceSessionTabKeyboardTarget(tabs, A, "End")).toBe(B);

    await selectWorkspaceSessionTab(controller, B);
    expect(select).toHaveBeenCalledWith(B, "push");

    await detachWorkspaceSessionTab(controller, A);
    expect(detach).toHaveBeenCalledWith(A, "replace");

    openWorkspaceSessionManager(controller);
    expect(openManager).toHaveBeenCalledOnce();
  });
});
