import type { TerminalPalette } from "@yas-run/core";
import { themeFor } from "./theme";

const SYSTEM_BAR_BACKGROUND = "--yas-system-bar-background";

/** Keep browser/installed-app chrome in the same polarity and colour as the
 * top workspace tab bar. Safari 26 samples the fixed safe-area backdrop rather
 * than `theme-color`; older browsers still use the meta tag. */
export function applySystemChrome(palette: TerminalPalette): void {
  const colorScheme = palette.dark ? "dark" : "light";
  const background = themeFor(palette).solidPanelBg;
  const root = document.documentElement;

  root.dataset.theme = colorScheme;
  root.style.colorScheme = colorScheme;
  root.style.setProperty(SYSTEM_BAR_BACKGROUND, background);
  document.body.style.backgroundColor = background;
  document
    .querySelector<HTMLMetaElement>('meta[name="theme-color"]')
    ?.setAttribute("content", background);
}
