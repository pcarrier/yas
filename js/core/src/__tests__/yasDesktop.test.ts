import { describe, expect, it, vi } from "vitest";
import {
  YAS_DESKTOP_NOTIFICATION_CLOSED_DISMISSED,
  YAS_DESKTOP_LIMIT_MAX_NOTIFICATIONS,
  YAS_DESKTOP_LIMIT_MAX_TRAY_ITEMS,
  YAS_DESKTOP_MAX_NOTIFICATIONS,
  YAS_DESKTOP_MAX_TRAY_ITEMS,
  YAS_DESKTOP_RECORD_NOTIFICATION,
  YAS_GOLDEN_VECTORS,
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YasDesktopCatalog,
  YasWriter,
  decodeDesktopNotificationPatch,
  decodeDesktopNotificationRecord,
  decodeDesktopNotificationRemoval,
  encodeDesktopNotificationPatch,
  encodeDesktopNotificationRecord,
  encodeDesktopNotificationRemoval,
  type YasConnection,
  type YasDesktopNotificationRemoval,
  type YasStateBatch,
} from "../yas";

function vector(name: string): Uint8Array {
  const value = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  );
  if (!value) throw new Error(`missing vector ${name}`);
  return Uint8Array.from(value.hex.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

function rejectsEveryTruncation(
  bytes: Uint8Array,
  decode: (bytes: Uint8Array) => unknown,
): void {
  for (let end = 0; end < bytes.length; end++)
    expect(() => decode(bytes.subarray(0, end)), `prefix ${end}`).toThrow();
}

function batch(
  phase: number,
  revision: bigint,
  kind?: number,
  body?: Uint8Array,
): YasStateBatch {
  return {
    phase,
    flags: 0,
    fromRevision: revision > 0n ? revision - 1n : 0n,
    toRevision: revision,
    records:
      kind === undefined || body === undefined
        ? []
        : [{ kind, flags: 0, body }],
  };
}

describe("YAS Desktop notification metadata", () => {
  it("round-trips and truncation-gates every canonical metadata vector", () => {
    const cases = [
      [
        "desktop.notification_record.payload",
        decodeDesktopNotificationRecord,
        encodeDesktopNotificationRecord,
      ],
      [
        "desktop.notification_patch.payload",
        decodeDesktopNotificationPatch,
        encodeDesktopNotificationPatch,
      ],
      [
        "desktop.notification_remove.payload",
        decodeDesktopNotificationRemoval,
        encodeDesktopNotificationRemoval,
      ],
    ] as const;
    for (const [name, decode, encode] of cases) {
      const bytes = vector(name);
      const decoded = decode(bytes as never);
      expect(encode(decoded as never)).toEqual(bytes);
      rejectsEveryTruncation(bytes, decode as (bytes: Uint8Array) => unknown);
    }
  });

  it("applies typed clear/set patches and publishes exact close reasons", () => {
    const connection = {
      onInvalidation: vi.fn(),
      family: vi.fn(() => ({
        limits: [
          {
            tag: YAS_DESKTOP_LIMIT_MAX_TRAY_ITEMS,
            value: new YasWriter().u32(YAS_DESKTOP_MAX_TRAY_ITEMS).finish(),
          },
          {
            tag: YAS_DESKTOP_LIMIT_MAX_NOTIFICATIONS,
            value: new YasWriter().u32(YAS_DESKTOP_MAX_NOTIFICATIONS).finish(),
          },
        ],
      })),
    } as unknown as YasConnection;
    const catalog = new YasDesktopCatalog(connection);
    const apply = (
      catalog as unknown as { apply(batch: YasStateBatch): void }
    ).apply.bind(catalog);
    const removals: YasDesktopNotificationRemoval[] = [];
    catalog.onNotificationRemoved((removal) => removals.push(removal));

    const complete = new YasWriter()
      .u16(YAS_DESKTOP_RECORD_NOTIFICATION)
      .u16(0)
      .bytes(vector("desktop.notification_record.payload"))
      .finish();
    apply(batch(YAS_STATE_SNAPSHOT_BEGIN, 0n));
    apply(batch(YAS_STATE_SNAPSHOT_END, 1n, YAS_STATE_ADD, complete));
    expect(catalog.snapshot.notifications[0]).toMatchObject({
      contentImageHash: new Uint8Array(32).fill(1),
      applicationIconHash: new Uint8Array(32).fill(2),
      progress: { value: 7, maximum: 10 },
      replyPlaceholder: "Reply",
    });

    apply(
      batch(
        YAS_STATE_DELTA,
        2n,
        YAS_STATE_PATCH,
        vector("desktop.notification_patch.payload"),
      ),
    );
    expect(catalog.snapshot.notifications[0]).toMatchObject({
      revision: 7n,
      contentImageHash: null,
      applicationIconHash: new Uint8Array(32).fill(3),
      progress: { value: 9, maximum: 10 },
      replyPlaceholder: null,
    });

    apply(
      batch(
        YAS_STATE_DELTA,
        3n,
        YAS_STATE_REMOVE,
        encodeDesktopNotificationRemoval({
          notificationHandle: 5n,
          revision: 8n,
          closeReason: YAS_DESKTOP_NOTIFICATION_CLOSED_DISMISSED,
        }),
      ),
    );
    expect(catalog.snapshot.notifications).toEqual([]);
    expect(removals).toEqual([
      {
        notificationHandle: 5n,
        revision: 8n,
        closeReason: YAS_DESKTOP_NOTIFICATION_CLOSED_DISMISSED,
      },
    ]);
  });
});
