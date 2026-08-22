import { lz4Decompress } from "../lz4";
import {
  YAS_TERMINAL_FRAME_CODEC_COMPRESSED,
  YAS_TERMINAL_FRAME_COMPONENTS,
  YAS_TERMINAL_FRAME_CURSOR,
  YAS_TERMINAL_FRAME_DIMENSIONS,
  YAS_TERMINAL_FRAME_EXPLICIT_BASE,
  YAS_TERMINAL_FRAME_FINAL_STATE,
  YAS_TERMINAL_FRAME_KEYFRAME,
  YAS_TERMINAL_FRAME_MODES,
  YAS_TERMINAL_FRAME_SCROLLBACK,
  YAS_TERMINAL_FRAME_TITLE,
  YAS_TERMINAL_FRAME_VIEW_OFFSET,
  YAS_TERMINAL_GRID_CODEC_V1,
  YAS_TERMINAL_CELL_BYTES,
} from "./generated";
import type { YasTerminalFrameEvent } from "./terminal";
import { YasCursor, YasProtocolError } from "./wire";

const FRAME_KNOWN_FLAGS =
  YAS_TERMINAL_FRAME_KEYFRAME |
  YAS_TERMINAL_FRAME_FINAL_STATE |
  YAS_TERMINAL_FRAME_DIMENSIONS |
  YAS_TERMINAL_FRAME_CURSOR |
  YAS_TERMINAL_FRAME_MODES |
  YAS_TERMINAL_FRAME_SCROLLBACK |
  YAS_TERMINAL_FRAME_VIEW_OFFSET |
  YAS_TERMINAL_FRAME_TITLE |
  YAS_TERMINAL_FRAME_COMPONENTS |
  YAS_TERMINAL_FRAME_CODEC_COMPRESSED |
  YAS_TERMINAL_FRAME_EXPLICIT_BASE;
const KEYFRAME_REQUIRED_FLAGS =
  YAS_TERMINAL_FRAME_DIMENSIONS |
  YAS_TERMINAL_FRAME_CURSOR |
  YAS_TERMINAL_FRAME_MODES |
  YAS_TERMINAL_FRAME_SCROLLBACK |
  YAS_TERMINAL_FRAME_VIEW_OFFSET |
  YAS_TERMINAL_FRAME_TITLE;
const CONTENT_OVERFLOW = 7;
const CELL_LINK = 1 << 6;

export interface YasTerminalHyperlinkRun {
  startCell: number;
  cellCount: number;
  linkId: number;
}

export interface YasTerminalGridState {
  sequence: number;
  rows: number;
  cols: number;
  cursorRow: number;
  cursorCol: number;
  modes: number;
  scrollbackLines: number;
  scrollOffset: bigint;
  title: string;
  cells: Uint8Array;
  lineFlags: Uint8Array;
  overflowStrings: ReadonlyMap<number, string>;
  hyperlinkUris: ReadonlyMap<number, string>;
  hyperlinkRuns: readonly YasTerminalHyperlinkRun[];
}

