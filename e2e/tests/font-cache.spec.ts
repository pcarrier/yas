import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import path from "node:path";
import { expect, test } from "@playwright/test";

// Run the actual store against browser IndexedDB, independent of a live YAS
// server. The Nix-pinned Node runtime strips types without requiring an
// installed UI workspace, which is absent in the isolated CI build.
const storeModule = stripTypeScriptTypes(
  readFileSync(path.resolve(__dirname, "../../js/ui/src/fontStore.ts"), "utf8"),
);
const origin = "http://yas-font-cache.test";

test("font bytes survive an immediate browser reload", async ({ page }) => {
  await page.route(`${origin}/**`, (route) => {
    const module = new URL(route.request().url()).pathname === "/fontStore.js";
    return route.fulfill({
      contentType: module ? "text/javascript" : "text/html",
      body: module
        ? storeModule
        : "<!doctype html><title>Font cache test</title>",
    });
  });
  await page.goto(origin);
  const hash = "01".repeat(32);
  await page.evaluate(async (hash) => {
    const moduleUrl = "/fontStore.js";
    const { saveFontFace } = await import(moduleUrl);
    await saveFontFace(hash, new Uint8Array([1, 2, 3, 4]));
  }, hash);

  await page.reload();
  const cached = await page.evaluate(async (hash) => {
    const moduleUrl = "/fontStore.js";
    const { loadFontFace } = await import(moduleUrl);
    const face = await loadFontFace(hash);
    return face ? Array.from(face.data) : null;
  }, hash);
  expect(cached).toEqual([1, 2, 3, 4]);
});
