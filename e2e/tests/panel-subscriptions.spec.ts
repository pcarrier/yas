import { test, expect, type Page } from "@playwright/test";
import { openReturningWorkspace } from "./workspace-auth";
import { closeAllTerminals, yas } from "./yas-cli";

/**
 * Closing a side panel drops the subscriptions only that panel needed.
 *
 * The oracle is the server's own client catalog (`yas client list`), not
 * anything the browser reports about itself: the point of the change is that
 * the server stops being asked for frames, and only the server can say
 * whether it still is. A parked terminal's thumbnail lives in the right-hand
 * preview panel, so with that panel closed its pty must not be subscribed.
 *
 * The terminals are created over the CLI rather than through the UI on
 * purpose. Creating one in the browser first asks *which remote* whenever the
 * developer running this has more than one configured, and that picker is not
 * what is under test.
 */

/** Terminal subscriptions per client row, e.g. ["1:80x24", "2:?"].
 *  `yas client list` filters its own short-lived connection, so the browser
 *  is the only row that can carry one. */
function subscribedTerminals(): string[] {
  const rows = yas("client", "list").trim().split("\n");
  const terminals: string[] = [];
  for (const row of rows) {
    // ID, AGE_S, OUT_BYTES_S, IN_BYTES_S, SUBSCRIPTIONS, TERMINALS, SURFACES
    const fields = row.split("\t");
    if (fields[0] === "ID") continue;
    const field = fields[5] ?? "";
    if (field) terminals.push(...field.split(","));
  }
  return terminals.sort();
}

async function open(page: Page) {
  await openReturningWorkspace(page);
  await expect(page.locator("canvas").first()).toBeVisible({
    timeout: 15_000,
  });
  // A previous run's overlay can be restored from storage and would sit over
  // the panel; Esc closes whichever one it is.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
}

async function setPreviewPanel(page: Page, open: boolean) {
  const panel = page.locator("[data-yas-preview-panel]");
  if ((await panel.count()) !== (open ? 1 : 0)) {
    await page.locator('[data-status-tool="preview"]').click();
  }
  if (open) await expect(panel).toBeVisible();
  else await expect(panel).toHaveCount(0);
  await page.waitForTimeout(500);
}

test.describe("side panel subscriptions", () => {
  test.afterAll(closeAllTerminals);

  test("a parked terminal is unsubscribed while the preview panel is closed", async ({
    page,
  }) => {
    closeAllTerminals();
    // New terminals split into panes, so park one explicitly after both are
    // visible to establish one live and one background subscription.
    yas("terminal", "start", "--", "cat");
    yas("terminal", "start", "--", "cat");

    // Preview panel open: both the focused pane and the parked card are
    // watching, so the server sees two terminal subscriptions.
    await open(page);
    await page
      .getByRole("button", { name: "Park", exact: true })
      .first()
      .click();
    await setPreviewPanel(page, true);
    await expect.poll(subscribedTerminals, { timeout: 15_000 }).toHaveLength(2);

    // Closed: preview-only terminal streams are gone with the panel. The live
    // terminal uses its foreground view path rather than this preview lease.
    await setPreviewPanel(page, false);
    await expect.poll(subscribedTerminals, { timeout: 15_000 }).toHaveLength(0);

    // Reopening resubscribes, so the drop is a lease and not a one-way latch.
    await setPreviewPanel(page, true);
    await expect.poll(subscribedTerminals, { timeout: 15_000 }).toHaveLength(2);
  });
});
