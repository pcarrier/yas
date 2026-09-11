import assert from "node:assert/strict";
import { test } from "node:test";
import type { RunOptions, StartProcessOptions } from "tensorlake";
import { launchYas } from "./launch.ts";
import { shareConfiguration } from "./startup.ts";

function mockRuntime(failInstallation = false, failEdge = false) {
  const calls: string[] = [];
  const starts: StartProcessOptions[] = [];
  const processes = new Map<string, {
    pid: number; status: string; args: string[]; managed: { name: string; status: string };
  }>();
  let saved: string | undefined;
  let info = { sandboxId: "test-sandbox", ingressEndpoint: "https://sandbox.tensorlake.ai", exposedPorts: [] as number[], allowUnauthenticatedAccess: false };
  let nextPid = 10;
  const runtime = {
    async info() { return info; },
    async update(options: { exposedPorts: number[]; allowUnauthenticatedAccess: boolean }) {
      calls.push("expose:edge"); info = { ...info, ...options }; return info;
    },
    async run(command: string, options: RunOptions = {}) {
      calls.push(`run:${command}:${options.args?.[0] ?? ""}`);
      const failed = command === "node" && ((failInstallation && options.args?.[0].endsWith("install-extensions.mjs")) || (failEdge && options.args?.[1]?.includes("YAS edge did not respond")));
      return {
        exitCode: command === "test" ? (saved === undefined ? 1 : 0) : (failed ? 1 : 0),
        stdout: "", stderr: failed ? "startup check failed" : "",
      };
    },
    async readFile() { return Buffer.from(saved!); },
    async writeFile(_path: string, bytes: Uint8Array) { saved = Buffer.from(bytes).toString(); },
    async listProcesses() { return [...processes.values()]; },
    async startProcess(_command: string, options: StartProcessOptions) {
      calls.push(`start:${options.name}`);
      starts.push(options);
      const process = { pid: nextPid++, status: "running", args: options.args ?? [], managed: { name: options.name!, status: "running" } };
      processes.set(options.name!, process);
      return process;
    },
    async restartProcess(name: string) { calls.push(`restart:${name}`); return processes.get(name); },
    async killProcess(name: string) { calls.push(`kill:${name}`); processes.delete(name); },
    async getProcess(name: string) { return processes.get(name); },
    async getStderr() { return { lines: [] }; },
  };
  return { runtime: runtime as unknown as Parameters<typeof launchYas>[0], calls, starts, processes, setSaved(value: string) { saved = value; }, getSaved() { return saved; } };
}

test("startup hosts share and edge in one supervised server and exposes its HTTPS URL", async () => {
  const mock = mockRuntime();
  const result = await launchYas(mock.runtime, "test secret");
  const server = mock.starts[0];
  assert.equal(mock.starts.length, 1);
  assert.deepEqual(server.args, ["server", "--share", "--edge", "--export-sock", "--inject-path"]);
  assert.equal(server.user, "yas");
  assert.equal(server.restart?.policy, "always");
  assert.equal(server.env?.YAS_PASSPHRASE, "test secret");
  assert.equal(server.env?.YAS_ADDR, "0.0.0.0:3264");
  assert.equal(server.env?.YAS_WEBRTC_VERBOSE, "1");
  assert.equal(result.shareUrl, "https://yas.run/s#psk=test%20secret");
  assert.equal(result.edgeUrl, "https://3264-test-sandbox.sandbox.tensorlake.ai/#psk=test%20secret");
  assert.deepEqual((await mock.runtime.info()).exposedPorts, [3264]);
  const ready = mock.calls.indexOf("run:node:-e");
  const installed = mock.calls.indexOf("run:node:/usr/local/lib/yas/install-extensions.mjs");
  assert(mock.calls.indexOf("start:yas-server") < ready);
  assert(ready < installed);
  assert(installed < mock.calls.indexOf("expose:edge"));
  assert(!mock.calls.some((call) => call.includes("systemctl")));
});

