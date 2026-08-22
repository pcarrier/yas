import { afterEach, describe, expect, it, vi } from "vitest";

async function freshStorage() {
  vi.resetModules();
  return await import("../storage");
}

describe("device-local preference storage", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  it("publishes local writes to every mounted frontend without a socket", async () => {
    const WebSocket = vi.fn();
    vi.stubGlobal("WebSocket", WebSocket);
    const storage = await freshStorage();
    const changes: string[] = [];
    storage.onStorageChange((key, value) => changes.push(`${key}=${value}`));

    storage.writeStorage(storage.FONT_SIZE_KEY, "18");

    expect(storage.readStorage(storage.FONT_SIZE_KEY)).toBe("18");
    expect(changes).toEqual(["yas.fontSize=18"]);
    expect(WebSocket).not.toHaveBeenCalled();
  });

  it("bounds large appearance and layout values before localStorage", async () => {
    const storage = await freshStorage();
    storage.writeStorage(
      storage.FONT_KEY,
      "x".repeat(storage.FONT_VALUE_MAX_CHARS + 1),
    );
    storage.writeStorage(
      "yas.layouts",
      "x".repeat(storage.STORAGE_VALUE_MAX_CHARS + 1),
    );

    expect(storage.readStorage(storage.FONT_KEY)).toBeNull();
    expect(storage.readStorage("yas.layouts")).toBeNull();
  });

  it("keeps media, streaming, and panel geometry on this device", async () => {
    const storage = await freshStorage();
    const localSettings = [
      storage.AUDIO_BITRATE_KEY,
      storage.AUDIO_MUTED_KEY,
      storage.VIDEO_BANDWIDTH_KEY,
      storage.VIDEO_SPEED_KEY,
      storage.SURFACE_STREAMING_KEY,
      storage.SURFACE_SMOOTHING_KEY,
      storage.LEFT_DOCK_WIDTH_KEY,
      storage.PREVIEW_PANEL_WIDTH_KEY,
      storage.LEFT_DOCK_OPEN_KEY,
      storage.PREVIEW_PANEL_OPEN_KEY,
      storage.LEFT_COLLAPSED_KEY,
    ];

    for (const key of localSettings) storage.writeStorage(key, "1");

    for (const key of localSettings) {
      expect(localStorage.getItem(key)).toBe("1");
    }
  });

  it("keeps an explicit right-sidebar choice across session fallbacks", async () => {
    const storage = await freshStorage();
    expect(storage.preferredPreviewPanelOpen(false)).toBe(false);

    storage.writeStorage(storage.PREVIEW_PANEL_OPEN_KEY, "1");
    expect(storage.preferredPreviewPanelOpen(false)).toBe(true);
    expect(storage.preferredPreviewPanelOpen(true)).toBe(true);
  });

  it("applies a local editor-wrap toggle to mounted editors", async () => {
    const storage = await freshStorage();
    const editorPrefs = await import("../ide/editorPrefs");
    expect(editorPrefs.lineWrap()).toBe(true);

    storage.writeStorage(storage.EDITOR_WRAP_KEY, "0");

    expect(editorPrefs.lineWrap()).toBe(false);
  });
});
