import { describe, expect, it } from "vitest";
import { YAS_SURFACE_STATE_TEXT_INPUT_REQUEST_REVISION_EXTENSION } from "../yas/generated";
import {
  decodeSurfaceRecord,
  encodeSurfaceRecord,
  surfaceTextInputRequestRevision,
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
const withRevision = (value: Uint8Array): YasSurfaceRecord => ({
  ...record,
  extensions: [
    {
      tag: YAS_SURFACE_STATE_TEXT_INPUT_REQUEST_REVISION_EXTENSION,
      required: false,
      value,
    },
  ],
});

describe("Surface text-input request revisions", () => {
  it("round trips the last request independently of geometry", () => {
    const decoded = decodeSurfaceRecord(
      encodeSurfaceRecord(withRevision(new YasWriter().u64(42n).finish())),
    );
    expect(surfaceTextInputRequestRevision(decoded)).toBe(42n);
    expect(decoded.compositeWidth).toBe(400);
    expect(surfaceTextInputRequestRevision(record)).toBeUndefined();
  });
  it.each([new Uint8Array(7), new Uint8Array(9), new Uint8Array(8)])(
    "rejects malformed or zero revisions",
    (value) => {
      expect(() => encodeSurfaceRecord(withRevision(value))).toThrow();
    },
  );
});
