import type { LayoutLeaf, LayoutNode, LayoutSplit } from "@yas-run/core/layout";
import { enumeratePanes } from "@yas-run/core/layout";
import { removePaneFromLayout } from "./paneRemoval";
import { insertTabAtPane } from "./tabGrouping";
import type { SpatialDirection } from "./spatialNavigation";

export interface ViewMovement {
  root: LayoutNode;
  assignments: Record<string, string | null>;
  focusedPaneId: string;
  /** Pane-id migration for pending durable references. */
  paneIdMap: ReadonlyMap<string, string>;
}

function assignmentsByLeaf(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
): Map<LayoutLeaf, string | null> {
  return new Map(
    enumeratePanes(root).map(({ id, leaf }) => [leaf, assignments[id] ?? null]),
  );
}

function finishMovement(
  oldRoot: LayoutNode,
  nextRoot: LayoutNode,
  oldAssignments: Readonly<Record<string, string | null>>,
  sourceLeaf: LayoutLeaf,
  destinationLeaf: LayoutLeaf,
): ViewMovement | null {
  const sourceValue = assignmentsByLeaf(oldRoot, oldAssignments).get(
    sourceLeaf,
  );
  if (!sourceValue) return null;
  const oldPanes = enumeratePanes(oldRoot);
  const oldIdsByLeaf = new Map(oldPanes.map(({ id, leaf }) => [leaf, id]));
  const valuesByLeaf = assignmentsByLeaf(oldRoot, oldAssignments);
  const assignments: Record<string, string | null> = {};
  const paneIdMap = new Map<string, string>();
  let focusedPaneId: string | null = null;

  for (const { id, leaf } of enumeratePanes(nextRoot)) {
    assignments[id] =
      leaf === destinationLeaf ? sourceValue : (valuesByLeaf.get(leaf) ?? null);
    if (leaf === destinationLeaf) focusedPaneId = id;
    const oldId = oldIdsByLeaf.get(leaf);
    if (oldId) paneIdMap.set(oldId, id);
  }
  const sourceOldId = oldIdsByLeaf.get(sourceLeaf);
  if (sourceOldId && focusedPaneId) paneIdMap.set(sourceOldId, focusedPaneId);
  return focusedPaneId
    ? { root: nextRoot, assignments, focusedPaneId, paneIdMap }
    : null;
}

/** Move one active view into another stack as its active tab. */
export function moveViewIntoStack(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  sourcePaneId: string,
  targetPaneId: string,
): ViewMovement | null {
  if (sourcePaneId === targetPaneId) return null;
  const sourceLeaf = enumeratePanes(root).find(
    ({ id }) => id === sourcePaneId,
  )?.leaf;
  if (!sourceLeaf || !assignments[sourcePaneId] || !assignments[targetPaneId]) {
    return null;
  }
  const inserted = insertTabAtPane(root, targetPaneId);
  if (!inserted) return null;
  const destinationLeaf = enumeratePanes(inserted.root).find(
    ({ id }) => id === inserted.newPaneId,
  )?.leaf;
  if (!destinationLeaf) return null;
  const movedSourceId = enumeratePanes(inserted.root).find(
    ({ leaf }) => leaf === sourceLeaf,
  )?.id;
  if (!movedSourceId) return null;
  const nextRoot = removePaneFromLayout(inserted.root, movedSourceId);
  if (!nextRoot) return null;
  return finishMovement(
    root,
    nextRoot,
    assignments,
    sourceLeaf,
    destinationLeaf,
  );
}

/** Move one active view to a newly-created stack at the workspace edge. */
export function moveViewToEdge(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  sourcePaneId: string,
  direction: SpatialDirection,
): ViewMovement | null {
  const sourceLeaf = enumeratePanes(root).find(
    ({ id }) => id === sourcePaneId,
  )?.leaf;
  if (!sourceLeaf || !assignments[sourcePaneId]) return null;
  const remaining = removePaneFromLayout(root, sourcePaneId);
  // Moving the only view would manufacture a second structural object without
  // changing what the user can see, so keep the single stack intact.
  if (!remaining) return null;

  const destinationLeaf: LayoutLeaf = { type: "leaf" };
  const destination = { node: destinationLeaf, weight: 1 };
  const rest = { node: remaining, weight: 1 };
  const leading = direction === "left" || direction === "up";
  const nextRoot: LayoutSplit = {
    type: "split",
    direction:
      direction === "left" || direction === "right" ? "horizontal" : "vertical",
    children: leading ? [destination, rest] : [rest, destination],
  };
  return finishMovement(
    root,
    nextRoot,
    assignments,
    sourceLeaf,
    destinationLeaf,
  );
}
