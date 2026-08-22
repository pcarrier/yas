export type SpatialDirection = "left" | "right" | "up" | "down";

export interface SpatialPaneRect {
  paneId: string;
  left: number;
  top: number;
  right: number;
  bottom: number;
}

function center(rect: SpatialPaneRect): { x: number; y: number } {
  return {
    x: (rect.left + rect.right) / 2,
    y: (rect.top + rect.bottom) / 2,
  };
}

function perpendicularOverlap(
  current: SpatialPaneRect,
  candidate: SpatialPaneRect,
  direction: SpatialDirection,
): number {
  if (direction === "left" || direction === "right") {
    return Math.max(
      0,
      Math.min(current.bottom, candidate.bottom) -
        Math.max(current.top, candidate.top),
    );
  }
  return Math.max(
    0,
    Math.min(current.right, candidate.right) -
      Math.max(current.left, candidate.left),
  );
}

function edgeDistance(
  current: SpatialPaneRect,
  candidate: SpatialPaneRect,
  direction: SpatialDirection,
): number {
  if (direction === "left") return Math.max(0, current.left - candidate.right);
  if (direction === "right") return Math.max(0, candidate.left - current.right);
  if (direction === "up") return Math.max(0, current.top - candidate.bottom);
  return Math.max(0, candidate.top - current.bottom);
}

/**
 * Pick the pane that feels adjacent on screen. A candidate must have its
 * centre in the requested half-plane. Candidates overlapping the current
 * pane on the other axis beat diagonal candidates, then the nearest edge
 * wins. `recentPaneIds` is only a deterministic tie-breaker.
 */
export function spatialNeighbor(
  panes: readonly SpatialPaneRect[],
  currentPaneId: string,
  direction: SpatialDirection,
  recentPaneIds: readonly string[] = [],
): string | null {
  const current = panes.find((pane) => pane.paneId === currentPaneId);
  if (!current) return null;
  const currentCenter = center(current);
  const recency = new Map(
    recentPaneIds.map((paneId, index) => [paneId, index]),
  );

  const candidates = panes
    .filter((pane) => {
      if (pane.paneId === currentPaneId) return false;
      const candidateCenter = center(pane);
      if (direction === "left") return candidateCenter.x < currentCenter.x;
      if (direction === "right") return candidateCenter.x > currentCenter.x;
      if (direction === "up") return candidateCenter.y < currentCenter.y;
      return candidateCenter.y > currentCenter.y;
    })
    .map((pane, order) => {
      const candidateCenter = center(pane);
      const perpendicularDistance =
        direction === "left" || direction === "right"
          ? Math.abs(candidateCenter.y - currentCenter.y)
          : Math.abs(candidateCenter.x - currentCenter.x);
      return {
        pane,
        order,
        overlaps: perpendicularOverlap(current, pane, direction) > 0,
        edgeDistance: edgeDistance(current, pane, direction),
        perpendicularDistance,
        recency: recency.get(pane.paneId) ?? Number.MAX_SAFE_INTEGER,
      };
    });

  candidates.sort((a, b) => {
    if (a.overlaps !== b.overlaps) return a.overlaps ? -1 : 1;
    return (
      a.edgeDistance - b.edgeDistance ||
      a.perpendicularDistance - b.perpendicularDistance ||
      a.recency - b.recency ||
      a.order - b.order
    );
  });
  return candidates[0]?.pane.paneId ?? null;
}
