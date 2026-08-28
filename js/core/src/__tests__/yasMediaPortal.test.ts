import { describe, expect, it } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YasProtocolError,
  decodeMediaPortalReply,
  decodeMediaPortalRequest,
  decodeMediaPortalClose,
  decodeMediaPortalRecord,
  decodeMediaPlayerRecord,
  encodeMediaPortalReply,
  encodeMediaPortalRequest,
  encodeMediaPortalClose,
  encodeMediaPortalRecord,
  encodeMediaPlayerRecord,
  mediaPlayerActive,
  YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
} from "../yas";

function payload(name: string): Uint8Array {
  const value = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  );
  if (!value) throw new Error(`missing vector ${name}`);
  return Uint8Array.from(value.hex.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

describe("YAS Media typed portal metadata", () => {
  it("retains and validates the exact selected MPRIS player", () => {
    const player = {
      kind: "player" as const,
      playerHandle: 0xfedc_ba98_7654_3210n,
      revision: 7n,
      state: 2,
      flags: 1,
      positionUs: 3n,
      durationUs: 9n,
      identity: "player",
      desktopEntry: "player",
      title: "track",
      artist: "artist",
      album: "album",
      extensions: [
        {
          tag: YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
          required: false,
          value: new Uint8Array([1]),
        },
      ],
    };
    const decoded = decodeMediaPlayerRecord(encodeMediaPlayerRecord(player));
    expect(decoded.playerHandle).toBe(player.playerHandle);
    expect(mediaPlayerActive(decoded)).toBe(true);
    expect(mediaPlayerActive({ extensions: [] })).toBeNull();
    expect(() =>
      encodeMediaPlayerRecord({
        ...player,
        extensions: [
          {
            tag: YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
            required: false,
            value: new Uint8Array([2]),
          },
        ],
      }),
    ).toThrow("active state");
  });

  for (const name of [
    "media.portal_access_request.payload",
    "media.portal_screencast_request.payload",
  ]) {
    it(`round-trips and rejects every truncation of ${name}`, () => {
      const bytes = payload(name);
      expect(encodeMediaPortalRequest(decodeMediaPortalRequest(bytes))).toEqual(
        bytes,
      );
      for (let end = 0; end < bytes.length; end++)
        expect(() => decodeMediaPortalRequest(bytes.subarray(0, end))).toThrow(
          YasProtocolError,
        );
    });
  }

  for (const name of [
    "media.portal_access_reply.payload",
    "media.portal_screencast_reply.payload",
  ]) {
    it(`round-trips and rejects every truncation of ${name}`, () => {
      const bytes = payload(name);
      expect(encodeMediaPortalReply(decodeMediaPortalReply(bytes))).toEqual(
        bytes,
      );
      for (let end = 0; end < bytes.length; end++)
        expect(() => decodeMediaPortalReply(bytes.subarray(0, end))).toThrow(
          YasProtocolError,
        );
    });
  }

  it("round-trips and rejects every PORTAL_CLOSE truncation", () => {
    const bytes = payload("media.portal_close.payload");
    expect(encodeMediaPortalClose(decodeMediaPortalClose(bytes))).toEqual(
      bytes,
    );
    for (let end = 0; end < bytes.length; end++)
      expect(() => decodeMediaPortalClose(bytes.subarray(0, end))).toThrow(
        YasProtocolError,
      );
  });

  it("round-trips and rejects every granted PortalRecord truncation", () => {
    const bytes = payload("media.portal_granted.payload");
    expect(encodeMediaPortalRecord(decodeMediaPortalRecord(bytes))).toEqual(
      bytes,
    );
    for (let end = 0; end < bytes.length; end++)
      expect(() => decodeMediaPortalRecord(bytes.subarray(0, end))).toThrow(
        YasProtocolError,
      );
  });
});
