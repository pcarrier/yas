import { describe, expect, it } from "vitest";

import {
  YAS_MEDIA_CODEC_AV1_444,
  YAS_MEDIA_CODEC_H264_444,
  YAS_MEDIA_DEVICE_AVAILABLE,
  YAS_MEDIA_KIND_CAMERA,
  YasWriter,
  decodeMediaDeviceRecord,
  encodeExtensions,
} from "../yas";

function mediaDeviceRecord(codecs: readonly number[]): Uint8Array {
  const writer = new YasWriter()
    .u64(1n)
    .u64(1n)
    .u8(YAS_MEDIA_KIND_CAMERA)
    .u8(YAS_MEDIA_DEVICE_AVAILABLE)
    .u16(0)
    .utf8U16("camera")
    .u16(codecs.length)
    .u16(0);
  for (const codec of codecs)
    writer
      .u16(codec)
      .u16(0)
      .u32(0)
      .u32(1920)
      .u32(1080)
      .u32(60_000)
      .bytes(encodeExtensions());
  return writer.bytes(encodeExtensions()).finish();
}

describe("YAS Media video codec validation", () => {
  it("decodes every generated 4:4:4 camera codec during bootstrap", () => {
    const codecs = [YAS_MEDIA_CODEC_H264_444, YAS_MEDIA_CODEC_AV1_444];
    expect(
      decodeMediaDeviceRecord(mediaDeviceRecord(codecs)).formats.map(
        (format) => format.codec,
      ),
    ).toEqual(codecs);
  });

  it.each([0, 255, 262, 0xffff])("rejects unknown camera codec %i", (codec) => {
    expect(() => decodeMediaDeviceRecord(mediaDeviceRecord([codec]))).toThrow(
      /invalid Media format/,
    );
  });
});
