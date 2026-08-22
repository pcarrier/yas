import { describe, expect, it } from "vitest";
import {
  YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
  YAS_CLIENT_ORIGIN_EXTENSION,
  YAS_FAMILY_FS,
  YasProtocolError,
  YasWriter,
  decodeClientActiveSubscriptions,
  type YasClientRecord,
} from "../yas";

function activeSubscriptionsValue(): Uint8Array {
  return new YasWriter()
    .u16(1)
    .u16(1)
    .u16(1)
    .u16(0)
    .u64(10n)
    .u32(1)
    .u16(24)
    .u16(80)
    .u64(20n)
    .u32(2)
    .u32(1280)
    .u32(720)
    .u16(120)
    .u16(0)
    .u16(YAS_FAMILY_FS)
    .u16(0)
    .u32(9)
    .u64(30n)
    .finish();
}

function clientRecord(): YasClientRecord {
  return {
    sessionId: new Uint8Array([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
    clientInstance: new Uint8Array([
      2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
    ]),
    connectedServerNs: 10_000_000_000n,
    idleNs: 1n,
    bytesReceived: 100n,
    bytesSent: 200n,
    name: "browser",
    release: "test",
    label: "tab",
    origin: {
      kind: YAS_CLIENT_ORIGIN_EXTENSION,
      extensionId: 41n,
      definitionRevision: 42n,
      attempt: 43n,
      taskId: 44,
      name: "worker",
    },
    extensions: [],
    activeSubscriptions: {
      terminals: [{ terminalHandle: 10n, viewId: 1, rows: 24, columns: 80 }],
      surfaces: [
        {
          surfaceHandle: 20n,
          viewId: 2,
          width: 1280,
          height: 720,
          scale120: 120,
        },
      ],
      auxiliary: [
        { family: YAS_FAMILY_FS, subscriptionId: 9, resourceHandle: 30n },
      ],
    },
    bandwidthRates: null,
  };
}

describe("YAS Client family", () => {
  it("decodes and validates the typed active-subscriptions extension", () => {
    const value = activeSubscriptionsValue();
    expect(
      decodeClientActiveSubscriptions([
        {
          tag: YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
          required: false,
          value,
        },
      ]),
    ).toEqual(clientRecord().activeSubscriptions);
    for (let end = 0; end < value.length; end++) {
      expect(() =>
        decodeClientActiveSubscriptions([
          {
            tag: YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
            required: false,
            value: value.subarray(0, end),
          },
        ]),
      ).toThrow(YasProtocolError);
    }
  });
});
