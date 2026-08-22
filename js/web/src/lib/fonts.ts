/**
 * The site's monospace faces.
 *
 * These are self-hosted under their real family names (see ../share.css), so
 * the canvas renderer and persisted font setting use stable family strings.
 *
 * - The terminal draws glyphs into a canvas atlas from a font *string*, not a
 *   variable, so it needs a name it can spell.
 * - That string is the user's font setting on /s: shown in the picker and
 *   persisted. A hashed name is unreadable in a menu, and stops matching
 *   anything the next deploy ships — a saved preference that quietly expires.
 *
 * So the names here are the names everywhere: in the atlas, in the picker, in
 * localStorage.
 */

/** The default face, and the one `--font-mono` resolves to. */
export const MONO_FAMILY = "JetBrains Mono";

const withFallbacks = (family: string) =>
  `"${family}", ui-monospace, monospace`;

/** Family plus fallbacks, for canvas measurement and `font-family`. */
export const MONO_STACK = withFallbacks(MONO_FAMILY);

/**
 * What the terminal's font picker offers on /s.
 *
 * A yas server serves any installed family on request, so the app's picker is
 * a search box. A static origin has no such route: the menu can only be what
 * the page bundled, which is this list — every entry declared in share.css,
 * so its @font-face exists before anyone picks it.
 */
export const MONO_CATALOG: readonly { label: string; stack: string }[] = [
  MONO_FAMILY,
  "Fira Code",
  "IBM Plex Mono",
  "Source Code Pro",
].map((label) => ({ label, stack: withFallbacks(label) }));

/** A `document.fonts.load()` shorthand for the default face. */
export function monoLoadSpec(sizePx: number): string {
  return `${sizePx}px "${MONO_FAMILY}"`;
}
