import { describe, expect, it, vi } from "vitest";
import { resolveInitialFileSnapshot } from "../ide/initialFileSnapshot";

describe("resolveInitialFileSnapshot", () => {
  it("does not report a held snapshot missing before its handle arrives", () => {
    const handle = { live: new Map([["", "contents"]]) };
    const load = vi.fn((h: typeof handle) => h.live.has(""));

    // onSync can be released before syncFs(...).then installs the handle.
    expect(resolveInitialFileSnapshot(null, true, load)).toBe("waiting");
    expect(load).not.toHaveBeenCalled();

    expect(resolveInitialFileSnapshot(handle, true, load)).toBe("loaded");
    expect(load).toHaveBeenCalledExactlyOnceWith(handle);
  });

  it("only reports absence after both handle and coherent snapshot exist", () => {
    const handle = { live: new Map<string, string>() };
    const load = vi.fn((h: typeof handle) => h.live.has(""));

    expect(resolveInitialFileSnapshot(handle, false, load)).toBe("waiting");
    expect(resolveInitialFileSnapshot(handle, true, load)).toBe("missing");
  });
});
