import type { YasSession, YasWorkspace, SessionId } from "@yas-run/core";

/**
 * Look up a single session by ID from the workspace's current snapshot.
 * This is a plain function (not reactive) — call it inside a `createEffect`
 * or `createMemo` to make it reactive.
 */
export function useYasSession(
  workspace: YasWorkspace,
  sessionId: SessionId | null,
): YasSession | null {
  if (!sessionId) return null;
  const snapshot = workspace.getSnapshot();
  return snapshot.sessions.find((s) => s.id === sessionId) ?? null;
}

export function useYasFocusedSession(
  workspace: YasWorkspace,
): YasSession | null {
  const snapshot = workspace.getSnapshot();
  if (!snapshot.focusedSessionId) return null;
  return (
    snapshot.sessions.find((s) => s.id === snapshot.focusedSessionId) ?? null
  );
}
