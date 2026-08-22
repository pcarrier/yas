import { describe, expect, it, vi } from "vitest";
import {
  YAS_EVENTS_EVENT_SERVER_ERROR,
  YAS_GOLDEN_VECTORS,
  YasEventsStream,
  YasProtocolError,
  decodeEventsDumpResult,
  decodeEventsRecordEvent,
  decodeEventsRecordingInfo,
  decodeEventsSetConfig,
  encodeEventsBatch,
  encodeEventsDumpResult,
  encodeEventsRecordEvent,
  encodeEventsRecordingInfo,
  encodeEventsSetConfig,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(
    hex.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
  );
}

describe("YAS Events v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    const cases = [
      [
        "events.set_config.payload",
        decodeEventsSetConfig,
        encodeEventsSetConfig,
      ],
      [
        "events.dump_result.payload",
        decodeEventsDumpResult,
        encodeEventsDumpResult,
      ],
      [
        "events.record.payload",
        decodeEventsRecordEvent,
        encodeEventsRecordEvent,
      ],
      [
        "events.recording_info.payload",
        decodeEventsRecordingInfo,
        encodeEventsRecordingInfo,
      ],
    ] as const;
    for (const [name, decode, encode] of cases) {
      const payload = bytes(name);
      const value = decode(payload as never);
      expect(encode(value as never)).toEqual(payload);
      for (let end = 0; end < payload.length; end++)
        expect(() => decode(payload.subarray(0, end) as never)).toThrow(
          YasProtocolError,
        );
    }
  });

  it("preserves event-specific flags and rejects unknown required IDs", () => {
    const event = decodeEventsRecordEvent(bytes("events.record.payload"));
    expect(event.batch.records[0]!.eventFlags).toBe(0x1234);
    expect(Array.from(event.batch.records[0]!.payload)).toEqual(
      Array.from(new TextEncoder().encode("yas")),
    );
    expect(() =>
      encodeEventsBatch({
        firstSequence: 1n,
        records: [
          {
            sequence: 1n,
            monotonicNs: 1n,
            eventId: YAS_EVENTS_EVENT_SERVER_ERROR + 1,
            required: true,
            eventFlags: 0,
            payload: new Uint8Array(0),
          },
        ],
      }),
    ).toThrow(/unknown required/);
    expect(() =>
      encodeEventsBatch({
        firstSequence: 1n,
        records: [
          {
            sequence: 1n,
            monotonicNs: 1n,
            eventId: YAS_EVENTS_EVENT_SERVER_ERROR + 1,
            required: false,
            eventFlags: 0xffff,
            payload: new Uint8Array([1]),
          },
        ],
      }),
    ).not.toThrow();
  });

  it("enforces an exact record-gap-record cursor", async () => {
    const client = {
      releaseStream: vi.fn(),
      stopStream: vi.fn(async () => undefined),
    };
    const stream = new YasEventsStream(client as never, {
      streamHandle: 7n,
      firstSequence: 10n,
      maxBatchBytes: 4096,
      extensions: [],
    });
    const record = (sequence: bigint) => ({
      streamHandle: 7n,
      batch: {
        firstSequence: sequence,
        records: [
          {
            sequence,
            monotonicNs: sequence,
            eventId: 1,
            required: true,
            eventFlags: 0,
            payload: new Uint8Array([Number(sequence)]),
          },
        ],
      },
    });
    stream.receiveRecords(record(10n));
    stream.receiveGap({
      streamHandle: 7n,
      lost: 2n,
      firstAvailableSequence: 13n,
    });
    stream.receiveRecords(record(13n));
    expect((await stream.next())!.type).toBe("records");
    expect(await stream.next()).toEqual({
      type: "gap",
      lost: 2n,
      firstAvailableSequence: 13n,
    });
    expect((await stream.next())!.type).toBe("records");
    expect(() => stream.receiveRecords(record(15n))).toThrow(/sequence/);
    stream.receiveStopped({
      streamHandle: 7n,
      status: 0,
      detail: "",
      extensions: [],
    });
    expect(await stream.next()).toEqual({
      type: "stopped",
      status: 0,
      detail: "",
    });
    expect(await stream.next()).toBeNull();
    expect(client.releaseStream).toHaveBeenCalledWith(7n, stream);
  });

  it("stops and fails an undrained stream at its item admission limit", async () => {
    const client = {
      releaseStream: vi.fn(),
      stopStream: vi.fn(async () => undefined),
    };
    const stream = new YasEventsStream(
      client as never,
      {
        streamHandle: 8n,
        firstSequence: 1n,
        maxBatchBytes: 4096,
        extensions: [],
      },
      { maxItems: 2, maxBytes: 4096 },
    );
    const record = (sequence: bigint) => ({
      streamHandle: 8n,
      batch: {
        firstSequence: sequence,
        records: [
          {
            sequence,
            monotonicNs: sequence,
            eventId: 1,
            required: true,
            eventFlags: 0,
            payload: new Uint8Array([Number(sequence)]),
          },
        ],
      },
    });
    stream.receiveRecords(record(1n));
    stream.receiveRecords(record(2n));
    stream.receiveRecords(record(3n));

    expect(client.stopStream).toHaveBeenCalledWith(8n);
    await expect(stream.next()).rejects.toThrow(/consumer queue limit/);
    // Pending RECORD events while STOP_STREAM is in flight are ignored rather
    // than converted into a synthetic GAP or escalated to a session failure.
    expect(() => stream.receiveRecords(record(4n))).not.toThrow();
    stream.receiveStopped({
      streamHandle: 8n,
      status: 0,
      detail: "",
      extensions: [],
    });
    expect(client.releaseStream).toHaveBeenCalledWith(8n, stream);
  });

  it("also fails an undrained stream at its retained-byte limit", async () => {
    const client = {
      releaseStream: vi.fn(),
      stopStream: vi.fn(async () => undefined),
    };
    const stream = new YasEventsStream(
      client as never,
      {
        streamHandle: 9n,
        firstSequence: 1n,
        maxBatchBytes: 64,
        extensions: [],
      },
      { maxItems: 10, maxBytes: 50 },
    );
    const record = (sequence: bigint) => ({
      streamHandle: 9n,
      batch: {
        firstSequence: sequence,
        records: [
          {
            sequence,
            monotonicNs: sequence,
            eventId: 1,
            required: true,
            eventFlags: 0,
            payload: new Uint8Array([1]),
          },
        ],
      },
    });
    stream.receiveRecords(record(1n));
    stream.receiveRecords(record(2n));

    expect(client.stopStream).toHaveBeenCalledWith(9n);
    await expect(stream.next()).rejects.toThrow(/consumer queue limit/);
  });
});
