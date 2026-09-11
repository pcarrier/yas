import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Sandbox } from "tensorlake";
import { sandboxTimeout } from "./startup.ts";
import { buildCasImage } from "./common.ts";

test("the pinned native SDK routes CAS builds to Image Service", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "yas-cas-test-"));
  const savedEnv = { ...process.env };
  const requests: { method: string; url: string; body: string }[] = [];
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    requests.push({ method: req.method!, url: req.url!, body });
    res.writeHead(req.method === "GET" ? 404 : 400, {
      "Content-Type": "application/json",
    });
    res.end('{"error":"intentional local test stop"}');
  });
  t.after(async () => {
    server.closeAllConnections();
    server.close();
    process.env = savedEnv;
    await rm(dir, { recursive: true, force: true });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address === "object");
  process.env.TENSORLAKE_API_KEY = "local-test-only";
  process.env.TENSORLAKE_API_URL = `http://127.0.0.1:${address.port}`;
  process.env.TENSORLAKE_IMAGE_SERVICE_URL = `${process.env.TENSORLAKE_API_URL}/images/v4`;
  process.env.DOCKER_CONFIG = dir;
  await writeFile(join(dir, "Dockerfile"), "FROM ubuntu:26.04\nRUN true\n");
  await assert.rejects(
    buildCasImage(join(dir, "Dockerfile"), "test-yas"),
    /intentional local test stop/,
  );
  const build = requests.find((request) => request.method === "POST");
  assert.equal(build?.url, "/images/v4/builds");
  assert(build);
  const payload = JSON.parse(build.body);
  assert.equal(payload.kind, "dockerfile");
  assert.equal(payload.name, "test-yas");
  assert.equal(payload.disk_mb, 32 * 1024);
  assert.deepEqual(payload.builder_resources, {
    cpus: 8,
    memory_mb: 16 * 1024,
  });
  assert.notEqual(payload.image_scope, "global");
  assert(requests.every((request) => request.url.startsWith("/images/v4/")));
});

test("the pinned SDK preserves a zero sandbox timeout in the API request", async (t) => {
  const requests: { url: string; body: string }[] = [];
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    requests.push({ url: req.url!, body });
    res.writeHead(400, { "Content-Type": "application/json" });
    res.end('{"error":"intentional local test stop"}');
  });
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address === "object");
  await assert.rejects(Sandbox.create({
    apiUrl: `http://127.0.0.1:${address.port}`,
    apiKey: "local-test-only",
    image: "test-yas",
    timeoutSecs: sandboxTimeout(),
  }), /intentional local test stop/);
  assert.equal(requests.length, 1);
  assert.match(requests[0].url, /\/sandboxes$/);
  assert.equal(JSON.parse(requests[0].body).timeout_secs, 0);
});