/** Decode and apply one normative `yas.terminal.grid/1` logical frame. */
export function decodeTerminalGridV1(
  frame: YasTerminalFrameEvent,
  base: YasTerminalGridState | null,
  maxDecodedFrame: number,
): YasTerminalGridState {
  if (frame.flags & ~FRAME_KNOWN_FLAGS)
    throw new YasProtocolError("reserved Terminal frame flags are nonzero");
  const keyframe = (frame.flags & YAS_TERMINAL_FRAME_KEYFRAME) !== 0;
  if (keyframe) {
    if (frame.explicitBase !== undefined)
      throw new YasProtocolError("Terminal keyframe has an explicit base");
    if ((frame.flags & KEYFRAME_REQUIRED_FLAGS) !== KEYFRAME_REQUIRED_FLAGS)
      throw new YasProtocolError("Terminal keyframe omits required state");
  } else {
    if (!base) throw new YasProtocolError("Terminal delta has no decoded base");
    const expected = frame.explicitBase ?? (frame.sequence - 1) >>> 0;
    if (base.sequence !== expected)
      throw new YasProtocolError("Terminal delta base is unavailable");
    if (frame.flags & YAS_TERMINAL_FRAME_DIMENSIONS)
      throw new YasProtocolError("Terminal delta changes dimensions");
  }

  let decoded = frame.gridPayload;
  if (frame.flags & YAS_TERMINAL_FRAME_CODEC_COMPRESSED) {
    if (decoded.length < 4)
      throw new YasProtocolError("truncated compressed Terminal grid");
    const declared = new DataView(
      decoded.buffer,
      decoded.byteOffset,
      4,
    ).getUint32(0, true);
    if (declared > maxDecodedFrame)
      throw new YasProtocolError("Terminal decoded grid exceeds view limit");
    const inflated = lz4Decompress(decoded);
    if (!inflated)
      throw new YasProtocolError("invalid Terminal grid LZ4 block");
    if (decoded.length + 8 > inflated.length)
      throw new YasProtocolError(
        "Terminal grid compression has insufficient savings",
      );
    decoded = inflated;
  } else if (decoded.length > maxDecodedFrame) {
    throw new YasProtocolError("Terminal decoded grid exceeds view limit");
  }

  const cursor = new YasCursor(decoded);
  let rows = base?.rows ?? 0;
  let cols = base?.cols ?? 0;
  if (frame.flags & YAS_TERMINAL_FRAME_DIMENSIONS) {
    rows = cursor.u16("Terminal grid rows");
    cols = cursor.u16("Terminal grid cols");
    const maxCells = Math.floor(maxDecodedFrame / YAS_TERMINAL_CELL_BYTES);
    if (rows === 0 || cols === 0 || rows * cols > maxCells)
      throw new YasProtocolError("invalid Terminal grid dimensions");
  }
  let cursorRow = base?.cursorRow ?? 0;
  let cursorCol = base?.cursorCol ?? 0;
  if (frame.flags & YAS_TERMINAL_FRAME_CURSOR) {
    cursorRow = cursor.u16("Terminal cursor row");
    cursorCol = cursor.u16("Terminal cursor column");
  }
  let modes = base?.modes ?? 0;
  if (frame.flags & YAS_TERMINAL_FRAME_MODES)
    modes = cursor.u16("Terminal modes");
  let scrollbackLines = base?.scrollbackLines ?? 0;
  if (frame.flags & YAS_TERMINAL_FRAME_SCROLLBACK)
    scrollbackLines = cursor.u32("Terminal scrollback lines");
  let scrollOffset = base?.scrollOffset ?? 0n;
  if (frame.flags & YAS_TERMINAL_FRAME_VIEW_OFFSET)
    scrollOffset = cursor.i64("Terminal scroll offset");
  let title = base?.title ?? "";
  if (frame.flags & YAS_TERMINAL_FRAME_TITLE)
    title = cursor.utf8U16("Terminal title");
  if (cursorRow >= rows || cursorCol >= cols)
    throw new YasProtocolError("Terminal cursor is outside the grid");

  const totalCells = rows * cols;
  const cells = keyframe
    ? new Uint8Array(totalCells * YAS_TERMINAL_CELL_BYTES)
    : new Uint8Array(base!.cells);
  const lineFlags = keyframe
    ? new Uint8Array(rows)
    : new Uint8Array(base!.lineFlags);
  const overflow = keyframe
    ? new Map<number, string>()
    : new Map(base!.overflowStrings);
  let hyperlinkUris = keyframe
    ? new Map<number, string>()
    : new Map(base!.hyperlinkUris);
  let hyperlinkRuns = keyframe ? [] : [...base!.hyperlinkRuns];
  const patchedCells = new Set<number>();

  const operationCount = readCanonicalUleb32(
    cursor,
    "Terminal operation count",
  );
  for (let operation = 0; operation < operationCount; operation++) {
    const opcode = cursor.u8("Terminal grid opcode");
    if (opcode === 0) {
      const start = readCanonicalUleb32(cursor, "Terminal patch start");
      const count = readCanonicalUleb32(cursor, "Terminal patch count");
      requireCellSpan(start, count, totalCells);
      applyTransposedCells(
        cursor,
        cells,
        range(start, count),
        patchedCells,
        overflow,
      );
    } else if (opcode === 1) {
      const count = readCanonicalUleb32(cursor, "Terminal patch-list count");
      if (count === 0)
        throw new YasProtocolError("Terminal patch-list is empty");
      const indices = [
        readCanonicalUleb32(cursor, "Terminal first patch-list cell"),
      ];
      for (let i = 1; i < count; i++) {
        const delta = readCanonicalUleb32(cursor, "Terminal patch-list delta");
        if (delta === 0)
          throw new YasProtocolError("Terminal patch-list delta is zero");
        indices.push(indices[i - 1]! + delta);
      }
      if (indices[indices.length - 1]! >= totalCells)
        throw new YasProtocolError("Terminal patch-list is outside the grid");
      applyTransposedCells(cursor, cells, indices, patchedCells, overflow);
    } else if (opcode === 2) {
      const start = readCanonicalUleb32(cursor, "Terminal bitmap start");
      const span = readCanonicalUleb32(cursor, "Terminal bitmap span");
      requireCellSpan(start, span, totalCells);
      const bitmap = cursor.take(Math.ceil(span / 8), "Terminal patch bitmap");
      if (
        (bitmap[0]! & 1) === 0 ||
        (bitmap[Math.floor((span - 1) / 8)]! & (1 << ((span - 1) & 7))) === 0 ||
        ((span & 7) !== 0 &&
          (bitmap[bitmap.length - 1]! & ~((1 << (span & 7)) - 1)) !== 0)
      )
        throw new YasProtocolError("Terminal patch bitmap is not canonical");
      const indices: number[] = [];
      for (let i = 0; i < span; i++)
        if (bitmap[i >> 3]! & (1 << (i & 7))) indices.push(start + i);
      applyTransposedCells(cursor, cells, indices, patchedCells, overflow);
    } else if (opcode === 3) {
      const srcRow = cursor.u16("Terminal copy source row");
      const srcCol = cursor.u16("Terminal copy source column");
      const dstRow = cursor.u16("Terminal copy destination row");
      const dstCol = cursor.u16("Terminal copy destination column");
      const copyRows = cursor.u16("Terminal copy rows");
      const copyCols = cursor.u16("Terminal copy columns");
      if (
        copyRows === 0 ||
        copyCols === 0 ||
        srcRow + copyRows > rows ||
        dstRow + copyRows > rows ||
        srcCol + copyCols > cols ||
        dstCol + copyCols > cols
      )
        throw new YasProtocolError("Terminal COPY_RECT is outside the grid");
      hyperlinkRuns = copyRectangle(
        cells,
        overflow,
        hyperlinkRuns,
        cols,
        srcRow,
        srcCol,
        dstRow,
        dstCol,
        copyRows,
        copyCols,
      );
    } else if (opcode === 4) {
      const row = cursor.u16("Terminal fill row");
      const col = cursor.u16("Terminal fill column");
      const fillRows = cursor.u16("Terminal fill rows");
      const fillCols = cursor.u16("Terminal fill columns");
      const cell = new Uint8Array(
        cursor.take(YAS_TERMINAL_CELL_BYTES, "Terminal fill cell"),
      );
      if (
        fillRows === 0 ||
        fillCols === 0 ||
        row + fillRows > rows ||
        col + fillCols > cols
      )
        throw new YasProtocolError("Terminal FILL_RECT is outside the grid");
      for (let r = 0; r < fillRows; r++) {
        for (let c = 0; c < fillCols; c++) {
          const index = (row + r) * cols + col + c;
          cells.set(cell, index * YAS_TERMINAL_CELL_BYTES);
          overflow.delete(index);
        }
      }
      hyperlinkRuns = removeHyperlinksInRectangle(
        hyperlinkRuns,
        cols,
        row,
        col,
        fillRows,
        fillCols,
      );
    } else {
      throw new YasProtocolError(`unknown Terminal grid opcode ${opcode}`);
    }
  }

  let sawOverflowComponent = false;
  if (frame.flags & YAS_TERMINAL_FRAME_COMPONENTS) {
    const componentCount = readCanonicalUleb32(
      cursor,
      "Terminal component count",
    );
    let previousKind = -1;
    for (let i = 0; i < componentCount; i++) {
      const kind = cursor.u8("Terminal component kind");
      const componentFlags = cursor.u8("Terminal component flags");
      if (kind <= previousKind || componentFlags & ~1)
        throw new YasProtocolError("invalid Terminal component header");
      previousKind = kind;
      const component = cursor.sub(
        readCanonicalUleb32(cursor, "Terminal component length"),
        "Terminal component",
      );
      if (kind === 0) {
        lineFlags.fill(0);
        const runCount = readCanonicalUleb32(component, "line-flag run count");
        let previousEnd = 0;
        for (let run = 0; run < runCount; run++) {
          const start = readCanonicalUleb32(component, "line-flag start");
          const count = readCanonicalUleb32(component, "line-flag count");
          const flags = component.u8("line flags");
          if (count === 0 || start < previousEnd || start + count > rows)
            throw new YasProtocolError("invalid Terminal line-flag run");
          lineFlags.fill(flags, start, start + count);
          previousEnd = start + count;
        }
      } else if (kind === 1) {
        sawOverflowComponent = true;
        const entryCount = readCanonicalUleb32(
          component,
          "overflow entry count",
        );
        let previousIndex = -1;
        for (let entry = 0; entry < entryCount; entry++) {
          const index = readCanonicalUleb32(component, "overflow cell index");
          const length = readCanonicalUleb32(
            component,
            "overflow UTF-8 length",
          );
          if (index <= previousIndex || !patchedCells.has(index))
            throw new YasProtocolError("invalid Terminal overflow index");
          previousIndex = index;
          const text = component.utf8(length, "Terminal overflow string");
          if (cellContentLength(cells, index) !== CONTENT_OVERFLOW)
            throw new YasProtocolError("overflow string names an inline cell");
          overflow.set(index, text);
        }
      } else if (kind === 2) {
        const uriCount = readCanonicalUleb32(component, "hyperlink URI count");
        hyperlinkUris = new Map();
        let previousId = 0;
        for (let uri = 0; uri < uriCount; uri++) {
          const id = readCanonicalUleb32(component, "hyperlink ID");
          const length = readCanonicalUleb32(component, "hyperlink URI length");
          if (id === 0 || id <= previousId || length > 4096)
            throw new YasProtocolError("invalid Terminal hyperlink URI");
          previousId = id;
          hyperlinkUris.set(id, component.utf8(length, "hyperlink URI"));
        }
        const runCount = readCanonicalUleb32(component, "hyperlink run count");
        hyperlinkRuns = [];
        let previousEnd = 0;
        for (let run = 0; run < runCount; run++) {
          const startCell = readCanonicalUleb32(
            component,
            "hyperlink run start",
          );
          const cellCount = readCanonicalUleb32(
            component,
            "hyperlink run length",
          );
          const linkId = readCanonicalUleb32(component, "hyperlink run ID");
          if (
            cellCount === 0 ||
            startCell < previousEnd ||
            startCell + cellCount > totalCells ||
            !hyperlinkUris.has(linkId)
          )
            throw new YasProtocolError("invalid Terminal hyperlink run");
          hyperlinkRuns.push({ startCell, cellCount, linkId });
          previousEnd = startCell + cellCount;
        }
      } else if (componentFlags & 1) {
        throw new YasProtocolError(
          `unknown required Terminal component ${kind}`,
        );
      }
      component.end("Terminal component");
    }
  }
  cursor.end("Terminal decoded grid");

  const patchedOverflow = [...patchedCells].filter(
    (index) => cellContentLength(cells, index) === CONTENT_OVERFLOW,
  );
  if (
    patchedOverflow.some((index) => !overflow.has(index)) ||
    (patchedOverflow.length !== 0 && !sawOverflowComponent)
  )
    throw new YasProtocolError("Terminal overflow strings are incomplete");
  validateLinks(cells, hyperlinkUris, hyperlinkRuns, totalCells);

  return {
    sequence: frame.sequence,
    rows,
    cols,
    cursorRow,
    cursorCol,
    modes,
    scrollbackLines,
    scrollOffset,
    title,
    cells,
    lineFlags,
    overflowStrings: overflow,
    hyperlinkUris,
    hyperlinkRuns,
  };
}

