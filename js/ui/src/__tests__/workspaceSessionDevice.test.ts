import { describe, expect, it, vi } from "vitest";
import { WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY } from "@yas-run/core";
import {
  getOrCreateWorkspaceSessionDeviceId,
  type WorkspaceSessionDeviceIdCoordinator,
  type WorkspaceSessionDeviceLockManager,
} from "../workspaceSessionDevice";

const ID = "123e4567-e89b-42d3-a456-426614174003";

class MemoryStorage {
  readonly values = new Map<string, string>();

  getItem(key: string) {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.values.set(key, value);
  }

  removeItem(key: string) {
    this.values.delete(key);
  }
}

class SerialLocks implements WorkspaceSessionDeviceLockManager {
  private tail: Promise<unknown> = Promise.resolve();

  request<T>(
    _name: string,
    _options: { mode: "exclusive" },
    callback: () => T | PromiseLike<T>,
  ): Promise<T> {
    const result = this.tail.then(() => callback());
    this.tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }
}

class AtomicCoordinator implements WorkspaceSessionDeviceIdCoordinator {
  private winner: string | null = null;
  private tail: Promise<unknown> = Promise.resolve();

  claim(proposedDeviceId: string): Promise<string> {
    const result = this.tail.then(() => {
      this.winner ??= proposedDeviceId;
      return this.winner;
    });
    this.tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }
}

describe("workspace device identity", () => {
  it("persists one ID and reuses it across application mounts", async () => {
    const storage = new MemoryStorage();
    const randomUUID = vi.fn(() => ID);
    const locks = new SerialLocks();

    await expect(
      getOrCreateWorkspaceSessionDeviceId(storage, randomUUID, locks),
    ).resolves.toBe(ID);
    await expect(
      getOrCreateWorkspaceSessionDeviceId(storage, randomUUID, locks),
    ).resolves.toBe(ID);
    expect(randomUUID).toHaveBeenCalledTimes(1);
    expect(storage.getItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY)).toBe(ID);
  });

  it("canonicalizes an existing uppercase device ID", async () => {
    const storage = new MemoryStorage();
    storage.setItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY, ID.toUpperCase());

    await expect(
      getOrCreateWorkspaceSessionDeviceId(
        storage,
        undefined,
        new SerialLocks(),
      ),
    ).resolves.toBe(ID);
    expect(storage.getItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY)).toBe(ID);
  });

  it("replaces malformed storage instead of addressing an invalid record", async () => {
    const storage = new MemoryStorage();
    storage.setItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY, "../bad");

    await expect(
      getOrCreateWorkspaceSessionDeviceId(storage, () => ID, new SerialLocks()),
    ).resolves.toBe(ID);
  });

  it("serializes simultaneous first-use browser tabs before either returns", async () => {
    const storage = new MemoryStorage();
    const locks = new SerialLocks();
    const [first, second] = await Promise.all([
      getOrCreateWorkspaceSessionDeviceId(storage, () => ID, locks),
      getOrCreateWorkspaceSessionDeviceId(
        storage,
        () => "123e4567-e89b-42d3-a456-426614174004",
        locks,
      ),
    ]);

    expect(first).toBe(ID);
    expect(second).toBe(ID);
    expect(storage.getItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY)).toBe(ID);
  });

  it("uses an atomic fallback so simultaneous no-lock tabs converge", async () => {
    const storage = new MemoryStorage();
    const coordinator = new AtomicCoordinator();
    const secondId = "123e4567-e89b-42d3-a456-426614174004";

    const [first, second] = await Promise.all([
      getOrCreateWorkspaceSessionDeviceId(storage, () => ID, null, coordinator),
      getOrCreateWorkspaceSessionDeviceId(
        storage,
        () => secondId,
        null,
        coordinator,
      ),
    ]);

    expect(first).toBe(ID);
    expect(second).toBe(ID);
    expect(storage.getItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY)).toBe(ID);
  });

  it("uses existing localStorage without invoking the fallback coordinator", async () => {
    const storage = new MemoryStorage();
    storage.setItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY, ID);
    const coordinator = {
      claim: vi.fn(async () => {
        throw new Error("must not run");
      }),
    };

    await expect(
      getOrCreateWorkspaceSessionDeviceId(
        storage,
        undefined,
        null,
        coordinator,
      ),
    ).resolves.toBe(ID);
    expect(coordinator.claim).not.toHaveBeenCalled();
  });

  it("fails explicitly when safe no-lock coordination is unavailable", async () => {
    const storage = new MemoryStorage();

    await expect(
      getOrCreateWorkspaceSessionDeviceId(storage, () => ID, null, null),
    ).rejects.toThrow("require Web Locks or IndexedDB");
  });

  it("surfaces storage denial instead of silently creating ephemeral tabs", async () => {
    const denied = {
      getItem: () => null,
      setItem: () => {
        throw new DOMException("denied", "SecurityError");
      },
    };

    await expect(
      getOrCreateWorkspaceSessionDeviceId(denied, () => ID, new SerialLocks()),
    ).rejects.toThrow("Could not persist the workspace device ID");
  });
});
