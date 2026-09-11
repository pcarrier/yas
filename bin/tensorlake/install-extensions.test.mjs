import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { installExtensions } from "./install-extensions.mjs";

function bundle(t, names) {
  const directory = mkdtempSync(join(tmpdir(), "yas-extension-install-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  writeFileSync(join(directory, "manifest.json"), JSON.stringify({
    extensions: names.map((name) => ({ name, file: `${name}.wasm` })),
  }));
  return directory;
}

test("every manifest entry is installed persistently, including new extensions", (t) => {
  const names = ["doctor", "muster", "systemd", "xdg-desktop", "future-extension"];
  const directory = bundle(t, names);
  const installed = new Map();
  const calls = [];
  const run = (args) => {
    if (args[1] === "list") return [...installed.values()].map(JSON.stringify).join("\n");
    calls.push(args);
    installed.set(args[5], { name: args[5], persistent: true, enabled: true });
    return "";
  };
  installExtensions(directory, run);
  assert.deepEqual(calls, names.map((name) => [
    "ext", "run", "--persist", "--restart", "always", name, join(directory, `${name}.wasm`),
  ]));
  installExtensions(directory, run);
  assert.equal(calls.length, names.length);
});

test("existing persistent definitions are preserved, transient names do not suppress installation", (t) => {
  const directory = bundle(t, ["disabled", "customized", "transient"]);
  const snapshot = [
    { name: "disabled", persistent: true, enabled: false },
    { name: "customized", persistent: true, hash: "user-version" },
    { name: "transient", persistent: false },
  ];
  const calls = [];
  installExtensions(directory, (args) => {
    if (args[1] === "list") return snapshot.map(JSON.stringify).join("\n");
    calls.push(args);
    return "";
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0][5], "transient");
});

test("installation failure fails startup", (t) => {
  const directory = bundle(t, ["broken"]);
  assert.throws(() => installExtensions(directory, (args) => {
    if (args[1] === "list") return "";
    throw new Error("extension failed to start");
  }), /extension failed to start/);
});