export function readCanonicalUleb32(
  cursor: YasCursor,
  field = "ULEB128",
): number {
  let value = 0;
  let shift = 0;
  for (let index = 0; index < 5; index++) {
    const byte = cursor.u8(field);
    if (index === 4 && (byte & 0xf0) !== 0)
      throw new YasProtocolError(`${field} exceeds u32`);
    value += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) {
      if (index !== 0 && byte === 0)
        throw new YasProtocolError(`${field} is not canonical`);
      return value >>> 0;
    }
    shift += 7;
  }
  throw new YasProtocolError(`${field} is too long`);
}

export function encodeCanonicalUleb32(value: number): Uint8Array {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff)
    throw new RangeError("value is not a u32");
  const bytes: number[] = [];
  do {
    let byte = value & 0x7f;
    value = Math.floor(value / 128);
    if (value !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (value !== 0);
  return new Uint8Array(bytes);
}

function applyTransposedCells(
  cursor: YasCursor,
  cells: Uint8Array,
  indices: readonly number[],
  patched: Set<number>,
  overflow: Map<number, string>,
): void {
  // One take for the whole plane-major block, then scatter. Reading it a byte
  // at a time cost a bounds check and a subarray per byte -- the decode half of
  // the same transpose the renderer ingest does, and as many per frame.
  const planes = cursor.take(
    indices.length * YAS_TERMINAL_CELL_BYTES,
    "Terminal cell plane",
  );
  for (let byte = 0; byte < YAS_TERMINAL_CELL_BYTES; byte++) {
    const base = byte * indices.length;
    for (let position = 0; position < indices.length; position++)
      cells[indices[position]! * YAS_TERMINAL_CELL_BYTES + byte] =
        planes[base + position]!;
  }
  for (const index of indices) {
    patched.add(index);
    overflow.delete(index);
  }
}

function copyRectangle(
  cells: Uint8Array,
  overflow: Map<number, string>,
  links: readonly YasTerminalHyperlinkRun[],
  cols: number,
  srcRow: number,
  srcCol: number,
  dstRow: number,
  dstCol: number,
  rows: number,
  width: number,
): YasTerminalHyperlinkRun[] {
  const linkCells = new Map<number, number>();
  for (const run of links)
    for (
      let index = run.startCell;
      index < run.startCell + run.cellCount;
      index++
    )
      linkCells.set(index, run.linkId);
  const cellCopies: {
    dst: number;
    cell: Uint8Array;
    overflow?: string;
    linkId?: number;
  }[] = [];
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < width; c++) {
      const src = (srcRow + r) * cols + srcCol + c;
      const dst = (dstRow + r) * cols + dstCol + c;
      cellCopies.push({
        dst,
        cell: cells.slice(
          src * YAS_TERMINAL_CELL_BYTES,
          (src + 1) * YAS_TERMINAL_CELL_BYTES,
        ),
        overflow: overflow.get(src),
        linkId: linkCells.get(src),
      });
    }
  }
  for (const copy of cellCopies) {
    cells.set(copy.cell, copy.dst * YAS_TERMINAL_CELL_BYTES);
    overflow.delete(copy.dst);
    if (copy.overflow !== undefined) overflow.set(copy.dst, copy.overflow);
    linkCells.delete(copy.dst);
    if (copy.linkId !== undefined) linkCells.set(copy.dst, copy.linkId);
  }
  return runsFromLinks(linkCells);
}

