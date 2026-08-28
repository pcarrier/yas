import { describe, expect, it } from "vitest";
import { enumeratePanes, parseDSL, serializeDSL } from "@yas-run/core/layout";
import { removePaneFromLayout, showEmptyPaneHint } from "../layout/paneRemoval";

describe("showEmptyPaneHint", () => {
  it("shows one focused hint when a multi-pane manager has no occupants", () => {
    expect(showEmptyPaneHint(true, false, true)).toBe(true);
    expect(showEmptyPaneHint(true, false, false)).toBe(false);
    expect(showEmptyPaneHint(true, true, true)).toBe(false);
    expect(showEmptyPaneHint(false, true, false)).toBe(true);
  });
});

describe("removePaneFromLayout", () => {
  it("removes a floating window instead of leaving an empty shell", () => {
    const root = parseDSL(
      "float(_ [3,4,30,31], _ [33,8,40,42], _ [17,51,45,46])",
    ).root;
    if (root.type !== "split") throw new Error("expected floating root");
    const first = root.children[0];
    const third = root.children[2];
    const next = removePaneFromLayout(root, "1");

    expect(next).not.toBeNull();
    expect(serializeDSL(next!)).toBe("float(_ [3,4,30,31], _ [17,51,45,46])");
    expect(enumeratePanes(next!).map((pane) => pane.id)).toEqual(["0", "1"]);
    if (next!.type !== "split") throw new Error("expected floating root");
    // Solid's keyed <For> can now retain both live frames. Exact child
    // identity matters here: replacing the later object remounts its terminal
    // or surface even when its serialized rectangle is unchanged.
    expect(next!.children[0]).toBe(first);
    expect(next!.children[1]).toBe(third);
  });

  it("keeps the floating manager around its sole surviving window", () => {
    const root = parseDSL("float(_ [6,6,58,58], _ [12,12,50,50])").root;
    const next = removePaneFromLayout(root, "0");

    expect(next).not.toBeNull();
    expect(serializeDSL(next!)).toBe("float(_ [12,12,50,50])");
    expect(enumeratePanes(next!).map((pane) => pane.id)).toEqual(["0"]);
  });

  it("collapses a singleton nested split", () => {
    const root = parseDSL("line(col(_, _), _)").root;
    const next = removePaneFromLayout(root, "0.1");

    expect(next).not.toBeNull();
    expect(serializeDSL(next!)).toBe("line(_, _)");
  });

  it("returns the sole surviving leaf for a two-pane layout", () => {
    const root = parseDSL("line(_, _)").root;
    const next = removePaneFromLayout(root, "0");

    expect(next).toEqual({ type: "leaf" });
  });

  it("does not change the tree for an unknown pane id", () => {
    const root = parseDSL("line(_, _)").root;
    expect(removePaneFromLayout(root, "9")).toBe(root);
  });
});
