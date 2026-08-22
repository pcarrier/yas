import { describe, expect, it } from "vitest";
import { enumeratePanes, parseDSL, serializeDSL } from "@yas-run/core/layout";
import {
  pruneUnassignedPanes,
  removePaneFromLayout,
  showEmptyPaneHint,
} from "../layout/paneRemoval";

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

  it("collapses a mixed scene after its last floating window is removed", () => {
    const root = parseDSL("scene(line(_, _), _ [12,12,50,50])").root;
    const next = removePaneFromLayout(root, "1");
    expect(next).not.toBeNull();
    expect(serializeDSL(next!)).toBe("line(_, _)");
  });

  it("keeps a mixed scene when only its floating window survives", () => {
    const root = parseDSL("scene(_ , _ [12,12,50,50])").root;
    const next = removePaneFromLayout(root, "0");
    expect(next).not.toBeNull();
    expect(serializeDSL(next!)).toBe("scene(_ [12,12,50,50])");
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

describe("pruneUnassignedPanes", () => {
  it("removes every empty branch and rekeys surviving assignments", () => {
    const root = parseDSL("line(_, col(_, _), _)").root;
    const result = pruneUnassignedPanes(root, {
      "0": null,
      "1.0": "terminal:a",
      "1.1": null,
      "2": "surface:local:3",
    });

    expect(result).not.toBeNull();
    expect(serializeDSL(result!.root)).toBe("line(_, _)");
    expect(result!.assignments).toEqual({
      "0": "terminal:a",
      "1": "surface:local:3",
    });
    expect(result!.paneIdMap.get("1.0")).toBe("0");
    expect(result!.paneIdMap.get("2")).toBe("1");
  });

  it("retains only one launcher leaf when the workspace is empty", () => {
    const root = parseDSL("tabs(_, _, _)").root;
    const result = pruneUnassignedPanes(root, {
      "0": null,
      "1": null,
      "2": null,
    });

    expect(result).not.toBeNull();
    expect(serializeDSL(result!.root)).toBe("_");
    expect(result!.assignments).toEqual({ "0": null });
  });

  it("does not create churn when no empty panes exist", () => {
    const root = parseDSL("line(_, _)").root;
    expect(
      pruneUnassignedPanes(root, { "0": "terminal:a", "1": "terminal:b" }),
    ).toBeNull();
  });
});
