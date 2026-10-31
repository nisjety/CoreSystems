import { expect, test, type Page } from "@playwright/test";

async function getHeroMediaInset(page: Page) {
  return page.locator(".hero-media").evaluate((element) => {
    const clipPath = getComputedStyle(element).clipPath;
    const match = clipPath.match(/^inset\(([\d.]+)%/);

    if (!match) {
      throw new Error(`Expected a percentage inset clip-path, received: ${clipPath}`);
    }

    return Number(match[1]);
  });
}

async function setScrollPosition(page: Page, top: number) {
  await page.evaluate(async (nextTop) => {
    window.scrollTo({ top: nextTop, behavior: "instant" });
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
  }, top);
}

test("scrubs the hero media directly in both scroll directions", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect
    .poll(() =>
      page.evaluate(() =>
        document.documentElement.classList.contains("lenis-smooth"),
      ),
    )
    .toBe(true);

  const initialInset = await getHeroMediaInset(page);
  await setScrollPosition(page, 240);
  const middleInset = await getHeroMediaInset(page);

  expect(middleInset).toBeLessThan(initialInset - 15);
  await expect(page.locator("header[data-site-nav]")).toHaveCSS(
    "background-color",
    "rgb(252, 252, 250)",
  );
  await expect(page.locator("header[data-site-nav]")).toHaveCSS(
    "box-shadow",
    "none",
  );

  const stickyHandoff = await page.locator("#hero").evaluate((element) =>
    Math.round(element.getBoundingClientRect().height - window.innerHeight),
  );
  await setScrollPosition(page, stickyHandoff);
  expect(await getHeroMediaInset(page)).toBeGreaterThan(5);

  await setScrollPosition(page, 0);
  expect(await getHeroMediaInset(page)).toBeCloseTo(initialInset, 0);
});
