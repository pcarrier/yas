import { test, expect, type Page } from "@playwright/test";
import { openReturningWorkspace } from "./workspace-auth";

/**
 * The hidden capture textarea is what the host IME anchors its candidate
 * window to, so it has to sit on the terminal's own cursor rather than in the
 * corner of the screen, where it used to live.
 *
 * No IME is needed to judge that, and neither are the terminal's cell
 * metrics: a cursor-position escape names a cell, and the box has to land on
 * it.  Cell size is measured from the terminal itself (typing n characters
 * walks the cursor n cells), and every assertion is a *difference* between
 * two placements, so a grid centred inside its canvas needs no special case.
 */

async function authenticate(page: Page) {
  await openReturningWorkspace(page);
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout: 15_000,
  });
  await page.waitForTimeout(500);
  const canvas = terminalCanvas(page);
  if (!(await canvas.isVisible().catch(() => false))) {
    await page.getByRole("button", { name: "New terminal" }).first().click();
  }
  await expect(canvas).toBeVisible({ timeout: 15_000 });
  await page.waitForTimeout(1500);
}

const focusedPaneSelector = '[data-yas-pane-focused="true"]';
const mainViewSelector = '[data-yas-workspace-focus-owner="main"]';

function terminalCanvas(page: Page) {
  return page
    .locator(`${focusedPaneSelector} canvas, ${mainViewSelector} canvas`)
    .first();
}

function captureTextarea(page: Page) {
  const input = 'textarea[aria-label="Terminal input"]:not([readonly])';
  return page.locator(
    `${focusedPaneSelector} ${input}, ${mainViewSelector} ${input}`,
  );
}

/** The capture textarea's box, after letting a frame render. */
async function caretBox(page: Page) {
  await page.waitForTimeout(400);
  const box = await captureTextarea(page).boundingBox();
  if (!box) throw new Error("no capture textarea");
  return box;
}

/** Park the terminal cursor on a 1-based (row, col) and hold it there. */
async function parkCursorAt(page: Page, row: number, col: number) {
  await page.keyboard.type(`printf '\\033[${row};${col}H'; sleep 4`, {
    delay: 30,
  });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(600);
}

test.describe("IME caret placement", () => {
  test("the capture textarea lands on the cell the cursor is in", async ({
    page,
  }) => {
    await authenticate(page);
    await captureTextarea(page).focus();

    const canvas = (await terminalCanvas(page).boundingBox())!;
    const start = await caretBox(page);
    // It left the corner for the terminal.
    expect(start.x).toBeGreaterThanOrEqual(canvas.x);
    expect(start.y).toBeGreaterThanOrEqual(canvas.y);
    expect(start.x).toBeLessThan(canvas.x + canvas.width);
    expect(start.y).toBeLessThan(canvas.y + canvas.height);
    // A caret is a line tall, not a pixel.
    const cellH = start.height;
    expect(cellH).toBeGreaterThan(4);

    // Five characters is five cells: that is the cell width, measured off the
    // terminal rather than assumed.
    await page.keyboard.type("abcde", { delay: 50 });
    const typed = await caretBox(page);
    expect(typed.y).toBeCloseTo(start.y, 0);
    const cellW = (typed.x - start.x) / 5;
    expect(cellW).toBeGreaterThan(2);
    // Shell line editing uses Control on every platform, including macOS.
    await page.keyboard.press("Control+u");
    await page.waitForTimeout(200);

    // Now name two cells outright. The escape moves the cursor and the sleep
    // holds it there while we measure.
    await parkCursorAt(page, 5, 10);
    const at5x10 = await caretBox(page);
    await page.waitForTimeout(4000);
    await parkCursorAt(page, 9, 20);
    const at9x20 = await caretBox(page);
    await page.waitForTimeout(4000);

    expect(at9x20.x - at5x10.x).toBeCloseTo(10 * cellW, 0);
    expect(at9x20.y - at5x10.y).toBeCloseTo(4 * cellH, 0);
  });

  test("an unfocused terminal parks its capture textarea in the corner", async ({
    page,
  }) => {
    await authenticate(page);
    const input = captureTextarea(page);
    await input.focus();
    await page.keyboard.type("x", { delay: 60 });
    const typing = await caretBox(page);
    expect(typing.x + typing.y).toBeGreaterThan(0);

    // A raw blur does not make this the unfocused view: the pane still
    // owns keyboard focus and may restore its capture target on the next
    // reactive update. Move focus to an actual control outside the pane.
    const menu = page.locator('button[title="Menu"]').first();
    await menu.focus();
    await expect(menu).toBeFocused();

    // Nothing composes into an unfocused view, and a box left over the pane
    // is one a software keyboard can cover.
    const parked = await caretBox(page);
    await expect(menu).toBeFocused();
    await expect(input).not.toBeFocused();
    expect([parked.x, parked.y]).toEqual([0, 0]);
  });
});
