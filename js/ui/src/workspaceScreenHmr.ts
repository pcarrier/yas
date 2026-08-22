import type { WorkspaceSessionWorkspace } from "@yas-run/core";
import { claimHmrLease, deferHmrRelease, type HmrLeaseState } from "./hmrLease";

export type WorkspaceScreenHmrState = HmrLeaseState & {
  sessionId: string | null;
  snapshot: WorkspaceSessionWorkspace | null;
  knownTopLevels: Set<string>;
  pendingPlacements: Set<string>;
  deferredRestoredPlacements: Set<string>;
};

export type WorkspaceScreenHmrCache = WeakMap<object, WorkspaceScreenHmrState>;

/** Hand off UI intent synchronously, before debounced backend writes settle. */
export function claimWorkspaceScreenHmr(
  cache: WorkspaceScreenHmrCache | undefined,
  workspace: object,
  sessionId: string | null,
): { state: WorkspaceScreenHmrState; release: () => void } {
  const previous = cache?.get(workspace);
  const owner = {};
  const state = claimHmrLease<WorkspaceScreenHmrState>(
    previous?.sessionId === sessionId
      ? previous
      : {
          sessionId,
          snapshot: null,
          knownTopLevels: new Set(),
          pendingPlacements: new Set(),
          deferredRestoredPlacements: new Set(),
        },
    owner,
  );
  cache?.set(workspace, state);
  return {
    state,
    release: () => {
      if (!cache) return;
      deferHmrRelease(
        state,
        owner,
        () => cache.get(workspace) === state,
        () => {},
        () => cache.delete(workspace),
      );
    },
  };
}
