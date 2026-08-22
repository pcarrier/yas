import { describe, expect, it, vi } from "vitest";
import {
  YAS_FAMILY_TERMINAL,
  YAS_GOLDEN_VECTORS,
  YAS_TERMINAL_CONTENT_TEXT,
  YAS_TERMINAL_QUERY_CONTENT_KIND,
  YAS_TERMINAL_QUERY_ENCODING_UTF8,
  YAS_TERMINAL_QUERY_TRANSFER,
  YAS_TERMINAL_VERSION,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YasProtocolError,
  decodeTerminalCopyRange,
  decodeTerminalCwdQuery,
  decodeTerminalJournal,
  decodeTerminalJournalResult,
  decodeTerminalOutput,
  decodeTerminalOutputResult,
  decodeTerminalQueryBody,
  decodeTerminalQueryResult,
  decodeTerminalRead,
  decodeTerminalSearch,
  decodeTerminalSearchResults,
  decodeTerminalStyledLines,
  decodeTerminalTextAndStyled,
  decodeTerminalWait,
  encodeTerminalCopyRange,
  encodeTerminalCwdQuery,
  encodeTerminalJournal,
  encodeTerminalJournalResult,
  encodeTerminalOutput,
  encodeTerminalOutputResult,
  encodeTerminalQueryBody,
  encodeTerminalRead,
  encodeTerminalSearch,
  encodeTerminalSearchResults,
  encodeTerminalStyledLines,
  encodeTerminalTextAndStyled,
  encodeTerminalWait,
} from "../yas";

function vector(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  )?.hex;
  if (!hex) throw new Error(`missing generated vector ${name}`);
  return Uint8Array.from(hex.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

const cases: readonly [
  string,
  (bytes: Uint8Array) => unknown,
  (value: never) => Uint8Array,
][] = [
  [
    "terminal.query_inline.payload",
    decodeTerminalQueryBody,
    encodeTerminalQueryBody,
  ],
  ["terminal.read.payload", decodeTerminalRead, encodeTerminalRead],
  ["terminal.search.payload", decodeTerminalSearch, encodeTerminalSearch],
  ["terminal.cwd.payload", decodeTerminalCwdQuery, encodeTerminalCwdQuery],
  ["terminal.journal.payload", decodeTerminalJournal, encodeTerminalJournal],
  ["terminal.output.payload", decodeTerminalOutput, encodeTerminalOutput],
  ["terminal.wait.payload", decodeTerminalWait, encodeTerminalWait],
  [
    "terminal.copy_range.payload",
    decodeTerminalCopyRange,
    encodeTerminalCopyRange,
  ],
  [
    "terminal.search_results.payload",
    decodeTerminalSearchResults,
    encodeTerminalSearchResults,
  ],
  [
    "terminal.journal_result.payload",
    decodeTerminalJournalResult,
    encodeTerminalJournalResult,
  ],
  [
    "terminal.output_result.payload",
    decodeTerminalOutputResult,
    encodeTerminalOutputResult,
  ],
  [
    "terminal.styled_lines.payload",
    decodeTerminalStyledLines,
    encodeTerminalStyledLines,
  ],
  [
    "terminal.text_and_styled.payload",
    decodeTerminalTextAndStyled,
    encodeTerminalTextAndStyled,
  ],
];

describe("YAS Terminal typed query codecs", () => {
  for (const [name, decode, encode] of cases) {
    it(`${name} matches Rust and rejects every truncation`, () => {
      const bytes = vector(name);
      const value = decode(bytes);
      expect(encode(value as never)).toEqual(bytes);
      for (let end = 0; end < bytes.length; end++)
        expect(() => decode(bytes.subarray(0, end))).toThrow(YasProtocolError);
    });
  }

  it("releases a pre-reserved lease when the server chooses inline", async () => {
    const release = vi.fn();
    const result = decodeTerminalQueryResult(
      vector("terminal.query_inline.payload"),
      undefined,
      { bytes: 1024n, release } as never,
    );
    expect(release).toHaveBeenCalledOnce();
    expect(Array.from(await result.bytes())).toEqual(
      Array.from(new TextEncoder().encode("hello")),
    );
  });

  it("accepts Transfer delivery only against the credit reserved before request", async () => {
    const descriptor = {
      transferId: 2,
      mode: YAS_TRANSFER_MODE_BYTE,
      direction: YAS_TRANSFER_SENDER_TO_RECEIVER,
      flags: 0,
      receiverSendCredit: 0n,
      senderSendCredit: 4n,
      maxItemBytes: 0n,
      maxChunkBytes: 4,
      contentFamily: YAS_FAMILY_TERMINAL,
      contentKind: YAS_TERMINAL_QUERY_CONTENT_KIND,
      contentVersion: YAS_TERMINAL_VERSION,
      extensions: [
        {
          tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
          required: true,
          value: new Uint8Array(),
        },
      ],
      maxOpenMessages: 1,
      sensitiveContent: true,
    };
    const body = encodeTerminalQueryBody({
      representation: YAS_TERMINAL_QUERY_TRANSFER,
      contentKind: YAS_TERMINAL_CONTENT_TEXT,
      encoding: YAS_TERMINAL_QUERY_ENCODING_UTF8,
      flags: 0,
      delivery: { kind: "transfer", descriptor },
      extensions: [],
    });
    const collect = vi.fn(async () => new TextEncoder().encode("text"));
    const acceptServerDescriptor = vi.fn(() => ({ collect }));
    const manager = { acceptServerDescriptor } as never;
    expect(() => decodeTerminalQueryResult(body, manager)).toThrow(/lease/);
    const lease = { bytes: 4n, release: vi.fn() };
    const result = decodeTerminalQueryResult(body, manager, lease as never);
    expect(acceptServerDescriptor).toHaveBeenCalledWith(descriptor, lease);
    expect(Array.from(await result.bytes())).toEqual(
      Array.from(new TextEncoder().encode("text")),
    );
  });
});
