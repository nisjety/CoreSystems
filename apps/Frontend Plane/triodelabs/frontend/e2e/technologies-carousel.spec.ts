import { expect, test } from "@playwright/test";

test("keeps technologies in a single branded carousel rail", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#teknologier");
  await section.evaluate((element) => {
    element.scrollIntoView({ block: "center" });
  });

  await expect(section).toBeVisible();
  await expect(
    section.getByText("Vi jobber med teknologien som funker deg best."),
  ).toBeVisible();

  const carousel = section.locator('[aria-label="Teknologier"]');
  await expect(carousel).toBeVisible();
  await expect(carousel.locator("article")).toHaveCount(48);
  await expect(carousel.locator('article[aria-hidden="true"]')).toHaveCount(32);
  await expect(
    carousel.locator("article").first().locator("svg path"),
  ).toHaveAttribute("fill", "#61DAFB");

  const initialScroll = await carousel.evaluate(
    (element) => element.scrollLeft,
  );
  await expect
    .poll(() => carousel.evaluate((element) => element.scrollLeft))
    .toBeGreaterThan(initialScroll + 8);

  await section.getByRole("button", { name: "Pause bevegelse" }).click();
  const pausedAt = await carousel.evaluate((element) => element.scrollLeft);
  await page.waitForTimeout(400);
  expect(await carousel.evaluate((element) => element.scrollLeft)).toBeCloseTo(
    pausedAt,
    0,
  );
  await section.getByRole("button", { name: "Fortsett bevegelse" }).click();

  const beforeScroll = await carousel.evaluate((element) => element.scrollLeft);
  await carousel.evaluate((element) => {
    element.scrollBy({ left: 200, behavior: "auto" });
  });
  await expect
    .poll(() => carousel.evaluate((element) => element.scrollLeft))
    .toBeGreaterThan(beforeScroll + 100);
});
