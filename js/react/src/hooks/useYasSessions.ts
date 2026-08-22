import { useSyncExternalStore } from "react";
import type { YasSession } from "@yas-run/core";
import { useRequiredYasWorkspace } from "../YasContext";

export function useYasSessions(): readonly YasSession[] {
  const workspace = useRequiredYasWorkspace();
  const snapshot = useSyncExternalStore(
    workspace.subscribe,
    workspace.getSnapshot,
    workspace.getSnapshot,
  );
  return snapshot.sessions;
}
