import { OutputMode, sandboxUrlFromIngressEndpoint, type ProcessInfo, type RunOptions, type Sandbox } from "tensorlake";
import { parseShareConfiguration, shareConfiguration } from "./startup.ts";

type Runtime = Pick<Sandbox,
  "run" | "listProcesses" | "startProcess" | "restartProcess" | "killProcess" |
  "getProcess" | "getStderr" | "readFile" | "writeFile" | "info" | "update"
>;

const edgePort = 3264;
const serverArgs = ["server", "--share", "--edge", "--export-sock", "--inject-path"];
const environment = {
  HOME: "/home/yas",
  USER: "yas",
  SHELL: "/bin/bash",
  XDG_RUNTIME_DIR: "/run/yas",
  YAS_SOCK: "/run/yas/yas-default.sock",
  MOZ_ENABLE_WAYLAND: "1",
  YAS_PROXY: "0",
  YAS_WEBRTC_VERBOSE: "1",
};
const asYas = { user: "yas", workingDir: "/home/yas", env: environment };
const shareFile = "/etc/yas/share.env";

// Probe the socket directly: a YAS CLI readiness check can auto-start an
// unsupervised server when the managed server has not bound its socket yet.
const waitForSocket = `
const net = require("node:net");
const deadline = Date.now() + 60000;
function connect() {
  const socket = net.createConnection(process.env.YAS_SOCK);
  socket.once("connect", () => { socket.destroy(); process.exit(0); });
  socket.once("error", () => {
    socket.destroy();
    if (Date.now() >= deadline) {
      console.error("YAS server did not open its socket within 60 seconds.");
      process.exit(1);
    }
    setTimeout(connect, 100);
  });
}
connect();
`;

const waitForEdge = `
const deadline = Date.now() + 60000;
(async () => {
  while (Date.now() < deadline) {
    try {
      const response = await fetch("http://127.0.0.1:${edgePort}/", { signal: AbortSignal.timeout(2000) });
      await response.body?.cancel();
      if (response.ok) return;
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error("YAS edge did not respond within 60 seconds.");
})().catch(error => { console.error(error.message); process.exit(1); });
`;

async function checked(runtime: Runtime, command: string, options: RunOptions = {}) {
  const result = await runtime.run(command, options);
  if (result.exitCode !== 0) {
    throw new Error(`${command} exited with status ${result.exitCode}:\n${result.stdout}\n${result.stderr}`);
  }
  return result.stdout;
}

async function ensureProcess(
  runtime: Runtime,
  processes: ProcessInfo[],
  name: string,
  args: string[],
  env: Record<string, string> = environment,
  replace = false,
) {
  const matches = processes.filter((process) =>
    process.managed?.name === name || process.managed?.id === name);
  const existing = matches.find((process) => process.status === "running") ?? matches[0];
  if (existing && !replace && JSON.stringify(existing.args) === JSON.stringify(args)) {
    if (existing.managed?.status === "stopped") await runtime.restartProcess(name);
    return;
  }
  // Migrating to hosted transports or changing their shared passphrase needs
  // a server replacement. Identical retries preserve the live session.
  if (existing) await runtime.killProcess(name);
  await runtime.startProcess("/usr/local/bin/yas", {
    ...asYas,
    name,
    args,
    env,
    restart: { policy: "always", initialBackoffMs: 1000, maxBackoffMs: 10000 },
    stdoutMode: OutputMode.CAPTURE,
    stderrMode: OutputMode.CAPTURE,
  });
}

export async function launchYas(runtime: Runtime, configuredPassphrase?: string) {
  const configured = configuredPassphrase === undefined ? undefined : shareConfiguration(configuredPassphrase);
  const info = await runtime.info();
  const ports = info.exposedPorts ?? [];
  // Tensorlake's proxy authentication switch applies to every exposed port.
  if (!info.allowUnauthenticatedAccess && ports.some(port => port !== edgePort)) {
    throw new Error("Cannot expose the YAS edge publicly while other ports require Tensorlake authentication.");
  }
  if (!info.ingressEndpoint) throw new Error("Tensorlake did not return an ingress endpoint for the edge URL.");
  const edgeBase = sandboxUrlFromIngressEndpoint(info.ingressEndpoint, info.sandboxId, edgePort);
  if (!edgeBase.startsWith("https://")) throw new Error("The public YAS edge requires an HTTPS ingress endpoint.");
  await checked(runtime, "install", {
    args: ["-d", "-o", "yas", "-g", "yas", "-m", "0700", "/run/yas"], user: "root",
  });
  await checked(runtime, "install", { args: ["-d", "-m", "0700", "/etc/yas"], user: "root" });
  const saved = await runtime.run("test", { args: ["-f", shareFile], user: "root" });
  if (saved.exitCode !== 0 && saved.exitCode !== 1) throw new Error(saved.stderr);
  const previous = saved.exitCode === 0
    ? Buffer.from(await runtime.readFile(shareFile)).toString("utf8")
    : undefined;
  const share = configured ?? (previous === undefined
    ? shareConfiguration() : parseShareConfiguration(previous));
  const processes = await runtime.listProcesses();
  if (processes.some(process =>
    (process.managed?.name === "yas-share" || process.managed?.id === "yas-share") &&
    process.managed.status !== "stopped")) {
    await runtime.killProcess("yas-share");
  }
  await ensureProcess(runtime, processes, "yas-server", serverArgs,
    { ...environment, YAS_PASSPHRASE: share.env.YAS_SHARE_PASSPHRASE, YAS_ADDR: `0.0.0.0:${edgePort}` },
    share.environment !== previous);
  try {
    await checked(runtime, "node", { ...asYas, args: ["-e", waitForSocket], timeout: 75 });
    await checked(runtime, "node", { ...asYas, args: ["-e", waitForEdge], timeout: 75 });
  } catch (error) {
    const logs = await runtime.getStderr("yas-server");
    throw new Error(`${error}\n${logs.lines.slice(-40).join("\n")}`, { cause: error });
  }
  // Commit credentials only after the replacement is ready. If startup fails,
  // a retry must still detect a requested passphrase change.
  await runtime.writeFile(shareFile, Buffer.from(share.environment));
  await checked(runtime, "chmod", { args: ["600", shareFile], user: "root" });
  await checked(runtime, "node", {
    ...asYas, args: ["/usr/local/lib/yas/install-extensions.mjs"], timeout: 180,
  });
  const gpu = await checked(runtime, "nvidia-smi", { args: ["--query-gpu=name", "--format=csv,noheader"] });
  await checked(runtime, "/usr/local/bin/yas", { ...asYas, args: ["terminal", "list"] });
  const extensions = await checked(runtime, "/usr/local/bin/yas", { ...asYas, args: ["ext", "list"] });
  const process = await runtime.getProcess("yas-server");
  if (process.status !== "running") {
    const logs = await runtime.getStderr("yas-server");
    throw new Error(`yas-server is ${process.status}:\n${logs.lines.slice(-40).join("\n")}`);
  }
  if (!ports.includes(edgePort) || !info.allowUnauthenticatedAccess) {
    await runtime.update({
      exposedPorts: [...new Set([...ports, edgePort])],
      allowUnauthenticatedAccess: true,
    });
  }
  return {
    edgeUrl: `${edgeBase}/#psk=${encodeURIComponent(share.env.YAS_SHARE_PASSPHRASE)}`,
    shareUrl: share.shareUrl, gpu: gpu.trim(), extensions: extensions.trim(),
  };
}
