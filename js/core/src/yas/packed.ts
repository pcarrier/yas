/** Validators for TOML-generated packed codecs. */

import * as g from "./generated";
import { decodeEventsBatch, encodeEventsBatch } from "./events";
import {
  decodeTerminalGridV1,
  type YasTerminalGridState,
} from "./terminal-grid";
import { YasCursor, YasProtocolError, YasWriter } from "./wire";

export interface YasSurfaceColorSpace {
  primaries: number;
  transfer: number;
  matrix: number;
  range: number;
}

export interface YasSurfaceDamageRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface YasSurfaceDimensions {
  width: number;
  height: number;
}

export interface YasSurfaceCodecPayload {
  colorSpace?: YasSurfaceColorSpace;
  damage?: readonly YasSurfaceDamageRect[];
  dimensions?: YasSurfaceDimensions;
  /** Full surface extent in logical pixels, independent of encoded resolution. */
  logicalDimensions?: YasSurfaceDimensions;
  bitstream: Uint8Array;
}

export function validateEventsCodecV1(payload: Uint8Array): Uint8Array {
  return encodeEventsBatch(decodeEventsBatch(payload));
}

export function validateMediaCodecPayload(
  codec: number,
  payload: Uint8Array,
  channels = 1,
): Uint8Array {
  if (!Number.isInteger(channels) || channels <= 0 || channels > 255)
    throw new YasProtocolError("invalid Media channel count");
  if (codec === g.YAS_MEDIA_CODEC_PCM_S16LE) {
    requireSampleFrames(payload, channels, 2);
  } else if (codec === g.YAS_MEDIA_CODEC_PCM_F32LE) {
    requireSampleFrames(payload, channels, 4);
    const view = new DataView(
      payload.buffer,
      payload.byteOffset,
      payload.length,
    );
    for (let offset = 0; offset < payload.length; offset += 4) {
      const value = view.getFloat32(offset, true);
      if (
        !Number.isFinite(value) ||
        value < -1 ||
        value > 1 ||
        Object.is(value, -0)
      )
        throw new YasProtocolError("invalid canonical Media f32 sample");
    }
  } else if (codec === g.YAS_MEDIA_CODEC_OPUS) {
    const cursor = new YasCursor(payload);
    const count = cursor.u16("Opus packet count");
    if (cursor.u16("Opus reserved") !== 0 || count === 0)
      throw new YasProtocolError("invalid Opus packet count");
    for (let index = 0; index < count; index++) {
      const packet = cursor.take(
        cursor.u16("Opus packet length"),
        "Opus packet",
      );
      if (packet.length === 0 || packet.length > g.YAS_MEDIA_MAX_PACKET_BYTES)
        throw new YasProtocolError("invalid Opus packet length");
    }
    cursor.end("Opus payload");
  } else if (
    codec === g.YAS_MEDIA_CODEC_H264 ||
    codec === g.YAS_MEDIA_CODEC_H264_444
  ) {
    requireH264(payload);
  } else if (
    codec === g.YAS_MEDIA_CODEC_AV1 ||
    codec === g.YAS_MEDIA_CODEC_AV1_444
  ) {
    requireAv1(payload);
  } else if (codec === g.YAS_MEDIA_CODEC_VP9) {
    if (payload.length === 0) throw new YasProtocolError("empty VP9 frame");
  } else if (codec === g.YAS_MEDIA_CODEC_MJPEG) {
    if (
      payload.length < 4 ||
      payload[0] !== 0xff ||
      payload[1] !== 0xd8 ||
      payload[payload.length - 2] !== 0xff ||
      payload[payload.length - 1] !== 0xd9
    )
      throw new YasProtocolError("invalid MJPEG interchange datastream");
  } else throw new YasProtocolError("unknown Media packed codec");
  return new Uint8Array(payload);
}

export function decodeSurfaceCodecPayload(
  codec: number,
  payload: Uint8Array,
): YasSurfaceCodecPayload {
  if (
    codec !== g.YAS_SURFACE_CODEC_H264_V1 &&
    codec !== g.YAS_SURFACE_CODEC_AV1_V1 &&
    codec !== g.YAS_SURFACE_CODEC_PNG_V1
  )
    throw new YasProtocolError("unknown Surface packed codec");
  const cursor = new YasCursor(payload);
  const count = cursor.u8("Surface metadata count");
  requireZero(cursor.take(3, "Surface metadata reserved"), "Surface metadata");
  let previous = -1;
  let colorSpace: YasSurfaceColorSpace | undefined;
  let damage: YasSurfaceDamageRect[] | undefined;
  let dimensions: YasSurfaceDimensions | undefined;
  let logicalDimensions: YasSurfaceDimensions | undefined;
  for (let index = 0; index < count; index++) {
    const tag = cursor.u16("Surface metadata tag");
    const flags = cursor.u16("Surface metadata flags");
    const body = cursor.sub(
      cursor.u32("Surface metadata length"),
      "Surface metadata",
    );
    if (tag <= previous || flags & ~1)
      throw new YasProtocolError("invalid Surface metadata ordering or flags");
    previous = tag;
    if (tag === g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_COLOR_SPACE) {
      colorSpace = {
        primaries: body.u8("Surface color primaries"),
        transfer: body.u8("Surface transfer"),
        matrix: body.u8("Surface matrix"),
        range: body.u8("Surface range"),
      };
      body.end("Surface color space");
    } else if (
      tag === g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_DAMAGE
    ) {
      const rects = body.u16("Surface damage count");
      if (
        body.u16("Surface damage reserved") !== 0 ||
        rects > g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_MAX_DAMAGE_RECTS ||
        rects > Math.floor(body.remaining / 16)
      )
        throw new YasProtocolError("invalid Surface damage count");
      damage = [];
      for (let rect = 0; rect < rects; rect++) {
        const value = {
          x: body.u32("Surface damage x"),
          y: body.u32("Surface damage y"),
          width: body.u32("Surface damage width"),
          height: body.u32("Surface damage height"),
        };
        if (value.width === 0 || value.height === 0)
          throw new YasProtocolError("empty Surface damage rectangle");
        damage.push(value);
      }
      body.end("Surface damage");
    } else if (
      tag === g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_DIMENSIONS ||
      tag ===
        g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_LOGICAL_DIMENSIONS
    ) {
      const size = {
        width: body.u32("Surface width"),
        height: body.u32("Surface height"),
      };
      if (size.width === 0 || size.height === 0)
        throw new YasProtocolError("empty Surface dimensions");
      body.end("Surface dimensions");
      if (tag === g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_DIMENSIONS)
        dimensions = size;
      else logicalDimensions = size;
    } else if (flags & 1)
      throw new YasProtocolError("unknown required Surface metadata");
  }
  const bitstream = new Uint8Array(
    cursor.take(cursor.remaining, "Surface bitstream"),
  );
  if (codec === g.YAS_SURFACE_CODEC_H264_V1) requireH264(bitstream);
  else if (codec === g.YAS_SURFACE_CODEC_AV1_V1) requireAv1(bitstream);
  else requirePng(bitstream);
  return { colorSpace, damage, dimensions, logicalDimensions, bitstream };
}

