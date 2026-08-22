import { readFileSync } from "node:fs";
import {
  YAS_TERMINAL_GRID_CODEC_V1,
  YasConnection,
  YasTerminalClient,
  decodeTerminalGridV1,
  yasBrowserConnectionOptions,
} from "@yas-run/core";
import { ReplayTransport, parseYasrec } from "../src/lib/replay";

const bytes = readFileSync(
  new URL("../public/demo/hero-tests.yasrec", import.meta.url),
);
const recording = parseYasrec(
  bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
);
const transport = new ReplayTransport([{ ...recording, tag: "tests" }], {
  static: true,
});
const connection = new YasConnection(transport, yasBrowserConnectionOptions());
await connection.connect();
const terminal = new YasTerminalClient(connection);
const snapshot = await terminal.list();
if (snapshot.terminals.length !== 1) throw new Error("missing replay Terminal");
const view = await terminal.openView({
  terminalHandle: recording.terminalHandle,
  rows: recording.rows,
  cols: recording.cols,
  maxFps: 60,
  codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
});
let frames = 0;
let finalGrid = null;
view.subscribe((frame) => {
  finalGrid = decodeTerminalGridV1(frame, null, 32 * 1024);
  frames++;
});
await Promise.resolve();
if (frames !== recording.frames.length || !finalGrid)
  throw new Error("native replay did not deliver every TerminalFrame");
connection.close();
