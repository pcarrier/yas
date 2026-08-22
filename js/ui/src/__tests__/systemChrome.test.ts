import { PALETTES } from "@yas-run/core";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { applySystemChrome } from "../systemChrome";
import { themeFor } from "../theme";

const palette = (id: string) => PALETTES.find((entry) => entry.id === id)!;

beforeEach(() => {
  document.head.innerHTML = '<meta name="theme-color" content="#000">';
});

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("style");
  document.body.removeAttribute("style");
  document.head.innerHTML = "";
});

describe("system chrome", () => {
  it.each(["default", "catppuccin-latte"])("follows the %s palette", (id) => {
    const selected = palette(id);
    const background = themeFor(selected).solidPanelBg;

    applySystemChrome(selected);

    expect(document.documentElement.dataset.theme).toBe(
      selected.dark ? "dark" : "light",
    );
    expect(document.documentElement.style.colorScheme).toBe(
      selected.dark ? "dark" : "light",
    );
    expect(
      document.documentElement.style.getPropertyValue(
        "--yas-system-bar-background",
      ),
    ).toBe(background);
    expect(document.body.style.backgroundColor).toBe(background);
    expect(
      document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
        ?.content,
    ).toBe(background);
  });
});
