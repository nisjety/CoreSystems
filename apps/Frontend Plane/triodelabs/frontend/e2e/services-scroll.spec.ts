import { expect, test } from "@playwright/test";

for (const viewport of [
  { width: 1693, height: 1226 },
  { width: 390, height: 844 },
]) {
  test(`scrolls the page over the services panel at ${viewport.width}px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await expect
      .poll(
        () =>
          page.evaluate(() =>
            document.documentElement.classList.contains("lenis-smooth"),
          ),
        { timeout: 20_000 },
      )
      .toBe(true);

    const section = page.locator("#tjenester");
    await expect(section).toBeVisible();
    await section.evaluate((element) => {
      window.scrollTo(0, element.getBoundingClientRect().top + window.scrollY);
    });

    const bounds = await section
      .getByRole("tab", { name: /Webløsninger/ })
      .boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) return;

    await page.mouse.move(
      bounds.x + bounds.width / 2,
      bounds.y + bounds.height / 2,
    );
    await page.waitForTimeout(300);
    const pointerIsOverPanel = await page.evaluate(
      ({ x, y }) =>
        document
          .elementFromPoint(x, y)
          ?.closest("[data-capabilities-frame]") !== null,
      { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 },
    );
    expect(pointerIsOverPanel).toBe(true);
    const before = await page.evaluate(() => window.scrollY);
    await page.mouse.wheel(0, 600);

    await expect
      .poll(() => page.evaluate(() => window.scrollY))
      .toBeGreaterThan(before + 50);
  });
}

test("expands the services panel within the page grid while scrolling", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1692, height: 1137 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#tjenester");
  await expect(section).toBeVisible();
  const frame = section.locator("[data-capabilities-frame]");
  const initialWidth = (await frame.boundingBox())?.width ?? 0;
  expect(initialWidth).toBeLessThan(1440);

  await frame.scrollIntoViewIfNeeded();
  await expect
    .poll(async () => Math.round((await frame.boundingBox())?.width ?? 0), {
      timeout: 10_000,
    })
    .toBe(1440);
  await expect
    .poll(async () => (await frame.boundingBox())?.x ?? 0)
    .toBeCloseTo((1692 - 1440) / 2, 0);
});
