import { describe, expect, it } from "vitest";
import {
  YAS_FONT_HARD_LIMITS,
  YAS_FONT_LIMIT_MAX_FACE_BYTES,
  YAS_RELAY_HARD_LIMITS,
  YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
  YasWriter,
  fontLimitsExtensions,
  fontLimitsFromExtensions,
  relayLimitsExtensions,
  relayLimitsFromExtensions,
} from "../yas";

describe("Relay and Font negotiated family limits", () => {
  it("round-trips their generated hard ceilings", () => {
    expect(
      relayLimitsFromExtensions(relayLimitsExtensions(YAS_RELAY_HARD_LIMITS)),
    ).toEqual(YAS_RELAY_HARD_LIMITS);
    expect(
      fontLimitsFromExtensions(fontLimitsExtensions(YAS_FONT_HARD_LIMITS)),
    ).toEqual(YAS_FONT_HARD_LIMITS);
  });

  it("rejects above-hard, inconsistent, and unknown required Relay limits", () => {
    expect(() =>
      relayLimitsExtensions({
        ...YAS_RELAY_HARD_LIMITS,
        maxPendingConnects: YAS_RELAY_HARD_LIMITS.maxLinksPerSession + 1,
      }),
    ).toThrow(/invalid Relay family limits/);

    const above = relayLimitsExtensions(YAS_RELAY_HARD_LIMITS).map(
      (extension) =>
        extension.tag === YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS
          ? {
              ...extension,
              value: new YasWriter()
                .u32(YAS_RELAY_HARD_LIMITS.maxPendingConnects + 1)
                .finish(),
            }
          : extension,
    );
    expect(() => relayLimitsFromExtensions(above)).toThrow(
      /invalid Relay family limits/,
    );
    expect(() =>
      relayLimitsFromExtensions([
        ...relayLimitsExtensions(YAS_RELAY_HARD_LIMITS),
        { tag: 99, required: true, value: new Uint8Array(0) },
      ]),
    ).toThrow(/unknown required Relay/);
  });

  it("allows immutable Font catalogues but rejects above-hard and unknown required limits", () => {
    expect(() =>
      fontLimitsExtensions({
        ...YAS_FONT_HARD_LIMITS,
        refreshIntervalNs: 0n,
      }),
    ).not.toThrow();

    const above = fontLimitsExtensions(YAS_FONT_HARD_LIMITS).map((extension) =>
      extension.tag === YAS_FONT_LIMIT_MAX_FACE_BYTES
        ? {
            ...extension,
            value: new YasWriter()
              .u64(YAS_FONT_HARD_LIMITS.maxFaceBytes + 1n)
              .finish(),
          }
        : extension,
    );
    expect(() => fontLimitsFromExtensions(above)).toThrow(
      /invalid Font family limits/,
    );
    expect(() =>
      fontLimitsFromExtensions([
        ...fontLimitsExtensions(YAS_FONT_HARD_LIMITS),
        { tag: 99, required: true, value: new Uint8Array(0) },
      ]),
    ).toThrow(/unknown required Font/);
  });
});
