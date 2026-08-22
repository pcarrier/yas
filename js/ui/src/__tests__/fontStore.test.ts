import { afterEach, describe, expect, it, vi } from "vitest";
import {
  boundedFontList,
  FONT_LIST_MAX_FAMILIES,
  FONT_LIST_MAX_FAMILY_CHARS,
  FONT_LIST_MAX_TOTAL_CHARS,
  FONT_STORE_BUDGET_BYTES,
  FONT_STORE_MAX_ENTRIES,
  selectEvictions,
} from "../fontStore";

/** Bounds for native content-addressed FONT face caching and catalogues. */

afterEach(() => {
  localStorage.clear();
  vi.useRealTimers();
});

describe("selectEvictions", () => {
  const entry = (key: string, mb: number, usedAt: number) => ({
    key,
    bytes: mb * 1024 * 1024,
    usedAt,
  });

  it("keeps everything while it fits", () => {
    expect(
      selectEvictions(
        [entry("a", 25, 1), entry("b", 25, 2)],
        FONT_STORE_BUDGET_BYTES,
      ),
    ).toEqual([]);
  });

  it("evicts least-recently-used first, and only as far as needed", () => {
    const entries = [
      entry("old", 25, 1),
      entry("older", 25, 0),
      entry("new", 25, 3),
    ];
    expect(selectEvictions(entries, FONT_STORE_BUDGET_BYTES)).toEqual([
      "older",
    ]);
  });

  it("never evicts the family just stored", () => {
    // The one in use is the oldest by `usedAt` — a fresh write has not been
    // read back yet — so LRU alone would throw away the font on screen.
    const entries = [
      entry("in-use", 40, 0),
      entry("a", 20, 5),
      entry("b", 20, 6),
    ];
    const evicted = selectEvictions(entries, FONT_STORE_BUDGET_BYTES, "in-use");
    expect(evicted).not.toContain("in-use");
    expect(evicted).toEqual(["a"]);
  });

  it("evicts even the in-use marker when it alone exceeds the hard budget", () => {
    const entries = [entry("huge", 200, 0)];
    expect(selectEvictions(entries, FONT_STORE_BUDGET_BYTES, "huge")).toEqual([
      "huge",
    ]);
  });

  it("bounds hostile key rotation independently of byte size", () => {
    const entries = Array.from(
      { length: FONT_STORE_MAX_ENTRIES + 1 },
      (_, index) => ({ key: `font-${index}`, bytes: 1, usedAt: index }),
    );
    expect(
      selectEvictions(entries, FONT_STORE_BUDGET_BYTES, entries.at(-1)?.key),
    ).toEqual(["font-0"]);
  });
});

describe("the family list", () => {
  it("bounds hostile family rotation and oversized names before storage", () => {
    const names = [
      "x".repeat(FONT_LIST_MAX_FAMILY_CHARS + 1),
      ...Array.from(
        { length: FONT_LIST_MAX_FAMILIES + 100 },
        (_, index) => `font-${index}`,
      ),
    ];
    const bounded = boundedFontList(names);
    expect(bounded).toHaveLength(FONT_LIST_MAX_FAMILIES);
    expect(
      bounded.every((font) => font.length <= FONT_LIST_MAX_FAMILY_CHARS),
    ).toBe(true);
    expect(
      bounded.reduce((sum, font) => sum + font.length, 0),
    ).toBeLessThanOrEqual(FONT_LIST_MAX_TOTAL_CHARS);
  });
});
