import { describe, expect, it } from "vitest";
import {
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_CLIENT,
  YAS_FAMILY_FONT,
  YAS_FAMILY_FS,
  YAS_FAMILY_GIT,
  YAS_FAMILY_KV,
  YAS_FAMILY_LSP,
  YAS_FAMILY_MEDIA,
  YAS_FAMILY_NET,
  YAS_FAMILY_RELAY,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TERMINAL,
  type YasClientInfo,
  type YasClientOrigin,
} from "@yas-run/core";
import {
  formatClientAge,
  formatClientBandwidth,
  formatClientLabel,
  formatClientOriginTag,
  formatClientSubscription,
  formatExtensionAttempt,
  formatExtensionTitle,
  formatKickAction,
  formatSurfaceViewSize,
  formatTerminalViewSize,
} from "../clientDisplay";

describe("client subscription sizes", () => {
  it("formats terminal dimensions as columns by rows", () => {
    expect(formatTerminalViewSize(120, 40)).toBe("120×40");
    expect(formatTerminalViewSize(null, null)).toBe("size not reported");
  });

  it("formats surface dimensions and fractional scale", () => {
    expect(formatSurfaceViewSize(1920, 1080, 120)).toBe("1920×1080 @ 1×");
    expect(formatSurfaceViewSize(1280, 720, 180)).toBe("1280×720 @ 1.5×");
    expect(formatSurfaceViewSize(800, 600, null)).toBe("800×600");
    expect(formatSurfaceViewSize(null, null, null)).toBe("size not reported");
  });

  it("keeps trailing zeros that belong to a scale's integer part", () => {
    // Trimming a trailing "0" after stripping ".00" turned 10× into 1×.
    expect(formatSurfaceViewSize(640, 480, 1200)).toBe("640×480 @ 10×");
    expect(formatSurfaceViewSize(640, 480, 2400)).toBe("640×480 @ 20×");
    // Fractional zeros should still go.
    expect(formatSurfaceViewSize(640, 480, 132)).toBe("640×480 @ 1.1×");
    expect(formatSurfaceViewSize(640, 480, 240)).toBe("640×480 @ 2×");
    // Sub-1× scales round to 2dp rather than disappearing.
    expect(formatSurfaceViewSize(640, 480, 100)).toBe("640×480 @ 0.83×");
  });

  it("formats client age and outbound bandwidth", () => {
    expect(formatClientAge(45)).toBe("45s");
    expect(formatClientAge(125)).toBe("2m 5s");
    expect(formatClientAge(7_380)).toBe("2h 3m");
    expect(formatClientAge(183_600)).toBe("2d 3h");
    expect(formatClientBandwidth(0)).toBe("0 B/s");
    expect(formatClientBandwidth(1_500)).toBe("1.5 kB/s");
    expect(formatClientBandwidth(1_500_000)).toBe("1.5 MB/s");
  });

  it("names every family the protocol defines", async () => {
    // The list a client really holds: Relay, Terminals, Clients, Surfaces,
    // Selection, Desktop, Media, Fonts, four KV namespaces, Channels. Every
    // one of these read "Unknown <id> #0" before.
    expect(formatClientSubscription(YAS_FAMILY_RELAY, 0n, 1)).toBe(
      "Relay routes · watch #1",
    );
    expect(formatClientSubscription(YAS_FAMILY_TERMINAL, 0n, 2)).toBe(
      "Terminals · watch #2",
    );
    expect(formatClientSubscription(YAS_FAMILY_CLIENT, 0n, 3)).toBe(
      "Clients · watch #3",
    );
    expect(formatClientSubscription(YAS_FAMILY_SURFACE, 0n, 4)).toBe(
      "Surfaces · watch #4",
    );
    expect(formatClientSubscription(YAS_FAMILY_FONT, 0n, 5)).toBe(
      "Fonts · watch #5",
    );
    expect(formatClientSubscription(YAS_FAMILY_CHANNEL, 0n, 6)).toBe(
      "Channels · watch #6",
    );
  });

  it("leaves no family to fall back on a bare id", async () => {
    const core = await import("@yas-run/core");
    const families = Object.entries(core).filter(
      ([name, value]) =>
        name.startsWith("YAS_FAMILY_") && typeof value === "number",
    );
    expect(families.length).toBeGreaterThan(15);
    for (const [name, id] of families) {
      expect(
        formatClientSubscription(id as number, 0n, 1),
        `${name} has no label`,
      ).not.toMatch(/^Family /);
    }
  });

  it("names the resource a watch points at, where there is one", () => {
    expect(formatClientSubscription(YAS_FAMILY_FS, 3n, 1)).toBe(
      "Filesystem root 3 · watch #1",
    );
    expect(formatClientSubscription(YAS_FAMILY_GIT, 4n, 2)).toBe(
      "Git repository 4 · watch #2",
    );
    expect(formatClientSubscription(YAS_FAMILY_LSP, 5n, 3)).toBe(
      "LSP workspace 5 · watch #3",
    );
    expect(formatClientSubscription(YAS_FAMILY_KV, 6n, 4)).toBe(
      "KV namespace 6 · watch #4",
    );
    expect(formatClientSubscription(YAS_FAMILY_MEDIA, 0n, 5)).toBe(
      "Media · watch #5",
    );
  });

  it("keeps a KV namespace 0 but drops an unresolved resource", () => {
    // The server reports zero for an FS/Git/LSP watch it has no mapping for,
    // and KV namespace 0 is a handle the peer really opened.
    expect(formatClientSubscription(YAS_FAMILY_KV, 0n, 1)).toBe(
      "KV namespace 0 · watch #1",
    );
    expect(formatClientSubscription(YAS_FAMILY_FS, 0n, 2)).toBe(
      "Filesystem · watch #2",
    );
  });

  it("separates two watches on one resource", () => {
    expect(formatClientSubscription(YAS_FAMILY_KV, 1n, 8)).not.toBe(
      formatClientSubscription(YAS_FAMILY_KV, 1n, 9),
    );
  });
});

