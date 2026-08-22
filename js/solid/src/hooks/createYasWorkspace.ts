import { createSignal, onCleanup } from "solid-js";
import type { YasWorkspace, YasWorkspaceSnapshot } from "@yas-run/core";
import { useRequiredYasWorkspace } from "../YasContext";

export function createYasWorkspace(): YasWorkspace {
  return useRequiredYasWorkspace();
}

export function createYasWorkspaceState(
  workspace?: YasWorkspace,
): () => YasWorkspaceSnapshot {
  const ws = workspace ?? useRequiredYasWorkspace();
  const [snapshot, setSnapshot] = createSignal(ws.getSnapshot());
  const unsub = ws.subscribe(() => setSnapshot(ws.getSnapshot()));
  onCleanup(unsub);
  return snapshot;
}
