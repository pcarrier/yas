import { test, expect } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { access, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

// This exercises the shipped browser -> Edge -> home server Relay -> uplink
// path, separately from the direct YasUplinkTransport SDK.
test("browser terminal input crosses the authenticated uplink", async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  page.setDefaultTimeout(10_000);
  const binary = path.resolve(__dirname, "../../target/debug/yas");
  const fixtureBinary =
    process.env.YAS_UPLINK_E2E_FIXTURE ??
    path.resolve(__dirname, "../../target/debug/examples/uplink-e2e-fixture");
  await access(fixtureBinary).catch(() => {
    throw new Error(
      "Build the uplink fixture with cargo build -p yas-cli --example uplink-e2e-fixture, or run ./bin/e2e",
    );
  });
  const fixture = spawn(fixtureBinary, [binary], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  let logs = "";
  fixture.stderr.on("data", (data) => {
    logs += data;
  });
  const lines = createInterface({ input: fixture.stdout });
  type Ready = { baseURL: string; markerPath: string; homeMarkerPath: string };
  type Report = {
    attaches: number;
    capturedBytes: number;
    plaintextLeaked: boolean;
    producerAlive: boolean;
  };
  const reports: unknown[] = [];
  lines.on("line", (line) => {
    reports.push(JSON.parse(line));
    logs += `fixture: ${line}\n`;
  });
  const exited = new Promise<number | null>((resolve, reject) => {
    fixture.once("error", reject);
    fixture.once("close", resolve);
  });
  const errors: string[] = [];
  page.on("console", (message) => {
    logs += `browser: ${message.text()}\n`;
  });
  page.on("pageerror", (error) => {
    errors.push(error.message);
    logs += `pageerror: ${error.stack}\n`;
  });
  try {
    await expect
      .poll(() => reports.length, {
        timeout: 30_000,
        message: "uplink fixture ready",
      })
      .toBeGreaterThan(0);
    const ready = reports[0] as Ready;
    await expect
      .poll(async () => {
        try {
          return (await fetch(ready.baseURL)).status;
        } catch {
          return 0;
        }
      })
      .toBe(200);
    await page.goto(`${ready.baseURL}/#psk=test-secret`);
    await expect(page.getByRole("status", { name: "Connected" })).toBeVisible();
    await page.getByRole("status").click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    // One configured remote besides local, so this names its membership input.
    await dialog
      .getByRole("checkbox", { name: "Add to workspace", exact: true })
      .check();
    await expect(
      dialog
        .getByRole("listitem")
        .filter({ hasText: "uplink-test" })
        .locator('[title="Connected"]'),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await page.keyboard.press("ControlOrMeta+b");
    await page.keyboard.press("k");
    await expect(page.getByRole("dialog")).toBeVisible();
    await page
      .getByRole("dialog")
      .getByText(/^uplink-test:\d+ › \/bin\/sh -i$/)
      .first()
      .click();
    await expect(page.locator("canvas").first()).toBeVisible();
    await page.keyboard.type(
      "printf 'browser-uplink-proof-73b1' > browser-proof",
    );
    await page.keyboard.press("Enter");
    await expect
      .poll(() => readFile(ready.markerPath, "utf8").catch(() => ""))
      .toBe("browser-uplink-proof-73b1");
    await expect(access(ready.homeMarkerPath)).rejects.toThrow();
    await expect(page.getByRole("status", { name: "Connected" })).toBeVisible();
    expect(errors).toEqual([]);
    await page.close();
    fixture.stdin.end();
    expect(await exited).toBe(0);
    const report = reports[1] as Report;
    expect(report.attaches).toBeGreaterThan(0);
    expect(report.capturedBytes).toBeGreaterThan(0);
    expect(report.plaintextLeaked).toBe(false);
    expect(report.producerAlive).toBe(true);
  } finally {
    if (!page.isClosed()) {
      logs += `UI before cleanup:\n${await page
        .locator("body")
        .ariaSnapshot()
        .catch(() => "snapshot unavailable")}\n`;
    }
    if (fixture.exitCode === null) {
      fixture.stdin.end();
      await Promise.race([
        exited.catch(() => {}),
        new Promise((resolve) => setTimeout(resolve, 3000)),
      ]);
      if (fixture.exitCode === null) fixture.kill();
    }
    const logPath = testInfo.outputPath("uplink-fixture.log");
    await writeFile(logPath, logs);
    await testInfo.attach("uplink-fixture.log", {
      path: logPath,
      contentType: "text/plain",
    });
  }
});
