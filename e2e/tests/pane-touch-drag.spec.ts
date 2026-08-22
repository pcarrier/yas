import { test, expect, type Page, type CDPSession } from "@playwright/test";
import { closeAllTerminals } from "./yas-cli";

/**
 * The grip must be draggable with a finger, not only a mouse.
 *
 * `startPanePointerDrag` bridges pointer events to the same DragEvents the
 * pane and park targets listen for.
 *
 * Send browser touch input so pointer capture, touch defaults and compatibility
 * mouse events participate. dispatchEvent alone bypasses those failure modes.
 */
test.use({
  hasTouch: true,
  isMobile: true,
  viewport: { width: 900, height: 700 },
});

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

/**
 * One touch step at a time, so the test can look at the page mid-drag.
 *
 * Chromium generates the actual pointer and touch event sequences. The pen
 * guard test only needs a synthetic event to assert that the bridge ignores it.
 */
const touchSessions = new WeakMap<Page, Promise<CDPSession>>();
async function touch(
  page: Page,
  type: "pointerdown" | "pointermove" | "pointerup",
  at: { x: number; y: number },
  pointerType: "touch" | "pen" = "touch",
) {
  if (pointerType === "touch") {
    let session = touchSessions.get(page);
    if (!session) {
      session = page.context().newCDPSession(page);
      touchSessions.set(page, session);
    }
    await (
      await session
    ).send("Input.dispatchTouchEvent", {
      type: {
        pointerdown: "touchStart",
        pointermove: "touchMove",
        pointerup: "touchEnd",
      }[type],
      touchPoints: type === "pointerup" ? [] : [{ ...at, id: 1 }],
    });
    return;
  }
  await page.evaluate(
    ([type, at, pointerType]) => {
      const ev = new PointerEvent(type as string, {
        pointerId: 1,
        pointerType: pointerType as string,
        isPrimary: true,
        clientX: (at as { x: number }).x,
        clientY: (at as { y: number }).y,
        bubbles: true,
        cancelable: true,
      });
      if (type === "pointerdown") {
        const el = document.elementFromPoint(
          (at as { x: number }).x,
          (at as { y: number }).y,
        );
        if (!el) throw new Error("nothing under the finger");
        el.dispatchEvent(ev);
      } else {
        window.dispatchEvent(ev);
      }
    },
    [type, at, pointerType] as const,
  );
}

async function centerOf(page: Page, selector: string) {
  const box = await page.locator(selector).first().boundingBox();
  if (!box) throw new Error(`no box for ${selector}`);
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

function towardViewportCenter(
  page: Page,
  at: { x: number; y: number },
  distance: number,
) {
  const viewport = page.viewportSize();
  if (!viewport) throw new Error("page has no viewport");
  return {
    x: at.x < viewport.width / 2 ? at.x + distance : at.x - distance,
    y: at.y < viewport.height / 2 ? at.y + distance : at.y - distance,
  };
}

test.describe("Grip drag with a finger", () => {
  test.beforeEach(closeAllTerminals);
  test.afterAll(closeAllTerminals);

  test("dragging the grip to the dock parks the pane", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    // On touch the toolbar is always shown — there is no hover to reveal it.
    const grip = page.getByRole("button", { name: "Drag to move" }).first();
    await expect(grip).toBeVisible();

    const gripAt = await centerOf(page, "button[title^='Drag to move']");
    await touch(page, "pointerdown", gripAt);
    // Two moves: the first crosses the drag threshold and starts the drag,
    // which is what reveals the dock — it is not in the DOM before that, so
    // it can only be measured now, not aimed at up front.
    // Move inward: the toolbar sits close to a viewport edge, and an
    // out-of-bounds browser touch is cancelled instead of becoming a drag.
    await touch(page, "pointermove", towardViewportCenter(page, gripAt, 40));
    await touch(page, "pointermove", towardViewportCenter(page, gripAt, 80));
    const dockAt = await centerOf(page, "[data-yas-preview-panel]");
    await touch(page, "pointermove", dockAt);
    await touch(page, "pointerup", dockAt);
    await page.waitForTimeout(600);

    // Parked: the main view fell back to the empty pane.
    await expect(
      page.getByRole("button", { name: "New terminal" }).first(),
    ).toBeVisible({ timeout: 10_000 });
  });

  // Review catch on #141: the guard used to exclude only `mouse`, letting a
  // pen through. A pen drives native drag-and-drop in Chromium, so both paths
  // would run and this one's `dragend` would clear the in-flight count and
  // unmount the dock underneath the native drag.
  test("a pen is left to the native path", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    const gripAt = await centerOf(page, "button[title^='Drag to move']");
    // Aim where the dock lives. Asserted on the outcome rather than on the
    // dock being absent: an earlier test may have left something parked, so
    // the dock can be on screen for reasons of its own. Either way the bridge
    // is what would reveal it and drop on it, so "nothing moved" is the
    // signal that the pen was left alone.
    const rightEdge = { x: 880, y: 300 };
    await touch(page, "pointerdown", gripAt, "pen");
    await touch(
      page,
      "pointermove",
      { x: gripAt.x - 60, y: gripAt.y + 60 },
      "pen",
    );
    await touch(page, "pointermove", rightEdge, "pen");
    await touch(page, "pointerup", rightEdge, "pen");
    await page.waitForTimeout(600);

    // Still displayed: not parked, so the main view never fell back to the
    // empty pane.
    await expect(page.locator("canvas").first()).toBeVisible();
    await expect(
      page.getByRole("button", { name: "New terminal" }),
    ).toHaveCount(0);
  });

  test("a tap does not start a drag", async ({ page }) => {
    await authenticate(page);
    await newTerminal(page);

    const grip = page.getByRole("button", { name: "Drag to move" }).first();
    await expect(grip).toBeVisible();
    const before = (await grip.boundingBox())!;

    await grip.tap();
    await page.waitForTimeout(400);

    const after = (await grip.boundingBox())!;
    expect(after).toEqual(before);
    await expect(page.locator("canvas").first()).toBeVisible();
  });

  test("dragging over content leaves the tab-bar toolbar fixed", async ({
    page,
  }) => {
    await authenticate(page);
    await newTerminal(page);

    const grip = page.getByRole("button", { name: "Drag to move" }).first();
    const close = page
      .getByRole("button", { name: "Close", exact: true })
      .first();
    await expect(grip).toBeVisible();
    let initialBox: Awaited<ReturnType<typeof grip.boundingBox>> = null;
    await expect
      .poll(
        async () => {
          initialBox = await grip.boundingBox();
          return initialBox;
        },
        {
          timeout: 10_000,
          message: "grip should have an initial bounding box",
        },
      )
      .not.toBeNull();
    if (!initialBox) throw new Error("grip has no initial bounding box");
    const before = initialBox;
    const gripAt = await centerOf(page, "button[title^='Drag to move']");
    const target = await centerOf(page, "canvas");

    await touch(page, "pointerdown", gripAt);
    await touch(page, "pointermove", {
      x: gripAt.x - 40,
      y: gripAt.y + 40,
    });
    await touch(page, "pointermove", target);

    await touch(page, "pointerup", target);
    await page.waitForTimeout(400);
    expect(await grip.boundingBox()).toEqual(before);
    expect(await close.boundingBox()).not.toBeNull();
    await expect(page.locator("canvas").first()).toBeVisible();
  });
});
