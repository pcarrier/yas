import { describe, expect, test } from "bun:test";
import { type YasHost, decodeUtf8, encodeUtf8 } from "./yas";
import { serveCommands } from "./command";

function fakeHost(
  incoming: Array<{ args: string[]; streamsStdin: boolean }>,
): YasHost & { calls: Array<[string, ...unknown[]]> } {
  const calls: Array<[string, ...unknown[]]> = [];
  return {
    context: {
      extensionHandle: 7n,
      generation: 8n,
      definitionRevision: 9n,
      attempt: 11n,
      taskId: 13,
      contentHash: "42".repeat(32),
      name: "doctor",
      argv: [],
      detached: true,
      persistent: true,
      enabled: true,
      desiredRunning: true,
      protocolMinor: 1,
      bootId: "11".repeat(16),
      sessionId: "22".repeat(16),
      serverName: "test",
      serverRelease: "1",
      families: [0, 3, 4, 14],
    },
    calls,
    registerCommand(descriptor) {
      calls.push(["register", JSON.parse(descriptor)]);
    },
    acceptCommand() {
      return incoming.shift();
    },
    commandStdout(data) {
      calls.push(["stdout", decodeUtf8(data)]);
    },
    commandStderr(data) {
      calls.push(["stderr", decodeUtf8(data)]);
    },
    commandResult(contentType, data) {
      calls.push(["result", contentType, decodeUtf8(data)]);
    },
    commandExit(code, detail) {
      calls.push(["exit", code, detail]);
    },
    commandCancel() {
      calls.push(["cancel"]);
    },
    wait: () => 2,
    waitUntil: () => 2,
    realtimeNow: () => 1n,
    monotonicNow: () => 1n,
    random: (length) => new Uint8Array(length),
    sleep() {},
    log() {},
  };
}

describe("QuickJS TypeScript support", () => {
  test("UTF-8 round-trips without web globals", () => {
    const text = "plain · café · 🚀";
    expect(decodeUtf8(encodeUtf8(text))).toBe(text);
    expect(() => decodeUtf8(Uint8Array.of(0xc0, 0x80))).toThrow(
      "invalid UTF-8",
    );
  });

  test("registers and serves through typed native command bindings", () => {
    const host = fakeHost([{ args: ["--json"], streamsStdin: false }]);
    const code = serveCommands(
      {
        protocol: "yas.cli.v1",
        summary: "test",
        commands: [{ path: [] }],
      },
      ({ args }) => ({
        stdout: `${args.length} argument\n`,
        result: { contentType: "application/json", data: '{"ok":true}\n' },
      }),
      host,
    );

    expect(code).toBe(0);
    expect(host.calls).toEqual([
      [
        "register",
        {
          protocol: "yas.cli.v1",
          summary: "test",
          commands: [{ path: [] }],
        },
      ],
      ["stdout", "1 argument\n"],
      ["result", "application/json", '{"ok":true}\n'],
      ["exit", 0, ""],
    ]);
  });
});
