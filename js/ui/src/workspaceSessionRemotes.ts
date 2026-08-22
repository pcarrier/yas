import {
  YAS_RELAY_AVAILABILITY_UNAVAILABLE,
  type YasRelayRoute,
  type YasWorkspaceConnection,
} from "@yas-run/core";
import { RelayConnectionCache } from "./relayTransportCache";

/** Presentation-only Relay route. Connector URIs remain server-side. */
export interface Remote {
  readonly name: string;
  readonly uri: string;
  readonly disabled: boolean;
}

export interface WorkspaceSessionRemoteOption {
  readonly name: string;
  readonly label: string;
  readonly available: boolean;
}

export interface WorkspaceSessionRelayConnection {
  readonly id: string;
  readonly label: string;
  readonly connection: YasWorkspaceConnection;
}

export type WorkspaceSessionRemoteMembershipSetter = (
  name: string,
  active: boolean,
) => void | Promise<void>;

export function setWorkspaceSessionRemoteMembership(
  setActive: WorkspaceSessionRemoteMembershipSetter | undefined,
  name: string,
  active: boolean,
): void | Promise<void> | undefined {
  return setActive?.(name, active);
}

/**
 * Reconcile the nested typed YAS connections selected by one stored session.
 * The input active-name collection is read-only and may contain routes absent
 * from the current catalogue; those names are persistence state, not cache
 * entries, and are deliberately left untouched.
 */
export function reconcileWorkspaceSessionRelayConnections(
  routes: readonly YasRelayRoute[],
  activeRemotes: readonly string[],
  cache: RelayConnectionCache,
  createConnection: ((route: YasRelayRoute) => YasWorkspaceConnection) | null,
): WorkspaceSessionRelayConnection[] {
  const active = new Set(activeRemotes);
  const seen = new Set<string>();
  const connections: WorkspaceSessionRelayConnection[] = [];
  for (const route of routes) {
    if (route.name === "local" || !active.has(route.name)) continue;
    seen.add(route.name);
    const routeKey = `${route.handle}:${route.generation}`;
    const cached = cache.get(route.name);
    if (route.availability === YAS_RELAY_AVAILABILITY_UNAVAILABLE) {
      cache.delete(route.name);
      continue;
    }
    if (cached?.routeKey === routeKey) {
      connections.push({
        id: route.name,
        label: route.label || route.name,
        connection: cached.connection,
      });
      continue;
    }
    if (cached) cache.delete(route.name);
    if (!createConnection) continue;
    const connection = createConnection(route);
    cache.set(route.name, routeKey, connection);
    connections.push({
      id: route.name,
      label: route.label || route.name,
      connection,
    });
  }
  cache.retain(seen);
  return connections;
}

/** Catalogue rows plus unavailable names retained by the attached session. */
export function workspaceSessionRemoteRows(
  available: readonly WorkspaceSessionRemoteOption[],
  activeRemotes: readonly string[],
  localLabel: string,
): WorkspaceSessionRemoteOption[] {
  const rows: WorkspaceSessionRemoteOption[] = [
    { name: "local", label: localLabel, available: true },
  ];
  const seen = new Set(["local"]);
  for (const remote of available) {
    if (!remote.name || seen.has(remote.name)) continue;
    seen.add(remote.name);
    rows.push(remote);
  }
  for (const name of activeRemotes) {
    if (!name || seen.has(name)) continue;
    seen.add(name);
    rows.push({ name, label: name, available: false });
  }
  return rows;
}
