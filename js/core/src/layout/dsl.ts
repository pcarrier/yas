/**
 * The workspace layout DSL: parser and serializer.
 *
 * Syntax:
 *   layout  = node
 *   node    = split | leaf
 *   split   = ("line" | "col" | "tabs" | "stack" | "scroll" | "float" | "scene") "(" entries ")"
 *   entries = entry ("," entry)*
 *   entry   = [label ":"] node [rect] [weight] ["=" command] ["@" fontSize]
 *   label   = identifier | quoted-string
 *   leaf    = "_"
 *   rect    = "[" number "," number "," number "," number "]"
 *   command = identifier | quoted-string
 *   weight  = number
 *   fontSize = number ["px" | "pt" | "%"]
 *   identifier = [a-zA-Z_][a-zA-Z0-9_-]*
 *
 * The root keyword is also the window manager. `line`/`col` tile every child;
 * `tabs`/`stack` select one child behind horizontal or vertical title bars.
 * `scroll` is a strip
 * that is allowed to be wider than the viewport — a weight is the column's
 * width as a fraction of the viewport, and the view follows the focus along
 * the strip (niri's model). `float` positions each child by an explicit
 * `[x,y,w,h]` rect in viewport percent, overlapping freely, last child on top.
 * `scene` is Sway's mixed model: at most one child without a rect is the tiled
 * workspace, while children with rects are floating windows above it.
 *
 * Panes are anonymous. A pane is identified by where it sits in the tree, and
 * a second name for the same thing only goes stale: it survived a layout being
 * resplit and then described the wrong pane. `_` is the pane. Splits still
 * take a label, because a tabs strip has to print something on its tabs.
 *
 * Examples:
 *   _
 *   line(_ 2, col(_, _, _))
 *   line(_ 3 @14, col(_ @11, _))
 *   tabs(_, _, _)
 *   stack(_, _, _)
 *   tabs("Editor": col(_, _), "Terminal": col(_, _))
 *   line(_ 2, tabs(_, _))
 *   col(_="htop", _="cd /src && make watch")
 */

export type LayoutNode = LayoutSplit | LayoutLeaf;

/** How a split arranges its children — and, at the root, which window
 *  manager is in charge. */
export type LayoutDirection =
  | "horizontal"
  | "vertical"
  | "tabs"
  | "stacking"
  | "scrolling"
  | "floating"
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
  /** Read under a `floating` or mixed `workspace` parent; carried verbatim
   *  elsewhere so a round trip through another manager does not lose it. */
  rect?: LayoutRect;
}

export interface LayoutLeaf {
  type: "leaf";
  /** Shell command to run when the pane is created. */
  command?: string;
  /** Raw font size, e.g. 14, "12px", "13pt", "80%" */
  fontSize?: number | string;
}

