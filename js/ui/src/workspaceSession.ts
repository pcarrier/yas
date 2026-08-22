import type { Accessor } from "solid-js";
import { createMemo, createSignal } from "solid-js";
import type {
  StoredWorkspaceSession,
  StoredWorkspaceSessionDevice,
  WorkspaceSessionPatch,
  WorkspaceSessionWorkspace,
} from "@yas-run/core";
import { WORKSPACE_SESSION_KEY_PREFIX } from "@yas-run/core";
import {
  type WorkspaceSessionHistoryMode,
  normalizeWorkspaceSessionId,
  workspaceSessionRequestFromHash,
  storedWorkspaceSessionId,
  writeWorkspaceSessionUrl,
} from "./workspaceSessionUrl";
import { t, tp } from "./i18n";

export interface WorkspaceSessionBinding {
  readonly id: string;
  readonly current: Accessor<StoredWorkspaceSession>;
  readonly restoring: Accessor<boolean>;
  patch(patch: WorkspaceSessionPatch): Promise<void>;
  setRemoteActive(name: string, active: boolean): Promise<void>;
  /** Workspace calls after its first stable-reference resolution pass. */
  finishRestoring(): void;
}

export interface WorkspaceSessionAttachmentLike {
  readonly id: string;
  getSnapshot(): StoredWorkspaceSession | null;
  subscribe(listener: () => void): () => void;
  update(patch: WorkspaceSessionPatch): Promise<StoredWorkspaceSession>;
  setRemoteActive(
    remoteName: string,
    active: boolean,
  ): Promise<StoredWorkspaceSession>;
  rename(name: string): Promise<StoredWorkspaceSession>;
  delete(): Promise<void>;
  detach(): void;
}

export interface WorkspaceSessionStoreSnapshotLike {
  readonly status: string;
  readonly sessions: readonly StoredWorkspaceSession[];
  readonly error: Error | null;
  readonly invalidRecords?: readonly { key: string; message: string }[];
  readonly quarantinedSessionIds?: readonly string[];
}

/** Structural seam keeps controller tests independent of the KV transport. */
export interface WorkspaceSessionStoreLike {
  start(): Promise<void>;
  subscribe(listener: () => void): () => void;
  getSnapshot(): WorkspaceSessionStoreSnapshotLike;
  getPresence?(id: string): "available" | "quarantined" | "absent";
  create(input?: {
    name?: string;
    activeRemotes?: readonly string[];
    workspace?: WorkspaceSessionWorkspace;
  }): Promise<StoredWorkspaceSession>;
  update(
    id: string,
    patch: WorkspaceSessionPatch,
  ): Promise<StoredWorkspaceSession>;
  setRemoteActive(
    id: string,
    remoteName: string,
    active: boolean,
  ): Promise<StoredWorkspaceSession>;
  rename(id: string, name: string): Promise<StoredWorkspaceSession>;
  delete(id: string): Promise<void>;
  attach(id: string): Promise<WorkspaceSessionAttachmentLike>;
}

export interface WorkspaceSessionDeviceStoreSnapshotLike {
  readonly status: string;
  readonly device: StoredWorkspaceSessionDevice | null;
  readonly error: Error | null;
}

/** Structural seam for the one durable, same-device tab-membership record. */
export interface WorkspaceSessionDeviceStoreLike {
  start(): Promise<void>;
  subscribe(listener: () => void): () => void;
  getSnapshot(): WorkspaceSessionDeviceStoreSnapshotLike;
  attach(
    sessionId: string,
    options?: { beforeSessionId?: string },
  ): Promise<StoredWorkspaceSessionDevice>;
  claimInitialSession(sessionId: string): Promise<{
    device: StoredWorkspaceSessionDevice;
    claimed: boolean;
  }>;
  detach(sessionId: string): Promise<StoredWorkspaceSessionDevice | null>;
  reorder(
    sessionIds: readonly string[],
  ): Promise<StoredWorkspaceSessionDevice | null>;
  pruneDeleted(
    validSessionIds: ReadonlySet<string> | readonly string[],
  ): Promise<StoredWorkspaceSessionDevice | null>;
}

