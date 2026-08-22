import { describe, expect, it } from "vitest";
import {
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE,
  YAS_SURFACE_FRAME_DISCARDABLE,
  YasProtocolError,
  YasReceiveBudget,
  YasSurfaceView,
  type YasSurfaceFrame,
} from "../yas";

function surfaceFrame(
  sequence: bigint,
  fragmentIndex: number,
  fragmentCount: number,
  byte: number,
): YasSurfaceFrame {
  return {
    viewId: 1,
    sequence,
    baseSequence: 0n,
    captureNs: sequence,
    presentationNs: sequence,
    flags: YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE | YAS_SURFACE_FRAME_DISCARDABLE,
    codecVersion: YAS_SURFACE_CODEC_H264_V1,
    fragmentIndex,
    fragmentCount,
    completeLength: fragmentCount,
    payload: Uint8Array.of(byte),
  };
}

function surfaceView(maxInflightFrames = 4, retainedBytes = 32n) {
  const budget = new YasReceiveBudget(retainedBytes);
  return new YasSurfaceView(
    {} as never,
    {
      viewId: 1,
      codecVersion: YAS_SURFACE_CODEC_H264_V1,
      maxInflightFrames,
      maxEncodedFrame: 32,
      maxDecodedFrame: 32,
      firstSequence: 1n,
      extensions: [],
    },
    budget.reserve(retainedBytes),
  );
}

describe("YAS Surface unreliable frame assembly", () => {
  it("allows synchronous feedback from a frame listener", () => {
    const view = surfaceView();
    view.subscribe((frame) =>
      view.recordFeedback({
        presentedSequence: frame.sequence,
        decoderQueueDepth: 0,
        availableSlots: 4,
      }),
    );

    expect(() => view.accept(surfaceFrame(1n, 0, 1, 0x10))).not.toThrow();
  });

  it("drops gaps and duplicates without stranding an assembly", () => {
    const view = surfaceView();
    const received: YasSurfaceFrame[] = [];
    view.subscribe((frame) => received.push(frame));

    view.accept(surfaceFrame(1n, 0, 3, 0x10), true);
    view.accept(surfaceFrame(1n, 2, 3, 0x12), true);

    view.accept(surfaceFrame(2n, 0, 2, 0x20), true);
    view.accept(surfaceFrame(2n, 0, 2, 0x20), true);
    view.accept(surfaceFrame(2n, 1, 2, 0x21), true);

    expect(received).toHaveLength(1);
    expect(received[0]!.sequence).toBe(2n);
    expect([...received[0]!.payload]).toEqual([0x20, 0x21]);

    view.accept(surfaceFrame(1n, 0, 1, 0x30), true);
    expect(received).toHaveLength(1);
  });

  it("lets a newer reliable frame reclaim lossy retained bytes", () => {
    const view = surfaceView(2, 2n);
    const received: YasSurfaceFrame[] = [];
    view.subscribe((frame) => received.push(frame));

    view.accept(surfaceFrame(1n, 0, 2, 0x10), true);
    view.accept(surfaceFrame(2n, 0, 2, 0x20));
    view.accept(surfaceFrame(2n, 1, 2, 0x21));

    expect(received.map((frame) => frame.sequence)).toEqual([2n]);
  });

  it("keeps reliable fragmentation strict", () => {
    const view = surfaceView();
    expect(() => view.accept(surfaceFrame(1n, 1, 2, 0x11))).toThrow(
      YasProtocolError,
    );

    view.accept(surfaceFrame(1n, 0, 3, 0x10));
    expect(() => view.accept(surfaceFrame(1n, 2, 3, 0x12))).toThrow(
      /inconsistent Surface frame fragments/,
    );
  });
});
