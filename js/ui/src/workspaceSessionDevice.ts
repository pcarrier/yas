import {
  WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY,
  createWorkspaceSessionDeviceId,
  isWorkspaceSessionDeviceId,
} from "@yas-run/core";

const DEVICE_ID_LOCK = `${WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY}.lock`;
const DEVICE_ID_DATABASE = "yas-workspace-session-device";
const DEVICE_ID_DATABASE_VERSION = 1;
const DEVICE_ID_STORE = "identity";
const DEVICE_ID_COORDINATION_TTL_MS = 10_000;

export interface WorkspaceSessionDeviceIdStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface WorkspaceSessionDeviceLockManager {
  request<T>(
    name: string,
    options: { mode: "exclusive" },
    callback: () => T | PromiseLike<T>,
  ): Promise<T>;
}

/** Atomic first-writer-wins storage used when Web Locks are unavailable. */
export interface WorkspaceSessionDeviceIdCoordinator {
  claim(proposedDeviceId: string): Promise<string>;
}

function storageError(message: string, cause: unknown): Error {
  return new Error(message, { cause });
}

function readStoredDeviceId(storage: WorkspaceSessionDeviceIdStorage) {
  let stored: string | null;
  try {
    stored = storage.getItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY);
  } catch (cause) {
    throw storageError("Workspace session tabs require localStorage", cause);
  }
  const normalized = stored?.trim().toLowerCase() ?? "";
  return { stored, normalized };
}

function persistDeviceId(
  storage: WorkspaceSessionDeviceIdStorage,
  value: string,
): void {
  try {
    storage.setItem(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY, value);
  } catch (cause) {
    throw storageError(
      "Could not persist the workspace session device ID",
      cause,
    );
  }
}

function getOrCreateWhileLocked(
  storage: WorkspaceSessionDeviceIdStorage,
  randomUUID?: () => string,
): string {
  const { stored, normalized } = readStoredDeviceId(storage);
  if (isWorkspaceSessionDeviceId(normalized)) {
    if (stored !== normalized) persistDeviceId(storage, normalized);
    return normalized;
  }
  const created = createWorkspaceSessionDeviceId(randomUUID);
  persistDeviceId(storage, created);
  return created;
}

function openDeviceIdDatabase(factory: IDBFactory): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    let request: IDBOpenDBRequest;
    try {
      request = factory.open(DEVICE_ID_DATABASE, DEVICE_ID_DATABASE_VERSION);
    } catch (cause) {
      reject(cause);
      return;
    }
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(DEVICE_ID_STORE)) {
        request.result.createObjectStore(DEVICE_ID_STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error("Could not open IndexedDB"));
    request.onblocked = () =>
      reject(new Error("Workspace session device database upgrade is blocked"));
  });
}

class IndexedDbWorkspaceSessionDeviceIdCoordinator implements WorkspaceSessionDeviceIdCoordinator {
  constructor(private readonly factory: IDBFactory) {}

  async claim(proposedDeviceId: string): Promise<string> {
    const database = await openDeviceIdDatabase(this.factory);
    try {
      return await new Promise<string>((resolve, reject) => {
        let winner = proposedDeviceId;
        let transaction: IDBTransaction;
        try {
          transaction = database.transaction(DEVICE_ID_STORE, "readwrite");
          const store = transaction.objectStore(DEVICE_ID_STORE);
          const request = store.get(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY);
          request.onsuccess = () => {
            const raw = request.result;
            const now = Date.now();
            const normalized =
              raw !== null && typeof raw === "object" && "deviceId" in raw
                ? String(raw.deviceId).trim().toLowerCase()
                : "";
            const expiresAt =
              raw !== null && typeof raw === "object" && "expiresAt" in raw
                ? Number(raw.expiresAt)
                : 0;
            if (
              isWorkspaceSessionDeviceId(normalized) &&
              Number.isFinite(expiresAt) &&
              expiresAt > now
            ) {
              winner = normalized;
            } else {
              store.put(
                {
                  deviceId: proposedDeviceId,
                  expiresAt: now + DEVICE_ID_COORDINATION_TTL_MS,
                },
                WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY,
              );
            }
          };
          request.onerror = () => transaction.abort();
        } catch (cause) {
          reject(cause);
          return;
        }
        transaction.oncomplete = () => resolve(winner);
        transaction.onerror = () =>
          reject(
            transaction.error ??
              new Error("Could not coordinate workspace session device ID"),
          );
        transaction.onabort = () =>
          reject(
            transaction.error ??
              new Error("Could not coordinate workspace session device ID"),
          );
      });
    } finally {
      database.close();
    }
  }
}

function browserLocks(): WorkspaceSessionDeviceLockManager | null {
  if (typeof navigator === "undefined" || !navigator.locks) return null;
  return navigator.locks as unknown as WorkspaceSessionDeviceLockManager;
}

function browserCoordinator(): WorkspaceSessionDeviceIdCoordinator | null {
  if (typeof indexedDB === "undefined") return null;
  return new IndexedDbWorkspaceSessionDeviceIdCoordinator(indexedDB);
}

/**
 * Return this browser device's durable workspace-session identity.
 *
 * Existing localStorage always wins. A Web Lock serializes simultaneous first
 * boots where available. IndexedDB's serialized readwrite transactions provide
 * the atomic first-writer-wins fallback; a localStorage lease cannot provide
 * that guarantee across renderer processes. The resolved ID is persisted back
 * to localStorage before App constructs either backend session store.
 */
export async function getOrCreateWorkspaceSessionDeviceId(
  storage: WorkspaceSessionDeviceIdStorage = localStorage,
  randomUUID?: () => string,
  locks: WorkspaceSessionDeviceLockManager | null = browserLocks(),
  coordinator: WorkspaceSessionDeviceIdCoordinator | null = browserCoordinator(),
): Promise<string> {
  const existing = readStoredDeviceId(storage);
  if (isWorkspaceSessionDeviceId(existing.normalized)) {
    if (existing.stored !== existing.normalized) {
      persistDeviceId(storage, existing.normalized);
    }
    return existing.normalized;
  }

  if (locks) {
    return locks.request(DEVICE_ID_LOCK, { mode: "exclusive" }, () =>
      getOrCreateWhileLocked(storage, randomUUID),
    );
  }
  if (!coordinator) {
    throw new Error(
      "Workspace session tabs require Web Locks or IndexedDB for safe cross-tab startup",
    );
  }

  const proposed = createWorkspaceSessionDeviceId(randomUUID);
  let claimed: string;
  try {
    claimed = (await coordinator.claim(proposed)).trim().toLowerCase();
  } catch (cause) {
    throw storageError(
      "Could not coordinate the workspace session device ID across tabs",
      cause,
    );
  }
  if (!isWorkspaceSessionDeviceId(claimed)) {
    throw new Error(
      "The workspace session device coordinator returned an invalid ID",
    );
  }

  // A Web-Locks-capable sibling may have populated localStorage while the
  // IndexedDB transaction was pending. localStorage is the canonical source.
  const raced = readStoredDeviceId(storage);
  const resolved = isWorkspaceSessionDeviceId(raced.normalized)
    ? raced.normalized
    : claimed;
  if (raced.stored !== resolved) persistDeviceId(storage, resolved);
  return resolved;
}
