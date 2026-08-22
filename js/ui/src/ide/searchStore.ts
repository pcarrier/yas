/**
 * Project-search state, held at module scope so it outlives the panel.
 *
 * The search pane is transient chrome — it unmounts when dismissed — but
 * the query and its results are work you did, and losing them on every
 * close would make the pane painful to use as a scratchpad. Keeping the
 * state here means reopening restores exactly what was on screen, and a
 * repeat of the same search does not re-walk the tree.
 *
 * Deliberately not persisted to disk: it is a session scratchpad, not a
 * preference, and a stale result set restored days later would be a lie.
 */
import { createSignal } from "solid-js";
import type { FsGrepFile } from "@yas-run/core";

export const [searchQuery, setSearchQuery] = createSignal("");
export const [searchCaseSensitive, setSearchCaseSensitive] =
  createSignal(false);
export const [searchRegex, setSearchRegex] = createSignal(false);
/** Include gitignored files (slower; they rank last). */
export const [searchNoIgnore, setSearchNoIgnore] = createSignal(false);
/** Match whole words only. */
export const [searchWord, setSearchWord] = createSignal(false);

export const [searchFiles, setSearchFiles] = createSignal<FsGrepFile[]>([]);
export const [searchTruncated, setSearchTruncated] = createSignal(false);
export const [searchError, setSearchError] = createSignal<string | null>(null);
export const [searchBusy, setSearchBusy] = createSignal(false);

/**
 * Whether the pane's query input currently holds focus.
 *
 * The shortcut is three-way rather than a plain toggle: closed opens and
 * focuses, open-but-unfocused focuses, and only open-and-focused dismisses.
 * Hiding a pane you were merely looking at — because focus happened to be
 * in the editor — is the annoying case, and this is what distinguishes it.
 */
export const [searchInputFocused, setSearchInputFocused] = createSignal(false);

/**
 * Identity of the search the current results came from — root, query and
 * flags. Reopening the pane re-runs the effect, and comparing against this
 * is what stops an unchanged query from paying for the walk again.
 * Null means "no results yet", which is distinct from an empty result set.
 */
const [lastKey, setLastKey] = createSignal<string | null>(null);
export { lastKey as searchKey, setLastKey as setSearchKey };

export function searchKeyFor(
  root: string,
  query: string,
  opts: {
    caseSensitive: boolean;
    regex: boolean;
    noIgnore: boolean;
    word: boolean;
  },
): string {
  return [
    root,
    opts.caseSensitive ? "c" : "-",
    opts.regex ? "r" : "-",
    opts.noIgnore ? "i" : "-",
    opts.word ? "w" : "-",
    query,
  ].join("\0");
}
