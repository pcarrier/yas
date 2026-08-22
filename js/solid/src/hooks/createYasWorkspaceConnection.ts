import { onCleanup } from "solid-js";
import type {
  YasTransport,
  YasWorkspace,
  ConnectionId,
  TransportConfig,
} from "@yas-run/core";

/**
 * Manage a connection's lifecycle within a Solid component.
 * Adds the connection on creation and removes it on cleanup.
 */
export function createYasWorkspaceConnection(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  transport: YasTransport | TransportConfig,
): void {
  workspace.addConnection({ id: connectionId, transport });
  onCleanup(() => workspace.removeConnection(connectionId));
}
