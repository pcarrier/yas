import { enumeratePanes } from "@yas-run/core/layout";
import type { LayoutChild, LayoutLeaf, LayoutNode } from "@yas-run/core/layout";

export interface PrunedLayout {
  root: LayoutNode;
  assignments: Record<string, string | null>;
  /** Every retained old pane id mapped to its new tree path. */
  paneIdMap: ReadonlyMap<string, string>;
}

/**
 * Remove every unassigned structural leaf and collapse redundant containers.
 * If the workspace has no content at all, retain one plain leaf as the empty
 * workspace launcher; it is not allowed to coexist with an occupied pane.
 */
export function pruneUnassignedPanes(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
): PrunedLayout | null {
  const oldPanes = enumeratePanes(root);
  const retained = new Set<LayoutLeaf>(
    oldPanes
      .filter(({ id, leaf }) => assignments[id] != null || !!leaf.command)
      .map(({ leaf }) => leaf),
  );
  const launcherOnly = retained.size === 0;
  if (launcherOnly && oldPanes[0]) retained.add(oldPanes[0].leaf);
  // The pruning walk rebuilds split objects even when it retains every leaf.
  // Avoid publishing a structurally identical tree (and remounting children)
  // when there is nothing to remove.
  if (retained.size === oldPanes.length) return null;

  const prune = (node: LayoutNode, isRoot: boolean): LayoutNode | null => {
    if (node.type === "leaf") return retained.has(node) ? node : null;
    const children = node.children.flatMap((child): LayoutChild[] => {
      const kept = prune(child.node, false);
      return kept ? [{ ...child, node: kept }] : [];
    });
    if (children.length === 0) return null;
    if (children.length === 1) {
      if (
        isRoot &&
        !launcherOnly &&
        (node.direction === "floating" ||
          node.direction === "scrolling" ||
          (node.direction === "workspace" && children[0].rect != null))
      ) {
        return { ...node, children };
      }
      return children[0].node;
    }
    return { ...node, children };
  };

  const nextRoot = prune(root, true);
  if (!nextRoot) return null;
  const nextPanes = enumeratePanes(nextRoot);
  const oldIds = new Map(oldPanes.map(({ id, leaf }) => [leaf, id]));
  const paneIdMap = new Map<string, string>();
  const nextAssignments: Record<string, string | null> = {};
  for (const { id, leaf } of nextPanes) {
    const oldId = oldIds.get(leaf);
    nextAssignments[id] = oldId ? (assignments[oldId] ?? null) : null;
    if (oldId) paneIdMap.set(oldId, id);
  }

  const unchanged =
    nextRoot === root &&
    oldPanes.length === nextPanes.length &&
    oldPanes.every(({ id }) => paneIdMap.get(id) === id);
  return unchanged
    ? null
    : { root: nextRoot, assignments: nextAssignments, paneIdMap };
}

/** Show one startup hint when a managed layout has no occupants at all. */
export function showEmptyPaneHint(
  multiPane: boolean,
  hasAssignedPane: boolean,
  isFocused: boolean,
): boolean {
  return !multiPane || (!hasAssignedPane && isFocused);
}

/** Resolve the child-index pane id used by enumeratePanes to a leaf path. */
function leafPath(node: LayoutNode, paneId: string): number[] | null {
  if (node.type === "leaf") return paneId === "0" ? [] : null;
  const path = paneId.split(".").map(Number);
  if (path.some((index) => !Number.isInteger(index))) return null;
  let current: LayoutNode = node;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current.type === "leaf" ? path : null;
}

/** Remove one leaf and collapse every split left with a single child. */
export function removePaneFromLayout(
  node: LayoutNode,
  paneId: string,
): LayoutNode | null {
  const path = leafPath(node, paneId);
  if (path === null) return node;

  const removeAt = (
    current: LayoutNode,
    remaining: readonly number[],
  ): LayoutNode | null => {
    if (remaining.length === 0) return null;
    if (current.type !== "split") return current;
    const [head, ...rest] = remaining;
    if (!current.children[head]) return current;
    const replacement = removeAt(current.children[head].node, rest);
    const children = replacement
      ? current.children.map((child, index) =>
          index === head ? { ...child, node: replacement } : child,
        )
      : current.children.filter((_, index) => index !== head);
    if (children.length === 0) return null;
    // A one-window managed root is still meaningful: it owns the floating
    // frame/titlebar or the scrolling column. Nested one-child splits and a
    // tiled root remain redundant and collapse normally.
    if (children.length === 1) {
      if (
        current === node &&
        (current.direction === "floating" ||
          current.direction === "scrolling" ||
          (current.direction === "workspace" && children[0].rect != null))
      ) {
        return { ...current, children };
      }
      return children[0].node;
    }
    return { ...current, children };
  };

  return removeAt(node, path);
}
