import { test, expect, type Page } from "@playwright/test";
import { closeAllTerminals } from "./yas-cli";

/**
 * PaneTools lives in the top tab bar: every focused pane kind gets one stable
 * action strip without covering terminal, editor, web, or Wayland content.
 */

const GRIP = "Drag to move";

async function authenticate(page: Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/#psk=test-secret");
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout: 10_000,
  });
  // With no sessions yet (a previous test may have closed them all), yas
  // offers the Remotes dialog; it is modal and would swallow later clicks.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
}

/** Live terminal count from the status bar's `{count}T` badge. */
async function terminalCount(page: Page) {
  const text = await page.locator('button[title="Menu"]').first().innerText();
  const match = /(\d+)T/.exec(text);
  if (!match) throw new Error(`no terminal count in ${JSON.stringify(text)}`);
  return Number(match[1]);
}

async function newTerminal(page: Page) {
  const before = await terminalCount(page);
  const button = page.getByRole("button", { name: "New terminal" }).first();
  if (await button.isVisible()) {
    await button.click();
  } else {
    await page.keyboard.press("Control+b");
    await page.keyboard.press("Enter");
  }
  await expect
    .poll(() => terminalCount(page), { timeout: 10_000 })
    .toBe(before + 1);
  await expect(page.locator("canvas").first()).toBeVisible({ timeout: 10_000 });
  await page.waitForTimeout(300);
}

/** The two terminals opened above occupy sibling panes. */
async function twoPanes(page: Page) {
  const panes = page.locator("[data-yas-pane-id]");
  await expect(panes).toHaveCount(2, { timeout: 10_000 });
  await page.waitForTimeout(500);
  return panes;
}

/** Managed workspaces persist panel state, so establish the test precondition. */
async function openPreviewPanel(page: Page) {
  const panel = page.locator("[data-yas-preview-panel]");
  if ((await panel.count()) === 0) {
    await page.locator('[data-status-tool="preview"]').click();
  }
  await expect(panel).toBeVisible({ timeout: 10_000 });
}

/**
 * Park the main view's content by grip-dragging it to the dock.
 *
 * The events are dispatched by hand with one shared DataTransfer rather than
 * via `dragTo`: the grip lives inside a hover-gated `Show`, so the pointer
 * travelling to the dock un-hovers the main view and unmounts the very element
 * a mouse-emulated drag is holding. A real drag is immune (the browser owns it
 * once dragstart fires); Playwright's is not. The production handlers still
 * run — startPaneTileDrag writes the payload, the dock's onDrop reads it.
 *
 * `dragenter` is not decoration: with nothing parked yet the dock is not in
 * the DOM at all, and it is that window-level event which reveals it as a
 * drop-to-park target.
 */
async function parkViaGrip(page: Page) {
  const grip = await revealGrip(page);
  const dt = await page.evaluateHandle(() => new DataTransfer());
  await grip.dispatchEvent("dragstart", { dataTransfer: dt });
  await page.locator("body").dispatchEvent("dragenter", { dataTransfer: dt });
  const dock = page.locator("[data-yas-preview-panel]");
  await expect(dock).toBeVisible({ timeout: 5_000 });
  await dock.dispatchEvent("dragover", { dataTransfer: dt });
  await dock.dispatchEvent("drop", { dataTransfer: dt });
  await page.waitForTimeout(500);
}

/** Focus the main view and return the tab bar's pane grip. */
async function revealGrip(page: Page) {
  await page.locator("canvas").first().hover({ force: true });
  const grip = page.getByRole("button", { name: GRIP }).first();
  await expect(grip).toBeVisible();
  return grip;
}

test.beforeEach(closeAllTerminals);
test.afterAll(closeAllTerminals);

