import { createRoot } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WORKSPACE_SESSION_STORAGE_KEY } from "../workspaceSessionUrl";
import type {
  StoredWorkspaceSession,
  StoredWorkspaceSessionDevice,
  WorkspaceSessionPatch,
} from "@yas-run/core";
import {
  createWorkspaceSessionController,
  type WorkspaceSessionAttachmentLike,
  type WorkspaceSessionDeviceStoreLike,
  type WorkspaceSessionStoreLike,
} from "../workspaceSession";

const A = "123e4567-e89b-42d3-a456-426614174000";
const B = "123e4567-e89b-42d3-a456-426614174001";
const C = "123e4567-e89b-42d3-a456-426614174002";
const D = "123e4567-e89b-42d3-a456-426614174003";
const E = "123e4567-e89b-42d3-a456-426614174004";

function record(
  id: string,
  name: string,
  updatedAtUnixMs: number,
  activeRemotes: string[] = [],
): StoredWorkspaceSession {
  return {
    version: 1,
    id,
    name,
    createdAtUnixMs: updatedAtUnixMs - 1,
    updatedAtUnixMs,
    activeRemotes,
    workspace: {
      layout: null,
      assignments: {},
      focusedPaneId: null,
      main: null,
      panels: {
        leftOpen: false,
        previewOpen: false,
        expandedSections: [],
        project: null,
        musterExpanded: false,
        debugOpen: false,
      },
    },
  };
}

function device(
  attachedSessionIds: readonly string[],
  updatedAtUnixMs = 10,
): StoredWorkspaceSessionDevice {
  return {
    version: 1,
    deviceId: D,
    attachedSessionIds: [...attachedSessionIds],
    createdAtUnixMs: 1,
    updatedAtUnixMs,
  };
}

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

class FakeStore implements WorkspaceSessionStoreLike {
  sessions: StoredWorkspaceSession[];
  readonly created: Array<Record<string, unknown>> = [];
  readonly attached: string[] = [];
  readonly remoteMutations: Array<{
    id: string;
    name: string;
    active: boolean;
  }> = [];
  readonly remoteMutationDelays: Promise<void>[] = [];
  readonly attachDelays = new Map<string, Promise<void>[]>();
  invalidRecords: Array<{ key: string; message: string }> = [];
  quarantinedSessionIds: string[] = [];
  attachmentMissing = new Set<string>();
  updateFailure: Error | null = null;
  storeError: Error | null = null;
  storeStatus = "ready";
  startFailure: Error | null = null;
  startDelay: Promise<void> | null = null;
  startStarted = 0;
  readonly createDelays: Promise<void>[] = [];
  deleteFailures = new Set<string>();
  private readonly listeners = new Set<() => void>();
  private readonly attachmentListeners = new Map<string, Set<() => void>>();

  constructor(sessions: StoredWorkspaceSession[] = []) {
    this.sessions = sessions;
  }

  async start() {
    this.startStarted++;
    if (this.startDelay) await this.startDelay;
    if (this.startFailure) {
      const error = this.startFailure;
      this.startFailure = null;
      throw error;
    }
  }

