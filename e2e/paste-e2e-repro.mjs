/* Live end-to-end paste-chain repro against a real Yas server and Edge.
 *
 * Stages probed:
 *  (A) browser -> server: native Yas frame metadata is logged at the
 *      WebSocket boundary without decoding sensitive family payloads.
 *  (B/C) server -> compositor -> wayland client: the paste_probe client
 *      (crates/compositor/examples/paste_probe.rs) logs every offer,
 *      selection, key, and the bytes receive() returns.
 *  (D) is the probe reading the bytes itself.
 *
 * Run: node e2e/paste-e2e-repro.mjs   (server+Edge must be up on 3391)
 */
import { chromium } from "@playwright/test";
import { spawn } from "child_process";

const BASE = process.env.YAS_PASTE_BASE ?? "http://127.0.0.1:3391";
const WAYLAND_SOCK =
  process.env.YAS_PASTE_WAYLAND ?? "/tmp/yas-paste-repro/wayland-0";
const PROBE =
  process.env.YAS_PASTE_PROBE ??
  `${process.cwd()}/target/debug/examples/paste_probe`;

const clientLog = [];
const wsLog = [];

// --- start the wayland probe client ---
const probe = spawn(PROBE, [WAYLAND_SOCK], {
  stdio: ["pipe", "pipe", "inherit"],
});
let probeStdout = "";
probe.stdout.on("data", (d) => {
  probeStdout += d;
  for (const line of String(d).split("\n")) {
    if (line.trim()) clientLog.push(`[client] ${line}`);
  }
});
probe.on("exit", (code) => clientLog.push(`[client] EXIT code=${code}`));

function waitFor(fn, timeoutMs, what) {
  return new Promise((resolve, reject) => {
    const t0 = Date.now();
    const iv = setInterval(() => {
      const v = fn();
      if (v) {
        clearInterval(iv);
        resolve(v);
      } else if (Date.now() - t0 > timeoutMs) {
        clearInterval(iv);
        reject(new Error(`timeout waiting for ${what}`));
      }
    }, 50);
  });
}

await waitFor(() => probeStdout.includes("READY"), 10000, "probe READY");

// --- browser ---
const browser = await chromium.launch({
  executablePath: "/etc/profiles/per-user/pcarrier/bin/chromium",
});
const context = await browser.newContext();
await context.grantPermissions(["clipboard-read", "clipboard-write"], {
  origin: BASE,
});
const page = await context.newPage();

function describeYasFrame(payload) {
  const bytes = Buffer.from(payload);
  if (bytes.equals(Buffer.from([0x59, 0x41, 0x53, 0, 1, 0, 0x0d, 0x0a])))
    return "PREFACE yas/1";
  if (bytes.length < 5) return `truncated len=${bytes.length}`;
  const family = bytes.readUInt16LE(0);
  const kind = bytes.readUInt16LE(2);
  const meta = bytes[4];
  const frameClass = ["Event", "Request", "Result"][meta & 3] ?? "reserved";
  const correlated =
    (meta & 3) === 1 || (meta & 3) === 2
      ? ` request=${bytes.length >= 9 ? bytes.readUInt32LE(5) : "truncated"}`
      : "";
  return (
    `family=0x${family.toString(16).padStart(4, "0")} ` +
    `kind=0x${kind.toString(16).padStart(4, "0")} class=${frameClass}` +
    `${correlated} sensitive=${(meta & 8) !== 0} compressed=${(meta & 4) !== 0} ` +
    `len=${bytes.length}`
  );
}

page.on("websocket", (ws) => {
  wsLog.push(`[ws] open ${ws.url()}`);
  ws.on("framesent", (frame) => {
    const p = frame.payload;
    if (typeof p === "string") {
      wsLog.push(`[ws] sent text ${JSON.stringify(p.slice(0, 80))}`);
      return;
    }
    wsLog.push(`[ws] sent ${describeYasFrame(p)}`);
  });
  ws.on("framereceived", (frame) => {
    const p = frame.payload;
    if (typeof p === "string") return;
    wsLog.push(`[ws] recv ${describeYasFrame(p)}`);
  });
});

function dump() {
  console.log("================ WS / UI LOG ================");
  console.log(wsLog.join("\n"));
  console.log("================ FULL CLIENT LOG ================");
  console.log(clientLog.join("\n"));
}

