import { test, expect, type Locator, type Page } from "@playwright/test";
import { closeAllTerminals } from "./yas-cli";

const TILE_DND_MIME = "application/x-yas-tile";

/**
 * Parked cards in the right-side preview panel can be dragged into the live
 * view, and are inert while parked.
 *
 * A terminal is parked explicitly before another fills the standalone view.
 * New terminals otherwise split the workspace and leave no parked card.
 */

async function authenticate(page: Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/#psk=test-secret");
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout: 10_000,
  });
  // Let hash encryption and connection setup settle.
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
  // The button belongs to an *empty* pane, so it is gone as soon as the first
  // session fills the view. Every session after that comes from the keyboard
  // shortcut the help overlay documents (mod+Enter, `help.newTerminal`).
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

/** The parked cards: draggable roots inside the preview panel. Explorer rows
 *  and commits are draggable tile sources too, so the panel has to scope it. */
function parkedCards(page: Page) {
  return page.locator('[data-yas-preview-panel] [draggable="true"]');
}

async function ensurePreviewPanelOpen(page: Page) {
  const panel = page.locator("[data-yas-preview-panel]");
  if ((await panel.count()) === 0) {
    await page.locator('[data-status-tool="preview"]').click();
  }
  await expect(panel).toBeVisible({ timeout: 10_000 });
}

/**
 * Drive the browser's custom tile payload explicitly. Playwright's native
 * `dragTo` can complete without carrying the `DataTransfer` populated by the
 * card's `dragstart`, which turns this into a pointer-motion test instead of a
 * test of YAS's drag/drop protocol.
 */
async function dragParkedCard(page: Page, card: Locator, target: Locator) {
  const source = await card.elementHandle();
  if (!source) throw new Error("parked card detached before dragstart");
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  let started = false;
  try {
    await source.dispatchEvent("dragstart", { dataTransfer: transfer });
    started = true;
    const payload = await transfer.evaluate(
      (data, mime) => ({
        types: [...data.types],
        assignment: data.getData(mime),
      }),
      TILE_DND_MIME,
    );
    expect(payload.types).toContain(TILE_DND_MIME);
    expect(payload.assignment.length).toBeGreaterThan(0);

    await target.dispatchEvent("dragenter", { dataTransfer: transfer });
    await target.dispatchEvent("dragover", { dataTransfer: transfer });
    await target.dispatchEvent("drop", { dataTransfer: transfer });
  } finally {
    if (started) {
      await source.dispatchEvent("dragend", { dataTransfer: transfer });
    }
    await transfer.dispose();
    await source.dispose();
  }
}

test.describe("Parked pane drag", () => {
  test.beforeEach(closeAllTerminals);
  test.afterAll(closeAllTerminals);

  test("a parked terminal is inert and drags into the main view", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);
    await page.keyboard.press("Control+b");
    await page.keyboard.press("q");
    await newTerminal(page);
    await ensurePreviewPanelOpen(page);

    // Sessions outlive a page, so what is parked here is "everything the main
    // view is not showing", not a number this test gets to fix.
    const cards = parkedCards(page);
    await expect(cards.first()).toBeVisible({ timeout: 10_000 });
    const parkedBefore = await cards.count();
    const card = cards.first();

    // Non-interactive while parked: the body wrapper is inert, which takes the
    // preview terminal's tabindex=0 input out of the tab order.
    const inertBody = card.locator("[inert]");
    await expect(inertBody).toHaveCount(1);
    expect(await inertBody.evaluate((el) => (el as HTMLElement).inert)).toBe(
      true,
    );
    // The parked terminal really is inside the inert subtree.
    await expect(inertBody.locator("canvas")).toHaveCount(1);

    const parkedLabel = (await card.innerText()).trim();
    expect(parkedLabel.length).toBeGreaterThan(0);

    // Drag it onto the main view (single-pane mode: one destination). The
    // terminal's own scroll surface covers the canvas, so the hit-target check
    // would refuse the drop it is aimed at; the events still land inside the
    // main view and bubble to its drop handler.
    const mainCanvas = page.locator("canvas").first();
    await dragParkedCard(page, card, mainCanvas);
    await page.waitForTimeout(500);

    // The dropped session leaves the dock; the displayed replacement is not
    // implicitly parked, because only explicit Park actions populate it.
    await expect(parkedCards(page)).toHaveCount(parkedBefore - 1);
    const nowParked = (await parkedCards(page).allInnerTexts()).map((t) =>
      t.trim(),
    );
    expect(nowParked).not.toContain(parkedLabel);
  });

  test("a parked card drags into a specific pane", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);
    await page.keyboard.press("Control+b");
    await page.keyboard.press("q");
    await newTerminal(page);
    await newTerminal(page);

    const panes = page.locator("[data-yas-pane-id]");
    await expect(panes).toHaveCount(2, { timeout: 10_000 });
    await ensurePreviewPanelOpen(page);

    // The layout fills two panes and leaves the third session parked. The
    // assertion is that the parked session lands in the pane it was dropped on.
    const cards = parkedCards(page);
    await expect(cards.first()).toBeVisible({ timeout: 10_000 });
    const parkedLabel = (await cards.first().innerText()).trim();

    // Drop onto the second pane specifically.
    const target = panes.nth(1);
    const targetId = await target.getAttribute("data-yas-pane-id");
    await dragParkedCard(page, cards.first(), target);
    await page.waitForTimeout(500);

    // That pane now holds the dropped session, and the card is gone from the
    // panel (offScreenSessions is derived from pane assignments).
    await expect(
      page.locator(`[data-yas-pane-id="${targetId}"] canvas`).first(),
    ).toBeVisible();
    const remaining = await parkedCards(page).allInnerTexts();
    expect(remaining.map((t) => t.trim())).not.toContain(parkedLabel);
  });
});
