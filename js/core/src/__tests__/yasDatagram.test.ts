import { describe, expect, it } from "vitest";
import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_DATAGRAM_FORBIDDEN,
  YAS_DATAGRAM_MEDIA_FRAME,
  YAS_DATAGRAM_NET_NATIVE_FLOW,
  YAS_DATAGRAM_SURFACE_FRAME,
  YAS_FAMILY_CORE,
  YAS_FAMILY_MEDIA,
  YAS_FAMILY_NET,
  YAS_FAMILY_SURFACE,
  YAS_MEDIA_CODEC_H264,
  YAS_MEDIA_FRAME,
  YAS_MEDIA_FRAME_DISCARDABLE,
  YAS_MEDIA_FRAME_KEYFRAME,
  YAS_NET_DATAGRAM,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_FRAME,
  YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE,
  YAS_SURFACE_FRAME_DISCARDABLE,
  YAS_SURFACE_FRAME_KEYFRAME,
  encodeMediaFrame,
  encodeNetDatagram,
  encodeSurfaceFrame,
  validateYasDatagramFrame,
  yasOperationPolicy,
  type YasFrame,
} from "../yas";

function event(family: number, kind: number, payload: Uint8Array): YasFrame {
  return {
    family,
    kind,
    class: YAS_CLASS_EVENT,
    compressed: false,
    sensitive: true,
    payload,
  };
}

describe("YAS transport datagram predicates", () => {
  it("exposes generated numeric policies for both reliable and datagram-safe operations", () => {
    expect(
      yasOperationPolicy(YAS_FAMILY_NET, YAS_CLASS_EVENT, YAS_NET_DATAGRAM)
        ?.datagram,
    ).toBe(YAS_DATAGRAM_NET_NATIVE_FLOW);
    expect(
      yasOperationPolicy(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_FRAME)
        ?.datagram,
    ).toBe(YAS_DATAGRAM_SURFACE_FRAME);
    expect(
      yasOperationPolicy(YAS_FAMILY_MEDIA, YAS_CLASS_EVENT, YAS_MEDIA_FRAME)
        ?.datagram,
    ).toBe(YAS_DATAGRAM_MEDIA_FRAME);
    expect(
      yasOperationPolicy(YAS_FAMILY_CORE, YAS_CLASS_EVENT, 0)?.datagram,
    ).toBe(YAS_DATAGRAM_FORBIDDEN);
  });

  it("parses Net datagrams while leaving selected-flow validation to dispatch", () => {
    const validated = validateYasDatagramFrame(
      event(
        YAS_FAMILY_NET,
        YAS_NET_DATAGRAM,
        encodeNetDatagram({
          flowHandle: 9n,
          sequence: 3n,
          payload: Uint8Array.of(1, 2),
        }),
      ),
    );
    expect(validated).toMatchObject({
      predicate: YAS_DATAGRAM_NET_NATIVE_FLOW,
      value: { flowHandle: 9n, sequence: 3n },
    });
  });

  it("requires discardable, explicitly eligible Surface frames", () => {
    const base = {
      viewId: 1,
      sequence: 2n,
      baseSequence: 1n,
      captureNs: 3n,
      presentationNs: 4n,
      flags:
        YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE | YAS_SURFACE_FRAME_DISCARDABLE,
      codecVersion: YAS_SURFACE_CODEC_H264_V1,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: 1,
      payload: Uint8Array.of(1),
    };
    expect(
      validateYasDatagramFrame(
        event(YAS_FAMILY_SURFACE, YAS_SURFACE_FRAME, encodeSurfaceFrame(base)),
      ).predicate,
    ).toBe(YAS_DATAGRAM_SURFACE_FRAME);
    expect(() =>
      validateYasDatagramFrame(
        event(
          YAS_FAMILY_SURFACE,
          YAS_SURFACE_FRAME,
          encodeSurfaceFrame({
            ...base,
            flags: base.flags | YAS_SURFACE_FRAME_KEYFRAME,
          }),
        ),
      ),
    ).toThrow(/not datagram-safe/);
  });

  it("requires discardable Media frames without reliable-only flags", () => {
    const base = {
      streamHandle: 1n,
      sequence: 2n,
      captureTime: 3n,
      presentationTime: 4n,
      codecVersion: YAS_MEDIA_CODEC_H264,
      flags: YAS_MEDIA_FRAME_DISCARDABLE,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: 1,
      payload: Uint8Array.of(1),
    };
    expect(
      validateYasDatagramFrame(
        event(YAS_FAMILY_MEDIA, YAS_MEDIA_FRAME, encodeMediaFrame(base)),
      ).predicate,
    ).toBe(YAS_DATAGRAM_MEDIA_FRAME);
    expect(() =>
      validateYasDatagramFrame(
        event(
          YAS_FAMILY_MEDIA,
          YAS_MEDIA_FRAME,
          encodeMediaFrame({
            ...base,
            flags: base.flags | YAS_MEDIA_FRAME_KEYFRAME,
          }),
        ),
      ),
    ).toThrow(/not datagram-safe/);
  });

  it("rejects forbidden, correlated, and compressed frames", () => {
    expect(() =>
      validateYasDatagramFrame(event(YAS_FAMILY_CORE, 0, new Uint8Array(0))),
    ).toThrow(/forbidden/);
    expect(() =>
      validateYasDatagramFrame({
        ...event(
          YAS_FAMILY_NET,
          YAS_NET_DATAGRAM,
          encodeNetDatagram({
            flowHandle: 1n,
            sequence: 1n,
            payload: new Uint8Array(0),
          }),
        ),
        class: YAS_CLASS_REQUEST,
        requestId: 1,
      }),
    ).toThrow(/must contain Events/);
    expect(() =>
      validateYasDatagramFrame({
        ...event(
          YAS_FAMILY_NET,
          YAS_NET_DATAGRAM,
          encodeNetDatagram({
            flowHandle: 1n,
            sequence: 1n,
            payload: new Uint8Array(0),
          }),
        ),
        compressed: true,
      }),
    ).toThrow(/compressed/);
  });
});