test.describe("Pane multitool on a main-view terminal", () => {
  test("the top tab bar exposes the focused pane actions", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    const toolbar = page
      .getByRole("navigation", { name: "Workspaces", exact: true })
      .getByRole("toolbar", { name: "Focused pane actions" });
    await expect(toolbar).toBeVisible();
    await expect(toolbar.getByRole("button", { name: GRIP })).toHaveAttribute(
      "draggable",
      "true",
    );
    await expect(
      toolbar.getByRole("button", { name: "Close", exact: true }),
    ).toBeVisible();
    await expect(
      page.locator('[data-yas-pane-id] [role="toolbar"]'),
    ).toHaveCount(0);
  });

  test("the grip exports the focused assignment", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    const grip = await revealGrip(page);
    const dt = await page.evaluateHandle(() => new DataTransfer());
    await grip.dispatchEvent("dragstart", { dataTransfer: dt });
    const payload = await dt.evaluate((data) => ({
      types: [...data.types],
      assignment: data.getData("application/x-yas-tile"),
    }));
    expect(payload.types).toContain("application/x-yas-tile");
    await expect(
      page.getByRole("toolbar", { name: "Focused pane actions" }),
    ).toHaveAttribute("data-yas-pane-tools-assignment", payload.assignment);
    await grip.dispatchEvent("dragend", { dataTransfer: dt });
    await dt.dispose();
  });

  test("the tab-bar actions follow focus to another pane", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);
    await newTerminal(page);
    const panes = await twoPanes(page);
    const toolbar = page.getByRole("toolbar", {
      name: "Focused pane actions",
    });
    const firstId = await panes.nth(0).getAttribute("data-yas-pane-id");
    const secondId = await panes.nth(1).getAttribute("data-yas-pane-id");

    await panes.nth(0).locator(".yas-scroll-surface").click({ force: true });
    await expect(toolbar).toHaveAttribute(
      "data-yas-pane-tools-pane-id",
      firstId!,
    );
    await panes.nth(1).locator(".yas-scroll-surface").click({ force: true });
    await expect(toolbar).toHaveAttribute(
      "data-yas-pane-tools-pane-id",
      secondId!,
    );
  });

  test("dragging the grip to the dock parks the terminal", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    await parkViaGrip(page);

    // Parked: the main view shows the empty pane, and the session is now a
    // card in the dock.
    await expect(
      page.getByRole("button", { name: "New terminal" }).first(),
    ).toBeVisible({ timeout: 10_000 });
    await openPreviewPanel(page);
    await expect(
      page.locator('[data-yas-preview-panel] [draggable="true"]').first(),
    ).toBeVisible();

    // And it comes back: clicking its card un-parks it.
    await page
      .locator('[data-yas-preview-panel] [draggable="true"]')
      .first()
      .click();
    await expect(page.locator("canvas").first()).toBeVisible({
      timeout: 10_000,
    });
  });

  test("the background shortcut leaves the standalone view empty", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);

    await page.keyboard.press("Control+b");
    await page.keyboard.press("q");

    await expect(
      page.getByRole("button", { name: "New terminal" }).first(),
    ).toBeVisible({ timeout: 10_000 });
    await openPreviewPanel(page);
    await expect(
      page.locator('[data-yas-preview-panel] [draggable="true"]').first(),
    ).toBeVisible();
  });

  test("no pinned close button anywhere: every ✕ rides the multitool", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);

    // The main-view-only "Close tab" ✕ is gone for good.
    await expect(page.locator('button[title="Close tab"]')).toHaveCount(0);

    // The multitool's ✕ closes the terminal.
    await page.locator("canvas").first().hover({ force: true });
    const close = page
      .getByRole("button", { name: "Close", exact: true })
      .first();
    await expect(close).toBeVisible();
    const terminalsBefore = await terminalCount(page);
    await close.click();
    await expect
      .poll(() => terminalCount(page), { timeout: 10_000 })
      .toBe(terminalsBefore - 1);
  });
});

test.describe("Parked terminal remains recoverable", () => {
  test("closing its replacement leaves the explicitly parked terminal in the dock", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);

    // Park A.
    await parkViaGrip(page);
    await expect(
      page.getByRole("button", { name: "New terminal" }).first(),
    ).toBeVisible({ timeout: 10_000 });

    // Open B: A un-parks into the dock and B takes the view.
    await newTerminal(page);
    await expect(page.locator("canvas").first()).toBeVisible();

    // Close B. Explicitly parked A stays parked rather than reappearing
    // merely because the replacement exited.
    await page.locator("canvas").first().hover({ force: true });
    await page
      .getByRole("button", { name: "Close", exact: true })
      .first()
      .click();
    await page.waitForTimeout(1000);
    await expect(
      page.getByRole("button", { name: "New terminal" }).first(),
    ).toBeVisible({
      timeout: 10_000,
    });
    await openPreviewPanel(page);
    await expect(
      page.locator('[data-yas-preview-panel] [draggable="true"]').first(),
    ).toBeVisible();
  });
});
