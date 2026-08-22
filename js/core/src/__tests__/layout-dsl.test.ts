import { describe, it, expect } from "vitest";
import {
  parseDSL,
  serializeDSL,
  leafCount,
  DSLParseError,
  LAYOUT_DSL_MAX_DEPTH,
  LAYOUT_DSL_MAX_PANES,
} from "../layout/dsl";
import type { LayoutNode, LayoutLeaf, LayoutSplit } from "../layout/dsl";

describe("parseDSL", () => {
  it("parses a single leaf", () => {
    const { root, weight } = parseDSL("_");
    expect(root.type).toBe("leaf");
    expect(weight).toBe(1);
  });

  it("parses a leaf with weight", () => {
    const { root, weight } = parseDSL("_ 3");
    expect(root.type).toBe("leaf");
    expect(weight).toBe(3);
  });

  it("parses a horizontal split", () => {
    const { root } = parseDSL("line(_, _)");
    expect(root.type).toBe("split");
    const split = root as LayoutSplit;
    expect(split.direction).toBe("horizontal");
    expect(split.children).toHaveLength(2);
    expect(split.children[0].node.type).toBe("leaf");
    expect(split.children[1].node.type).toBe("leaf");
  });

  it("parses a vertical split", () => {
    const { root } = parseDSL("col(_, _)");
    expect(root.type).toBe("split");
    expect((root as LayoutSplit).direction).toBe("vertical");
  });

  it("parses tabs", () => {
    const { root } = parseDSL("tabs(_, _, _)");
    expect(root.type).toBe("split");
    const split = root as LayoutSplit;
    expect(split.direction).toBe("tabs");
    expect(split.children).toHaveLength(3);
  });

  it("parses nested splits", () => {
    const { root } = parseDSL("line(_ 2, col(_, _))");
    expect(root.type).toBe("split");
    const split = root as LayoutSplit;
    expect(split.children[0].weight).toBe(2);
    expect(split.children[1].node.type).toBe("split");
    const inner = split.children[1].node as LayoutSplit;
    expect(inner.direction).toBe("vertical");
    expect(inner.children).toHaveLength(2);
  });

  it("parses a leaf with command", () => {
    const { root } = parseDSL('_="cd /src && make"');
    expect(root.type).toBe("leaf");
    const leaf = root as LayoutLeaf;
    expect(leaf.command).toBe("cd /src && make");
  });

  it("parses a leaf with fontSize (bare number)", () => {
    const { root } = parseDSL("_ @14");
    expect(root.type).toBe("leaf");
    expect((root as LayoutLeaf).fontSize).toBe(14);
  });

  it("parses a leaf with fontSize px unit", () => {
    const { root } = parseDSL("_ @12px");
    expect((root as LayoutLeaf).fontSize).toBe("12px");
  });

  it("parses a leaf with fontSize pt unit", () => {
    const { root } = parseDSL("_ @13pt");
    expect((root as LayoutLeaf).fontSize).toBe("13pt");
  });

  it("parses a leaf with fontSize % unit", () => {
    const { root } = parseDSL("_ @80%");
    expect((root as LayoutLeaf).fontSize).toBe("80%");
  });

  it("parses labels on entries", () => {
    const { root } = parseDSL('tabs("Editor": _, "Term": _)');
    const split = root as LayoutSplit;
    expect(split.children[0].label).toBe("Editor");
    expect(split.children[1].label).toBe("Term");
  });

  it("parses unquoted labels", () => {
    const { root } = parseDSL("tabs(myLabel: _, other: _)");
    const split = root as LayoutSplit;
    expect(split.children[0].label).toBe("myLabel");
    expect(split.children[1].label).toBe("other");
  });

  it("does not treat line/col/tabs as labels", () => {
    const { root } = parseDSL("tabs(line(_, _), col(_, _))");
    const split = root as LayoutSplit;
    expect(split.children[0].label).toBeUndefined();
    expect(split.children[0].node.type).toBe("split");
  });

  it("keeps quoted-string escapes working in the places that still name things", () => {
    const { root } = parseDSL('tabs("my\\"label": _, _)');
    expect((root as LayoutSplit).children[0].label).toBe('my"label');
  });

  it("rejects a named pane", () => {
    expect(() => parseDSL("shell")).toThrow("Panes are anonymous");
    expect(() => parseDSL("line(shell, _)")).toThrow("Panes are anonymous");
  });

  it("throws on empty input", () => {
    expect(() => parseDSL("")).toThrow(DSLParseError);
    expect(() => parseDSL("   ")).toThrow(DSLParseError);
  });

  it("throws on split with single child", () => {
    expect(() => parseDSL("line(_)")).toThrow("at least 2 children");
  });

  it("throws on trailing garbage", () => {
    expect(() => parseDSL("_ _")).toThrow(DSLParseError);
  });

  it("throws on unknown font unit", () => {
    expect(() => parseDSL("_ @14em")).toThrow("Unknown font size unit");
  });

  it("throws on command on split node", () => {
    expect(() => parseDSL('line(_, _)="cmd"')).toThrow(
      "command can only be applied to leaf",
    );
  });

  it("throws on unterminated string", () => {
    expect(() => parseDSL('"unterminated')).toThrow("Unterminated string");
  });

  it("bounds flat layouts before constructing unbounded pane arrays", () => {
    const atLimit = `line(${Array.from({ length: LAYOUT_DSL_MAX_PANES }, () => "_").join(",")})`;
    expect(leafCount(parseDSL(atLimit).root)).toBe(LAYOUT_DSL_MAX_PANES);
    expect(() => parseDSL(`${atLimit.slice(0, -1)},_)`)).toThrow(
      `maximum pane count of ${LAYOUT_DSL_MAX_PANES}`,
    );
  });

  it("bounds recursive layout depth before the JavaScript stack", () => {
    const nested = (splitCount: number): string => {
      let dsl = "_";
      for (let index = 0; index < splitCount; index++) dsl = `line(_,${dsl})`;
      return dsl;
    };
    expect(() => parseDSL(nested(LAYOUT_DSL_MAX_DEPTH - 1))).not.toThrow();
    expect(() => parseDSL(nested(LAYOUT_DSL_MAX_DEPTH))).toThrow(
      `maximum depth of ${LAYOUT_DSL_MAX_DEPTH}`,
    );
  });

  it("parses weight + command + fontSize together", () => {
    const { root, weight } = parseDSL('_ 2 ="vim" @14');
    expect(root.type).toBe("leaf");
    const leaf = root as LayoutLeaf;
    expect(weight).toBe(2);
    expect(leaf.command).toBe("vim");
    expect(leaf.fontSize).toBe(14);
  });
});

