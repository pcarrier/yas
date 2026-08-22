import { describe, expect, it } from "vitest";
import { searchKeyFor } from "../ide/searchStore";

describe("project search identity", () => {
  const options = {
    caseSensitive: false,
    regex: false,
    noIgnore: false,
    word: false,
  };

  it("changes when whole-word matching changes", () => {
    expect(searchKeyFor("/repo", "needle", options)).not.toBe(
      searchKeyFor("/repo", "needle", { ...options, word: true }),
    );
  });
});
