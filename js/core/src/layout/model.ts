/** Structured workspace layouts, shared by persistence and rendering. */

export type LayoutNode = LayoutSplit | LayoutLeaf;

/** How a container arranges its children. */
export type LayoutDirection =
  | "horizontal"
  | "vertical"
  | "tabs"
  | "stacking"
  | "workspace";

export interface LayoutSplit {
  type: "split";
  direction: LayoutDirection;
  children: LayoutChild[];
}

/** A floating child's frame, in percent of the viewport. */
export interface LayoutRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LayoutChild {
  node: LayoutNode;
  weight: number;
  label?: string;
  /** Placement of a floating window under a `workspace` parent. */
  rect?: LayoutRect;
}

export interface LayoutLeaf {
  type: "leaf";
  /** Shell command to run when the pane is created. */
  command?: string;
  /** Raw font size, e.g. 14, "12px", "13pt", "80%" */
  fontSize?: number | string;
}

export interface WorkspaceLayout {
  name: string;
  root: LayoutNode;
}

export const LAYOUT_MAX_PANES = 2_048;
export const LAYOUT_MAX_DEPTH = 64;
export const LAYOUT_MAX_BYTES = 256 * 1024;
export const LAYOUT_MAX_NAME_BYTES = 256;

const encoder = new TextEncoder();

function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Layout must contain objects");
  if (Object.keys(value).some((key) => !keys.includes(key)))
    throw new Error("Unknown layout field");
  return value as Record<string, unknown>;
}

function text(value: unknown, maxBytes = LAYOUT_MAX_BYTES): string {
  if (
    typeof value !== "string" ||
    value.includes("\0") ||
    encoder.encode(value).length > maxBytes
  )
    throw new Error("Invalid layout text");
  return value;
}

function positive(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0)
    throw new Error(
      "Layout dimensions and weights must be positive finite numbers",
    );
  return value;
}

/** Validate and copy untrusted trees before they enter reactive UI state. */
export function validateLayoutNode(value: unknown): LayoutNode {
  let panes = 0;
  let nodes = 0;
  function node(value: unknown, depth: number): LayoutNode {
    if (depth > LAYOUT_MAX_DEPTH)
      throw new Error("Layout exceeds maximum depth");
    // Also bounds chains of one-child workspace containers and hostile widths.
    if (++nodes > LAYOUT_MAX_PANES * (LAYOUT_MAX_DEPTH + 1))
      throw new Error("Layout exceeds maximum node count");
    const input = record(value, [
      "type",
      "command",
      "fontSize",
      "direction",
      "children",
    ]);
    if (input.type === "leaf") {
      record(value, ["type", "command", "fontSize"]);
      if (++panes > LAYOUT_MAX_PANES)
        throw new Error("Layout exceeds maximum pane count");
      const leaf: LayoutLeaf = { type: "leaf" };
      if (input.command !== undefined) leaf.command = text(input.command);
      if (input.fontSize !== undefined) {
        if (typeof input.fontSize === "number")
          leaf.fontSize = positive(input.fontSize);
        else {
          const size = text(input.fontSize, 64);
          if (
            !/^(?:\d+(?:\.\d+)?|\.\d+)(?:px|pt|%)$/.test(size) ||
            parseFloat(size) <= 0
          )
            throw new Error("Invalid layout font size");
          leaf.fontSize = size;
        }
      }
      return leaf;
    }
    record(value, ["type", "direction", "children"]);
    if (
      input.type !== "split" ||
      !["horizontal", "vertical", "tabs", "stacking", "workspace"].includes(
        input.direction as string,
      )
    )
      throw new Error("Invalid layout container");
    const direction = input.direction as LayoutDirection;
    if (
      !Array.isArray(input.children) ||
      input.children.length < (direction === "workspace" ? 1 : 2) ||
      input.children.length > LAYOUT_MAX_PANES
    )
      throw new Error("Invalid layout child count");
    let tiledBases = 0;
    const children = input.children.map((value): LayoutChild => {
      const child = record(value, ["node", "weight", "label", "rect"]);
      const result: LayoutChild = {
        node: node(child.node, depth + 1),
        weight: positive(child.weight),
      };
      if (child.label !== undefined) result.label = text(child.label);
      if (child.rect !== undefined) {
        if (direction !== "workspace")
          throw new Error("Floating frames require a workspace container");
        const frame = record(child.rect, ["x", "y", "width", "height"]);
        const { x, y } = frame;
        const width = positive(frame.width);
        const height = positive(frame.height);
        if (
          typeof x !== "number" ||
          !Number.isFinite(x) ||
          x < -100 ||
          x > 200 ||
          typeof y !== "number" ||
          !Number.isFinite(y) ||
          y < -100 ||
          y > 200 ||
          width > 200 ||
          height > 200
        )
          throw new Error("Layout frame is out of range");
        result.rect = { x, y, width, height };
      } else if (direction === "workspace" && ++tiledBases > 1) {
        throw new Error("Workspace has more than one tiled base");
      }
      return result;
    });
    return { type: "split", direction, children };
  }
  const root = node(value, 0);
  if (encoder.encode(JSON.stringify(root)).length > LAYOUT_MAX_BYTES)
    throw new Error("Layout exceeds its byte limit");
  return root;
}

export function validateWorkspaceLayout(value: unknown): WorkspaceLayout {
  const input = record(value, ["name", "root"]);
  return {
    name: text(input.name, LAYOUT_MAX_NAME_BYTES),
    root: validateLayoutNode(input.root),
  };
}

export function leafCount(node: LayoutNode): number {
  return node.type === "leaf"
    ? 1
    : node.children.reduce((sum, child) => sum + leafCount(child.node), 0);
}

/** Compare geometry and pane configuration independently of object identity. */
export function sameLayoutTree(a: LayoutNode, b: LayoutNode): boolean {
  if (a === b) return true;
  if (a.type === "leaf")
    return (
      b.type === "leaf" && a.command === b.command && a.fontSize === b.fontSize
    );
  return (
    b.type === "split" &&
    a.direction === b.direction &&
    a.children.length === b.children.length &&
    a.children.every((child, index) => {
      const other = b.children[index];
      return (
        child.weight === other.weight &&
        child.label === other.label &&
        child.rect?.x === other.rect?.x &&
        child.rect?.y === other.rect?.y &&
        child.rect?.width === other.rect?.width &&
        child.rect?.height === other.rect?.height &&
        sameLayoutTree(child.node, other.node)
      );
    })
  );
}
