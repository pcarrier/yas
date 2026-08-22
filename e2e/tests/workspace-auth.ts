import { expect, type Page } from "@playwright/test";

const RESET_GUARD = "yas-e2e-storage-reset-complete";

/** Start a browser context with clean device-local state before YAS boots. */
export async function openFreshWorkspace(page: Page): Promise<void> {
  await page.addInitScript((guard) => {
    if (sessionStorage.getItem(guard) === "1") return;
    localStorage.clear();
    sessionStorage.setItem(guard, "1");
  }, RESET_GUARD);
  await page.goto("/#psk=test-secret");
}

/**
 * Re-enter an authenticated workspace as a returning visitor. The canonical
 * workspace proves first-boot device/session claiming completed before the
 * reload, while the sessionStorage guard preserves that device on the reload.
 */
export async function openReturningWorkspace(
  page: Page,
  timeout = 15_000,
): Promise<void> {
  await openFreshWorkspace(page);
  await expect(page.getByRole("status", { name: "Connected" })).toBeVisible({
    timeout,
  });
  await page.reload();
}

/**
 * Open the switcher.
 *
 * YAS reserves one chord — Ctrl+B — and everything else is a key behind it,
 * so reaching the switcher is two keystrokes rather than one.
 */
export async function openSwitcher(page: Page): Promise<void> {
  await page.keyboard.press("Control+b");
  await page.keyboard.press("k");
}
