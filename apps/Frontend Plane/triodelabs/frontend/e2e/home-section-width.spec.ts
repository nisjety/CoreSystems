import { expect, test } from "@playwright/test";

const viewport = { width: 1692, height: 1137 };
const expectedWidth = viewport.width * 0.92;
const expectedLeft = viewport.width * 0.04;

test("keeps home-page content sections on the shared viewport frame", async ({
  page,
}) => {
  await page.setViewportSize(viewport);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const frames = [
    page.locator("#teknologier > div").first(),
    page.locator("#introduksjon > div"),
    page.locator("#prosjekter > div > header"),
    page.locator("#prosjekter > div > div").first(),
    page.locator("#faq > div"),
    page.locator('section[aria-labelledby="project-start-heading"] > div'),
  ];

  for (const frame of frames) {
    await frame.evaluate((element) => {
      element.scrollIntoView({ block: "center" });
    });
    await expect(frame).toBeVisible();

    const bounds = await frame.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) continue;

    expect(bounds.width).toBeCloseTo(expectedWidth, 0);
    expect(bounds.x).toBeCloseTo(expectedLeft, 0);
  }
});

test("keeps the shared home-page frame inside the mobile viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const frames = [
    page.locator("#teknologier > div").first(),
    page.locator("#introduksjon > div"),
    page.locator("#prosjekter > div > header"),
    page.locator("#faq > div"),
    page.locator('section[aria-labelledby="project-start-heading"] > div'),
  ];

  for (const frame of frames) {
    await frame.evaluate((element) => {
      element.scrollIntoView({ block: "center" });
    });
    await expect(frame).toBeVisible();

    const bounds = await frame.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) continue;

    expect(bounds.x).toBeCloseTo(24, 0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(366);
  }
});