const SPECIAL_CHARS = /[\s(),@'"\\:=[\]]/;

/** Split keywords, which are therefore never a label or a pane. */
const KEYWORDS: Readonly<Record<string, LayoutDirection>> = {
  line: "horizontal",
  col: "vertical",
  tabs: "tabs",
  stack: "stacking",
  scroll: "scrolling",
  float: "floating",
  scene: "workspace",
};

const DIRECTION_KEYWORD: Readonly<Record<LayoutDirection, string>> = {
  horizontal: "line",
  vertical: "col",
  tabs: "tabs",
  stacking: "stack",
  scrolling: "scroll",
  floating: "float",
  workspace: "scene",
};

/** Structural limits shared by persisted sessions and every UI parser. */
export const LAYOUT_DSL_MAX_PANES = 2_048;
export const LAYOUT_DSL_MAX_DEPTH = 64;

export class DSLParseError extends Error {
  constructor(
    message: string,
    public readonly offset: number,
  ) {
    super(message);
  }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

export function parseDSL(input: string): { root: LayoutNode; weight: number } {
  const trimmed = input.trim();
  if (!trimmed) throw new DSLParseError("Empty layout", 0);

  let pos = 0;
  let paneCount = 0;

  function peek(): string {
    return trimmed[pos] ?? "";
  }

  function skipWhitespace(): void {
    while (pos < trimmed.length && /\s/.test(trimmed[pos])) pos++;
  }

  function expect(ch: string): void {
    skipWhitespace();
    if (trimmed[pos] !== ch) {
      throw new DSLParseError(`Expected '${ch}' at position ${pos}`, pos);
    }
    pos++;
  }

  /** Weights and font sizes are positive; a rect's x/y may be zero or, for a
   *  window dragged off the left edge, negative. */
  function parseNumber(signed = false): number {
    skipWhitespace();
    const start = pos;
    if (signed && trimmed[pos] === "-") pos++;
    while (pos < trimmed.length && /[0-9.]/.test(trimmed[pos])) pos++;
    if (pos === start)
      throw new DSLParseError(`Expected number at position ${pos}`, pos);
    const n = Number(trimmed.slice(start, pos));
    if (!Number.isFinite(n) || (!signed && n <= 0)) {
      throw new DSLParseError(`Invalid number at position ${start}`, start);
    }
    return n;
  }

  function parseIdentifier(): string {
    skipWhitespace();
    // Quoted string
    if (peek() === '"' || peek() === "'") {
      const quote = trimmed[pos];
      pos++;
      let value = "";
      while (pos < trimmed.length && trimmed[pos] !== quote) {
        if (trimmed[pos] === "\\" && pos + 1 < trimmed.length) {
          pos++;
          value += trimmed[pos];
        } else {
          value += trimmed[pos];
        }
        pos++;
      }
      if (pos >= trimmed.length)
        throw new DSLParseError(`Unterminated string at position ${pos}`, pos);
      pos++; // skip closing quote
      return value;
    }
    const start = pos;
    while (pos < trimmed.length && !SPECIAL_CHARS.test(trimmed[pos])) pos++;
    if (pos === start)
      throw new DSLParseError(`Expected identifier at position ${pos}`, pos);
    return trimmed.slice(start, pos);
  }

  function parseFontSize(): number | string {
    const n = parseNumber();
    const unitStart = pos;
    if (pos < trimmed.length && /[a-z%]/.test(trimmed[pos])) {
      while (pos < trimmed.length && /[a-z%]/.test(trimmed[pos])) pos++;
      const unit = trimmed.slice(unitStart, pos);
      if (unit === "px" || unit === "pt" || unit === "%") {
        return `${n}${unit}`;
      }
      throw new DSLParseError(
        `Unknown font size unit '${unit}' at position ${unitStart}`,
        unitStart,
      );
    }
    return n;
  }

  function parseEntry(depth: number): LayoutChild {
    let label: string | undefined;

    skipWhitespace();
    const savedPos = pos;
    if (peek() === '"' || peek() === "'") {
      const candidate = parseIdentifier();
      skipWhitespace();
      if (peek() === ":") {
        pos++;
        label = candidate;
      } else {
        pos = savedPos;
      }
    } else if (/[a-zA-Z_]/.test(peek())) {
      const candidate = parseIdentifier();
      skipWhitespace();
      if (peek() === ":" && !(candidate in KEYWORDS)) {
        pos++;
        label = candidate;
      } else {
        pos = savedPos;
      }
    }

    const node = parseNode(depth);
    skipWhitespace();

    let rect: LayoutRect | undefined;
    if (peek() === "[") {
      pos++;
      const values: number[] = [];
      for (let index = 0; index < 4; index++) {
        if (index > 0) expect(",");
        values.push(parseNumber(true));
      }
      expect("]");
      const [x, y, width, height] = values;
      if (width <= 0 || height <= 0) {
        throw new DSLParseError(`Rect has no area at position ${pos}`, pos);
      }
      if (
        [x, y].some((value) => value < -100 || value > 200) ||
        [width, height].some((value) => value > 200)
      ) {
        throw new DSLParseError(`Rect is out of range at position ${pos}`, pos);
      }
      rect = { x, y, width, height };
      skipWhitespace();
    }

    let weight = 1;
    if (pos < trimmed.length && /[0-9]/.test(peek())) {
      weight = parseNumber();
    }

    skipWhitespace();
    if (peek() === "=") {
      pos++;
      const command = parseIdentifier();
      if (node.type !== "leaf") {
        throw new DSLParseError(
          `command can only be applied to leaf nodes at position ${pos}`,
          pos,
        );
      }
      node.command = command;
    }

    skipWhitespace();
    if (peek() === "@") {
      pos++;
      const fontSize = parseFontSize();
      if (node.type !== "leaf") {
        throw new DSLParseError(
          `fontSize can only be applied to leaf nodes at position ${pos}`,
          pos,
        );
      }
      node.fontSize = fontSize;
    }

    return {
      node,
      weight,
      ...(label != null && { label }),
      ...(rect != null && { rect }),
    };
  }

  function parseNode(depth: number): LayoutNode {
    skipWhitespace();
    const start = pos;
    if (depth > LAYOUT_DSL_MAX_DEPTH) {
      throw new DSLParseError(
        `Layout exceeds maximum depth of ${LAYOUT_DSL_MAX_DEPTH} at position ${start}`,
        start,
      );
    }
    const id = parseIdentifier();

    if (id in KEYWORDS && (skipWhitespace(), peek() === "(")) {
      const direction = KEYWORDS[id];
      expect("(");
      const children: LayoutChild[] = [parseEntry(depth + 1)];

      skipWhitespace();
      while (peek() === ",") {
        pos++;
        children.push(parseEntry(depth + 1));
        skipWhitespace();
      }

      expect(")");

      // A tiling split of one draws a divider against nothing. A strip of one
      // column, or a floating layer holding a single window, is an ordinary
      // state of those managers — the workspace starts there.
      if (
        children.length < 2 &&
        direction !== "scrolling" &&
        direction !== "floating" &&
        direction !== "workspace"
      ) {
        throw new DSLParseError(
          `Split needs at least 2 children at position ${start}`,
          start,
        );
      }
      if (
        direction === "workspace" &&
        children.filter((child) => child.rect == null).length > 1
      ) {
        throw new DSLParseError(
          `Scene has more than one tiled base at position ${start}`,
          start,
        );
      }

      return { type: "split", direction, children };
    }

    if (id !== "_") {
      throw new DSLParseError(
        `Panes are anonymous: expected '_' at position ${start}, found '${id}'`,
        start,
      );
    }
    paneCount++;
    if (paneCount > LAYOUT_DSL_MAX_PANES) {
      throw new DSLParseError(
        `Layout exceeds maximum pane count of ${LAYOUT_DSL_MAX_PANES} at position ${start}`,
        start,
      );
    }
    return { type: "leaf" };
  }

  const entry = parseEntry(1);

  skipWhitespace();
  if (pos < trimmed.length) {
    throw new DSLParseError(
      `Unexpected '${trimmed[pos]}' at position ${pos}`,
      pos,
    );
  }

  return { root: entry.node, weight: entry.weight };
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

function quoteIfNeeded(value: string): string {
  return value.length > 0 && !SPECIAL_CHARS.test(value)
    ? value
    : `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function number(value: number): string {
  return String(Math.round(value * 100) / 100);
}

function serializeNode(
  node: LayoutNode,
  weight: number,
  label?: string,
  rect?: LayoutRect,
): string {
  let s: string;
  if (node.type === "leaf") {
    s = "_";
  } else {
    const inner = node.children
      .map((c) => serializeNode(c.node, c.weight, c.label, c.rect))
      .join(", ");
    s = `${DIRECTION_KEYWORD[node.direction]}(${inner})`;
  }
  if (rect) {
    s += ` [${number(rect.x)},${number(rect.y)},${number(rect.width)},${number(rect.height)}]`;
  }
  if (weight !== 1) s += ` ${number(weight)}`;
  if (node.type === "leaf" && node.command != null) {
    s += `="${node.command.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  }
  if (node.type === "leaf" && node.fontSize != null) s += ` @${node.fontSize}`;
  if (label != null) s = `${quoteIfNeeded(label)}: ${s}`;
  return s;
}

export function serializeDSL(root: LayoutNode, weight = 1): string {
  return serializeNode(root, weight);
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

export function leafCount(node: LayoutNode): number {
  if (node.type === "leaf") return 1;
  return node.children.reduce((sum, c) => sum + leafCount(c.node), 0);
}
