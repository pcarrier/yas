import { describe, expect, it } from "vitest";
import * as core from "../index";

describe("@yas-run/core public surface", () => {
  it("does not publish retired packet runtimes or packet builders", () => {
    const exported = core as Record<string, unknown>;
    for (const name of [
      "YasCompatibilityTransport",
      "C2S_NET_OPEN",
      "S2C_NET_OPENED",
      "buildNetOpenMessage",
      "parseNetOpenedMessage",
      "C2S_EXT_RUN",
      "buildExtensionRunMessage",
      "parseExtensionMessage",
      "buildFsOpenMessage",
      "parseGitMessage",
    ]) {
      expect(exported, name).not.toHaveProperty(name);
    }
    expect(
      Object.keys(exported).filter((name) => /^(?:C2S|S2C)_/.test(name)),
    ).toEqual([]);
  });
});
