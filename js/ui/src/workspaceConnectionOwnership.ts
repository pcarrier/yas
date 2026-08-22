import type { YasWorkspace, ConnectionId } from "@yas-run/core";

export type WorkspaceTransportOwnership = "workspace" | "external";

function closeTransport(
  ownership: WorkspaceTransportOwnership | undefined,
): boolean {
  return ownership !== "external";
}

/** Remove one protocol consumer while respecting the transport's owner. */
export function removeOwnedWorkspaceConnection(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  ownership?: WorkspaceTransportOwnership,
): void {
  workspace.removeConnection(connectionId, {
    closeTransport: closeTransport(ownership),
  });
}

/** Dispose every protocol consumer; external transport owners close later. */
export function removeOwnedWorkspaceConnections(
  workspace: YasWorkspace,
  ownership?: WorkspaceTransportOwnership,
): void {
  for (const connection of workspace.getSnapshot().connections) {
    removeOwnedWorkspaceConnection(workspace, connection.id, ownership);
  }
}
