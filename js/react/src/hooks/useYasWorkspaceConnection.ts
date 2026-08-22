import { useEffect } from "react";
import type {
  YasTransport,
  ConnectionId,
  TransportConfig,
} from "@yas-run/core";
import type { YasWorkspace } from "@yas-run/core";

export function useYasWorkspaceConnection(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  transport: YasTransport | TransportConfig,
): void {
  useEffect(() => {
    workspace.addConnection({ id: connectionId, transport });
    return () => workspace.removeConnection(connectionId);
  }, [workspace, connectionId, transport]);
}
