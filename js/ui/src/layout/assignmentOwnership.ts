/**
 * Keep the first pane owning each non-empty value. Pane assignments and their
 * stable backend references both describe visual ownership, so neither may be
 * duplicated during restore.
 */
export function uniquePaneValues(
  values: Readonly<Record<string, string | null | undefined>>,
  paneIds: readonly string[],
): Record<string, string> {
  const unique: Record<string, string> = {};
  const seen = new Set<string>();
  for (const paneId of paneIds) {
    const value = values[paneId];
    if (value == null || seen.has(value)) continue;
    seen.add(value);
    unique[paneId] = value;
  }
  return unique;
}

/** Merge a restore result without ever publishing two owners for one value. */
export function mergeUniquePaneAssignments(
  previous: Readonly<Record<string, string | null>>,
  resolved: Readonly<Record<string, string>>,
  paneIds: readonly string[],
): Record<string, string | null> {
  const merged: Record<string, string | null> = { ...previous, ...resolved };
  const seen = new Set<string>();
  for (const paneId of paneIds) {
    const value = merged[paneId] ?? null;
    if (value != null && seen.has(value)) {
      merged[paneId] = null;
      continue;
    }
    if (value != null) seen.add(value);
  }
  return merged;
}
