import { useSyncExternalStore } from "react";
import type { YasSession, SessionId } from "@yas-run/core";
import { useRequiredYasWorkspace } from "../YasContext";

export function useYasSession(sessionId: SessionId | null): YasSession | null {
  const workspace = useRequiredYasWorkspace();
  const snapshot = useSyncExternalStore(
    workspace.subscribe,
    workspace.getSnapshot,
    workspace.getSnapshot,
  );

  if (!sessionId) return null;
  return snapshot.sessions.find((session) => session.id === sessionId) ?? null;
}

export function useYasFocusedSession(): YasSession | null {
  const workspace = useRequiredYasWorkspace();
  const snapshot = useSyncExternalStore(
    workspace.subscribe,
    workspace.getSnapshot,
    workspace.getSnapshot,
  );

  if (!snapshot.focusedSessionId) return null;
  return (
    snapshot.sessions.find(
      (session) => session.id === snapshot.focusedSessionId,
    ) ?? null
  );
}
