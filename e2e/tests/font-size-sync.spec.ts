import { expect, test, type Page } from "@playwright/test";

const PASSPHRASE = process.env.YAS_PASSPHRASE ?? "test-secret";
const FONT_BUTTON = 'button[title^="Font:"]';

async function fontButtonSize(page: Page): Promise<string> {
  return page
    .locator(FONT_BUTTON)
    .evaluate((el) => getComputedStyle(el).fontSize);
}

test("font size preview stays local and Apply persists per browser", async ({
  browser,
}) => {
  const baseURL = test.info().project.use.baseURL as string;
  const firstContext = await browser.newContext();
  const secondContext = await browser.newContext();
  let first: Page | undefined;
  let originalValue: string | undefined;

  try {
    first = await firstContext.newPage();
    const second = await secondContext.newPage();
    await Promise.all([
      first.goto(`${baseURL}/#psk=${encodeURIComponent(PASSPHRASE)}`),
      second.goto(`${baseURL}/#psk=${encodeURIComponent(PASSPHRASE)}`),
    ]);

    const firstFontButton = first.locator(FONT_BUTTON);
    const secondFontButton = second.locator(FONT_BUTTON);
    await expect(firstFontButton).toBeVisible();
    await expect(secondFontButton).toBeVisible();

    const original = await fontButtonSize(second);
    await firstFontButton.click();
    const sizeInput = first.locator('input[name="yas-font-size"]');
    originalValue = await sizeInput.inputValue();
    const previewValue = originalValue === "22" ? "20" : "22";
    const previewPixels = Number.parseInt(previewValue, 10);
    await sizeInput.fill(previewValue);

    await expect.poll(() => fontButtonSize(first)).toBe(`${previewPixels}px`);
    await second.waitForTimeout(300);
    expect(await fontButtonSize(second)).toBe(original);

    await first.getByRole("button", { name: "Apply", exact: true }).click();
    await first.reload();
    await expect(first.locator(FONT_BUTTON)).toBeVisible();
    await expect.poll(() => fontButtonSize(first)).toBe(`${previewPixels}px`);

    // Appearance preferences are device-local. A separate browser context
    // has separate localStorage and must not inherit this one's choice.
    await second.waitForTimeout(300);
    expect(await fontButtonSize(second)).toBe(original);
  } finally {
    // Do not leave the shared developer/e2e config changed after the test.
    if (first && originalValue) {
      try {
        const sizeInput = first.locator('input[name="yas-font-size"]');
        if (!(await sizeInput.isVisible())) {
          await first.locator(FONT_BUTTON).click();
        }
        await sizeInput.fill(originalValue);
        await first.getByRole("button", { name: "Apply", exact: true }).click();
      } catch {}
    }
    await firstContext.close();
    await secondContext.close();
  }
});
