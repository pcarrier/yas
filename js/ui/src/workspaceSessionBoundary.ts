import type { Accessor } from "solid-js";
import type { WorkspaceSessionBinding } from "./workspaceSession";

export interface WorkspaceSessionBoundary {
  /** False for standalone/embed callers that do not use backend sessions. */
  readonly managed: boolean;
  readonly current: Accessor<WorkspaceSessionBinding | null>;
}

/**
 * Distinguish an omitted session feature from a managed controller that
 * currently has no selected tab. Both otherwise read as undefined/null in
 * JSX, but embed mode must continue rendering its ordinary workspace.
 */
export function workspaceSessionBoundary(
  source:
    | WorkspaceSessionBinding
    | Accessor<WorkspaceSessionBinding | null>
    | undefined,
): WorkspaceSessionBoundary {
  if (source === undefined) {
    return { managed: false, current: () => null };
  }
  return {
    managed: true,
    current: typeof source === "function" ? source : () => source,
  };
}
