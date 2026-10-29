import { expect, test } from "@playwright/test";

test("expands the capabilities panel to the current editorial width while scrolling", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1051, height: 946 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const panel = page.locator("[data-capabilities-frame]");
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("tab", { name: /Webløsninger/ })).toBeEnabled({
    timeout: 20_000,
  });

  const initial = await panel.boundingBox();
  expect(initial?.width).toBeLessThan(1051 * 0.92 - 100);

  await panel.scrollIntoViewIfNeeded();
  await expect
    .poll(async () => Math.round((await panel.boundingBox())!.width), {
      timeout: 10_000,
    })
    .toBe(Math.round(1051 * 0.92));
  await expect
    .poll(async () => Math.round((await panel.boundingBox())!.height), {
      timeout: 10_000,
    })
    .toBeGreaterThan(Math.round(initial!.height) + 30);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth))
    .toBe(1051);
});

test("keeps the same readable panel with reduced motion", async ({ page }) => {
  const hydrationErrors: string[] = [];
  page.on("console", (message) => {
    if (
      message.type() === "error" &&
      /Hydration failed|didn't match the client/.test(message.text())
    ) {
      hydrationErrors.push(message.text());
    }
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 390, height: 600 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const panel = page.locator("[data-capabilities-frame]");
  await expect(panel).toBeVisible();
  await expect
    .poll(async () => Math.round((await panel.boundingBox())!.width))
    .toBe(358);
  await expect(panel.getByRole("tab", { name: /Webløsninger/ })).toBeVisible();
  await expect(panel.getByRole("tab", { name: /Webløsninger/ })).toBeEnabled({
    timeout: 20_000,
  });
  expect(hydrationErrors).toEqual([]);
});
