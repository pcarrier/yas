import type { SessionId } from "@yas-run/core";

/**
 * A layout only owns terminal focus when its focused pane contains a terminal.
 * Surfaces, IDE tiles, web panes, and empty panes leave the last terminal focus
 * alone: YasWorkspace deliberately resolves a null focus back to a live native
 * terminal on its next connection snapshot.
 */
export function terminalFocusRequest(
  paneSessionId: SessionId | null,
  workspaceSessionId: SessionId | null,
): SessionId | null {
  if (paneSessionId === null || paneSessionId === workspaceSessionId)
    return null;
  return paneSessionId;
}
