import { describe, expect, it } from "vitest";
import { debugPanelOpenFromHash, withDebugPanelState } from "../workspaceUrl";

describe("workspace debug URL state", () => {
  it("recognizes the established bare debug flag", () => {
    expect(debugPanelOpenFromHash("#session=one&debug")).toBe(true);
    expect(debugPanelOpenFromHash("#session=one")).toBe(false);
  });

  it("accepts parameter-shaped and encoded debug flags", () => {
    expect(debugPanelOpenFromHash("debug=1")).toBe(true);
    expect(debugPanelOpenFromHash("%64ebug")).toBe(true);
    expect(debugPanelOpenFromHash("debugger")).toBe(false);
  });

  it("adds one canonical flag without rewriting other URL state", () => {
    expect(withDebugPanelState("session=one", true)).toBe("session=one&debug");
    expect(withDebugPanelState("secret&debug=1", true)).toBe("secret&debug");
  });

  it("removes the flag without disturbing other URL state", () => {
    expect(withDebugPanelState("session=one&debug", false)).toBe("session=one");
  });
});
