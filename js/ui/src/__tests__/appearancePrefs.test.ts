import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_TEXT_GAMMA, PALETTES } from "@yas-run/core";
import {
  FONT_KEY,
  FONT_SIZE_KEY,
  PALETTE_KEY,
  TEXT_GAMMA_KEY,
  preferredFont,
  preferredFontSize,
  preferredPalette,
  preferredTextGamma,
} from "../storage";

afterEach(() => {
  localStorage.clear();
});

describe("appearance preferences", () => {
  it("uses stored choices", () => {
    localStorage.setItem(FONT_KEY, "Iosevka");
    localStorage.setItem(FONT_SIZE_KEY, "17");
    localStorage.setItem(TEXT_GAMMA_KEY, "1.2");
    localStorage.setItem(PALETTE_KEY, "catppuccin");

    expect(preferredFont()).toBe("Iosevka");
    expect(preferredFontSize()).toBe(17);
    expect(preferredTextGamma()).toBe(1.2);
    expect(preferredPalette().id).toBe("catppuccin");
  });

  it("uses defaults for absent or invalid choices", () => {
    localStorage.setItem(FONT_KEY, "   ");
    localStorage.setItem(FONT_SIZE_KEY, "0");
    localStorage.setItem(TEXT_GAMMA_KEY, "9");
    localStorage.setItem(PALETTE_KEY, "missing");

    expect(preferredFontSize()).toBe(13);
    expect(preferredTextGamma()).toBe(DEFAULT_TEXT_GAMMA);
    expect(preferredPalette().id).toBe(PALETTES[0].id);
  });
});
