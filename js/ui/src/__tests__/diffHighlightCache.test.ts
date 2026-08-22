import type { Highlighter } from "@lezer/highlight";
import { javascript } from "@codemirror/lang-javascript";
import { afterEach, describe, expect, it } from "vitest";
import {
  LINE_COLOR_CACHE_MAX_BYTES,
  LINE_COLOR_CACHE_MAX_ITEMS,
  LINE_COLOR_MAX_CHARS,
  lineColorCacheStats,
  lineColors,
  resetLineColorCache,
} from "../ide/diff-highlight";

const highlighter: Highlighter = { style: () => "#fff" };
const language = javascript();

afterEach(resetLineColorCache);

describe("diff line color cache bounds", () => {
  it("reuses a cached line without exceeding its accounting", () => {
    const first = lineColors("const answer = 42", language, highlighter);
    const before = lineColorCacheStats();
    const second = lineColors("const answer = 42", language, highlighter);
    expect(second).toBe(first);
    expect(lineColorCacheStats()).toEqual(before);
  });

  it("renders a hostile oversized line as plain text without retaining it", () => {
    const colors = lineColors(
      "x".repeat(LINE_COLOR_MAX_CHARS + 1),
      language,
      highlighter,
    );
    expect(colors).toEqual([]);
    expect(
      lineColors("x".repeat(LINE_COLOR_MAX_CHARS + 1), null, highlighter),
    ).toEqual([]);
    expect(lineColorCacheStats()).toEqual({ items: 0, bytes: 0 });
  });

  it("keeps unique-line rotation under both item and byte budgets", () => {
    for (let i = 0; i < LINE_COLOR_CACHE_MAX_ITEMS + 32; i++) {
      lineColors(
        `const value${i} = "${"x".repeat(256)}"`,
        language,
        highlighter,
      );
    }
    const stats = lineColorCacheStats();
    expect(stats.items).toBeLessThanOrEqual(LINE_COLOR_CACHE_MAX_ITEMS);
    expect(stats.bytes).toBeLessThanOrEqual(LINE_COLOR_CACHE_MAX_BYTES);
  });
});