export interface WorkspaceSessionControllerOptions {
  store: WorkspaceSessionStoreLike;
  deviceStore: WorkspaceSessionDeviceStoreLike;
  /** Snapshot after credential removal, before canonical workspace replacement. */
  initialHash: string;
}

export interface WorkspaceSessionController {
  /** Every valid backend record, for the manager. */
  readonly sessions: Accessor<readonly StoredWorkspaceSession[]>;
  /** Device-attached records in durable tab order. */
  readonly attachedSessions: Accessor<readonly StoredWorkspaceSession[]>;
  readonly attachedSessionIds: Accessor<readonly string[]>;
  readonly current: Accessor<StoredWorkspaceSession | null>;
  readonly binding: Accessor<WorkspaceSessionBinding | null>;
  readonly loading: Accessor<boolean>;
  readonly restoring: Accessor<boolean>;
  readonly error: Accessor<string | null>;
  readonly warnings: Accessor<readonly string[]>;
  readonly managerOpen: Accessor<boolean>;
  start(): Promise<void>;
  retry(): Promise<void>;
  /** Attach to this device if needed, then select it in this browser tab. */
  attach(id: string, mode?: WorkspaceSessionHistoryMode): Promise<void>;
  /** Select an already attached tab. Falls back to attach if necessary. */
  select(id: string, mode?: WorkspaceSessionHistoryMode): Promise<void>;
  /** Remove one tab from this device without deleting its backend record. */
  detach(id?: string, mode?: WorkspaceSessionHistoryMode): Promise<void>;
  /** Create the next numerically named workspace, attach it, and select it. */
  create(): Promise<void>;
  rename(id: string, name: string): Promise<void>;
  delete(id: string, mode?: WorkspaceSessionHistoryMode): Promise<void>;
  setRemoteActive(name: string, active: boolean): Promise<void>;
  openManager(): void;
  closeManager(): void;
  dispose(): void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function adjacentSessionId(
  before: readonly string[],
  after: readonly string[],
  removedId: string,
): string | null {
  if (after.length === 0) return null;
  const removedIndex = before.indexOf(removedId);
  if (removedIndex < 0) return after[0] ?? null;
  return after[removedIndex] ?? after[removedIndex - 1] ?? after[0] ?? null;
}

export function createWorkspaceSessionController(
  options: WorkspaceSessionControllerOptions,
): WorkspaceSessionController {
  const [catalogue, setCatalogue] = createSignal<
    readonly StoredWorkspaceSession[]
  >([]);
  const [attachedIds, setAttachedIds] = createSignal<readonly string[]>([]);
  const [current, setCurrent] = createSignal<StoredWorkspaceSession | null>(
    null,
  );
  const [binding, setBinding] = createSignal<WorkspaceSessionBinding | null>(
    null,
  );
  const [loading, setLoading] = createSignal(true);
  const [restoring, setRestoring] = createSignal(false);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [sessionStoreError, setSessionStoreError] = createSignal<string | null>(
    null,
  );
  const [deviceStoreError, setDeviceStoreError] = createSignal<string | null>(
    null,
  );
  const [warnings, setWarnings] = createSignal<readonly string[]>([]);
  const [managerOpen, setManagerOpen] = createSignal(false);

  const sessions = createMemo(() =>
    [...catalogue()].sort(
      (left, right) =>
        right.updatedAtUnixMs - left.updatedAtUnixMs ||
        left.id.localeCompare(right.id),
    ),
  );
  const attachedSessions = createMemo(() => {
    const byId = new Map(catalogue().map((session) => [session.id, session]));
    return attachedIds()
      .map((id) => byId.get(id))
      .filter((session): session is StoredWorkspaceSession => !!session);
  });
  const error = createMemo(
    () => actionError() ?? sessionStoreError() ?? deviceStoreError(),
  );

  let attachment: WorkspaceSessionAttachmentLike | null = null;
  let unsubscribeAttachment: (() => void) | null = null;
  let unsubscribeStore: (() => void) | null = null;
  let unsubscribeDeviceStore: (() => void) | null = null;
  let startPromise: Promise<void> | null = null;
  let started = false;
  let operation = 0;
  let disposed = false;
  let listening = false;
  let navigationTarget:
    | { readonly id: string | null; readonly ticket: number }
    | undefined;
  let failedAction: (() => Promise<void>) | null = null;
  let localDeviceMutations = 0;
  let deviceMutationTail: Promise<void> = Promise.resolve();
  let deviceReconcileTimer: ReturnType<typeof setTimeout> | undefined;
  let deviceRecoveryId: string | null = null;
  let removingSessionId: string | null = null;
  let pruneScheduled = false;
  let pruneTimer: ReturnType<typeof setTimeout> | undefined;
  let pruneAttemptKey = "";
  let pruneAttempts = 0;
  let initialWorkspaceCandidate: StoredWorkspaceSession | null = null;
  let latestSelectionIntentId: string | null = null;
  const remoteMutationTails = new Map<string, Promise<void>>();

  const updateDeviceSnapshot = () => {
    if (disposed) return;
    const snapshot = options.deviceStore.getSnapshot();
    const previous = attachedIds();
    const next = snapshot.device?.attachedSessionIds ?? [];
    setAttachedIds(next);
    setDeviceStoreError(snapshot.error ? snapshot.error.message : null);
    schedulePruneDeleted();

    const selectedId = current()?.id;
    if (
      started &&
      selectedId &&
      !next.includes(selectedId) &&
      localDeviceMutations === 0 &&
      removingSessionId !== selectedId &&
      deviceRecoveryId !== selectedId &&
      (latestSelectionIntentId == null ||
        latestSelectionIntentId === selectedId)
    ) {
      deviceRecoveryId = selectedId;
      void followExternalDeviceDetach(selectedId, previous, next).finally(
        () => {
          if (deviceRecoveryId === selectedId) deviceRecoveryId = null;
        },
      );
    }
  };

  const syncStore = () => {
    const snapshot = options.store.getSnapshot();
    setCatalogue(snapshot.sessions);
    const invalidWarnings = (snapshot.invalidRecords ?? []).map(
      (record) => `${record.key}: ${record.message}`,
    );
    setWarnings(invalidWarnings);
    // A ready catalogue reports quarantined documents through both `error`
    // and `invalidRecords`; those are warnings, not a transport failure.
    const operationalError =
      snapshot.status === "ready" && invalidWarnings.length > 0
        ? null
        : snapshot.error;
    setSessionStoreError(operationalError?.message ?? null);
    schedulePruneDeleted();
  };

  const schedulePostMutationReconcile = () => {
    if (deviceReconcileTimer !== undefined || disposed) return;
    deviceReconcileTimer = setTimeout(() => {
      deviceReconcileTimer = undefined;
      if (disposed) return;
      if (localDeviceMutations > 0) {
        schedulePostMutationReconcile();
        return;
      }
      updateDeviceSnapshot();
    }, 0);
  };

  const withLocalDeviceMutation = <T>(action: () => Promise<T>): Promise<T> => {
    const mutation = deviceMutationTail
      .catch(() => {})
      .then(async () => {
        localDeviceMutations++;
        try {
          return await action();
        } finally {
          // Device writes are serialized in user-intent order. Reconcile while
          // suppressed, then once more after the awaiting caller has committed
          // its corresponding selection/detach state.
          updateDeviceSnapshot();
          localDeviceMutations--;
          if (localDeviceMutations === 0) schedulePostMutationReconcile();
        }
      });
    deviceMutationTail = mutation.then(
      () => undefined,
      () => undefined,
    );
    return mutation;
  };

  function schedulePruneDeleted(): void {
    if (pruneScheduled || !started || disposed) return;
    const sessionSnapshot = options.store.getSnapshot();
    const deviceSnapshot = options.deviceStore.getSnapshot();
    if (
      sessionSnapshot.status !== "ready" ||
      deviceSnapshot.status !== "ready"
    ) {
      return;
    }
    const valid = new Set(
      sessionSnapshot.sessions.map((session) => session.id),
    );
    for (const id of sessionSnapshot.quarantinedSessionIds ?? []) valid.add(id);
    // Invalid documents are quarantined out of `sessions`, but their keys are
    // still present. Keep their device memberships so a repaired document
    // returns to the same tab instead of being mistaken for a deletion.
    for (const record of sessionSnapshot.invalidRecords ?? []) {
      if (!record.key.startsWith(WORKSPACE_SESSION_KEY_PREFIX)) continue;
      const id = normalizeWorkspaceSessionId(
        record.key.slice(WORKSPACE_SESSION_KEY_PREFIX.length),
      );
      if (id) valid.add(id);
    }
    const invalidIds = attachedIds().filter((id) => !valid.has(id));
    if (invalidIds.length === 0) {
      pruneAttemptKey = "";
      pruneAttempts = 0;
      return;
    }
    const attemptKey = `${deviceSnapshot.device?.updatedAtUnixMs ?? 0}:${invalidIds.join(",")}`;
    if (attemptKey !== pruneAttemptKey) {
      pruneAttemptKey = attemptKey;
      pruneAttempts = 0;
    }
    if (pruneAttempts >= 3) return;
    pruneAttempts++;
    pruneScheduled = true;
    const retryDelay = pruneAttempts === 1 ? 0 : 20 * 2 ** (pruneAttempts - 2);
    pruneTimer = setTimeout(() => {
      pruneTimer = undefined;
      if (disposed) {
        pruneScheduled = false;
        return;
      }
      void withLocalDeviceMutation(() =>
        options.deviceStore.pruneDeleted(valid),
      )
        .catch((cause) => {
          setActionError(message(cause));
        })
        .finally(() => {
          pruneScheduled = false;
          schedulePruneDeleted();
        });
    }, retryDelay);
  }

  const clearSelected = (invalidate = true) => {
    if (invalidate) {
      operation++;
      latestSelectionIntentId = null;
    }
    unsubscribeAttachment?.();
    unsubscribeAttachment = null;
    attachment?.detach();
    attachment = null;
    setCurrent(null);
    setBinding(null);
    setRestoring(false);
    failedAction = null;
  };

  const syncAttachment = (expected: WorkspaceSessionAttachmentLike) => {
    if (attachment !== expected) return;
    const value = expected.getSnapshot();
    if (value) {
      setCurrent(value);
      return;
    }
    // A malformed replacement is present, not deleted. Keep the selected
    // last-good binding and device tab until the document is repaired.
    if (
      options.store.getPresence?.(expected.id) === "quarantined" ||
      options.store.getSnapshot().quarantinedSessionIds?.includes(expected.id)
    ) {
      return;
    }
    if (removingSessionId === expected.id || deviceRecoveryId === expected.id) {
      return;
    }
    deviceRecoveryId = expected.id;
    void recoverDeletedSelection(expected.id).finally(() => {
      if (deviceRecoveryId === expected.id) deviceRecoveryId = null;
    });
  };

  const patchSession = async (id: string, value: WorkspaceSessionPatch) => {
    setActionError(null);
    try {
      const updated = await options.store.update(id, value);
      failedAction = null;
      syncStore();
      if (attachment?.id === id) setCurrent(updated);
    } catch (cause) {
      setActionError(message(cause));
      failedAction = () => patchSession(id, value);
      throw cause;
    }
  };

  const applyRemoteActiveFor = async (
    id: string,
    name: string,
    active: boolean,
  ) => {
    if (name === "local") return;
    setActionError(null);
    try {
      const updated = await options.store.setRemoteActive(id, name, active);
      failedAction = null;
      syncStore();
      if (attachment?.id === id) setCurrent(updated);
    } catch (cause) {
      setActionError(message(cause));
      failedAction = () => enqueueRemoteActiveFor(id, name, active);
      throw cause;
    }
  };

  const enqueueRemoteActiveFor = (
    id: string,
    name: string,
    active: boolean,
  ): Promise<void> => {
    const previous = remoteMutationTails.get(id) ?? Promise.resolve();
    const mutation = previous
      .catch(() => {})
      .then(() => applyRemoteActiveFor(id, name, active));
    // Keep the per-workspace queue usable after a failed durable mutation. The
    // returned promise still rejects so the caller and manager see the error.
    remoteMutationTails.set(
      id,
      mutation.catch(() => {}),
    );
    return mutation;
  };

  const reserveSelectionIntent = (id: string | null): number => {
    const ticket = ++operation;
    latestSelectionIntentId = id;
    return ticket;
  };

  const selectSession = async (
    id: string,
    mode: WorkspaceSessionHistoryMode,
    _ensureDeviceAttachment: boolean,
    reservedTicket?: number,
  ) => {
    if (disposed) return;
    // Every selection intent supersedes earlier async attachment work, even
    // when it re-selects the tab that is still currently visible.
    const ticket = reservedTicket ?? reserveSelectionIntent(id);
    if (ticket !== operation) return;
    latestSelectionIntentId = id;
    if (attachment?.id === id) {
      setActionError(null);
      try {
        await withLocalDeviceMutation(() => options.deviceStore.attach(id));
        if (disposed || ticket !== operation) return;
        failedAction = null;
        writeWorkspaceSessionUrl(id, mode);
        setManagerOpen(false);
        return;
      } catch (cause) {
        if (disposed || ticket !== operation) return;
        latestSelectionIntentId = attachment.id;
        setActionError(message(cause));
        failedAction = () => selectSession(id, "replace", true);
        schedulePostMutationReconcile();
        throw cause;
      }
    }

    setActionError(null);
    let next: WorkspaceSessionAttachmentLike | null = null;
    try {
      next = await options.store.attach(id);
      if (disposed || ticket !== operation) {
        next.detach();
        return;
      }
      let value = next.getSnapshot();
      if (!value) throw new Error(t("sessions.notFound"));
      // Selection and durable tab membership commit together. Even callers
      // that observed this ID in an earlier device snapshot must attach
      // idempotently: another browser tab can detach it during store.attach.
      await withLocalDeviceMutation(() => options.deviceStore.attach(id));
      if (disposed || ticket !== operation) {
        next.detach();
        return;
      }

      // The record may have been deleted while its device attachment CAS was
      // pending, before this handle was subscribed. Never install the stale
      // pre-CAS snapshot or leave a deleted UUID in the device tab record.
      value = next.getSnapshot();
      if (!value) {
        const quarantined =
          options.store.getPresence?.(id) === "quarantined" ||
          options.store.getSnapshot().quarantinedSessionIds?.includes(id);
        if (!quarantined) {
          await withLocalDeviceMutation(() =>
            options.deviceStore.detach(id),
          ).catch(() => {});
        }
        throw new Error(
          quarantined ? t("sessions.quarantined") : t("sessions.notFound"),
        );
      }

      unsubscribeAttachment?.();
      attachment?.detach();
      attachment = next;
      unsubscribeAttachment = next.subscribe(() => syncAttachment(next!));
      // Assert the new binding's hydration barrier only after swapping the
      // attachment identity. The old binding can no longer clear this gate.
      setRestoring(true);
      setCurrent(value);
      let lastValue = value;
      setBinding({
        id: value.id,
        current: () => {
          const record = current();
          if (record?.id === value.id) lastValue = record;
          // A keyed Workspace cleanup may sample its old binding once after a
          // new workspace was selected. Never let it read or patch the new one.
          return lastValue;
        },
        restoring: () => attachment === next && restoring(),
        patch: (patchValue) => patchSession(id, patchValue),
        setRemoteActive: (name, active) =>
          enqueueRemoteActiveFor(id, name, active),
        finishRestoring: () => {
          if (attachment === next) setRestoring(false);
        },
      });
      failedAction = null;
      writeWorkspaceSessionUrl(value.id, mode);
      setManagerOpen(false);
    } catch (cause) {
      next?.detach();
      // A superseded selection is cancellation, even if its delayed backend
      // attach eventually reports that the record was deleted.
      if (disposed || ticket !== operation) return;
      latestSelectionIntentId = attachment?.id ?? null;
      setActionError(message(cause));
      schedulePostMutationReconcile();
      throw cause;
    }
  };

  async function followExternalDeviceDetach(
    id: string,
    before: readonly string[],
    after: readonly string[],
  ): Promise<void> {
    if (current()?.id !== id) return;
    const target = adjacentSessionId(before, after, id);
    clearSelected();
    if (target) {
      try {
        await selectSession(target, "replace", false);
      } catch {
        // selectSession retains the backend error for the tab-bar badge and
        // the manager's next explicit opening.
      }
    } else {
      writeWorkspaceSessionUrl(null, "replace");
    }
  }

  async function recoverDeletedSelection(id: string): Promise<void> {
    const ticket = operation;
    if (
      current()?.id !== id ||
      (latestSelectionIntentId != null && latestSelectionIntentId !== id)
    ) {
      return;
    }
    const before = attachedIds();
    setActionError(t("sessions.selectedMissing"));
    let after = before.filter((candidate) => candidate !== id);
    try {
      const device = await withLocalDeviceMutation(() =>
        options.deviceStore.detach(id),
      );
      after = device?.attachedSessionIds ?? [];
    } catch (cause) {
      if (ticket === operation) setActionError(message(cause));
    }
    if (disposed || ticket !== operation || current()?.id !== id) return;
    const target = adjacentSessionId(before, after, id);
    clearSelected();
    if (target) {
      try {
        await selectSession(target, "replace", false);
      } catch {
        // The manager will contain the actionable error when opened.
      }
    } else {
      writeWorkspaceSessionUrl(null, "replace");
    }
  }

  const createAndSelect = async (
    name: string | undefined,
    workspace: WorkspaceSessionWorkspace | undefined,
    mode: WorkspaceSessionHistoryMode,
    reservedTicket = reserveSelectionIntent(null),
  ) => {
    const created = await options.store.create({
      name,
      activeRemotes: [],
      workspace,
    });
    syncStore();
    // Creation includes durable membership on this device. Selection may be
    // superseded by newer navigation, but that must not turn a newly created
    // workspace into an unattached saved record.
    await withLocalDeviceMutation(() => options.deviceStore.attach(created.id));
    if (disposed || reservedTicket !== operation) return;
    await selectSession(created.id, mode, true, reservedTicket);
  };

  const selectBase = async (
    mode: WorkspaceSessionHistoryMode,
    reservedTicket = reserveSelectionIntent(null),
  ) => {
    if (reservedTicket !== operation) return;
    const device = options.deviceStore.getSnapshot().device;
    const valid = new Set(catalogue().map((session) => session.id));
    const target = device?.attachedSessionIds.find((id) => valid.has(id));
    if (target) {
      await selectSession(target, mode, false, reservedTicket);
      return;
    }
    if (device) {
      if (reservedTicket !== operation) return;
      clearSelected(false);
      writeWorkspaceSessionUrl(null, mode);
      return;
    }

    // Only an absent device record gets an automatic workspace. Two browser
    // tabs can race first boot, so each creates a candidate and the device
    // store atomically claims exactly one of them.
    initialWorkspaceCandidate ??= await options.store.create({
      activeRemotes: [],
    });
    syncStore();
    const candidate = initialWorkspaceCandidate;
    if (disposed || reservedTicket !== operation) {
      initialWorkspaceCandidate = null;
      void options.store.delete(candidate.id).catch(() => {});
      return;
    }
    const claim = await withLocalDeviceMutation(() =>
      options.deviceStore.claimInitialSession(candidate.id),
    );
    if (disposed || reservedTicket !== operation) {
      initialWorkspaceCandidate = null;
      if (!claim.claimed)
        void options.store.delete(candidate.id).catch(() => {});
      return;
    }
    if (claim.claimed) {
      initialWorkspaceCandidate = null;
      await selectSession(candidate.id, mode, false, reservedTicket);
      return;
    }

    // Another tab won, or an intentionally-empty device record appeared.
    // The losing candidate is not attached anywhere and can be removed.
    initialWorkspaceCandidate = null;
    let cleanupError: string | null = null;
    try {
      await options.store.delete(candidate.id);
      syncStore();
    } catch (cause) {
      // Cleanup is best-effort: a transient delete failure must not strand
      // this tab instead of selecting the candidate another tab claimed.
      cleanupError = tp("sessions.cleanupUnusedFailed", {
        error: message(cause),
      });
    }
    const claimedTarget = claim.device.attachedSessionIds[0];
    if (claimedTarget) {
      await selectSession(claimedTarget, mode, false, reservedTicket);
      if (cleanupError) setActionError(cleanupError);
    } else {
      clearSelected(false);
      writeWorkspaceSessionUrl(null, mode);
      if (cleanupError) setActionError(cleanupError);
    }
  };

  const bootstrap = async () => {
    if (disposed) return;
    setLoading(true);
    setActionError(null);
    try {
      await Promise.all([options.store.start(), options.deviceStore.start()]);
      if (disposed) return;
      unsubscribeStore ??= options.store.subscribe(syncStore);
      unsubscribeDeviceStore ??=
        options.deviceStore.subscribe(updateDeviceSnapshot);
      syncStore();
      updateDeviceSnapshot();

      // Store readiness, subscriptions, and navigation form the controller
      // lifecycle. Keep them active even when the initially requested workspace
      // is missing or quarantined so the manager can recover without a remount.
      if (!listening) {
        window.addEventListener("popstate", navigate);
        window.addEventListener("hashchange", navigate);
        listening = true;
      }
      started = true;
      schedulePruneDeleted();

      const bootstrapHash =
        typeof location === "undefined" ? options.initialHash : location.hash;
      const request = workspaceSessionRequestFromHash(bootstrapHash);
      // A share link wins over what this device last had open: following a
      // link someone handed you is an explicit act, and it is the only way a
      // workspace id reaches an address bar now. Otherwise the attachment comes
      // from storage, which is where selecting a tab records it.
      const requested =
        request.id ?? (request.present ? null : storedWorkspaceSessionId());
      if (request.present && !request.id) {
        reserveSelectionIntent(attachment?.id ?? null);
        setActionError(t("sessions.invalidUrlId"));
      } else if (requested) {
        await selectSession(requested, "replace", true);
      } else {
        await selectBase("replace");
      }
    } catch (cause) {
      if (!disposed) {
        setActionError(message(cause));
      }
    } finally {
      if (!disposed) setLoading(false);
    }
  };

  const start = () => {
    if (started) return Promise.resolve();
    startPromise ??= bootstrap().finally(() => {
      startPromise = null;
    });
    return startPromise;
  };

  const retry = async () => {
    const action = failedAction;
    if (action) {
      failedAction = null;
      await action();
      return;
    }
    setActionError(null);
    if (!started) {
      await start();
      return;
    }
    await Promise.all([options.store.start(), options.deviceStore.start()]);
    syncStore();
    updateDeviceSnapshot();
  };

  const navigate = () => {
    if (disposed) return;
    const request = workspaceSessionRequestFromHash(location.hash);
    if (request.present && !request.id) {
      reserveSelectionIntent(attachment?.id ?? null);
      setActionError(t("sessions.invalidUrlId"));
      navigationTarget = undefined;
      return;
    }
    const id = request.id;
    // Browsers can deliver both popstate and hashchange for one address
    // transition. Only deduplicate while that exact selection generation is
    // still current: a user tab selection between two visits to the same URL
    // must make the second visit a fresh, authoritative intent.
    if (navigationTarget?.id === id && navigationTarget.ticket === operation) {
      return;
    }
    const ticket = reserveSelectionIntent(id);
    const target = { id, ticket };
    navigationTarget = target;
    const action = id
      ? selectSession(id, "replace", true, ticket)
      : selectBase("replace", ticket);
    void action
      .catch(() => {})
      .finally(() => {
        if (navigationTarget === target) navigationTarget = undefined;
      });
  };

  const controller: WorkspaceSessionController = {
    sessions,
    attachedSessions,
    attachedSessionIds: attachedIds,
    current,
    binding,
    loading,
    restoring,
    error,
    warnings,
    managerOpen,
    start,
    retry,
    attach(id, mode = "push") {
      return selectSession(id, mode, true);
    },
    select(id, mode = "push") {
      return selectSession(id, mode, true);
    },
    async detach(id = current()?.id, mode = "replace") {
      if (!id) return;
      const before = attachedIds();
      const selected = current()?.id === id;
      const pending = latestSelectionIntentId === id;
      const ticket =
        selected || pending
          ? reserveSelectionIntent(selected ? null : (attachment?.id ?? null))
          : operation;
      if (!attachedIds().includes(id)) return;
      setActionError(null);
      if (selected) removingSessionId = id;
      try {
        const device = await withLocalDeviceMutation(() =>
          options.deviceStore.detach(id),
        );
        const after = device?.attachedSessionIds ?? [];
        if (!selected || ticket !== operation || current()?.id !== id) {
          return;
        }
        const target = adjacentSessionId(before, after, id);
        clearSelected();
        if (target) await selectSession(target, mode, false);
        else {
          writeWorkspaceSessionUrl(null, mode);
        }
      } catch (cause) {
        if (selected && ticket === operation) {
          latestSelectionIntentId = attachment?.id ?? null;
          schedulePostMutationReconcile();
        }
        setActionError(message(cause));
        throw cause;
      } finally {
        if (removingSessionId === id) removingSessionId = null;
      }
    },
    async create() {
      setActionError(null);
      try {
        await createAndSelect(undefined, undefined, "push");
      } catch (cause) {
        setActionError(message(cause));
        throw cause;
      }
    },
    async rename(id, name) {
      const trimmed = name.trim();
      if (!trimmed) return;
      setActionError(null);
      try {
        const updated =
          attachment?.id === id
            ? await attachment.rename(trimmed)
            : await options.store.rename(id, trimmed);
        if (attachment?.id === id) setCurrent(updated);
        syncStore();
      } catch (cause) {
        setActionError(message(cause));
        throw cause;
      }
    },
    async delete(id, mode = "replace") {
      const before = attachedIds();
      const selected = current()?.id === id;
      const pending = latestSelectionIntentId === id;
      const ticket =
        selected || pending
          ? reserveSelectionIntent(selected ? null : (attachment?.id ?? null))
          : operation;
      const expected = selected ? attachment : null;
      setActionError(null);
      removingSessionId = id;
      let deleted = false;
      let after = before.filter((candidate) => candidate !== id);
      try {
        if (expected) await expected.delete();
        else await options.store.delete(id);
        deleted = true;
        syncStore();
        try {
          const device = await withLocalDeviceMutation(() =>
            options.deviceStore.detach(id),
          );
          after = device?.attachedSessionIds ?? [];
        } catch (cause) {
          // The deleted record cannot remain a visible tab; the catalogue
          // pruner will retry removing the stale device membership.
          setActionError(message(cause));
        }
      } catch (cause) {
        if (selected && ticket === operation) {
          latestSelectionIntentId = attachment?.id ?? null;
          schedulePostMutationReconcile();
        }
        setActionError(message(cause));
        throw cause;
      } finally {
        removingSessionId = null;
      }

      if (
        !deleted ||
        !selected ||
        ticket !== operation ||
        current()?.id !== id
      ) {
        return;
      }
      const target = adjacentSessionId(before, after, id);
      clearSelected();
      if (target) {
        await selectSession(target, mode, false);
      } else {
        writeWorkspaceSessionUrl(null, mode);
      }
    },
    async setRemoteActive(name, active) {
      const expected = attachment;
      if (!expected) return;
      await enqueueRemoteActiveFor(expected.id, name, active);
    },
    openManager() {
      setManagerOpen(true);
    },
    closeManager() {
      setManagerOpen(false);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      clearTimeout(deviceReconcileTimer);
      deviceReconcileTimer = undefined;
      clearTimeout(pruneTimer);
      pruneTimer = undefined;
      clearSelected();
      unsubscribeStore?.();
      unsubscribeStore = null;
      unsubscribeDeviceStore?.();
      unsubscribeDeviceStore = null;
      if (listening) {
        window.removeEventListener("popstate", navigate);
        window.removeEventListener("hashchange", navigate);
        listening = false;
      }
    },
  };

  return controller;
}