describe("serializeDSL", () => {
  it("serializes a single leaf", () => {
    const node: LayoutNode = { type: "leaf" };
    expect(serializeDSL(node)).toBe("_");
  });

  it("serializes with weight", () => {
    const node: LayoutNode = { type: "leaf" };
    expect(serializeDSL(node, 3)).toBe("_ 3");
  });

  it("serializes a split", () => {
    const node: LayoutNode = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    };
    expect(serializeDSL(node)).toBe("line(_, _)");
  });

  it("serializes command with escaping", () => {
    const node: LayoutNode = {
      type: "leaf",
      command: 'echo "hi"',
    };
    expect(serializeDSL(node)).toBe('_="echo \\"hi\\""');
  });

  it("serializes fontSize", () => {
    const node: LayoutNode = { type: "leaf", fontSize: 14 };
    expect(serializeDSL(node)).toBe("_ @14");
  });

  it("quotes labels with special characters", () => {
    const node: LayoutNode = {
      type: "split",
      direction: "tabs",
      children: [
        { node: { type: "leaf" }, weight: 1, label: "my shell" },
        { node: { type: "leaf" }, weight: 1 },
      ],
    };
    expect(serializeDSL(node)).toBe('tabs("my shell": _, _)');
  });
});

describe("parseDSL / serializeDSL round-trip", () => {
  const cases = [
    "_",
    "line(_, _)",
    "col(_ 2, _)",
    "tabs(_, _, _)",
    "line(_ 2, col(_, _))",
  ];

  for (const dsl of cases) {
    it(`round-trips: ${dsl}`, () => {
      const { root, weight } = parseDSL(dsl);
      const serialized = serializeDSL(root, weight);
      const reparsed = parseDSL(serialized);
      expect(reparsed.root).toEqual(root);
      expect(reparsed.weight).toBe(weight);
    });
  }
});

describe("leafCount", () => {
  it("returns 1 for a leaf", () => {
    const { root } = parseDSL("_");
    expect(leafCount(root)).toBe(1);
  });

  it("returns 2 for simple split", () => {
    const { root } = parseDSL("line(_, _)");
    expect(leafCount(root)).toBe(2);
  });

  it("returns 4 for a grid", () => {
    const { root } = parseDSL("col(line(_, _), line(_, _))");
    expect(leafCount(root)).toBe(4);
  });
});