  subscribe(listener: () => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  getSnapshot() {
    return {
      status: this.storeStatus,
      sessions: this.sessions,
      error: this.storeError,
      invalidRecords: this.invalidRecords,
      quarantinedSessionIds: this.quarantinedSessionIds,
    };
  }

  getPresence(id: string): "available" | "quarantined" | "absent" {
    if (this.quarantinedSessionIds.includes(id)) return "quarantined";
    return this.sessions.some((session) => session.id === id)
      ? "available"
      : "absent";
  }

  async create(input: Record<string, unknown> = {}) {
    const delay = this.createDelays.shift();
    if (delay) await delay;
    this.created.push(input);
    const id = [C, E, B, A].find(
      (candidate) => !this.sessions.some((session) => session.id === candidate),
    );
    if (!id) throw new Error("test session IDs exhausted");
    let next = record(
      id,
      String(input.name ?? "New workspace"),
      100 + this.sessions.length,
      [...((input.activeRemotes as readonly string[] | undefined) ?? [])],
    );
    if (input.workspace) {
      next = { ...next, workspace: input.workspace as never };
    }
    this.sessions = [...this.sessions, next];
    this.emit();
    return next;
  }

  async rename(id: string, name: string) {
    return this.update(id, { name });
  }

  async delete(id: string) {
    if (this.deleteFailures.delete(id)) throw new Error("delete unavailable");
    this.sessions = this.sessions.filter((session) => session.id !== id);
    this.emit();
    this.emitAttachment(id);
  }

  async attach(id: string): Promise<WorkspaceSessionAttachmentLike> {
    this.attached.push(id);
    const delays = this.attachDelays.get(id);
    const delay = delays?.shift();
    if (delay) await delay;
    if (!this.find(id)) throw new Error("Workspace session not found");
    let detached = false;
    return {
      id,
      getSnapshot: () =>
        detached || this.attachmentMissing.has(id)
          ? null
          : (this.find(id) ?? null),
      subscribe: (listener) => {
        let listeners = this.attachmentListeners.get(id);
        if (!listeners) {
          listeners = new Set();
          this.attachmentListeners.set(id, listeners);
        }
        listeners.add(listener);
        return () => {
          listeners?.delete(listener);
        };
      },
      update: (patch) => this.update(id, patch),
      setRemoteActive: (name, active) => this.setRemoteActive(id, name, active),
      rename: (name) => this.update(id, { name }),
      delete: () => this.delete(id),
      detach: () => {
        detached = true;
      },
    };
  }

  publish() {
    this.emit();
  }

  publishAttachment(id: string) {
    this.emitAttachment(id);
  }

  private find(id: string) {
    return this.sessions.find((session) => session.id === id);
  }

  async setRemoteActive(id: string, name: string, active: boolean) {
    this.remoteMutations.push({ id, name, active });
    const delay = this.remoteMutationDelays.shift();
    if (delay) await delay;
    const previous = this.find(id);
    if (!previous) throw new Error("Workspace session not found");
    const names = new Set(previous.activeRemotes);
    if (active) names.add(name);
    else names.delete(name);
    return this.update(id, { activeRemotes: [...names] });
  }

  async update(id: string, patch: WorkspaceSessionPatch) {
    if (this.updateFailure) {
      const error = this.updateFailure;
      this.updateFailure = null;
      throw error;
    }
    const previous = this.find(id);
    if (!previous) throw new Error("Workspace session not found");
    const next = {
      ...previous,
      ...patch,
      workspace: patch.workspace
        ? { ...previous.workspace, ...patch.workspace }
        : previous.workspace,
      updatedAtUnixMs: previous.updatedAtUnixMs + 1,
    } as StoredWorkspaceSession;
    this.sessions = this.sessions.map((session) =>
      session.id === id ? next : session,
    );
    this.emit();
    this.emitAttachment(id);
    return next;
  }

  private emit() {
    for (const listener of this.listeners) listener();
  }

  private emitAttachment(id: string) {
    for (const listener of this.attachmentListeners.get(id) ?? []) listener();
  }
}

class FakeDeviceBackend {
  readonly listeners = new Set<() => void>();
  value: StoredWorkspaceSessionDevice | null;

  constructor(initial: StoredWorkspaceSessionDevice | null) {
    this.value = initial;
  }

  emit() {
    for (const listener of this.listeners) listener();
  }
}

class FakeDeviceStore implements WorkspaceSessionDeviceStoreLike {
  status = "ready";
  error: Error | null = null;
  readonly attached: string[] = [];
  readonly attachDelays = new Map<string, Promise<void>[]>();
  readonly attachFailures = new Set<string>();
  readonly detached: string[] = [];
  readonly detachDelays = new Map<string, Promise<void>[]>();
  readonly pruneCalls: string[][] = [];
  pruneNoops = 0;
  claimOverride:
    | ((sessionId: string) => {
        device: StoredWorkspaceSessionDevice;
        claimed: boolean;
      })
    | null = null;

  constructor(readonly backend: FakeDeviceBackend) {}

  async start() {}

  subscribe(listener: () => void) {
    this.backend.listeners.add(listener);
    return () => this.backend.listeners.delete(listener);
  }

  getSnapshot() {
    return {
      status: this.status,
      device: this.backend.value,
      error: this.error,
    };
  }

  async attach(sessionId: string) {
    this.attached.push(sessionId);
    const delay = this.attachDelays.get(sessionId)?.shift();
    if (delay) await delay;
    if (this.attachFailures.delete(sessionId)) {
      throw new Error("device attach unavailable");
    }
    const previous = this.backend.value;
    const ids = previous?.attachedSessionIds ?? [];
    if (!ids.includes(sessionId)) {
      this.backend.value = device(
        [...ids, sessionId],
        (previous?.updatedAtUnixMs ?? 0) + 1,
      );
      this.backend.emit();
    }
    return this.backend.value!;
  }