describe("client identity", () => {
  function client(origin: YasClientOrigin | null): YasClientInfo {
    return {
      id: "00000000000000000000000000000007",
      ageSeconds: 11,
      outboundBytesPerSecond: 0,
      inboundBytesPerSecond: 0,
      subscriptions: [],
      terminals: [],
      surfaces: [],
      origin,
    };
  }

  const extension: YasClientOrigin = {
    kind: "extension",
    extensionId: 0x05a3415a2dd1ef9bn,
    definitionRevision: 2n,
    attempt: 3n,
    taskId: 4,
    name: "systemd",
  };

  it("names an extension by its definition, not its connection id", () => {
    expect(formatClientLabel(client(extension))).toBe("systemd");
    expect(formatClientOriginTag(client(extension))).toBe("extension");
    expect(formatExtensionAttempt(client(extension))).toBe("attempt 3");
    // The task id is a random 32-bit handle, not an ordinal, so it stays in
    // the tooltip where it costs no attention until someone wants it.
    expect(formatExtensionTitle(client(extension))).toBe(
      "Extension id:05a3415a2dd1ef9b · revision 2 · task 4",
    );
  });

  it("falls back to the id an unnamed transient run is addressed by", () => {
    // The same handle the extensions panel shows and `ext status` accepts.
    expect(formatClientLabel(client({ ...extension, name: "" }))).toBe(
      "id:05a3415a2dd1ef9b",
    );
  });

  it("leaves an ordinary client, and an unasked one, unadorned", () => {
    for (const origin of [{ kind: "network" } as const, null]) {
      expect(formatClientLabel(client(origin))).toBe(
        "Client 00000000000000000000000000000007",
      );
      expect(formatClientOriginTag(client(origin))).toBeNull();
      expect(formatExtensionAttempt(client(origin))).toBeNull();
      expect(formatKickAction(client(origin)).idle).toBe("Kick");
    }
  });

  it("says a kind it cannot name is not an ordinary client", () => {
    const unknown = client({ kind: "unknown", originKind: 200 });
    expect(formatClientLabel(unknown)).toBe(
      "Client 00000000000000000000000000000007",
    );
    expect(formatClientOriginTag(unknown)).toBe("unrecognized");
  });

  it("tells the viewer that kicking an extension ends its attempt", () => {
    expect(formatKickAction(client(extension))).toEqual({
      idle: "Stop attempt",
      confirm: "Confirm stop",
      busy: "Stopping…",
    });
  });
});
