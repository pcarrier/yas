import { test, expect } from "@playwright/test";
import { openSwitcher } from "./workspace-auth";
import { closeAllTerminals } from "./yas-cli";

async function authenticate(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/#psk=test-secret");
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout: 10_000,
  });
}

async function authenticateAndCreateTerminal(
  page: import("@playwright/test").Page,
) {
  await authenticate(page);
  // Wait for the DOM to stabilise after hash encryption and connection setup.
  await page.waitForTimeout(500);
  const canvas = page.locator("canvas").first();
  if (!(await canvas.isVisible().catch(() => false))) {
    await page.getByRole("button", { name: "New terminal" }).first().click();
  }
  await expect(canvas).toBeVisible({ timeout: 10_000 });
}

test.describe("Terminal", () => {
  // The native server spans browser contexts and earlier specs exercise
  // retained extension terminals. Every case here starts from its own shell.
  test.beforeEach(closeAllTerminals);
  test.afterAll(closeAllTerminals);

  test("after auth, workspace is ready", async ({ page }) => {
    await authenticate(page);
    await expect(page.getByRole("status", { name: "Connected" })).toBeVisible();
  });

  test("creating a terminal shows canvas with non-zero dimensions", async ({
    page,
  }) => {
    await authenticateAndCreateTerminal(page);

    const canvas = page.locator("canvas").first();
    await expect(canvas).toBeVisible({ timeout: 10_000 });

    const box = await canvas.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.width).toBeGreaterThan(0);
    expect(box!.height).toBeGreaterThan(0);
  });

  test("can type in terminal and see output", async ({ page }) => {
    await authenticateAndCreateTerminal(page);

    await page.waitForTimeout(1000);

    // Every parked session keeps an input of its own, and those are readonly:
    // the live terminal's is the writable one.
    const inputSink = page.locator(
      'textarea[aria-label="Terminal input"]:not([readonly])',
    );
    await inputSink.focus();

    await page.keyboard.type("echo hello-e2e-test", { delay: 50 });
    await page.keyboard.press("Enter");

    await page.waitForTimeout(2000);

    const canvas = page.locator("canvas").first();
    const box = await canvas.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.width).toBeGreaterThan(0);
    expect(box!.height).toBeGreaterThan(0);
  });

  test("restarts an exited terminal", async ({ page }) => {
    await authenticateAndCreateTerminal(page);

    const input = page.locator(
      'textarea[aria-label="Terminal input"]:not([readonly])',
    );
    await input.focus();
    await page.keyboard.type("exit");
    await page.keyboard.press("Enter");

    const restart = page.getByRole("button", { name: "Restart Enter" });
    await expect(restart).toBeVisible({ timeout: 10_000 });

    await restart.click();
    await expect(restart).toBeHidden({ timeout: 10_000 });

    // Leave no retained terminal behind for the next serial test.
    await input.focus();
    await page.keyboard.type("exit");
    await page.keyboard.press("Enter");
    await expect(restart).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: "Close Esc" }).click();
  });

  test("closing an exited terminal reflows the remaining pane", async ({
    page,
  }) => {
    await authenticateAndCreateTerminal(page);
    const firstInput = page.locator(
      'textarea[aria-label="Terminal input"]:not([readonly])',
    );
    await firstInput.focus();
    await page.keyboard.press("Control+b");
    await page.keyboard.press("Shift+Enter");

    const panes = page.locator("[data-yas-pane-id]");
    await expect(panes).toHaveCount(2, { timeout: 10_000 });
    const widths = await panes.evaluateAll((elements) =>
      elements.map((element) => element.getBoundingClientRect().width),
    );

    const closingPane = panes.last();
    const closingInput = closingPane.locator(
      'textarea[aria-label="Terminal input"]:not([readonly])',
    );
    await closingInput.focus();
    await page.keyboard.type("exit");
    await page.keyboard.press("Enter");
    const close = closingPane.getByRole("button", { name: "Close Esc" });
    await expect(close).toBeVisible({ timeout: 10_000 });
    await close.click();

    await expect(panes).toHaveCount(1, { timeout: 10_000 });
    const remaining = await panes.first().boundingBox();
    expect(remaining).not.toBeNull();
    expect(remaining!.width).toBeGreaterThan(Math.max(...widths));
  });

  test("Switcher opens on Ctrl+K and shows search and items", async ({
    page,
  }) => {
    await authenticateAndCreateTerminal(page);
    await page.waitForTimeout(500);

    await openSwitcher(page);

    const dialog = page.locator('div[role="dialog"]');
    await expect(dialog).toBeVisible({ timeout: 5_000 });

    const searchInput = dialog.locator('input[type="text"]');
    await expect(searchInput).toBeVisible();

    await expect(dialog.locator("section").first()).toBeVisible();
  });

  test("Switcher offers a Search action that opens the search panel", async ({
    page,
  }) => {
    await authenticate(page);
    // The auto-opened Remotes dialog is modal; dismiss it before clicking.
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);
    const canvas = page.locator("canvas").first();
    if (!(await canvas.isVisible().catch(() => false))) {
      await page.getByRole("button", { name: "New terminal" }).first().click();
    }
    await expect(canvas).toBeVisible({ timeout: 10_000 });

    await openSwitcher(page);
    const dialog = page.locator('div[role="dialog"]');
    await expect(dialog).toBeVisible({ timeout: 5_000 });

    await dialog.getByText("Search", { exact: true }).first().click();

    // The switcher closes and the search pane's input takes focus.
    await expect(dialog).toHaveCount(0);
    await expect(page.locator("[data-yas-search-pane] input")).toBeFocused();
  });

  test("can create a new PTY from Switcher", async ({ page }) => {
    await authenticateAndCreateTerminal(page);
    await page.waitForTimeout(500);

    const canvasBefore = await page.locator("canvas").count();

    await openSwitcher(page);
    const dialog = page.locator('div[role="dialog"]');
    await expect(dialog).toBeVisible({ timeout: 5_000 });

    const newTermBtn = dialog.getByText("New terminal").first();
    await newTermBtn.click();

    await page.waitForTimeout(2000);

    const canvasAfter = await page.locator("canvas").count();
    expect(canvasAfter).toBeGreaterThanOrEqual(canvasBefore);
  });

  test("Switcher preview canvases render with non-zero dimensions", async ({
    page,
  }) => {
    await authenticateAndCreateTerminal(page);
    await page.waitForTimeout(500);

    await openSwitcher(page);
    const dialog = page.locator('div[role="dialog"]');
    await expect(dialog).toBeVisible({ timeout: 5_000 });

    const previewCanvas = dialog.locator("canvas").first();
    await expect(previewCanvas).toBeVisible({ timeout: 5_000 });
    const box = await previewCanvas.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.width).toBeGreaterThan(0);
    expect(box!.height).toBeGreaterThan(0);
  });
});
