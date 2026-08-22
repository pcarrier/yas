/** Compact relative age used by commit rows in the sidebar. */
import { tp } from "../i18n";

export function relativeTime(seconds: bigint, nowSec: number): string {
  let d = nowSec - Number(seconds);
  if (d < 0) d = 0;
  if (d < 60) return tp("relative.seconds", { count: d });
  if (d < 3600) return tp("relative.minutes", { count: Math.floor(d / 60) });
  if (d < 86400) return tp("relative.hours", { count: Math.floor(d / 3600) });
  if (d < 86400 * 30)
    return tp("relative.days", { count: Math.floor(d / 86400) });
  return tp("relative.months", { count: Math.floor(d / (86400 * 30)) });
}

/** Delay to the next Unix-second boundary, without accumulating drift. */
export function msUntilNextSecond(nowMs: number): number {
  const remainder = ((nowMs % 1000) + 1000) % 1000;
  return remainder === 0 ? 1000 : 1000 - remainder;
}
