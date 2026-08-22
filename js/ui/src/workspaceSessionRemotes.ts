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

export interface StoredRemotePresentation {
  readonly name: string;
  readonly disabled: boolean;
}

/** Merge the durable catalogue with its enabled Relay projection.
 * Disabled catalogue entries have no Relay route by design, but they remain
 * editable rows and return in place when re-enabled. */
export function mergeWorkspaceSessionRemotes(
  routes: readonly YasRelayRoute[],
  stored: readonly StoredRemotePresentation[],
): Remote[] {
  const rows: Remote[] = [
    { name: "local", uri: "home server", disabled: false },
  ];
  const routesByName = new Map(routes.map((route) => [route.name, route]));
  const seen = new Set(["local"]);
  const append = (name: string, disabled: boolean) => {
    if (!name || seen.has(name)) return;
    seen.add(name);
    const route = routesByName.get(name);
    rows.push({
      name,
      uri: route?.description || "home-server relay",
      disabled,
    });
  };
  for (const remote of stored) append(remote.name, remote.disabled);
  for (const route of routes) append(route.name, false);
  return rows;
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

export interface WorkspaceSessionRemoteMembership {
  setRemoteActive(name: string, active: boolean): void | Promise<void>;
}

/** Expose remote membership only while a workspace is attached.
 * Passing an optional-chaining wrapper instead would make the overlay render
 * a live checkbox whose change handler silently does nothing. */
export function workspaceSessionRemoteMembershipSetter(
  session: WorkspaceSessionRemoteMembership | null | undefined,
): WorkspaceSessionRemoteMembershipSetter | undefined {
  if (!session) return undefined;
  return (name, active) => session.setRemoteActive(name, active);
}

export function setWorkspaceSessionRemoteMembership(
  setActive: WorkspaceSessionRemoteMembershipSetter | undefined,
  name: string,
  active: boolean,
): void | Promise<void> | undefined {
  return setActive?.(name, active);
}

/** Persist a catalogue entry before selecting it for the current workspace.
 * A failed catalogue write must not leave a durable workspace reference to a
 * remote that was never added. */
export async function storeAndActivateWorkspaceSessionRemote(
  store: () => void | Promise<void>,
  setActive: WorkspaceSessionRemoteMembershipSetter | undefined,
  name: string,
): Promise<void> {
  await store();
  await setWorkspaceSessionRemoteMembership(setActive, name, true);
}

/**
 * Reconcile the nested typed YAS connections selected by one stored workspace.
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

/** Catalogue rows plus unavailable names retained by the attached workspace. */
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
