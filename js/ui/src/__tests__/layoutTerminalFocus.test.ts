import { describe, expect, it } from "vitest";
import { terminalFocusRequest } from "../layout/terminalFocus";

describe("layout terminal focus", () => {
  it("publishes a newly focused terminal", () => {
    expect(terminalFocusRequest("relay:terminal:2", "main:terminal:1")).toBe(
      "relay:terminal:2",
    );
  });

  it("does not clear terminal focus for non-terminal panes", () => {
    expect(terminalFocusRequest(null, "relay:terminal:2")).toBeNull();
  });

  it("does not republish unchanged terminal focus", () => {
    expect(
      terminalFocusRequest("relay:terminal:2", "relay:terminal:2"),
    ).toBeNull();
  });
});
