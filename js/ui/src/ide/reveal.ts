/**
 * Ephemeral "reveal this position when the editor opens" intents.
 *
 * The commit view can't map a historical line to the live file precisely
 * (line numbers drift), so it hands the editor the line's *text* plus its
 * old line number as a hint. YasEditor consumes the intent once on mount and
 * relocates the position by matching the text (nearest to the hint line),
 * falling back to the line number. Keyed by connectionId + absolute path.
 */

import { createSignal } from "solid-js";

export interface RevealIntent {
  /** The source line's text, to relocate it in the current file. */
  text: string;
  /** Source line number (1-based) as a fallback / tie-breaker. */
  line: number;
  /** Optional 0-based UTF-8 byte column to land on within the line — an
   *  LSP jump's exact position, or a search hit's match offset. Applied
   *  whether or not `text` relocated the line: relocation picks the line,
   *  this picks the spot in it. */
  col?: number;
}

const pending = new Map<string, RevealIntent>();
const key = (connectionId: string, path: string) => `${connectionId}\0${path}`;

/** Bumped by every {@link setReveal}. A tile already mounted on the target
 *  file gets no new component instance to consume the intent — clicking a
 *  second problem in the open file, or the same one twice — so the editor
 *  watches this and re-consumes. */
const [revealVersion, bumpReveal] = createSignal(0);
export { revealVersion };

export function setReveal(
  connectionId: string,
  path: string,
  intent: RevealIntent,
): void {
  pending.set(key(connectionId, path), intent);
  bumpReveal((v) => v + 1);
}

/** Read and clear the pending reveal for (connectionId, path), if any. */
export function consumeReveal(
  connectionId: string,
  path: string,
): RevealIntent | null {
  const k = key(connectionId, path);
  const v = pending.get(k) ?? null;
  if (v) pending.delete(k);
  return v;
}
