/**
 * Per-file cursor + scroll memory. An editor tile is re-created when navigation
 * swaps a pane to another file (YasTile is keyed on its assignment), so without
 * this, returning to a file lands at the top with the cursor reset. Each editor
 * saves its position on unmount and restores it on its first load, making
 * navigation feel like the file's editor was kept alive.
 *
 * Session-scoped (a plain module Map); not persisted across reloads.
 */
export type EditorPosition = { anchor: number; head: number; top: number };

const positions = new Map<string, EditorPosition>();
export const EDITOR_POSITION_MAX_ITEMS = 4_096;
export const EDITOR_POSITION_MAX_BYTES = 8 * 1024 * 1024;
export const EDITOR_POSITION_MAX_KEY_CHARS = 8_192;
let positionBytes = 0;

// NUL separator: can't appear in a connection id or a path, so prefix
// matching in `editorRecencySnapshot` is exact. Kept as an escape — a raw
// NUL byte makes git treat the file as binary.
const SEP = "\u0000";

const key = (connectionId: string, path: string): string =>
  `${connectionId}${SEP}${path}`;

export function rememberEditorPosition(
  connectionId: string,
  path: string,
  pos: EditorPosition,
): void {
  // Delete-then-set keeps the map ordered by last touch, which is what
  // `editorRecencySnapshot` reads off the iteration order.
  const k = key(connectionId, path);
  if (k.length > EDITOR_POSITION_MAX_KEY_CHARS) return;
  if (positions.delete(k)) positionBytes -= 64 + k.length * 2;
  positions.set(k, pos);
  positionBytes += 64 + k.length * 2;
  while (
    positions.size > EDITOR_POSITION_MAX_ITEMS ||
    positionBytes > EDITOR_POSITION_MAX_BYTES
  ) {
    const oldest = positions.keys().next().value;
    if (oldest === undefined) break;
    positions.delete(oldest);
    positionBytes -= 64 + oldest.length * 2;
  }
}

export function recallEditorPosition(
  connectionId: string,
  path: string,
): EditorPosition | null {
  return positions.get(key(connectionId, path)) ?? null;
}

/** Absolute path → recency rank (0 = most recently touched) for one
 *  connection's remembered files. Feeds the @-search recency boost
 *  (ide/fileIndex.ts). */
export function editorRecencySnapshot(
  connectionId: string,
): Map<string, number> {
  const prefix = `${connectionId}${SEP}`;
  const ranked = new Map<string, number>();
  const keys = [...positions.keys()];
  for (let i = keys.length - 1; i >= 0; i--) {
    const k = keys[i];
    if (k.startsWith(prefix)) {
      ranked.set(k.slice(prefix.length), ranked.size);
    }
  }
  return ranked;
}

/** Forget cursor/scroll state for a route that left the workspace. */
export function dropEditorPositions(connectionId: string): void {
  const prefix = `${connectionId}${SEP}`;
  for (const key of [...positions.keys()]) {
    if (!key.startsWith(prefix)) continue;
    positions.delete(key);
    positionBytes -= 64 + key.length * 2;
  }
}

/** Test/diagnostic seam. */
export function editorPositionCacheStats(): { items: number; bytes: number } {
  return { items: positions.size, bytes: positionBytes };
}