test("retry preserves both URLs and does not restart the running server", async () => {
  const mock = mockRuntime();
  const first = await launchYas(mock.runtime);
  const pid = mock.processes.get("yas-server")!.pid;
  const second = await launchYas(mock.runtime);
  assert.equal(first.shareUrl, second.shareUrl);
  assert.equal(first.edgeUrl, second.edgeUrl);
  assert.equal(mock.processes.get("yas-server")!.pid, pid);
  assert.equal(mock.starts.length, 1);
  assert.equal(mock.calls.filter(call => call === "expose:edge").length, 1);
  assert(!mock.calls.some((call) => /^(kill|restart):/.test(call)));
});

test("migration stops the standalone share and replaces the old server once", async () => {
  const mock = mockRuntime();
  mock.setSaved(shareConfiguration("saved secret").environment);
  mock.processes.set("yas-server", { pid: 1, status: "running", args: ["server", "--export-sock", "--inject-path"], managed: { name: "yas-server", status: "running" } });
  mock.processes.set("yas-share", { pid: 2, status: "running", args: ["share"], managed: { name: "yas-share", status: "running" } });
  const result = await launchYas(mock.runtime);
  assert.equal(result.shareUrl, shareConfiguration("saved secret").shareUrl);
  assert.deepEqual(mock.calls.filter(call => call.startsWith("kill:")), ["kill:yas-share", "kill:yas-server"]);
  assert.equal(mock.starts[0].env?.YAS_PASSPHRASE, "saved secret");
  await launchYas(mock.runtime);
  assert.equal(mock.starts.length, 1);
});

test("a passphrase change replaces the combined server", async () => {
  const mock = mockRuntime();
  await launchYas(mock.runtime, "old secret");
  const serverPid = mock.processes.get("yas-server")!.pid;
  const result = await launchYas(mock.runtime, "new secret");
  assert.notEqual(mock.processes.get("yas-server")!.pid, serverPid);
  assert.equal(result.shareUrl, shareConfiguration("new secret").shareUrl);
  assert(result.edgeUrl.endsWith("#psk=new%20secret"));
  assert.deepEqual(mock.calls.filter(call => call.startsWith("kill:")), ["kill:yas-server"]);
  assert.equal(mock.starts.at(-1)?.env?.YAS_PASSPHRASE, "new secret");
});

test("failed extension installation leaves the server running without exposing the edge", async () => {
  const mock = mockRuntime(true);
  await assert.rejects(launchYas(mock.runtime), /startup check failed/);
  assert(mock.processes.has("yas-server"));
  assert(!mock.processes.has("yas-share"));
  assert(!mock.calls.some(call => call.startsWith("kill:") || call === "expose:edge"));
});

test("failed edge readiness does not commit new credentials or expose the port", async () => {
  const mock = mockRuntime(false, true);
  const saved = shareConfiguration("old secret").environment;
  mock.setSaved(saved);
  await assert.rejects(launchYas(mock.runtime, "new secret"), /startup check failed/);
  assert.equal(mock.getSaved(), saved);
  assert(!mock.calls.includes("expose:edge"));
});

test("edge exposure preserves already-public ports", async () => {
  const mock = mockRuntime();
  const info = await mock.runtime.info();
  info.exposedPorts = [8080];
  info.allowUnauthenticatedAccess = true;
  await launchYas(mock.runtime);
  assert.deepEqual((await mock.runtime.info()).exposedPorts, [8080, 3264]);
});

test("edge exposure does not remove Tensorlake authentication from unrelated ports", async () => {
  const mock = mockRuntime();
  (await mock.runtime.info()).exposedPorts = [8080];
  await assert.rejects(launchYas(mock.runtime), /other ports require Tensorlake authentication/);
  assert.equal(mock.starts.length, 0);
  assert.equal(mock.getSaved(), undefined);
});
