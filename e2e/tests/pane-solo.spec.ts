import { test, expect, type Page } from "@playwright/test";
import { closeAllTerminals } from "./yas-cli";

/**
 * Solo: one pane fills the workspace, its siblings hidden rather than
 * unmounted. Reachable from the multitool's ▣ segment and from Ctrl+Shift+K
 * (the chord workspace roots gave up).
 *
 * Hidden, not unmounted, is the property worth pinning: pane ids are
 * positional paths, so rewriting the tree would renumber them and dispose
 * every sibling's terminal surface. The canvases must survive a solo.
 */

async function authenticate(page: Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/#psk=test-secret");
  await expect(
    page
      .getByRole("button", { name: "New terminal" })
      .first()
      .or(page.locator("canvas").first()),
  ).toBeVisible({ timeout: 10_000 });
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
    await page.keyboard.press("ControlOrMeta+Enter");
  }
  await expect
    .poll(() => terminalCount(page), { timeout: 10_000 })
    .toBe(before + 1);
  await expect(page.locator("canvas").first()).toBeVisible({ timeout: 10_000 });
  await page.waitForTimeout(300);
}

/** Apply a two-pane layout through the managed workspace-session UI. */
async function twoPanes(page: Page) {
  await page.locator('button[title="Menu"]').first().click();
  const search = page.locator('input[name="yas-switcher-search"]');
  await expect(search).toBeVisible();
  await search.fill("Two panes:line(_,_)");
  await search.press("Enter");
  await page.waitForTimeout(800);
  const panes = page.locator("[data-yas-pane-id]");
  await expect(panes).toHaveCount(2, { timeout: 10_000 });
  return panes;
}

/** Panes whose box actually has area — a soloed sibling is display:none. */
async function visiblePaneCount(page: Page) {
  const boxes = await page
    .locator("[data-yas-pane-id]")
    .evaluateAll((els) =>
      els.map((el) => (el as HTMLElement).getBoundingClientRect().width),
    );
  return boxes.filter((w) => w > 0).length;
}

test.describe("Pane solo", () => {
  test.beforeEach(closeAllTerminals);
  test.afterAll(closeAllTerminals);

  test("the multitool's solo segment fills the workspace and restores", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);
    await newTerminal(page);
    const panes = await twoPanes(page);

    const first = panes.nth(0);
    const widthBefore = (await first.boundingBox())!.width;
    expect(await visiblePaneCount(page)).toBe(2);
    const canvasesBefore = await page.locator("canvas").count();

    await first.hover({ force: true });
    const solo = page.getByRole("button", { name: /Solo this pane/ }).first();
    await expect(solo).toBeVisible();
    await solo.click();
    await page.waitForTimeout(500);

    // One pane on screen, and it grew.
    expect(await visiblePaneCount(page)).toBe(1);
    expect((await first.boundingBox())!.width).toBeGreaterThan(widthBefore);
    // Nothing was torn down: the hidden pane's canvas is still mounted.
    expect(await page.locator("canvas").count()).toBe(canvasesBefore);

    // The segment now offers the way back.
    await first.hover({ force: true });
    const unsolo = page.getByRole("button", { name: /Show all panes/ }).first();
    await expect(unsolo).toBeVisible();
    await unsolo.click();
    await page.waitForTimeout(500);

    expect(await visiblePaneCount(page)).toBe(2);
    expect((await first.boundingBox())!.width).toBeCloseTo(widthBefore, 0);
  });

  test("Ctrl+Shift+K toggles solo on the focused pane", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);
    await newTerminal(page);
    await twoPanes(page);

    expect(await visiblePaneCount(page)).toBe(2);
    await page.keyboard.press("Control+Shift+K");
    await page.waitForTimeout(500);
    expect(await visiblePaneCount(page)).toBe(1);
    await page.keyboard.press("Control+Shift+K");
    await page.waitForTimeout(500);
    expect(await visiblePaneCount(page)).toBe(2);
  });

  test("a single-pane layout offers no solo", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    // The single-view main view has nothing to solo against.
    await page.locator("canvas").first().hover({ force: true });
    await expect(
      page.getByRole("button", { name: /Solo this pane/ }),
    ).toHaveCount(0);
  });
});
