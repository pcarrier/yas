import { createSignal } from "solid-js";

interface ClosingTab {
  operation: number;
  settled: boolean;
}

/** Hide a closed tab until both its KV deletion and the registry watch agree.
 *
 * Removing a pane is synchronous, while the shared tab registry is not. Without
 * this tombstone the registry's old record makes the closed pane appear in the
 * sidebar until the delete reaches the watch.
 */
export function createTabCloseTracker() {
  const [closing, setClosing] = createSignal<
    ReadonlyMap<string, ClosingTab>
  >(new Map());
  let nextOperation = 0;

  const remove = (assignment: string, operation?: number) => {
    setClosing((previous) => {
      const current = previous.get(assignment);
      if (!current || (operation != null && current.operation !== operation))
        return previous;
      const next = new Map(previous);
      next.delete(assignment);
      return next;
    });
  };

  return {
    isClosing(assignment: string): boolean {
      return closing().has(assignment);
    },
    begin(assignment: string): number {
      const operation = ++nextOperation;
      setClosing((previous) => {
        const next = new Map(previous);
        next.set(assignment, { operation, settled: false });
        return next;
      });
      return operation;
    },
    /** A new open wins over an older close still awaiting its watch delta. */
    reopen(assignment: string): void {
      remove(assignment);
    },
    settle(
      assignment: string,
      operation: number,
      succeeded: boolean,
      stillRegistered: boolean,
    ): void {
      if (!succeeded || !stillRegistered) {
        remove(assignment, operation);
        return;
      }
      setClosing((previous) => {
        const current = previous.get(assignment);
        if (!current || current.operation !== operation) return previous;
        const next = new Map(previous);
        next.set(assignment, { ...current, settled: true });
        return next;
      });
    },
    reconcile(registered: ReadonlySet<string>): void {
      setClosing((previous) => {
        let next: Map<string, ClosingTab> | undefined;
        for (const [assignment, state] of previous) {
          if (!state.settled || registered.has(assignment)) continue;
          next ??= new Map(previous);
          next.delete(assignment);
        }
        return next ?? previous;
      });
    },
  };
}
