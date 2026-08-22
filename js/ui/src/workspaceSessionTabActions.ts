import type { StoredWorkspaceSession } from "@yas-run/core";
import type { WorkspaceSessionController } from "./workspaceSession";

/** Pure UI seam used by the tab strip and its non-JSX regression tests. */
export function orderedWorkspaceSessionTabs(
  controller: WorkspaceSessionController,
): readonly StoredWorkspaceSession[] {
  return controller.attachedSessions();
}

export function selectWorkspaceSessionTab(
  controller: WorkspaceSessionController,
  id: string,
): Promise<void> {
  return controller.select(id, "push");
}

export function detachWorkspaceSessionTab(
  controller: WorkspaceSessionController,
  id: string,
): Promise<void> {
  return controller.detach(id, "replace");
}

export function openWorkspaceSessionManager(
  controller: WorkspaceSessionController,
): void {
  controller.openManager();
}

export function workspaceSessionTabKeyboardTarget(
  sessions: readonly Pick<StoredWorkspaceSession, "id">[],
  currentId: string,
  key: string,
): string | null {
  const index = sessions.findIndex((session) => session.id === currentId);
  if (index < 0 || sessions.length === 0) return null;
  if (key === "ArrowLeft") {
    return (
      sessions[(index - 1 + sessions.length) % sessions.length]?.id ?? null
    );
  }
  if (key === "ArrowRight") {
    return sessions[(index + 1) % sessions.length]?.id ?? null;
  }
  if (key === "Home") return sessions[0]?.id ?? null;
  if (key === "End") return sessions.at(-1)?.id ?? null;
  return null;
}
