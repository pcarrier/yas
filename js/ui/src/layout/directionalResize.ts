import type { LayoutNode, LayoutSplit } from "@yas-run/core/layout";
import { adjustWeights } from "@yas-run/core/layout";
import type { SpatialDirection } from "./spatialNavigation";

function nodeAtPath(
  root: LayoutNode,
  path: readonly number[],
): LayoutNode | null {
  let node = root;
  for (const index of path) {
    if (node.type !== "split" || !node.children[index]) return null;
    node = node.children[index].node;
  }
  return node;
}

function replaceAtPath(
  root: LayoutNode,
  path: readonly number[],
  replacement: LayoutNode,
): LayoutNode {
  if (path.length === 0) return replacement;
  if (root.type !== "split") return root;
  const [index, ...rest] = path;
  return {
    ...root,
    children: root.children.map((child, at) =>
      at === index
        ? { ...child, node: replaceAtPath(child.node, rest, replacement) }
        : child,
    ),
  };
}

/** Move the nearest boundary of `paneId` in `direction`. */
export function resizePaneInDirection(
  root: LayoutNode,
  paneId: string,
  direction: SpatialDirection,
  fraction = 0.05,
): LayoutNode | null {
  if (!Number.isFinite(fraction) || fraction <= 0) return null;
  const path = paneId.split(".").map(Number);
  const axis =
    direction === "left" || direction === "right" ? "horizontal" : "vertical";
  const towardStart = direction === "left" || direction === "up";

  for (let depth = path.length - 1; depth >= 0; depth -= 1) {
    const parentPath = path.slice(0, depth);
    const parent = nodeAtPath(root, parentPath);
    if (parent?.type !== "split" || parent.direction !== axis) continue;
    const childIndex = path[depth];
    const boundaryIndex = towardStart ? childIndex - 1 : childIndex;
    if (boundaryIndex < 0 || boundaryIndex + 1 >= parent.children.length) {
      continue;
    }
    const signedFraction = towardStart ? -fraction : fraction;
    const resized: LayoutSplit = adjustWeights(
      parent,
      boundaryIndex,
      boundaryIndex + 1,
      signedFraction,
    );
    return replaceAtPath(root, parentPath, resized);
  }
  return null;
}

/** Equalize every tiled split without changing stack or view order. */
export function balanceLayout(root: LayoutNode): LayoutNode {
  if (root.type === "leaf") return root;
  const tiled =
    root.direction === "horizontal" || root.direction === "vertical";
  return {
    ...root,
    children: root.children.map((child) => ({
      ...child,
      weight: tiled ? 1 : child.weight,
      node: balanceLayout(child.node),
    })),
  };
}
