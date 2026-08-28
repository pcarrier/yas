import type { LayoutNode } from "@yas-run/core/layout";

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
        (current.direction === "floating" || current.direction === "scrolling")
      ) {
        return { ...current, children };
      }
      return children[0].node;
    }
    return { ...current, children };
  };

  return removeAt(node, path);
}
