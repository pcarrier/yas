/** A user-visible operation that may take long enough to need status UI. */
export interface YasActivity {
  id: number;
  kind: "upload" | "download" | "sync" | "search" | "operation";
  /** The item being acted on: normally a file name or path. */
  label: string;
  /** Optional destination/source, such as a surface title or terminal cwd. */
  target?: string;
  /** Completed and total work in producer-defined units (bytes for uploads). */
  completed?: number;
  total?: number;
  startedAt: number;
}

export interface YasActivityUpdate {
  label?: string;
  target?: string;
  completed?: number;
  total?: number;
}

export interface YasActivityHandle {
  readonly id: number;
  update(update: YasActivityUpdate): void;
  finish(): void;
}

/** Workspace-scoped registry consumed by status bars and other shell chrome.
 * Producers own handles, update them as work advances, and always finish them
 * on success, failure, or cancellation. */
export class YasActivityStore {
  private readonly records = new Map<number, YasActivity>();
  private readonly listeners = new Set<() => void>();
  private snapshot: readonly YasActivity[] = [];
  private nextId = 1;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): readonly YasActivity[] => this.snapshot;

  begin(activity: Omit<YasActivity, "id" | "startedAt">): YasActivityHandle {
    const id = this.nextId++;
    this.records.set(id, {
      ...activity,
      id,
      startedAt: Date.now(),
    });
    this.emit();
    let finished = false;
    return {
      id,
      update: (update) => {
        if (finished) return;
        const current = this.records.get(id);
        if (!current) return;
        this.records.set(id, { ...current, ...update });
        this.emit();
      },
      finish: () => {
        if (finished) return;
        finished = true;
        if (this.records.delete(id)) this.emit();
      },
    };
  }

  clear(): void {
    if (this.records.size === 0) return;
    this.records.clear();
    this.emit();
  }

  private emit(): void {
    this.snapshot = [...this.records.values()];
    for (const listener of this.listeners) listener();
  }
}
