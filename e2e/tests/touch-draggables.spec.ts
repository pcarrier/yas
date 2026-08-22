import { test, expect, type Locator, type Page } from "@playwright/test";
import { closeAllTerminals } from "./yas-cli";

/**
 * Every draggable, not just the pane grip, must work with a finger.
 *
 * HTML5 drag-and-drop never fires from touch, so explorer rows, changed
 * files, search hits, problems, commits and dock cards were all mouse-only.
 * They share the grip's bridge but start differently: a list row cannot carry
 * `touch-action: none` without losing its scrolling, so a hold — not a
 * movement — is what begins the drag.
 *
 * The two properties worth pinning are therefore both directions: a hold
 * drags, and a swipe still scrolls.
 */
test.use({
  hasTouch: true,
  isMobile: true,
  viewport: { width: 900, height: 700 },
});

/** Matches `LONG_PRESS_MS` in tileDrag.ts, with room for scheduling. */
const HOLD_MS = 700;

async function authenticate(page: Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/#psk=test-secret");
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout: 10_000,
  });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
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

async function touchWindow(
  page: Page,
  type: "pointermove" | "pointerup",
  at: { x: number; y: number },
) {
  await page.evaluate(
    ([type, at]) => {
      const ev = new PointerEvent(type as string, {
        pointerId: 1,
        pointerType: "touch",
        isPrimary: true,
        clientX: (at as { x: number }).x,
        clientY: (at as { y: number }).y,
        bubbles: true,
        cancelable: true,
      });
      window.dispatchEvent(ev);
    },
    [type, at] as const,
  );
}

async function touchDown(card: Locator, expectedLabel: string) {
  const dispatched: { at: { x: number; y: number } | null } = { at: null };
  await expect
    .poll(
      async () => {
        try {
          const at = await card.evaluate((element, label) => {
            const card = element as HTMLElement;
            const box = card.getBoundingClientRect();
            if (
              !card.isConnected ||
              box.width === 0 ||
              box.height === 0 ||
              card.innerText.trim() !== label
            ) {
              return null;
            }
            const at = {
              x: box.x + box.width / 2,
              y: box.y + box.height / 2,
            };
            const down = new PointerEvent("pointerdown", {
              pointerId: 1,
              pointerType: "touch",
              isPrimary: true,
              clientX: at.x,
              clientY: at.y,
              bubbles: true,
              cancelable: true,
            });
            card.dispatchEvent(down);
            return at;
          }, expectedLabel);
          if (!at) return false;
          dispatched.at = at;
          return true;
        } catch {
          return false;
        }
      },
      {
        timeout: 10_000,
        message: "current parked card accepts the touch",
      },
    )
    .toBe(true);
  if (!dispatched.at) throw new Error("touch was not dispatched");
  return dispatched.at;
}

/**
 * Swipe horizontally across an element, as a finger does: both event families
 * at once.
 *
 * Pointer events alone would prove only that the drag bridge stays inert.
 * Swipe-to-dismiss is wired to `onTouchStart`/`onTouchMove`/`onTouchEnd` and
 * reads `TouchEvent.touches`, so without real touch events the gesture this
 * must not steal never actually happens, and the test would pass whatever the
 * bridge did to it.
 */
async function swipe(
  card: Locator,
  expectedLabel: string,
  dx: number,
  canceled = false,
) {
  await expect
    .poll(
      async () => {
        try {
          return await card.evaluate(
            (element, [expectedLabel, dx, canceled]) => {
              const el = element as HTMLElement;
              const box = el.getBoundingClientRect();
              if (
                !el.isConnected ||
                box.width === 0 ||
                box.height === 0 ||
                el.innerText.trim() !== expectedLabel
              ) {
                return false;
              }
              const start = {
                x: box.x + box.width / 2,
                y: box.y + box.height / 2,
              };
              const touch = (x: number) =>
                new Touch({
                  identifier: 1,
                  target: el,
                  clientX: x,
                  clientY: start.y,
                });
              const touchEvent = (type: string, x: number) => {
                const t = touch(x);
                const terminal = type === "touchend" || type === "touchcancel";
                return new TouchEvent(type, {
                  touches: terminal ? [] : [t],
                  targetTouches: terminal ? [] : [t],
                  changedTouches: [t],
                  bubbles: true,
                  cancelable: true,
                });
              };
              const pointerEvent = (type: string, x: number) =>
                new PointerEvent(type, {
                  pointerId: 1,
                  pointerType: "touch",
                  isPrimary: true,
                  clientX: x,
                  clientY: start.y,
                  bubbles: true,
                  cancelable: true,
                });

              // Build the complete sequence before the first event. A retry
              // can therefore happen only on the precondition above, never
              // after a partial gesture has reached the page.
              const pointerDown = pointerEvent("pointerdown", start.x);
              const touchStart = touchEvent("touchstart", start.x);
              const steps = 6;
              const moves = Array.from({ length: steps }, (_, index) => {
                const x = start.x + ((dx as number) * (index + 1)) / steps;
                return {
                  pointer: pointerEvent("pointermove", x),
                  touch: touchEvent("touchmove", x),
                };
              });
              const end = start.x + (dx as number);
              const pointerEnd = pointerEvent(
                canceled ? "pointercancel" : "pointerup",
                end,
              );
              const touchEnd = touchEvent(
                canceled ? "touchcancel" : "touchend",
                end,
              );

              el.dispatchEvent(pointerDown);
              el.dispatchEvent(touchStart);
              for (const move of moves) {
                window.dispatchEvent(move.pointer);
                el.dispatchEvent(move.touch);
              }
              window.dispatchEvent(pointerEnd);
              el.dispatchEvent(touchEnd);
              return true;
            },
            [expectedLabel, dx, canceled] as const,
          );
        } catch {
          return false;
        }
      },
      {
        timeout: 10_000,
        message: "current parked card accepts the swipe",
      },
    )
    .toBe(true);
}