function removeHyperlinksInRectangle(
  runs: readonly YasTerminalHyperlinkRun[],
  cols: number,
  row: number,
  col: number,
  rows: number,
  width: number,
): YasTerminalHyperlinkRun[] {
  const removed = new Set<number>();
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < width; c++) removed.add((row + r) * cols + col + c);
  const cells = new Map<number, number>();
  for (const run of runs)
    for (
      let index = run.startCell;
      index < run.startCell + run.cellCount;
      index++
    )
      if (!removed.has(index)) cells.set(index, run.linkId);
  return runsFromLinks(cells);
}

function validateLinks(
  cells: Uint8Array,
  uris: ReadonlyMap<number, string>,
  runs: readonly YasTerminalHyperlinkRun[],
  total: number,
): void {
  const linked = new Map<number, number>();
  for (const run of runs) {
    if (!uris.has(run.linkId))
      throw new YasProtocolError("Terminal hyperlink run has no URI");
    for (
      let index = run.startCell;
      index < run.startCell + run.cellCount;
      index++
    ) {
      if (linked.has(index))
        throw new YasProtocolError("overlapping Terminal hyperlink runs");
      linked.set(index, run.linkId);
    }
  }
  for (let index = 0; index < total; index++) {
    const marked =
      (cells[index * YAS_TERMINAL_CELL_BYTES + 1]! & CELL_LINK) !== 0;
    if (marked !== linked.has(index))
      throw new YasProtocolError("Terminal hyperlink cell map is inconsistent");
  }
}

function runsFromLinks(
  links: ReadonlyMap<number, number>,
): YasTerminalHyperlinkRun[] {
  const sorted = [...links.entries()].sort((left, right) => left[0] - right[0]);
  const runs: YasTerminalHyperlinkRun[] = [];
  for (const [index, linkId] of sorted) {
    const previous = runs[runs.length - 1];
    if (
      previous &&
      previous.linkId === linkId &&
      previous.startCell + previous.cellCount === index
    )
      previous.cellCount++;
    else runs.push({ startCell: index, cellCount: 1, linkId });
  }
  return runs;
}

function cellContentLength(cells: Uint8Array, index: number): number {
  return (cells[index * YAS_TERMINAL_CELL_BYTES + 1]! >> 3) & 7;
}

function requireCellSpan(start: number, count: number, total: number): void {
  if (count === 0 || start >= total || start + count > total)
    throw new YasProtocolError("Terminal cell span is outside the grid");
}

function range(start: number, count: number): number[] {
  return Array.from({ length: count }, (_, index) => start + index);
}
