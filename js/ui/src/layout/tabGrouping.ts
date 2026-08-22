import {
  isSurfaceAssignment,
  type LayoutLeaf,
  type LayoutNode,
  type LayoutSplit,
} from "@yas-run/core/layout";

/**
 * Whether a parked sidebar card can turn a dragged live pane into tabs.
 *
 * A parked surface is only a preview, not a tab host. Surfaces can still host
 * a drop once they are live in a pane; terminals and tiles remain valid parked
 * targets for the sidebar shortcut.
 */
export function isParkedTabDropTarget(assignment: string): boolean {
  return !isSurfaceAssignment(assignment);
}

export interface TabInsertion {
  root: LayoutNode;
  /** The pane carrying the original occupant after the tree rewrite. */
  sourcePaneId: string;
  /** The new pane created for the parked assignment. */
  newPaneId: string;
}

function paneId(path: readonly number[]): string {
  return path.length === 0 ? "0" : path.join(".");
}

function leafPath(node: LayoutNode, id: string): number[] | null {
  if (node.type === "leaf") return id === "0" ? [] : null;
  const path = id.split(".").map(Number);
  if (path.some((index) => !Number.isInteger(index))) return null;
  let current: LayoutNode = node;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current.type === "leaf" ? path : null;
}

function nodeAtPath(
  node: LayoutNode,
  path: readonly number[],
): LayoutNode | null {
  let current = node;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current;
}

function replaceNodeAtPath(
  node: LayoutNode,
  path: readonly number[],
  replacement: LayoutNode,
): LayoutNode {
  if (path.length === 0) return replacement;
  if (node.type !== "split") return node;
  const [head, ...rest] = path;
  return {
    ...node,
    children: node.children.map((child, index) =>
      index === head
        ? {
            ...child,
            node: replaceNodeAtPath(child.node, rest, replacement),
          }
        : child,
    ),
  };
}

/**
 * Add a sibling tab beside `sourcePaneId` without moving its enclosing frame.
 *
 * If the source is already a direct child of a tabs or stacking split, append
 * to that container rather than nesting another tab bar. Otherwise replace
 * only the source leaf with a two-child tabs split. This is what lets the same
 * operation work for a fullscreen BSP leaf and for a leaf inside one floating
 * window.
 */
export function insertTabAtPane(
  root: LayoutNode,
  sourcePaneId: string,
): TabInsertion | null {
  const path = leafPath(root, sourcePaneId);
  if (path === null) return null;

  if (path.length > 0) {
    const parentPath = path.slice(0, -1);
    const parent = nodeAtPath(root, parentPath);
    if (
      parent?.type === "split" &&
      (parent.direction === "tabs" || parent.direction === "stacking")
    ) {
      const index = parent.children.length;
      const leaf: LayoutLeaf = { type: "leaf" };
      const replacement: LayoutSplit = {
        ...parent,
        children: [...parent.children, { node: leaf, weight: 1 }],
      };
      return {
        root: replaceNodeAtPath(root, parentPath, replacement),
        sourcePaneId,
        newPaneId: paneId([...parentPath, index]),
      };
    }
  }

  const source = nodeAtPath(root, path);
  if (!source || source.type !== "leaf") return null;
  const leaf: LayoutLeaf = { type: "leaf" };
  const tabs: LayoutSplit = {
    type: "split",
    direction: "tabs",
    children: [
      { node: source, weight: 1 },
      { node: leaf, weight: 1 },
    ],
  };
  return {
    root: replaceNodeAtPath(root, path, tabs),
    sourcePaneId: paneId([...path, 0]),
    newPaneId: paneId([...path, 1]),
  };
}
