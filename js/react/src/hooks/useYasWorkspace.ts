import { useSyncExternalStore } from "react";
import type { YasWorkspaceSnapshot } from "@yas-run/core";
import { useRequiredYasWorkspace } from "../YasContext";

export function useYasWorkspace() {
  return useRequiredYasWorkspace();
}

export function useYasWorkspaceState(): YasWorkspaceSnapshot {
  const workspace = useRequiredYasWorkspace();
  return useSyncExternalStore(
    workspace.subscribe,
    workspace.getSnapshot,
    workspace.getSnapshot,
  );
}
