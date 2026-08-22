import { readFileSync } from "node:fs";
import path from "node:path";
import { test, expect } from "@playwright/test";

// Exercise the actual document shell without connecting to a workspace. The
// iOS outer-scroll lock must hold during loading and authentication too.
const shell = readFileSync(
  path.resolve(__dirname, "../../js/ui/index.html"),
  "utf8",
).replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, "");

test.use({ hasTouch: true, isMobile: true });

for (const viewport of [
  { width: 820, height: 1180 },
  { width: 1180, height: 820 },
]) {
  test(`document stays pinned with scrollable panes at ${viewport.width}x${viewport.height}`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.setContent(shell);
    await page.evaluate(() => {
      const root = document.getElementById("root")!;
      // Include a safe-area inset and deliberately overflowing content: even
      // focus/scrollIntoView must not move the shell and hide its tab bar.
      document.documentElement.style.setProperty(
        "--yas-system-bar-inset-top",
        "24px",
      );
      root.innerHTML = `
        <nav style="height: 40px">Workspace tabs</nav>
        <div id="pane" style="height: 200px; overflow: auto">
          <div style="height: 2000px"><button id="pane-end" style="margin-top: 1900px">End</button></div>
        </div>
        <div style="height: 2000px"><button id="root-end" style="margin-top: 1900px">Overflow</button></div>
      `;
    });

    const tabs = page.getByRole("navigation");
    const before = await tabs.boundingBox();
    expect(before?.y).toBe(24);

    await page.evaluate(() => {
      document.getElementById("root-end")!.focus();
      document.getElementById("root-end")!.scrollIntoView();
      for (const element of [
        document.documentElement,
        document.body,
        document.getElementById("root")!,
      ]) {
        element.scrollTop = 400;
      }
      window.scrollTo(0, 400);
    });

    expect(await tabs.boundingBox()).toEqual(before);
    expect(
      await page.evaluate(() => [
        window.scrollY,
        document.documentElement.scrollTop,
        document.body.scrollTop,
        document.getElementById("root")!.scrollTop,
      ]),
    ).toEqual([0, 0, 0, 0]);
    // iOS can move the visual viewport even without DOM overflow. Pinning
    // the body is required in addition to clipping its scroll containers.
    await expect(page.locator("body")).toHaveCSS("position", "fixed");

    await page.locator("#pane-end").evaluate((button) => {
      button.scrollIntoView();
    });
    expect(
      await page.locator("#pane").evaluate((pane) => pane.scrollTop),
    ).toBeGreaterThan(0);
    expect(await tabs.boundingBox()).toEqual(before);

    await page.setViewportSize({ width: viewport.width, height: 500 });
    await expect(page.locator("#root")).toHaveCSS("height", "500px");
    expect(await tabs.boundingBox()).toEqual(before);
  });
}
