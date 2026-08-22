import type { YasSurface } from "@yas-run/core";
import {
  parseSurfaceAssignment,
  surfaceAssignment,
} from "@yas-run/core/layout";
import { parseWorkspaceRef, surfaceIdForWorkspaceRef } from "./store";

export type ManagedWindowTarget = { paneId: string; split: boolean };

/** Prefer an existing empty pane; otherwise split the focused/first pane. */
export function managedWindowTarget(
  paneIds: readonly string[],
  assignments: Readonly<Record<string, string | null | undefined>>,
  focusedPaneId: string | null,
): ManagedWindowTarget | null {
  const focused =
    focusedPaneId != null && paneIds.includes(focusedPaneId)
      ? focusedPaneId
      : null;
  const empty =
    (focused != null && assignments[focused] == null ? focused : null) ??
    paneIds.find((paneId) => assignments[paneId] == null);
  if (empty != null) return { paneId: empty, split: false };
  const paneId = focused ?? paneIds[0];
  return paneId == null ? null : { paneId, split: true };
}

/**
 * Surface handles are scoped to one server boot. Include that generation in
 * arrival identity so a restarted compositor may reuse handle 1 without the
 * browser mistaking its new window for one observed before the restart.
 */
export function surfacePlacementIdentity(
  surface: Pick<YasSurface, "connectionId" | "surfaceId">,
  bootGeneration: bigint | null,
): string {
  return `${surface.connectionId}:${bootGeneration?.toString() ?? "unknown"}:${surface.surfaceId}`;
}

/**
 * Record the first observation of a top-level surface.
 *
 * A catalogue entry may be observed as a child before its final role is known.
 * Recording that child as "seen" loses the later child→toplevel transition and
 * leaves the window parked until a page refresh rebuilds the observation set.
 */
export function observeTopLevelSurface(
  knownTopLevels: Set<string>,
  identity: string,
  isTopLevel: boolean,
): boolean {
  if (!isTopLevel || knownTopLevels.has(identity)) return false;
  knownTopLevels.add(identity);
  return true;
}

/**
 * Whether a queued placement's missing surface is authoritative.
 *
 * Reconnect and family reconfiguration clear the Surface catalogue before
 * publishing its replacement. During that gap the connection is non-ready,
 * so retiring the request would lose it: the replacement carries the same
 * boot-scoped identity and therefore is not a second first observation.
 */
export function pendingSurfacePlacementIsRetired(
  assignment: string,
  liveTopLevels: ReadonlySet<string>,
  readyConnectionIds: ReadonlySet<string>,
): boolean {
  if (liveTopLevels.has(assignment)) return false;
  const surface = parseSurfaceAssignment(assignment);
  return surface == null || readyConnectionIds.has(surface.connectionId);
}

/** Surface assignments explicitly owned by the workspace being restored. */
export function restoredSurfaceAssignments(
  storedAssignments: Readonly<Record<string, string>>,
): ReadonlySet<string> {
  const restored = new Set<string>();
  for (const stored of Object.values(storedAssignments)) {
    const ref = parseWorkspaceRef(stored);
    if (ref?.kind !== "surface") continue;
    restored.add(
      surfaceAssignment(ref.connectionId, surfaceIdForWorkspaceRef(ref)),
    );
  }
  return restored;
}
