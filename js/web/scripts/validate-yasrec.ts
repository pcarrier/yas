/** Validate a native YASREC1 recording and print its final terminal grid. */

import { readFileSync } from "node:fs";
import {
  YAS_HARD_MAX_DECODED_FRAME,
  YAS_TERMINAL_FRAME_KEYFRAME,
  decodeTerminalGridV1,
  decodeYasTerminalRecording,
  type YasTerminalGridState,
} from "@yas-run/core";

const input = process.argv[2];
if (!input) throw new Error("usage: validate-yasrec <recording.yasrec>");
const bytes = new Uint8Array(readFileSync(input));
const recording = decodeYasTerminalRecording(bytes);
const grids = new Map<number, YasTerminalGridState>();
let finalGrid: YasTerminalGridState | null = null;
for (const { frame } of recording.frames) {
  const baseSequence =
    frame.flags & YAS_TERMINAL_FRAME_KEYFRAME
      ? undefined
      : (frame.explicitBase ?? (frame.sequence - 1) >>> 0);
  finalGrid = decodeTerminalGridV1(
    frame,
    baseSequence === undefined ? null : (grids.get(baseSequence) ?? null),
    YAS_HARD_MAX_DECODED_FRAME,
  );
  grids.set(frame.sequence, finalGrid);
}
if (!finalGrid) throw new Error("YASREC1 recording is empty");
const last = recording.frames.at(-1)!;
console.log(
  `${recording.frames.length} frames, ${(Number(last.timestampTicks) / 1e6).toFixed(1)}s, ${bytes.length} bytes`,
);
console.log("--- final screen ---");
console.log(renderPlainText(finalGrid).trimEnd());

function renderPlainText(grid: YasTerminalGridState): string {
  const decoder = new TextDecoder();
  const lines: string[] = [];
  for (let row = 0; row < grid.rows; row++) {
    let line = "";
    for (let col = 0; col < grid.cols; col++) {
      const cellIndex = row * grid.cols + col;
      const offset = cellIndex * 12;
      const flags = grid.cells[offset + 1]!;
      if (flags & 4) continue;
      const length = (flags >> 3) & 7;
      line +=
        length === 7
          ? (grid.overflowStrings.get(cellIndex) ?? " ")
          : length === 0
            ? " "
            : decoder.decode(
                grid.cells.subarray(offset + 8, offset + 8 + length),
              );
    }
    lines.push(line.trimEnd());
  }
  return lines.join("\n");
}