export function encodeSurfaceCodecPayload(
  codec: number,
  value: YasSurfaceCodecPayload,
): Uint8Array {
  if (
    codec !== g.YAS_SURFACE_CODEC_H264_V1 &&
    codec !== g.YAS_SURFACE_CODEC_AV1_V1 &&
    codec !== g.YAS_SURFACE_CODEC_PNG_V1
  )
    throw new YasProtocolError("unknown Surface packed codec");
  const count =
    Number(value.colorSpace !== undefined) +
    Number(value.damage !== undefined) +
    Number(value.dimensions !== undefined) +
    Number(value.logicalDimensions !== undefined);
  const writer = new YasWriter().u8(count).bytes(new Uint8Array(3));
  if (value.colorSpace) {
    const body = new Uint8Array([
      value.colorSpace.primaries,
      value.colorSpace.transfer,
      value.colorSpace.matrix,
      value.colorSpace.range,
    ]);
    writer
      .u16(g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_COLOR_SPACE)
      .u16(0)
      .bytesU32(body);
  }
  if (value.damage) {
    if (
      value.damage.length >
      g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_MAX_DAMAGE_RECTS
    )
      throw new YasProtocolError("too many Surface damage rectangles");
    const body = new YasWriter().u16(value.damage.length).u16(0);
    for (const rect of value.damage) {
      if (rect.width === 0 || rect.height === 0)
        throw new YasProtocolError("empty Surface damage rectangle");
      body.u32(rect.x).u32(rect.y).u32(rect.width).u32(rect.height);
    }
    writer
      .u16(g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_DAMAGE)
      .u16(0)
      .bytesU32(body.finish());
  }
  for (const [tag, dimensions] of [
    [
      g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_DIMENSIONS,
      value.dimensions,
    ],
    [
      g.YAS_PACKED_CODEC_SURFACE_CODEC_H264_V1_METADATA_LOGICAL_DIMENSIONS,
      value.logicalDimensions,
    ],
  ] as const) {
    if (!dimensions) continue;
    if (
      !Number.isInteger(dimensions.width) ||
      !Number.isInteger(dimensions.height) ||
      dimensions.width <= 0 ||
      dimensions.height <= 0 ||
      dimensions.width > 0xffff_ffff ||
      dimensions.height > 0xffff_ffff
    )
      throw new YasProtocolError("invalid Surface dimensions");
    writer.u16(tag).u16(0).u32(8).u32(dimensions.width).u32(dimensions.height);
  }
  const output = writer.bytes(value.bitstream).finish();
  decodeSurfaceCodecPayload(codec, output);
  return output;
}

export function validateTerminalGridCodecPayload(
  payload: Uint8Array,
  flags: number,
  maxDecodedFrame = 4 * 1024 * 1024,
): YasTerminalGridState {
  return decodeTerminalGridV1(
    { viewId: 1, sequence: 1, flags, gridPayload: payload },
    null,
    maxDecodedFrame,
  );
}

function requireSampleFrames(
  payload: Uint8Array,
  channels: number,
  sampleBytes: number,
): void {
  if (payload.length === 0 || payload.length % (channels * sampleBytes) !== 0)
    throw new YasProtocolError("Media PCM payload splits a sample frame");
}

function requireH264(payload: Uint8Array): void {
  if (
    payload.length < 4 ||
    !(
      (payload[0] === 0 && payload[1] === 0 && payload[2] === 1) ||
      (payload[0] === 0 &&
        payload[1] === 0 &&
        payload[2] === 0 &&
        payload[3] === 1)
    )
  )
    throw new YasProtocolError("H.264 access unit lacks Annex-B start code");
}

function requireAv1(payload: Uint8Array): void {
  // Temporal delimiter OBU: forbidden=0, type=2. Extension and size-field
  // details remain codec-owned, but the unit must start with this delimiter.
  if (payload.length === 0 || ((payload[0]! >> 3) & 0x0f) !== 2)
    throw new YasProtocolError("AV1 temporal unit lacks temporal delimiter");
}

function requirePng(payload: Uint8Array): void {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (
    payload.length < signature.length ||
    signature.some((byte, index) => payload[index] !== byte)
  )
    throw new YasProtocolError("invalid PNG signature");
}

function requireZero(value: Uint8Array, context: string): void {
  if (value.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}
