import { describe, expect, it } from "vitest";
import { YAS_SURFACE_STATE_MINIMUM_SIZE_EXTENSION } from "../yas/generated";
import {
  decodeSurfaceRecord,
  encodeSurfaceRecord,
  surfaceMinimumSize,
  type YasSurfaceRecord,
} from "../yas/surface";
import { YasWriter } from "../yas/wire";

const record: YasSurfaceRecord = {
  surfaceHandle: 1n,
  revision: 1n,
  parentHandle: 0n,
  appHandle: 0n,
  lifecycle: 0,
  compositeWidth: 400,
  compositeHeight: 800,
  logicalWidth32_32: 400n << 32n,
  logicalHeight32_32: 800n << 32n,
  applicationId: "test",
  title: "Test",
  extensions: [],
};
const withMinimum = (value: Uint8Array): YasSurfaceRecord => ({
  ...record,
  extensions: [
    { tag: YAS_SURFACE_STATE_MINIMUM_SIZE_EXTENSION, required: false, value },
  ],
});

describe("Surface minimum size hints", () => {
  it("round trips a minimum independently of the currently rendered geometry", () => {
    const decoded = decodeSurfaceRecord(
      encodeSurfaceRecord(
        withMinimum(new YasWriter().u32(500).u32(0).finish()),
      ),
    );
    expect(surfaceMinimumSize(decoded)).toEqual({ width: 500, height: 0 });
    expect(decoded.compositeWidth).toBe(400);
  });
  it("supports absent hints and explicit release", () => {
    expect(surfaceMinimumSize(record)).toBeUndefined();
    expect(surfaceMinimumSize(withMinimum(new Uint8Array(8)))).toEqual({
      width: 0,
      height: 0,
    });
  });
  it.each([new Uint8Array(7), new Uint8Array(9), new Uint8Array(8).fill(255)])(
    "rejects malformed or out-of-range hints",
    (value) => {
      expect(() => encodeSurfaceRecord(withMinimum(value))).toThrow();
    },
  );
});
