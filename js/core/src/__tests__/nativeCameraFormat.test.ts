import { describe, expect, it } from "vitest";
import type { YasMediaFormat } from "../yas/media";
import { cameraCaptureFormat } from "../yas/nativeDesktopMedia";

describe("cameraCaptureFormat", () => {
  it("offers the physical camera mode instead of the catalogue placeholder", () => {
    const advertised: YasMediaFormat = {
      codec: 259,
      channels: 0,
      sampleRate: 0,
      width: 1920,
      height: 1080,
      frameRateMilli: 30_000,
      extensions: [],
    };

    expect(cameraCaptureFormat(advertised, 1280, 720, 60)).toEqual({
      ...advertised,
      width: 1280,
      height: 720,
      frameRateMilli: 60_000,
    });
  });
});
