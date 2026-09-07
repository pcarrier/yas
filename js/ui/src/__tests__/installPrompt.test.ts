import { describe, expect, it, vi } from "vitest";

describe("installPrompt", () => {
  it("retains the prompt without suppressing the browser install UI", async () => {
    const installPrompt = await import("../installPrompt");
    const event = new Event("beforeinstallprompt", { cancelable: true });
    const prompt = vi.fn(() => Promise.resolve());
    Object.defineProperty(event, "prompt", { value: prompt });

    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(installPrompt.getInstallPrompt()).toBe(event);

    window.dispatchEvent(new Event("appinstalled"));
    expect(installPrompt.getInstallPrompt()).toBeNull();
  });
});
