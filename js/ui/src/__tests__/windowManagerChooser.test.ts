import { describe, expect, it } from "vitest";
import {
  nextManagerChoice,
  WINDOW_MANAGERS,
} from "../layout/windowManagerChoice";

describe("window manager chooser", () => {
  it("offers only the active window managers", () => {
    expect(WINDOW_MANAGERS).toEqual(["tiling", "floating"]);
  });

  it("walks and wraps the list with arrow keys", () => {
    expect(nextManagerChoice(0, "ArrowDown")).toBe(1);
    expect(nextManagerChoice(1, "ArrowDown")).toBe(0);
    expect(nextManagerChoice(0, "ArrowUp")).toBe(1);
    expect(nextManagerChoice(1, "ArrowLeft")).toBe(0);
  });

  it("supports list endpoints and ignores unrelated keys", () => {
    expect(nextManagerChoice(1, "Home")).toBe(0);
    expect(nextManagerChoice(1, "End")).toBe(1);
    expect(nextManagerChoice(1, "Enter")).toBeNull();
  });
});
