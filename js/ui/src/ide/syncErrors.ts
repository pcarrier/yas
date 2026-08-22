import { YasResultError, YAS_STATUS_RESOURCE_EXHAUSTED } from "@yas-run/core";

/** The per-connection sync cap refused us (docs/design/fs-watch.md
 * budgets). Transient in practice — slots free as idle warm sessions expire
 * and dock cards close — so openers re-attempt on a timer. */
export function isSyncLimitError(error: unknown): boolean {
  if (
    error instanceof YasResultError &&
    error.status === YAS_STATUS_RESOURCE_EXHAUSTED
  )
    return true;
  const message = error instanceof Error ? error.message : String(error);
  return /resource limit/i.test(message);
}
