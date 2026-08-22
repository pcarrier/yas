import { describe, expect, it } from "vitest";
import {
  YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
  YAS_GOLDEN_VECTORS,
  YAS_MEDIA_CODEC_AV1,
  YAS_MEDIA_CODEC_AV1_444,
  YAS_MEDIA_CODEC_H264,
  YAS_MEDIA_CODEC_H264_444,
  YAS_MEDIA_CODEC_MJPEG,
  YAS_MEDIA_CODEC_OPUS,
  YAS_MEDIA_CODEC_PCM_F32LE,
  YAS_MEDIA_CODEC_PCM_S16LE,
  YAS_MEDIA_CODEC_VP9,
  YAS_SCHEMA,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_CODEC_PNG_V1,
  YAS_TERMINAL_GOLDEN_FRAME_FLAGS,
  YasCursor,
  YasWriter,
  decodeChannelListen,
  decodeClientBandwidthRates,
  decodeDesktopTrayAction,
  decodeEnvGet,
  decodeEventsSetConfig,
  decodeExtensionDeploy,
  decodeFontFamily,
  decodeFsOpen,
  decodeGitOpen,
  decodeKvOpen,
  decodeLspOpen,
  decodeMediaPortalClose,
  decodeNetOpen,
  decodePing,
  decodeProcessSpawn,
  decodeRelayRoute,
  decodeSelectionDragDrop,
  decodeSurfaceCodecPayload,
  decodeSurfaceRemoteInput,
  decodeTerminalFrame,
  decodeTransferDescriptor,
  encodeExtensions,
  validateEventsCodecV1,
  validateMediaCodecPayload,
  validateTerminalGridCodecPayload,
} from "../yas";

type Decoder = (bytes: Uint8Array) => unknown;

interface FuzzTarget {
  name: string;
  decode: Decoder;
  valid: Uint8Array;
}

const familyTargets: readonly FuzzTarget[] = [
  target("yas.core", decodePing, "core.ping.payload"),
  target(
    "yas.transfer",
    (bytes) => decodeTransferDescriptor(new YasCursor(bytes)),
    "transfer.descriptor.payload",
  ),
  {
    name: "yas.relay",
    decode: decodeRelayRoute,
    valid: new YasWriter()
      .u64(1n)
      .u64(1n)
      .u8(0)
      .u8(0)
      .u16(0)
      .utf8U16("local")
      .utf8U16("Local")
      .utf8U32("")
      .bytes(encodeExtensions())
      .finish(),
  },
  target(
    "yas.terminal",
    decodeTerminalFrame,
    "terminal.frame.byte_budget.payload",
  ),
  {
    name: "yas.client",
    decode: (bytes) =>
      decodeClientBandwidthRates([
        {
          tag: YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
          flags: 0,
          value: bytes,
        },
      ]),
    valid: golden("client.bandwidth_rates.payload"),
  },
  target(
    "yas.surface",
    decodeSurfaceRemoteInput,
    "surface.remote_input.payload",
  ),
  target(
    "yas.selection",
    decodeSelectionDragDrop,
    "selection.drag_drop.payload",
  ),
  target("yas.desktop", decodeDesktopTrayAction, "desktop.tray_action.payload"),
  target("yas.media", decodeMediaPortalClose, "media.portal_close.payload"),
  {
    name: "yas.font",
    decode: decodeFontFamily,
    valid: new YasWriter()
      .u64(1n)
      .u64(1n)
      .u16(0)
      .u16(0)
      .utf8U16("YAS Mono")
      .utf8U16("YAS Mono")
      .bytes(encodeExtensions())
      .finish(),
  },
  target("yas.fs", decodeFsOpen, "fs.open.payload"),
  target("yas.git", decodeGitOpen, "git.open.payload"),
  target("yas.lsp", decodeLspOpen, "lsp.open.payload"),
  target("yas.kv", decodeKvOpen, "kv.open.payload"),
  target("yas.process", decodeProcessSpawn, "process.spawn.payload"),
  target("yas.net", decodeNetOpen, "net.open.payload"),
  target("yas.channel", decodeChannelListen, "channel.listen.payload"),
  target("yas.extension", decodeExtensionDeploy, "extension.deploy.payload"),
  target("yas.events", decodeEventsSetConfig, "events.set_config.payload"),
  target("yas.env", decodeEnvGet, "env.get.payload"),
];

