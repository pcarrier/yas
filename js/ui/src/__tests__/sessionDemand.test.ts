import { describe, expect, it } from "vitest";
import { shouldKeepIdeSession } from "../ide/sessionDemand";

describe("IDE session demand", () => {
  it("keeps the project root available when search is open alone", () => {
    expect(shouldKeepIdeSession(false, true)).toBe(true);
    expect(shouldKeepIdeSession(true, true)).toBe(true);
  });

  it("releases the session when no IDE surface can use it", () => {
    expect(shouldKeepIdeSession(false, false)).toBe(false);
  });

  it("keeps repository discovery alive for an open dock, even with every section folded", () => {
    expect(shouldKeepIdeSession(true, false)).toBe(true);
  });
});
