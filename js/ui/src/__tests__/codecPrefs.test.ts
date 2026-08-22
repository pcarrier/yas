import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Codec negotiation is configurable in both directions: which codecs this
// device accepts for surface video, and which it uses to send camera and
// microphone. All four preferences are device-local, like the rest of the
// media settings — decoder and encoder support is a fact about this machine.

const MICROPHONE_CODEC_KEY = "yas.microphoneCodec";

/** Import a fresh preference module for each isolated storage fixture. */
async function freshStorage() {
  vi.resetModules();
  return await import("../storage");
}

/** The sandbox environment has no working `localStorage`, so provide one. */
function stubLocalStorage() {
  const map = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
  });
}

describe("codec preferences", () => {
  beforeEach(stubLocalStorage);
  afterEach(() => vi.unstubAllGlobals());

  it("defaults every axis to no opinion", async () => {
    const storage = await freshStorage();
    expect(storage.preferredSurfaceCodecs()).toBe(0);
    expect(storage.preferredCameraCodec()).toBe("auto");
    expect(storage.preferredCameraChroma()).toBe("auto");
    expect(storage.preferredMicrophoneCodec()).toBe("auto");
  });

  it("reads back a stored selection", async () => {
    localStorage.setItem("yas.surfaceCodecs", "10");
    localStorage.setItem("yas.cameraCodec", "av1");
    localStorage.setItem("yas.cameraChroma", "444");
    localStorage.setItem(MICROPHONE_CODEC_KEY, "opus");
    const storage = await freshStorage();
    expect(storage.preferredSurfaceCodecs()).toBe(10);
    expect(storage.preferredCameraCodec()).toBe("av1");
    expect(storage.preferredCameraChroma()).toBe("444");
    expect(storage.preferredMicrophoneCodec()).toBe("opus");
  });

  it("falls back to auto on a value it does not know", async () => {
    // A preference written by a newer build must not wedge an older one, and
    // an out-of-range mask must not reach the wire.
    localStorage.setItem("yas.cameraCodec", "vp9");
    localStorage.setItem("yas.cameraChroma", "422");
    localStorage.setItem(MICROPHONE_CODEC_KEY, "mp3");
    localStorage.setItem("yas.surfaceCodecs", "999");
    const storage = await freshStorage();
    expect(storage.preferredCameraCodec()).toBe("auto");
    expect(storage.preferredCameraChroma()).toBe("auto");
    expect(storage.preferredMicrophoneCodec()).toBe("auto");
    expect(storage.preferredSurfaceCodecs()).toBe(0);
  });
});
