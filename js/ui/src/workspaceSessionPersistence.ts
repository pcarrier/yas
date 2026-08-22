import type {
  WorkspaceSessionPatch,
  WorkspaceSessionWorkspace,
} from "@yas-run/core";

function json(value: unknown): string {
  return JSON.stringify(value);
}

function assignmentsJson(value: Readonly<Record<string, string>>): string {
  return json(
    Object.fromEntries(
      Object.entries(value).sort(([left], [right]) =>
        left.localeCompare(right),
      ),
    ),
  );
}

function stringSetJson(value: readonly string[]): string {
  return json([...new Set(value)].sort());
}

/**
 * Field-semantic patch from a stored snapshot to the current UI. Unchanged
 * workspace fields are absent, allowing the store's CAS retry to merge a
 * concurrent layout edit with a panel edit instead of replacing either one.
 */
export function workspaceSessionPatch(
  stored: WorkspaceSessionWorkspace,
  current: WorkspaceSessionWorkspace,
): WorkspaceSessionPatch | null {
  const workspace: NonNullable<WorkspaceSessionPatch["workspace"]> = {};
  if (json(stored.layout) !== json(current.layout)) {
    workspace.layout = current.layout;
  }
  if (
    assignmentsJson(stored.assignments) !== assignmentsJson(current.assignments)
  ) {
    workspace.assignments = current.assignments;
  }
  // Focus and the active view tab are client-local. Keep accepting the legacy
  // field in records, but never publish this client's navigation into the
  // shared workspace document.
  if (stored.main !== current.main) workspace.main = current.main;

  const panels: NonNullable<
    NonNullable<WorkspaceSessionPatch["workspace"]>["panels"]
  > = {};
  if (stored.panels.leftOpen !== current.panels.leftOpen) {
    panels.leftOpen = current.panels.leftOpen;
  }
  if (stored.panels.previewOpen !== current.panels.previewOpen) {
    panels.previewOpen = current.panels.previewOpen;
  }
  if (
    stringSetJson(stored.panels.expandedSections) !==
    stringSetJson(current.panels.expandedSections)
  ) {
    panels.expandedSections = current.panels.expandedSections;
  }
  if (json(stored.panels.project) !== json(current.panels.project)) {
    panels.project = current.panels.project;
  }
  if (stored.panels.musterExpanded !== current.panels.musterExpanded) {
    panels.musterExpanded = current.panels.musterExpanded;
  }
  if (stored.panels.debugOpen !== current.panels.debugOpen) {
    panels.debugOpen = current.panels.debugOpen;
  }
  if (Object.keys(panels).length > 0) workspace.panels = panels;
  return Object.keys(workspace).length > 0 ? { workspace } : null;
}

export interface WorkspaceSessionPatchTarget {
  patch(patch: WorkspaceSessionPatch): Promise<void>;
}

/**
 * Serialize persistence for one attached workspace.
 *
 * A semantic CAS retry inside the backend cannot establish ordering between
 * two independent client calls. Keeping one call in flight prevents an older
 * layout/panel write from landing after a newer one. Changes observed during
 * the request are coalesced and diffed from the last successful UI baseline.
 */
export class WorkspaceSessionPatchSequencer {
  private target: WorkspaceSessionPatchTarget | null = null;
  private committed: WorkspaceSessionWorkspace | null = null;
  private desired: WorkspaceSessionWorkspace | null = null;
  private inFlight = false;
  private generation = 0;
  private disposed = false;
  private finishing = false;

  reset(
    target: WorkspaceSessionPatchTarget | null,
    baseline: WorkspaceSessionWorkspace,
  ): void {
    if (this.disposed) return;
    this.generation++;
    this.finishing = false;
    this.target = target;
    this.committed = baseline;
    this.desired = baseline;
    this.inFlight = false;
  }

  /** Hold UI changes behind hydration while retaining the stored baseline. */
  stage(
    target: WorkspaceSessionPatchTarget,
    stored: WorkspaceSessionWorkspace,
    workspace: WorkspaceSessionWorkspace,
  ): void {
    if (this.disposed) return;
    if (this.target !== target) this.reset(target, stored);
    if (!this.inFlight) this.committed = stored;
    this.desired = workspace;
  }

  submit(
    target: WorkspaceSessionPatchTarget,
    workspace: WorkspaceSessionWorkspace,
  ): void {
    if (this.disposed) return;
    if (this.target !== target || !this.committed) {
      // A newly attached workspace starts from its restored UI. Never compare
      // it with the previous workspace's baseline.
      this.reset(target, workspace);
      return;
    }
    this.desired = workspace;
    this.drain();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.target = null;
    this.committed = null;
    this.desired = null;
    this.inFlight = false;
  }

  /** Flush the latest desired state, then release this queue after it drains. */
  finishAfterDrain(): void {
    if (this.disposed) return;
    this.finishing = true;
    this.drain();
    if (!this.inFlight && !this.pendingPatch()) this.dispose();
  }

  private pendingPatch(): WorkspaceSessionPatch | null {
    return this.committed && this.desired
      ? workspaceSessionPatch(this.committed, this.desired)
      : null;
  }

  private drain(): void {
    const target = this.target;
    const committed = this.committed;
    const desired = this.desired;
    if (this.disposed || this.inFlight || !target || !committed || !desired) {
      return;
    }
    const patch = workspaceSessionPatch(committed, desired);
    if (!patch) {
      if (this.finishing) this.dispose();
      return;
    }

    const generation = this.generation;
    const submitted = desired;
    this.inFlight = true;
    void target.patch(patch).then(
      () => {
        if (this.disposed || generation !== this.generation) return;
        this.committed = submitted;
        this.inFlight = false;
        this.drain();
      },
      () => {
        if (this.disposed || generation !== this.generation) return;
        this.inFlight = false;
        // Avoid an unbounded retry loop for one failed snapshot. If the UI
        // changed while it was pending, however, send one coalesced latest
        // patch from the still-authoritative successful baseline.
        if (
          this.desired &&
          workspaceSessionPatch(submitted, this.desired) !== null
        ) {
          this.drain();
        } else if (this.finishing) {
          this.dispose();
        }
      },
    );
  }
}