  async claimInitialSession(sessionId: string) {
    if (this.claimOverride) {
      const result = this.claimOverride(sessionId);
      this.backend.value = result.device;
      this.backend.emit();
      return result;
    }
    if (this.backend.value) {
      return { device: this.backend.value, claimed: false };
    }
    this.backend.value = device([sessionId]);
    this.backend.emit();
    return { device: this.backend.value, claimed: true };
  }

  async detach(sessionId: string) {
    this.detached.push(sessionId);
    const delay = this.detachDelays.get(sessionId)?.shift();
    if (delay) await delay;
    const previous = this.backend.value;
    if (!previous) return null;
    this.backend.value = device(
      previous.attachedSessionIds.filter((id) => id !== sessionId),
      previous.updatedAtUnixMs + 1,
    );
    this.backend.emit();
    return this.backend.value;
  }

  async reorder(sessionIds: readonly string[]) {
    if (!this.backend.value) return null;
    this.backend.value = device(
      sessionIds,
      this.backend.value.updatedAtUnixMs + 1,
    );
    this.backend.emit();
    return this.backend.value;
  }

  async pruneDeleted(validSessionIds: ReadonlySet<string> | readonly string[]) {
    const valid = new Set(validSessionIds);
    this.pruneCalls.push([...valid]);
    if (!this.backend.value) return null;
    if (this.pruneNoops > 0) {
      this.pruneNoops--;
      return this.backend.value;
    }
    this.backend.value = device(
      this.backend.value.attachedSessionIds.filter((id) => valid.has(id)),
      this.backend.value.updatedAtUnixMs + 1,
    );
    this.backend.emit();
    return this.backend.value;
  }
}

function setup(
  store: FakeStore,
  initialHash: string,
  deviceStore = new FakeDeviceStore(new FakeDeviceBackend(null)),
) {
  history.replaceState(null, "", `/app${initialHash}`);
  let dispose = () => {};
  const controller = createRoot((rootDispose) => {
    dispose = rootDispose;
    return createWorkspaceSessionController({
      store,
      deviceStore,
      initialHash,
    });
  });
  return { controller, deviceStore, dispose };
}

afterEach(() => {
  vi.restoreAllMocks();
  // The attachment is device state now, not URL state, so it outlives a test
  // unless it is cleared: without this, one test's selection is the next
  // test's boot.
  localStorage.removeItem(WORKSPACE_SESSION_STORAGE_KEY);
});

describe("workspace session controller", () => {
  it("retains backend startup failure without opening the manager", async () => {
    const store = new FakeStore([record(A, "Backend", 10)]);
    store.startFailure = new Error("home KV unavailable");
    const deviceStore = new FakeDeviceStore(new FakeDeviceBackend(device([A])));
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);

    await controller.start();
    expect(controller.binding()).toBeNull();
    expect(controller.managerOpen()).toBe(false);
    expect(controller.error()).toBe("home KV unavailable");
    controller.openManager();
    expect(controller.managerOpen()).toBe(true);
    controller.closeManager();
    expect(controller.managerOpen()).toBe(false);

    await controller.retry();
    expect(controller.current()?.id).toBe(A);
    dispose();
  });

  it("retains and retries a failed durable patch without opening the manager", async () => {
    const store = new FakeStore([record(A, "Backend", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();
    controller.binding()?.finishRestoring();

    store.updateFailure = new Error("durable commit failed");
    await expect(
      controller.binding()!.patch({ name: "Retried" }),
    ).rejects.toThrow("durable commit failed");
    expect(controller.managerOpen()).toBe(false);
    expect(controller.error()).toBe("durable commit failed");

    await controller.retry();
    expect(controller.current()?.name).toBe("Retried");
    expect(controller.error()).toBeNull();
    dispose();
  });

  it("retains and retries a failed current-tab device reattach", async () => {
    const store = new FakeStore([record(A, "Backend", 10)]);
    const backend = new FakeDeviceBackend(device([A]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    backend.value = device([]);
    deviceStore.attachFailures.add(A);

    await expect(controller.select(A)).rejects.toThrow(
      "device attach unavailable",
    );
    expect(controller.current()?.id).toBe(A);
    expect(controller.managerOpen()).toBe(false);
    expect(controller.error()).toBe("device attach unavailable");

    await controller.retry();
    expect(backend.value?.attachedSessionIds).toEqual([A]);
    expect(controller.error()).toBeNull();
    expect(controller.managerOpen()).toBe(false);
    dispose();
  });

  it("warns about invalid records without hiding a valid selected session", async () => {
    const store = new FakeStore([record(A, "Valid", 10)]);
    store.invalidRecords = [
      {
        key: "ui/workspace-sessions/v1/broken",
        message: "invalid JSON",
      },
    ];
    store.storeError = new Error("1 workspace session record quarantined");
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();

    expect(controller.current()?.name).toBe("Valid");
    expect(controller.binding()).not.toBeNull();
    expect(controller.managerOpen()).toBe(false);
    expect(controller.warnings()).toEqual([
      "ui/workspace-sessions/v1/broken: invalid JSON",
    ]);
    expect(controller.error()).toBeNull();

    store.storeStatus = "loading";
    store.storeError = new Error("watch disconnected");
    store.publish();
    expect(controller.error()).toBe("watch disconnected");
    store.storeStatus = "ready";
    store.storeError = new Error("1 workspace session record quarantined");
    store.publish();
    expect(controller.error()).toBeNull();
    dispose();
  });

  it("preserves a quarantined session ID in device membership", async () => {
    const store = new FakeStore([record(A, "Valid", 10)]);
    store.invalidRecords = [
      {
        key: `ui/workspace-sessions/v1/${B}`,
        message: "invalid JSON",
      },
    ];
    store.storeError = new Error("1 workspace session record quarantined");
    const deviceStore = new FakeDeviceStore(
      new FakeDeviceBackend(device([A, B])),
    );
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    await Promise.resolve();

    expect(controller.attachedSessionIds()).toEqual([A, B]);
    expect(deviceStore.pruneCalls.every((call) => call.includes(B))).toBe(true);
    dispose();
  });

  it("keeps a selected last-good binding when its replacement is quarantined", async () => {
    const store = new FakeStore([
      record(A, "Last good", 10),
      record(B, "Other tab", 9),
    ]);
    const deviceStore = new FakeDeviceStore(
      new FakeDeviceBackend(device([A, B])),
    );
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();

    store.quarantinedSessionIds = [A];
    store.invalidRecords = [
      {
        key: `ui/workspace-sessions/v1/${A}`,
        message: "invalid replacement",
      },
    ];
    store.storeError = new Error("1 workspace session record quarantined");
    store.attachmentMissing.add(A);
    store.publish();
    store.publishAttachment(A);
    await Promise.resolve();

    expect(controller.current()?.name).toBe("Last good");
    expect(controller.binding()?.id).toBe(A);
    expect(controller.attachedSessionIds()).toEqual([A, B]);
    expect(deviceStore.detached).not.toContain(A);
    expect(controller.warnings()[0]).toContain("invalid replacement");
    dispose();
  });

  it("selects only from this device's tab list, not the newest server session", async () => {
    const store = new FakeStore([
      record(A, "Device tab", 10),
      record(B, "New but unattached", 20),
    ]);
    const { controller, dispose } = setup(
      store,
      "",
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();

    expect(controller.current()?.id).toBe(A);
    expect(controller.attachedSessionIds()).toEqual([A]);
    expect(location.pathname + location.search + location.hash).toBe("/app");
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    dispose();
  });

  it("atomically creates a first local-only Default for an absent device", async () => {
    const store = new FakeStore([record(A, "Unattached", 20, ["hound"])]);
    const { controller, deviceStore, dispose } = setup(store, "");
    await controller.start();

    expect(store.created[0]).toMatchObject({
      name: "Default",
      activeRemotes: [],
    });
    expect(controller.current()?.name).toBe("Default");
    expect(controller.current()?.activeRemotes).toEqual([]);
    expect(deviceStore.backend.value?.attachedSessionIds).toEqual([C]);
    dispose();
  });

  it("does not recreate Default for an intentionally empty device", async () => {
    const store = new FakeStore([record(A, "Saved", 20)]);
    const { controller, dispose } = setup(
      store,
      "",
      new FakeDeviceStore(new FakeDeviceBackend(device([]))),
    );
    await controller.start();

    expect(store.created).toHaveLength(0);
    expect(controller.current()).toBeNull();
    expect(controller.managerOpen()).toBe(false);
    expect(location.hash).toBe("");
    dispose();
  });

  it("retains an error for a malformed explicit session ID", async () => {
    const store = new FakeStore([record(A, "Device tab", 20)]);
    const { controller, dispose } = setup(
      store,
      "#session=malformed",
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();

    expect(controller.current()).toBeNull();
    expect(controller.managerOpen()).toBe(false);
    expect(controller.error()).toBe(
      "The URL contains an invalid workspace session ID",
    );
    expect(store.created).toHaveLength(0);
    dispose();
  });

  it("loses an initial claim race without blocking on orphan cleanup", async () => {
    const store = new FakeStore([record(B, "Winner", 20)]);
    const deviceStore = new FakeDeviceStore(new FakeDeviceBackend(null));
    deviceStore.claimOverride = () => ({
      device: device([B]),
      claimed: false,
    });
    const { controller, dispose } = setup(store, "", deviceStore);
    // The candidate is C with this catalogue; make best-effort cleanup fail.
    store.deleteFailures.add(C);
    await controller.start();

    expect(controller.current()?.id).toBe(B);
    expect(controller.error()).toContain("unused Default");
    dispose();
  });

  it("auto-attaches a URL session to this device without pushing history", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const push = vi.spyOn(history, "pushState");
    const deviceStore = new FakeDeviceStore(new FakeDeviceBackend(device([A])));
    const { controller, dispose } = setup(store, `#session=${B}`, deviceStore);
    push.mockClear();
    await controller.start();

    expect(controller.current()?.id).toBe(B);
    expect(controller.attachedSessionIds()).toEqual([A, B]);
    expect(push).not.toHaveBeenCalled();
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });

  it("creates, renames, switches, detaches adjacent, and deletes tabs", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const deviceStore = new FakeDeviceStore(
      new FakeDeviceBackend(device([A, B])),
    );
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();

    await controller.create("Three");
    expect(controller.attachedSessionIds()).toEqual([A, B, C]);
    expect(controller.current()?.id).toBe(C);
    expect(controller.current()?.activeRemotes).toEqual([]);

    await controller.rename(C, "Renamed");
    expect(
      controller.attachedSessions().map((session) => session.name),
    ).toEqual(["One", "Two", "Renamed"]);

    await controller.select(B);
    await controller.detach(B);
    expect(controller.attachedSessionIds()).toEqual([A, C]);
    expect(controller.current()?.id).toBe(C);

    await controller.delete(C);
    expect(controller.attachedSessionIds()).toEqual([A]);
    expect(controller.current()?.id).toBe(A);
    expect(store.sessions.some((session) => session.id === C)).toBe(false);
    dispose();
  });

  it("takes the session out of the address and never puts one back", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const push = vi.spyOn(history, "pushState");
    const replace = vi.spyOn(history, "replaceState");

    // The one address rewrite there is: cleaning the share link this test
    // booted with. After that the attachment is device state, so selecting and
    // closing tabs touch storage and leave history alone entirely.
    // Boot already cleaned the share link this test arrived with, so from
    // here the address is finished: selecting and closing tabs are device
    // state and touch history not at all.
    expect(location.hash).toBe("");
    await controller.select(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    await controller.detach(B);
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    expect(location.hash).toBe("");
    expect(controller.attachedSessionIds()).toEqual([A]);
    dispose();
  });

  it("reflects same-device tab, rename, detach, and selection changes live", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A]));
    const first = setup(store, "", new FakeDeviceStore(backend));
    await first.controller.start();
    const second = setup(store, "", new FakeDeviceStore(backend));
    await second.controller.start();

    await first.controller.attach(B);
    expect(second.controller.attachedSessionIds()).toEqual([A, B]);
    await first.controller.rename(B, "Two renamed");
    expect(
      second.controller.attachedSessions().find((session) => session.id === B)
        ?.name,
    ).toBe("Two renamed");

    await second.controller.select(B);
    await first.controller.select(A);
    await first.controller.detach(B);
    await vi.waitFor(() => expect(second.controller.current()?.id).toBe(A));
    expect(second.controller.attachedSessionIds()).toEqual([A]);

    first.dispose();
    second.dispose();
  });

  it("uses semantic remote membership mutations and preserves missing names", async () => {
    const store = new FakeStore([
      record(A, "One", 20, ["temporarily-missing", "hound"]),
    ]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();
    await controller.setRemoteActive("lab", true);
    await controller.setRemoteActive("hound", false);

    expect(store.remoteMutations).toEqual([
      { id: A, name: "lab", active: true },
      { id: A, name: "hound", active: false },
    ]);
    expect(controller.current()?.activeRemotes).toEqual([
      "temporarily-missing",
      "lab",
    ]);
    dispose();
  });

  it("serializes rapid remote toggles so the latest intent lands last", async () => {
    const store = new FakeStore([record(A, "One", 20)]);
    const firstMutation = deferred();
    store.remoteMutationDelays.push(firstMutation.promise);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );
    await controller.start();

    const activate = controller.setRemoteActive("lab", true);
    const deactivate = controller.setRemoteActive("lab", false);
    await vi.waitFor(() =>
      expect(store.remoteMutations).toEqual([
        { id: A, name: "lab", active: true },
      ]),
    );

    firstMutation.resolve();
    await Promise.all([activate, deactivate]);
    expect(store.remoteMutations).toEqual([
      { id: A, name: "lab", active: true },
      { id: A, name: "lab", active: false },
    ]);
    expect(controller.current()?.activeRemotes).toEqual([]);
    dispose();
  });

  it("cancels a delayed tab selection when the current tab is reselected", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    controller.binding()?.finishRestoring();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selectB = controller.select(B);
    await Promise.resolve();
    await controller.select(A);
    delayed.resolve();
    await selectB;

    expect(controller.current()?.id).toBe(A);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    expect(location.hash).toBe("");
    dispose();
  });

  it("cancels delayed history attachment when navigation returns to the current tab", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    history.pushState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    history.pushState(null, "", `/app#session=${A}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await Promise.resolve();
    delayed.resolve();
    await vi.waitFor(() => expect(controller.current()?.id).toBe(A));

    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    // The address keeps whatever this synthetic navigation set. Cleaning
    // happens where an id actually reaches a person — at boot, and when a
    // tab is selected — and a navigation that lands back on the tab already
    // open selects nothing, so nothing cleans it.
    dispose();
  });

  it("treats a repeated history target as new after an intervening tab selection", async () => {
    const store = new FakeStore([
      record(A, "One", 30),
      record(B, "Two", 20),
      record(C, "Three", 10),
    ]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B, C]))),
    );
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    history.pushState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );

    await controller.select(C);
    history.replaceState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() => expect(controller.current()?.id).toBe(B));
    expect(store.attached.filter((id) => id === B)).toHaveLength(2);

    delayed.resolve();
    await Promise.resolve();
    expect(controller.current()?.id).toBe(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });

  it("reattaches a tab detached on this device while its selection was pending", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selecting = controller.select(B);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    await new FakeDeviceStore(backend).detach(B);
    delayed.resolve();
    await selecting;

    expect(controller.current()?.id).toBe(B);
    expect(backend.value?.attachedSessionIds).toEqual([A, B]);
    expect(deviceStore.attached).toContain(B);
    dispose();
  });

  it("reattaches an adjacent target detached while selected-tab detach is pending", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const detaching = controller.detach(A);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    await new FakeDeviceStore(backend).detach(B);
    delayed.resolve();
    await detaching;

    expect(controller.current()?.id).toBe(B);
    expect(backend.value?.attachedSessionIds).toEqual([B]);
    expect(deviceStore.attached).toContain(B);
    dispose();
  });

  it("lets a newer detach cancel a pending selection of that tab", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selecting = controller.select(B);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    await controller.detach(B);
    delayed.resolve();
    await selecting;

    expect(controller.current()?.id).toBe(A);
    expect(backend.value?.attachedSessionIds).toEqual([A]);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    expect(location.hash).toBe("");
    dispose();
  });

  it("lets a newer delete cancel a pending selection of that session", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(backend),
    );
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selecting = controller.select(B);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    await controller.delete(B);
    delayed.resolve();
    await selecting;

    expect(controller.current()?.id).toBe(A);
    expect(store.sessions.some((session) => session.id === B)).toBe(false);
    expect(backend.value?.attachedSessionIds).toEqual([A]);
    dispose();
  });

  it("does not attach a canceled slow selection after a newer tab wins", async () => {
    const store = new FakeStore([
      record(A, "One", 30),
      record(B, "Two", 20),
      record(C, "Three", 10),
    ]);
    const backend = new FakeDeviceBackend(device([A]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selectB = controller.select(B);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    await controller.select(C);
    delayed.resolve();
    await selectB;

    expect(controller.current()?.id).toBe(C);
    expect(backend.value?.attachedSessionIds).toEqual([A, C]);
    expect(deviceStore.attached).not.toContain(B);
    dispose();
  });

  it("does not commit a session deleted while device attachment is pending", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    deviceStore.attachDelays.set(B, [delayed.promise]);

    const selecting = controller.select(B);
    await vi.waitFor(() => expect(deviceStore.attached).toContain(B));
    await store.delete(B);
    delayed.resolve();
    await expect(selecting).rejects.toThrow("Workspace session not found");

    expect(controller.current()?.id).toBe(A);
    expect(backend.value?.attachedSessionIds).toEqual([A]);
    expect(deviceStore.detached).toContain(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    expect(location.hash).toBe("");
    dispose();
  });

  it("keeps lifecycle listeners after a missing initial URL is recovered in the manager", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${C}`, deviceStore);
    await controller.start();

    expect(controller.current()).toBeNull();
    expect(controller.error()).toContain("not found");
    await controller.attach(A);

    history.pushState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() => expect(controller.current()?.id).toBe(B));
    await new FakeDeviceStore(backend).detach(B);
    await vi.waitFor(() => expect(controller.current()?.id).toBe(A));

    expect(controller.attachedSessionIds()).toEqual([A]);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(A);
    expect(location.hash).toBe("");
    dispose();
  });

  it("malformed navigation cancels a delayed valid selection", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    controller.binding()?.finishRestoring();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    history.pushState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    history.pushState(null, "", "/app#session=malformed");
    window.dispatchEvent(new PopStateEvent("popstate"));
    delayed.resolve();
    await vi.waitFor(() => expect(controller.restoring()).toBe(false));

    expect(controller.current()?.id).toBe(A);
    expect(controller.managerOpen()).toBe(false);
    expect(controller.error()).toContain("invalid workspace session ID");
    // A malformed request is reported, and left in the address rather than
    // silently rewritten out from under the person who typed it.
    expect(location.hash).toBe("#session=malformed");
    dispose();
  });

  it("preserves durable membership when selection is canceled after attach", async () => {
    const store = new FakeStore([
      record(A, "One", 30),
      record(B, "Two", 20),
      record(C, "Three", 10),
    ]);
    const backend = new FakeDeviceBackend(device([A]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const firstAttach = deferred();
    deviceStore.attachDelays.set(B, [firstAttach.promise]);

    const firstB = controller.select(B);
    await vi.waitFor(() => expect(deviceStore.attached).toContain(B));
    const selectC = controller.select(C);
    firstAttach.resolve();
    await Promise.all([firstB, selectC]);

    expect(backend.value?.attachedSessionIds).toEqual([A, B, C]);
    expect(controller.current()?.id).toBe(C);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(C);
    expect(location.hash).toBe("");
    dispose();
  });

  it("uses the latest URL after slow backend startup", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const delayed = deferred();
    store.startDelay = delayed.promise;
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A]))),
    );

    const starting = controller.start();
    await vi.waitFor(() => expect(store.startStarted).toBe(1));
    history.replaceState(null, "", `/app#session=${B}`);
    delayed.resolve();
    await starting;

    expect(controller.current()?.id).toBe(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });

  it("reconciles an external selected-tab detach after a local device mutation", async () => {
    const store = new FakeStore([
      record(A, "One", 30),
      record(B, "Two", 20),
      record(C, "Three", 10),
    ]);
    const backend = new FakeDeviceBackend(device([A, B, C]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const delayed = deferred();
    deviceStore.detachDelays.set(C, [delayed.promise]);

    const localDetach = controller.detach(C);
    await vi.waitFor(() => expect(deviceStore.detached).toContain(C));
    await new FakeDeviceStore(backend).detach(A);
    delayed.resolve();
    await localDetach;
    await vi.waitFor(() => expect(controller.current()?.id).toBe(B));

    expect(controller.attachedSessionIds()).toEqual([B]);
    dispose();
  });

  it("does not let an older selected detach cancel a newer selection", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const detachDelay = deferred();
    const selectDelay = deferred();
    deviceStore.detachDelays.set(A, [detachDelay.promise]);
    store.attachDelays.set(B, [selectDelay.promise]);

    const detaching = controller.detach(A);
    await vi.waitFor(() => expect(deviceStore.detached).toContain(A));
    const selecting = controller.select(B);
    detachDelay.resolve();
    await detaching;
    selectDelay.resolve();
    await selecting;

    expect(controller.current()?.id).toBe(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });

  it("does not let delayed deletion recovery replace a newer selection", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const backend = new FakeDeviceBackend(device([A, B]));
    const deviceStore = new FakeDeviceStore(backend);
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);
    await controller.start();
    const recoveryDelay = deferred();
    deviceStore.detachDelays.set(A, [recoveryDelay.promise]);

    await store.delete(A);
    await vi.waitFor(() => expect(deviceStore.detached).toContain(A));
    const selecting = controller.select(B);
    recoveryDelay.resolve();
    await selecting;

    expect(controller.current()?.id).toBe(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });

  it("retries a stale-membership prune after a device CAS conflict", async () => {
    const store = new FakeStore([record(A, "One", 20)]);
    const backend = new FakeDeviceBackend(device([A, C]));
    const deviceStore = new FakeDeviceStore(backend);
    deviceStore.pruneNoops = 1;
    const { controller, dispose } = setup(store, `#session=${A}`, deviceStore);

    await controller.start();
    await vi.waitFor(() =>
      expect(deviceStore.pruneCalls.length).toBeGreaterThanOrEqual(2),
    );
    await vi.waitFor(() =>
      expect(backend.value?.attachedSessionIds).toEqual([A]),
    );
    dispose();
  });

  it("scopes restoration to the newly committed attachment", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const oldBinding = controller.binding()!;
    oldBinding.finishRestoring();
    const delayed = deferred();
    store.attachDelays.set(B, [delayed.promise]);

    const selecting = controller.select(B);
    await vi.waitFor(() =>
      expect(store.attached.filter((id) => id === B)).toHaveLength(1),
    );
    expect(controller.restoring()).toBe(false);
    delayed.resolve();
    await selecting;
    expect(controller.restoring()).toBe(true);
    expect(oldBinding.restoring()).toBe(false);
    oldBinding.finishRestoring();
    expect(controller.restoring()).toBe(true);
    controller.binding()!.finishRestoring();
    expect(controller.restoring()).toBe(false);
    dispose();
  });

  it("allows an old binding to durably flush after switching sessions", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const oldBinding = controller.binding()!;
    await controller.select(B);
    await oldBinding.patch({ workspace: { panels: { debugOpen: true } } });

    expect(
      store.sessions.find((session) => session.id === A)?.workspace.panels
        .debugOpen,
    ).toBe(true);
    expect(controller.current()?.id).toBe(B);
    dispose();
  });

  it("drains remote membership intent after switching away", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const oldBinding = controller.binding()!;
    const delayed = deferred();
    store.remoteMutationDelays.push(delayed.promise);

    const activate = oldBinding.setRemoteActive("lab", true);
    const deactivate = oldBinding.setRemoteActive("lab", false);
    await vi.waitFor(() => expect(store.remoteMutations).toHaveLength(1));
    await controller.select(B);
    delayed.resolve();
    await Promise.all([activate, deactivate]);

    expect(
      store.sessions.find((session) => session.id === A)?.activeRemotes,
    ).toEqual([]);
    expect(controller.current()?.id).toBe(B);
    dispose();
  });

  it("replace-canonicalizes valid noncanonical history navigation", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();

    history.pushState(null, "", `/app?ignored=1#debug&session=${B}&psk=secret`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() => expect(controller.current()?.id).toBe(B));

    // The session and the passphrase are taken out; `debug` is somebody
    // else's fragment state and survives.
    expect(location.pathname + location.search + location.hash).toBe(
      "/app#debug",
    );
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    dispose();
  });

  it("does not let a slow create override newer history navigation", async () => {
    const store = new FakeStore([record(A, "One", 20), record(B, "Two", 10)]);
    const { controller, dispose } = setup(
      store,
      `#session=${A}`,
      new FakeDeviceStore(new FakeDeviceBackend(device([A, B]))),
    );
    await controller.start();
    const delayed = deferred();
    store.createDelays.push(delayed.promise);

    const creating = controller.create("Created later");
    await Promise.resolve();
    history.pushState(null, "", `/app#session=${B}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
    await vi.waitFor(() => expect(controller.current()?.id).toBe(B));
    delayed.resolve();
    await creating;

    expect(controller.current()?.id).toBe(B);
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(B);
    expect(location.hash).toBe("");
    dispose();
  });
});
