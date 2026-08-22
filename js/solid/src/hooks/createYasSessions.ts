import type { YasSession, YasWorkspace } from "@yas-run/core";
import { createYasWorkspaceState } from "./createYasWorkspace";

export function createYasSessions(
  workspace?: YasWorkspace,
): () => readonly YasSession[] {
  const snapshot = createYasWorkspaceState(workspace);
  return () => snapshot().sessions;
}
