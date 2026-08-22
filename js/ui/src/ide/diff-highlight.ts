/**
 * Standalone syntax highlighting for the diff viewer.
 *
 * YasDiff renders its own rows (not a CodeMirror editor), so it can't lean on
 * CM's `syntaxHighlighting`. Instead we drive the language's Lezer parser
 * directly and map highlight tags to palette colors — the same tag→color scheme
 * as {@link cmTheme}, so the diff and the editor look consistent.
 *
 * Highlighting is per-line: each hunk row is parsed on its own. Cross-line
 * constructs (block comments, multi-line strings) aren't tracked, which is an
 * acceptable approximation for a diff.
 */

import {
  highlightTree,
  tags as t,
  type Tag,
  type Highlighter,
} from "@lezer/highlight";
import type { LanguageSupport } from "@codemirror/language";
import type { TerminalPalette } from "@yas-run/core";
import type { Theme } from "../theme";

function rgb(c: [number, number, number]): string {
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

// One highlighter per (theme, palette) pair, so every diff/commit tile
// shares one identity and the line-color cache below stays coherent
// across tiles.
const highlighterCache = new WeakMap<
  Theme,
  WeakMap<TerminalPalette, Highlighter>
>();

/** Build a tag→color highlighter mirroring cmTheme's palette-derived scheme.
 *  Cached by (theme, palette) identity. */
export function buildDiffHighlighter(
  theme: Theme,
  palette: TerminalPalette,
): Highlighter {
  let byPalette = highlighterCache.get(theme);
  if (!byPalette) {
    byPalette = new WeakMap();
    highlighterCache.set(theme, byPalette);
  }
  const cached = byPalette.get(palette);
  if (cached) return cached;
  const built = makeHighlighter(theme, palette);
  byPalette.set(palette, built);
  return built;
}

function makeHighlighter(theme: Theme, palette: TerminalPalette): Highlighter {
  const ansi = palette.ansi;
  const at = (i: number, fallback: string) =>
    ansi[i] ? rgb(ansi[i]) : fallback;
  const green = at(10, theme.success);
  const yellow = at(11, theme.warning);
  const blue = at(12, theme.accent);
  const magenta = at(13, theme.accent);
  const cyan = at(14, theme.accent);
  const comment = theme.dimFg;

  const map = new Map<Tag, string>();
  const put = (tags: readonly Tag[], color: string) =>
    tags.forEach((tag) => map.set(tag, color));
  put([t.keyword, t.controlKeyword, t.moduleKeyword], magenta);
  put([t.typeName, t.className, t.namespace], yellow);
  put([t.function(t.variableName), t.function(t.propertyName)], blue);
  put([t.string, t.special(t.string)], green);
  put([t.number, t.bool, t.atom], cyan);
  put([t.comment, t.lineComment, t.blockComment, t.docComment], comment);
  // Red is reserved for t.invalid, matching cm-theme: a macro is not
  // an error.
  put([t.macroName], magenta);
  put([t.meta], cyan);
  put([t.operator, t.punctuation, t.separator, t.bracket], theme.dimFg);
  put([t.propertyName, t.attributeName], cyan);
  put([t.invalid], theme.errorText);

  return {
    style(tags: readonly Tag[]): string | null {
      for (const tag of tags) {
        // `tag.set` walks the tag and the tags it derives from, matching the
        // inheritance HighlightStyle uses.
        for (const anc of tag.set) {
          const color = map.get(anc);
          if (color) return color;
        }
      }
      return null;
    },
  };
}

/**
 * Per-character syntax colors for one line of text (index i → color or null).
 * Returns all-null when there's no language (plain text) or the line is empty.
 *
 * Cached by (highlighter, language, line content): diffs repeat lines across
 * refetches (and the split view shows a context line twice), so each distinct
 * line parses once. A theme/palette change swaps the highlighter identity and
 * drops the cache wholesale. The cache is a global byte-and-item LRU: a peer
 * can rotate unique patch lines and one long line costs much more than one
 * short line, so a per-language entry count is not a useful memory bound.
 */
export const LINE_COLOR_CACHE_MAX_ITEMS = 4_096;
export const LINE_COLOR_CACHE_MAX_BYTES = 8 * 1024 * 1024;
/** Parsing and materialising one color slot per character above this size is
 * not useful in a viewport. The renderer treats an empty color array as plain
 * text, so an adversarial single line stays cheap without hiding its text. */
export const LINE_COLOR_MAX_CHARS = 64 * 1024;

interface LineCacheEntry {
  readonly lang: LanguageSupport;
  readonly text: string;
  readonly colors: (string | null)[];
  readonly bytes: number;
}

let lineCacheHl: Highlighter | null = null;
const lineCache = new Map<LanguageSupport, Map<string, LineCacheEntry>>();
const lineLru = new Set<LineCacheEntry>();
let lineCacheBytes = 0;

function clearLineCache(): void {
  lineCache.clear();
  lineLru.clear();
  lineCacheBytes = 0;
}

function removeLineEntry(entry: LineCacheEntry): void {
  const byText = lineCache.get(entry.lang);
  if (byText?.get(entry.text) === entry) {
    byText.delete(entry.text);
    if (byText.size === 0) lineCache.delete(entry.lang);
  }
  if (lineLru.delete(entry)) lineCacheBytes -= entry.bytes;
}

function retainLineEntry(entry: LineCacheEntry): void {
  lineLru.delete(entry);
  lineLru.add(entry);
}

function pruneLineCache(): void {
  while (
    lineLru.size > LINE_COLOR_CACHE_MAX_ITEMS ||
    lineCacheBytes > LINE_COLOR_CACHE_MAX_BYTES
  ) {
    const oldest = lineLru.values().next().value as LineCacheEntry | undefined;
    if (!oldest) break;
    removeLineEntry(oldest);
  }
}

/** Test/diagnostic seam. */
export function lineColorCacheStats(): { items: number; bytes: number } {
  return { items: lineLru.size, bytes: lineCacheBytes };
}

/** Test seam; a highlighter change performs the same reset in production. */
export function resetLineColorCache(): void {
  clearLineCache();
  lineCacheHl = null;
}

export function lineColors(
  text: string,
  lang: LanguageSupport | null,
  hl: Highlighter,
): (string | null)[] {
  if (text.length === 0) return [];
  if (text.length > LINE_COLOR_MAX_CHARS) return [];
  if (!lang) {
    return new Array<string | null>(text.length).fill(null);
  }
  if (hl !== lineCacheHl) {
    clearLineCache();
    lineCacheHl = hl;
  }
  let byText = lineCache.get(lang);
  if (!byText) {
    byText = new Map();
    lineCache.set(lang, byText);
  }
  const cached = byText.get(text);
  if (cached) {
    retainLineEntry(cached);
    return cached.colors;
  }
  const colors: (string | null)[] = new Array(text.length).fill(null);
  const tree = lang.language.parser.parse(text);
  highlightTree(tree, hl, (from, to, color) => {
    for (let i = from; i < to && i < colors.length; i++) colors[i] = color;
  });
  // UTF-16 text plus one pointer-sized color slot per character, with a small
  // fixed allowance for the Map/Set and array records. Color strings are
  // palette-owned and shared, so they are not charged per character.
  const bytes = 128 + text.length * 2 + colors.length * 8;
  if (bytes <= LINE_COLOR_CACHE_MAX_BYTES) {
    const entry = { lang, text, colors, bytes } satisfies LineCacheEntry;
    byText.set(text, entry);
    lineLru.add(entry);
    lineCacheBytes += bytes;
    pruneLineCache();
  }
  return colors;
}