/**
 * Live terminal count, from the status bar's `{count}T`.
 *
 * The distinguishing signal between the two ways a card can leave the dock: a
 * dismiss closes its session, a drag merely displays it somewhere. Asserting
 * only that the card is gone would pass for either, which is how a first
 * version of this test survived making the bridge steal the swipe.
 */
async function terminalCount(page: Page) {
  // The status bar's menu button, not merely the first button on the page —
  // the left dock's gear comes earlier in the DOM.
  const text = await page.locator('button[title="Menu"]').first().innerText();
  const m = /(\d+)T/.exec(text);
  if (!m) throw new Error(`no terminal count in ${JSON.stringify(text)}`);
  return Number(m[1]);
}

async function ensurePreviewPanelOpen(page: Page) {
  const panel = page.locator("[data-yas-preview-panel]");
  if ((await panel.count()) === 0) {
    await page.locator('[data-status-tool="preview"]').click();
  }
  await expect(panel).toBeVisible({ timeout: 10_000 });
}

/** Park one terminal, then fill the standalone view with another. */
async function parkedCard(page: Page) {
  const initialCount = await terminalCount(page);
  await newTerminal(page);
  await page.keyboard.press("Control+b");
  await page.keyboard.press("q");
  await newTerminal(page);
  await ensurePreviewPanelOpen(page);
  const card = page
    .locator('[data-yas-preview-panel] [draggable="true"]:visible')
    .first();
  // Opening and sizing the preview can legitimately RESET its catalogue and
  // replace this card. Discover one current card here; the gesture helpers
  // independently retry their preconditions and dispatch atomically.
  let label = "";
  await expect
    .poll(
      async () => {
        try {
          const current = await card.evaluate((element) => {
            const card = element as HTMLElement;
            const box = card.getBoundingClientRect();
            const label = card.innerText.trim();
            return card.isConnected && box.width > 0 && box.height > 0 && label
              ? label
              : null;
          });
          if (!current) return false;
          label = current;
          return true;
        } catch {
          return false;
        }
      },
      {
        timeout: 10_000,
        message: "a current parked card is visible",
      },
    )
    .toBe(true);
  if (!label) throw new Error("parked card has no label");
  return { card, label, expectedCount: initialCount + 2 };
}

test.describe("Touch drag on list rows", () => {
  test.beforeEach(closeAllTerminals);
  test.afterAll(closeAllTerminals);

  test("holding a dock card drags it into the main view", async ({ page }) => {
    await authenticate(page);
    const { card, label } = await parkedCard(page);
    expect(label.length).toBeGreaterThan(0);

    const cardAt = await touchDown(card, label);
    const mainAt = { x: 400, y: 350 };

    // Hold still: movement here would be read as a scroll or a swipe.
    await page.waitForTimeout(HOLD_MS);
    await touchWindow(page, "pointermove", {
      x: cardAt.x - 60,
      y: cardAt.y,
    });
    await touchWindow(page, "pointermove", mainAt);
    await touchWindow(page, "pointerup", mainAt);
    await page.waitForTimeout(700);

    // It moved into the main view, so the dock no longer lists that label.
    const parked = await page
      .locator('[data-yas-preview-panel] [draggable="true"]')
      .allInnerTexts();
    expect(parked.map((t) => t.trim())).not.toContain(label);
  });

  test("a swipe across a dock card still dismisses it", async ({ page }) => {
    await authenticate(page);
    const { card, label, expectedCount } = await parkedCard(page);
    const selector = '[data-yas-preview-panel] [draggable="true"]';

    // Moving straight away, with no hold: the gesture belongs to the card,
    // not to the drag bridge. Past SWIPE_THRESHOLD and horizontal, so it is
    // unambiguously a dismiss.
    await swipe(card, label, 160);
    await page.waitForTimeout(800);

    // Gone from the dock *and* closed. The second half is what makes this a
    // test of the swipe rather than of the drag: a card the bridge stole and
    // dropped somewhere would also leave the dock, but its session would
    // still be running.
    await expect
      .poll(async () => {
        const parked = await page.locator(selector).allInnerTexts();
        return parked.map((t) => t.trim());
      })
      .not.toContain(label);
    await expect.poll(() => terminalCount(page)).toBe(expectedCount - 1);
  });

  test("a cancelled dock-card swipe does not dismiss it", async ({ page }) => {
    await authenticate(page);
    const { card, label, expectedCount } = await parkedCard(page);
    const selector = '[data-yas-preview-panel] [draggable="true"]';

    await swipe(card, label, 160, true);
    await page.waitForTimeout(500);

    await expect
      .poll(async () => {
        const parked = await page.locator(selector).allInnerTexts();
        return parked.map((text) => text.trim());
      })
      .toContain(label);
    await expect.poll(() => terminalCount(page)).toBe(expectedCount);
  });
});
