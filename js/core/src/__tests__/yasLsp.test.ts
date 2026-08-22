import { describe, expect, it } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YasProtocolError,
  decodeLspBufferBegin,
  decodeLspBufferBeginResult,
  decodeLspBufferClose,
  decodeLspBufferCommit,
  decodeLspBufferPut,
  decodeLspClose,
  decodeLspClosed,
  decodeLspDiagnosticRecord,
  decodeLspListServers,
  decodeLspLocationRecord,
  decodeLspHoverRecord,
  decodeLspSymbolRecord,
  decodeLspEditRecord,
  decodeLspSignatureRecord,
  decodeLspOpen,
  decodeLspOpenResult,
  decodeLspWorkspaceSource,
  decodeLspQuery,
  decodeLspQueryBody,
  decodeLspQueryPage,
  decodeLspRemovedEntity,
  decodeLspServerList,
  decodeLspServerRecord,
  decodeLspStopServer,
  decodeLspUnwatch,
  decodeLspWatch,
  encodeLspBufferBegin,
  encodeLspBufferBeginResult,
  encodeLspBufferClose,
  encodeLspBufferCommit,
  encodeLspBufferPut,
  encodeLspClose,
  encodeLspClosed,
  encodeLspDiagnosticRecord,
  encodeLspListServers,
  encodeLspLocationRecord,
  encodeLspHoverRecord,
  encodeLspSymbolRecord,
  encodeLspEditRecord,
  encodeLspSignatureRecord,
  encodeLspOpen,
  encodeLspOpenResult,
  encodeLspWorkspaceSource,
  encodeLspQuery,
  encodeLspQueryBody,
  encodeLspQueryPage,
  encodeLspRemovedEntity,
  encodeLspServerList,
  encodeLspServerRecord,
  encodeLspStopServer,
  encodeLspUnwatch,
  encodeLspWatch,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(hex.match(/../g)!, (byte) =>
    Number.parseInt(byte, 16),
  );
}

const cases: readonly [string, (payload: Uint8Array) => Uint8Array][] = [
  ["lsp.open.payload", (payload) => encodeLspOpen(decodeLspOpen(payload))],
  ["lsp.open_auto.payload", (payload) => encodeLspOpen(decodeLspOpen(payload))],
  [
    "lsp.open_result.payload",
    (payload) => encodeLspOpenResult(decodeLspOpenResult(payload)),
  ],
  [
    "lsp.open_result_no_backend.payload",
    (payload) => encodeLspOpenResult(decodeLspOpenResult(payload)),
  ],
  [
    "lsp.workspace_source.platform.payload",
    (payload) => encodeLspWorkspaceSource(decodeLspWorkspaceSource(payload)),
  ],
  [
    "lsp.closed.payload",
    (payload) => encodeLspClosed(decodeLspClosed(payload)),
  ],
  [
    "lsp.close.payload",
    (payload) => {
      const value = decodeLspClose(payload);
      return encodeLspClose(value.workspaceHandle, value.extensions);
    },
  ],
  [
    "lsp.watch.payload",
    (payload) => {
      const value = decodeLspWatch(payload);
      return encodeLspWatch(
        value.workspaceHandle,
        value.datasets,
        value.encodedStateWatch,
      );
    },
  ],
  [
    "lsp.unwatch.payload",
    (payload) => encodeLspUnwatch(decodeLspUnwatch(payload)),
  ],
  ["lsp.query.payload", (payload) => encodeLspQuery(decodeLspQuery(payload))],
  [
    "lsp.signature_query.payload",
    (payload) => encodeLspQueryBody(decodeLspQueryBody(payload)),
  ],
  [
    "lsp.buffer_put.payload",
    (payload) => encodeLspBufferPut(decodeLspBufferPut(payload)),
  ],
  [
    "lsp.buffer_begin.payload",
    (payload) => encodeLspBufferBegin(decodeLspBufferBegin(payload)),
  ],
  [
    "lsp.buffer_commit.payload",
    (payload) => encodeLspBufferCommit(decodeLspBufferCommit(payload)),
  ],
  [
    "lsp.buffer_close.payload",
    (payload) => encodeLspBufferClose(decodeLspBufferClose(payload)),
  ],
  [
    "lsp.list_servers.payload",
    (payload) => {
      const value = decodeLspListServers(payload);
      return encodeLspListServers(value.workspaceHandle, value.extensions);
    },
  ],
  [
    "lsp.stop_server.payload",
    (payload) => encodeLspStopServer(decodeLspStopServer(payload)),
  ],
  [
    "lsp.buffer_begin_result.payload",
    (payload) =>
      encodeLspBufferBeginResult(decodeLspBufferBeginResult(payload)),
  ],
  [
    "lsp.query_page.payload",
    (payload) => encodeLspQueryPage(decodeLspQueryPage(payload)),
  ],
  [
    "lsp.query_page_incomplete.payload",
    (payload) => encodeLspQueryPage(decodeLspQueryPage(payload)),
  ],
  [
    "lsp.location.payload",
    (payload) => encodeLspLocationRecord(decodeLspLocationRecord(payload)),
  ],
  [
    "lsp.hover.payload",
    (payload) => encodeLspHoverRecord(decodeLspHoverRecord(payload)),
  ],
  [
    "lsp.symbol.payload",
    (payload) => encodeLspSymbolRecord(decodeLspSymbolRecord(payload)),
  ],
  [
    "lsp.edit.payload",
    (payload) => encodeLspEditRecord(decodeLspEditRecord(payload)),
  ],
  [
    "lsp.signature.payload",
    (payload) => encodeLspSignatureRecord(decodeLspSignatureRecord(payload)),
  ],
  [
    "lsp.server.payload",
    (payload) => encodeLspServerRecord(decodeLspServerRecord(payload)),
  ],
  [
    "lsp.diagnostics.payload",
    (payload) => encodeLspDiagnosticRecord(decodeLspDiagnosticRecord(payload)),
  ],
  [
    "lsp.remove.payload",
    (payload) => encodeLspRemovedEntity(decodeLspRemovedEntity(payload)),
  ],
];

describe("YAS LSP v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    for (const [name, roundTrip] of cases) {
      const payload = bytes(name);
      expect(roundTrip(payload), name).toEqual(payload);
      for (let end = 0; end < payload.length; end++)
        expect(
          () => roundTrip(payload.subarray(0, end)),
          `${name}@${end}`,
        ).toThrow(YasProtocolError);
    }
  });

  it("round-trips server lists without losing typed server state", () => {
    const server = decodeLspServerRecord(bytes("lsp.server.payload"));
    expect(decodeLspServerList(encodeLspServerList([server]))).toEqual([
      server,
    ]);

    const detached = { ...server, workspaceHandle: 0n };
    expect(decodeLspServerList(encodeLspServerList([detached]))).toEqual([
      detached,
    ]);
    expect(() => encodeLspServerRecord(detached)).toThrow(YasProtocolError);
  });
});