// Wrap WebSocket.send before the app loads and log only the native Yas header.
// Sensitive Selection and Surface bodies intentionally remain opaque here.
// Also record keydown/paste events at document level.
await context.addInitScript(() => {
  window.__yasSent = [];
  window.__evts = [];
  const origSend = WebSocket.prototype.send;
  WebSocket.prototype.send = function (data) {
    try {
      const note = (buffer) => {
        const bytes = new Uint8Array(buffer);
        const preface = [0x59, 0x41, 0x53, 0, 1, 0, 0x0d, 0x0a];
        if (
          bytes.length === preface.length &&
          bytes.every((byte, index) => byte === preface[index])
        ) {
          window.__yasSent.push("PREFACE yas/1");
          return;
        }
        if (bytes.length < 5) {
          window.__yasSent.push(`truncated len=${bytes.length}`);
          return;
        }
        const view = new DataView(buffer);
        const meta = view.getUint8(4);
        const frameClass =
          ["Event", "Request", "Result"][meta & 3] ?? "reserved";
        const correlated =
          ((meta & 3) === 1 || (meta & 3) === 2) && bytes.length >= 9
            ? ` request=${view.getUint32(5, true)}`
            : "";
        window.__yasSent.push(
          `family=0x${view.getUint16(0, true).toString(16).padStart(4, "0")} ` +
            `kind=0x${view.getUint16(2, true).toString(16).padStart(4, "0")} ` +
            `class=${frameClass}${correlated} sensitive=${(meta & 8) !== 0} ` +
            `compressed=${(meta & 4) !== 0} len=${bytes.length}`,
        );
      };
      if (data instanceof ArrayBuffer) note(data);
      else if (ArrayBuffer.isView(data))
        note(
          data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength),
        );
      else if (data instanceof Blob) data.arrayBuffer().then(note);
    } catch {}
    return origSend.call(this, data);
  };
  document.addEventListener(
    "keydown",
    (e) =>
      window.__evts.push(
        `keydown key=${e.key} code=${e.code} ctrl=${e.ctrlKey} target=${e.target?.tagName}/${e.target?.getAttribute?.("aria-label") ?? ""}`,
      ),
    true,
  );
  document.addEventListener(
    "paste",
    (e) => {
      const items = [];
      if (e.clipboardData)
        for (const it of e.clipboardData.items)
          items.push(it.kind + ":" + it.type);
      window.__evts.push(
        `paste target=${e.target?.tagName}/${e.target?.getAttribute?.("aria-label") ?? ""} items=[${items.join(",")}] text=${JSON.stringify(e.clipboardData?.getData("text/plain") ?? "")}`,
      );
    },
    true,
  );
});

const drainPage = async (label) => {
  const s = await page.evaluate(() => {
    const s = window.__yasSent.splice(0);
    const e = window.__evts.splice(0);
    return { s, e };
  });
  for (const l of s.s) wsLog.push(`[page-ws] ${label} sent ${l}`);
  for (const l of s.e) wsLog.push(`[page-evt] ${label} ${l}`);
};

try {
  await page.goto(BASE);
  // Wait for the workspace: the probe's surface should show up as a pane with
  // a "Surface input" textarea.
  await waitFor(() => wsLog.length > 0, 10000, "websocket open");
  const surfaceInput = page.locator('textarea[aria-label="Surface input"]');
  await surfaceInput.waitFor({ state: "attached", timeout: 15000 });
  wsLog.push("[ui] surface pane present");
  await page.waitForTimeout(2000); // let frames flow so _displaySize is set

  // Click the canvas in the same pane as the surface textarea.
  const clickPoint = await surfaceInput.evaluate((el) => {
    let node = el.parentElement;
    let canvas = null;
    while (node && !canvas) {
      canvas = node.querySelector("canvas");
      node = node.parentElement;
    }
    if (!canvas) return null;
    const r = canvas.getBoundingClientRect();
    return {
      x: r.x + r.width / 2,
      y: r.y + r.height / 2,
      w: r.width,
      h: r.height,
    };
  });
  wsLog.push(`[ui] canvas rect: ${JSON.stringify(clickPoint)}`);
  if (clickPoint) await page.mouse.click(clickPoint.x, clickPoint.y);
  wsLog.push("[ui] clicked surface canvas");

  await waitFor(
    () => probeStdout.includes("KBD-ENTER"),
    8000,
    "probe KBD-ENTER (surface focused)",
  ).catch(async (e) => {
    wsLog.push(`[warn] ${e.message}`);
    wsLog.push(
      `[warn] activeElement=${await page.evaluate(() => document.activeElement?.outerHTML?.slice(0, 200))}`,
    );
  });
  await drainPage("after-focus");

  // --- Test 1: image paste ---
  wsLog.push("=== TEST 1: image/png paste ===");
  await page.evaluate(async () => {
    const c = document.createElement("canvas");
    c.width = 8;
    c.height = 8;
    const g = c.getContext("2d");
    g.fillStyle = "#f00";
    g.fillRect(0, 0, 8, 8);
    const blob = await new Promise((r) => c.toBlob(r, "image/png"));
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  });
  wsLog.push("[ui] clipboard now holds image/png");
  const t1Mark = clientLog.length;
  await page.keyboard.press("Control+V");
  await page.waitForTimeout(2500);
  await drainPage("test1");
  wsLog.push(
    `[test1] client log after Ctrl+V:\n${clientLog.slice(t1Mark).join("\n")}`,
  );

  // --- Test 2: text paste freshness ---
  wsLog.push("=== TEST 2: text paste freshness ===");
  await page.evaluate(() =>
    navigator.clipboard.writeText("MARKER-FROM-BROWSER"),
  );
  const t2Mark = clientLog.length;
  await page.keyboard.press("Control+V");
  await page.waitForTimeout(2500);
  await drainPage("test2");
  wsLog.push(
    `[test2] client log after Ctrl+V:\n${clientLog.slice(t2Mark).join("\n")}`,
  );

  // --- Test 3: copy-out (wayland client -> browser clipboard) ---
  wsLog.push("=== TEST 3: copy-out ===");
  probe.stdin.write("copy\n");
  await page.waitForTimeout(2500);
  let browserClip = "<readText failed>";
  try {
    browserClip = await page.evaluate(() => navigator.clipboard.readText());
  } catch (e) {
    browserClip = `<readText threw: ${e}>`;
  }
  wsLog.push(
    `[test3] browser clipboard readText() = ${JSON.stringify(browserClip)}`,
  );

  // --- summary ---
} catch (e) {
  wsLog.push(`[fatal] ${e}`);
  try {
    await page.screenshot({ path: "/tmp/yas-paste-repro/failure.png" });
    const html = await page.evaluate(() =>
      document.body.innerHTML.slice(0, 4000),
    );
    wsLog.push(`[dom] ${html}`);
  } catch {}
}
dump();

await browser.close();
probe.kill();