const packedTargets: readonly FuzzTarget[] = [
  target("events-v1", validateEventsCodecV1, "packed_codec.events-v1.payload"),
  mediaTarget("media-av1-444-v1", YAS_MEDIA_CODEC_AV1_444, 1),
  mediaTarget("media-av1-v1", YAS_MEDIA_CODEC_AV1, 1),
  mediaTarget("media-h264-444-v1", YAS_MEDIA_CODEC_H264_444, 1),
  mediaTarget("media-h264-v1", YAS_MEDIA_CODEC_H264, 1),
  mediaTarget("media-mjpeg-v1", YAS_MEDIA_CODEC_MJPEG, 1),
  mediaTarget("media-opus-v1", YAS_MEDIA_CODEC_OPUS, 1),
  mediaTarget("media-pcm-f32le-v1", YAS_MEDIA_CODEC_PCM_F32LE, 1),
  mediaTarget("media-pcm-s16le-v1", YAS_MEDIA_CODEC_PCM_S16LE, 2),
  mediaTarget("media-vp9-v1", YAS_MEDIA_CODEC_VP9, 1),
  surfaceTarget("surface-av1-v1", YAS_SURFACE_CODEC_AV1_V1),
  surfaceTarget("surface-h264-v1", YAS_SURFACE_CODEC_H264_V1),
  surfaceTarget("surface-png-v1", YAS_SURFACE_CODEC_PNG_V1),
  target(
    "terminal-grid-v1",
    (bytes) =>
      validateTerminalGridCodecPayload(bytes, YAS_TERMINAL_GOLDEN_FRAME_FLAGS),
    "packed_codec.terminal-grid-v1.payload",
  ),
];

describe("YAS deterministic decoder fuzz corpus", () => {
  it("keeps an explicit fuzz target for every registered family", () => {
    expect(familyTargets.map(({ name }) => name)).toEqual(
      YAS_SCHEMA.families.map(({ name }) => name),
    );
  });

  it("accepts each family's valid seed", () => {
    for (const fuzzTarget of familyTargets) {
      expect(
        () => fuzzTarget.decode(fuzzTarget.valid),
        fuzzTarget.name,
      ).not.toThrow();
    }
  });

  it("routes deterministic arbitrary bytes through every family decoder", () => {
    const corpus = deterministicCorpus(familyTargets.map(({ valid }) => valid));
    for (const fuzzTarget of familyTargets) {
      for (const bytes of corpus) exercise(fuzzTarget, bytes);
    }
  });

  it("accepts every packed codec seed", () => {
    for (const fuzzTarget of packedTargets) {
      expect(
        () => fuzzTarget.decode(fuzzTarget.valid),
        fuzzTarget.name,
      ).not.toThrow();
    }
  });

  it("routes deterministic arbitrary bytes through every packed codec", () => {
    const corpus = deterministicCorpus(packedTargets.map(({ valid }) => valid));
    for (const fuzzTarget of packedTargets) {
      for (const bytes of corpus) exercise(fuzzTarget, bytes);
    }
  });
});

function target(name: string, decode: Decoder, vector: string): FuzzTarget {
  return { name, decode, valid: golden(vector) };
}

function mediaTarget(
  name: string,
  codec: number,
  channels: number,
): FuzzTarget {
  return target(
    name,
    (bytes) => validateMediaCodecPayload(codec, bytes, channels),
    `packed_codec.${name}.payload`,
  );
}

function surfaceTarget(name: string, codec: number): FuzzTarget {
  return target(
    name,
    (bytes) => decodeSurfaceCodecPayload(codec, bytes),
    `packed_codec.${name}.logical_dimensions.payload`,
  );
}

function golden(name: string): Uint8Array {
  const value = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  );
  if (!value) throw new Error(`missing generated YAS vector ${name}`);
  return Uint8Array.from(value.hex.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

function deterministicCorpus(seeds: readonly Uint8Array[]): Uint8Array[] {
  const corpus = [
    new Uint8Array(),
    new Uint8Array([0]),
    new Uint8Array([0xff]),
    new Uint8Array(96),
    new Uint8Array(96).fill(0xff),
  ];
  for (const seed of seeds) {
    corpus.push(new Uint8Array(seed));
    for (const length of new Set([
      0,
      1,
      Math.floor(seed.length / 2),
      Math.max(0, seed.length - 1),
    ])) {
      corpus.push(seed.slice(0, length));
    }
    for (const offset of new Set([
      0,
      Math.floor(seed.length / 2),
      Math.max(0, seed.length - 1),
    ])) {
      if (seed.length === 0) continue;
      const changed = new Uint8Array(seed);
      changed[offset] ^= 0xff;
      corpus.push(changed);
    }
    const extended = new Uint8Array(seed.length + 1);
    extended.set(seed);
    extended[seed.length] = 0xa5;
    corpus.push(extended);
  }

  let state = 0x59_41_53_31;
  const next = (): number => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return state >>> 0;
  };
  for (let sample = 0; sample < 256; sample++) {
    const bytes = new Uint8Array(next() % 97);
    for (let offset = 0; offset < bytes.length; offset++) {
      bytes[offset] = next() & 0xff;
    }
    corpus.push(bytes);
  }
  return corpus;
}

function exercise(fuzzTarget: FuzzTarget, bytes: Uint8Array): void {
  try {
    fuzzTarget.decode(bytes);
  } catch (error) {
    if (error instanceof Error) return;
    throw new Error(
      `${fuzzTarget.name} decoder threw a non-Error for ${toHex(bytes)}`,
      { cause: error },
    );
  }
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}
