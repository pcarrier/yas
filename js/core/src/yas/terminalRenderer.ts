import { lz4Compress } from "../lz4";
import type { YasTerminalGridState } from "./terminal";
import { YasWriter } from "./wire";

// Browser renderer ingest codec. This is an internal WASM boundary, not a YAS
// transport frame: native handles and family frames never enter this byte
// representation.
const RENDER_OP_FILL_RECT = 0x02;
const RENDER_OP_PATCH_CELLS = 0x03;
const RENDER_TITLE_PRESENT = 1 << 15;
const RENDER_OPS_PRESENT = 1 << 14;
const RENDER_STRINGS_PRESENT = 1 << 13;
const RENDER_LINE_FLAGS_PRESENT = 1 << 12;
const RENDER_TITLE_LENGTH_MASK = 0x0fff;
const RENDER_CELL_BYTES = 12;

const encoder = new TextEncoder();

/** Encode one decoded native YAS grid for the existing browser WASM renderer.
 * The returned bytes contain no transport opcode or resource identifier. */
export function encodeBrowserTerminalGrid(
  grid: YasTerminalGridState,
): Uint8Array {
  const title = truncateUtf8(
    encoder.encode(grid.title),
    RENDER_TITLE_LENGTH_MASK,
  );
  const overflow = [...grid.overflowStrings.entries()].sort(
    (left, right) => left[0] - right[0],
  );
  const titleFlags =
    RENDER_TITLE_PRESENT |
    RENDER_OPS_PRESENT |
    RENDER_LINE_FLAGS_PRESENT |
    (overflow.length === 0 ? 0 : RENDER_STRINGS_PRESENT) |
    title.length;
  const totalCells = grid.rows * grid.cols;
  const bitmask = new Uint8Array(Math.ceil(totalCells / 8)).fill(0xff);
  if (totalCells % 8 !== 0)
    bitmask[bitmask.length - 1] = (1 << (totalCells % 8)) - 1;
  const writer = new YasWriter()
    .u16(grid.rows)
    .u16(grid.cols)
    .u16(grid.cursorRow)
    .u16(grid.cursorCol)
    .u16(grid.modes)
    .u16(titleFlags)
    .bytes(title)
    .u16(2)
    .u8(RENDER_OP_FILL_RECT)
    .u16(0)
    .u16(0)
    .u16(grid.rows)
    .u16(grid.cols)
    .bytes(new Uint8Array(RENDER_CELL_BYTES))
    .u8(RENDER_OP_PATCH_CELLS)
    .bytes(bitmask);
  // The renderer takes the cells plane-major where the native grid is
  // cell-major. Transposing a byte at a time through the writer allocated a
  // one-byte array and a DataView per byte -- twelve per cell, so around
  // 170,000 allocations for one full-screen frame, and as many one-byte chunks
  // for `finish` to concatenate. That was where a busy terminal spent most of
  // its main thread. One buffer, one append.
  const planes = new Uint8Array(totalCells * RENDER_CELL_BYTES);
  const cells = grid.cells;
  for (let plane = 0; plane < RENDER_CELL_BYTES; plane++) {
    const base = plane * totalCells;
    for (
      let cell = 0, source = plane;
      cell < totalCells;
      cell++, source += RENDER_CELL_BYTES
    )
      planes[base + cell] = cells[source]!;
  }
  writer.bytes(planes);
  if (overflow.length !== 0) {
    writer.u16(overflow.length);
    for (const [index, value] of overflow) {
      const bytes = encoder.encode(value).subarray(0, 0xffff);
      writer.u32(index).u16(bytes.length).bytes(bytes);
    }
  }
  writer.bytes(grid.lineFlags).u32(grid.scrollbackLines);
  const linkIds = new Map<number, number>();
  for (const id of grid.hyperlinkUris.keys()) {
    if (linkIds.size === 0xffff) break;
    linkIds.set(id, linkIds.size + 1);
  }
  writer.u16(linkIds.size);
  for (const [nativeId, rendererId] of linkIds) {
    const uri = truncateUtf8(
      encoder.encode(grid.hyperlinkUris.get(nativeId)!),
      0xffff,
    );
    writer.u16(rendererId).u16(uri.length).bytes(uri);
  }
  const runs = grid.hyperlinkRuns.filter(
    (run) => linkIds.has(run.linkId) && run.cellCount <= 0xffff,
  );
  writer.u16(Math.min(runs.length, 0xffff));
  for (const run of runs.slice(0, 0xffff))
    writer.u32(run.startCell).u16(run.cellCount).u16(linkIds.get(run.linkId)!);
  return lz4Compress(writer.finish());
}

function truncateUtf8(bytes: Uint8Array, maximum: number): Uint8Array {
  if (bytes.length <= maximum) return bytes;
  let end = maximum;
  while (end > 0 && (bytes[end]! & 0xc0) === 0x80) end--;
  return bytes.subarray(0, end);
}
