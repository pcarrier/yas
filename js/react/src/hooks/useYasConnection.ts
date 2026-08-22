import { useSyncExternalStore } from "react";
import type { YasConnectionSnapshot, ConnectionId } from "@yas-run/core";
import { useRequiredYasWorkspace } from "../YasContext";

export function useYasConnection(
  connectionId?: ConnectionId,
): YasConnectionSnapshot | null {
  const workspace = useRequiredYasWorkspace();
  const snapshot = useSyncExternalStore(
    workspace.subscribe,
    workspace.getSnapshot,
    workspace.getSnapshot,
  );

  if (connectionId) {
    return (
      snapshot.connections.find(
        (connection) => connection.id === connectionId,
      ) ?? null
    );
  }

  if (snapshot.connections.length === 1) {
    return snapshot.connections[0];
  }

  if (snapshot.focusedSessionId) {
    const focused = snapshot.sessions.find(
      (session) => session.id === snapshot.focusedSessionId,
    );
    if (focused) {
      return (
        snapshot.connections.find(
          (connection) => connection.id === focused.connectionId,
        ) ?? null
      );
    }
  }

  return snapshot.connections[0] ?? null;
}
