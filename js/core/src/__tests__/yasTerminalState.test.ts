import { describe, expect, it } from "vitest";
import {
  YAS_TERMINAL_EXIT_KIND_CODE,
  YAS_TERMINAL_EXIT_REASON_UNKNOWN,
  YAS_TERMINAL_LIFECYCLE_EXITED,
  YAS_TERMINAL_LIFECYCLE_RUNNING,
  YAS_TERMINAL_STATE_EXIT_EXTENSION,
  YasWriter,
  decodeTerminalRecord,
  encodeExtensions,
} from "../yas";

const UNKNOWN_STATE_EXTENSION = 0xfffe;

function exitRecord(trailing: readonly number[] = []): Uint8Array {
  return new YasWriter()
    .u8(YAS_TERMINAL_EXIT_KIND_CODE)
    .u8(YAS_TERMINAL_EXIT_REASON_UNKNOWN)
    .u16(0)
    .i32(23)
    .utf8U32("done")
    .bytes(new Uint8Array(trailing))
    .finish();
}

function terminalRecord(
  extensions: readonly {
    tag: number;
    required?: boolean;
    value: Uint8Array;
  }[],
  lifecycle = YAS_TERMINAL_LIFECYCLE_RUNNING,
): Uint8Array {
  return new YasWriter()
    .u64(1n)
    .u8(lifecycle)
    .u8(0)
    .u16(24)
    .u16(80)
    .u32(1)
    .u32(24)
    .bytes(encodeExtensions(extensions))
    .finish();
}

describe("Terminal state records", () => {
  it("consumes and decodes the exit extension", () => {
    const record = terminalRecord(
      [
        {
          tag: YAS_TERMINAL_STATE_EXIT_EXTENSION,
          value: exitRecord(),
        },
      ],
      YAS_TERMINAL_LIFECYCLE_EXITED,
    );

    expect(decodeTerminalRecord(record).exit).toEqual({
      kind: YAS_TERMINAL_EXIT_KIND_CODE,
      code: 23,
      detail: "done",
    });
  });

  it("skips an unknown optional state extension with a nonempty body", () => {
    const value = new Uint8Array([0xde, 0xad, 0xbe, 0xef]);
    const decoded = decodeTerminalRecord(
      terminalRecord([{ tag: UNKNOWN_STATE_EXTENSION, value }]),
    );

    expect(decoded.exit).toBeUndefined();
    expect(decoded.extensions).toEqual([
      { tag: UNKNOWN_STATE_EXTENSION, required: false, value },
    ]);
  });

  it("rejects an unknown required state extension", () => {
    expect(() =>
      decodeTerminalRecord(
        terminalRecord([
          {
            tag: UNKNOWN_STATE_EXTENSION,
            required: true,
            value: new Uint8Array([1]),
          },
        ]),
      ),
    ).toThrow(/unknown required extension/);
  });

  it("rejects trailing bytes in the known exit extension", () => {
    expect(() =>
      decodeTerminalRecord(
        terminalRecord(
          [
            {
              tag: YAS_TERMINAL_STATE_EXIT_EXTENSION,
              value: exitRecord([0xff]),
            },
          ],
          YAS_TERMINAL_LIFECYCLE_EXITED,
        ),
      ),
    ).toThrow(/unconsumed bytes in Terminal exit record/);
  });
});
