/**
 * Decide whether an editor's first single-file snapshot can be resolved.
 *
 * FS lifecycle callbacks may be released before the promise returning their
 * handle reaches its caller. Absence is therefore conclusive only after both
 * the handle and its coherent snapshot are available. A present node may be
 * loaded as soon as the handle exposes it.
 */
export function resolveInitialFileSnapshot<T>(
  handle: T | null,
  synced: boolean,
  loadIfPresent: (handle: T) => boolean,
): "waiting" | "loaded" | "missing" {
  if (!handle) return "waiting";
  if (loadIfPresent(handle)) return "loaded";
  return synced ? "missing" : "waiting";
}
