import { expect, test, type Locator } from "@playwright/test";

async function expectLoadedServiceImage(image: Locator) {
  await image.scrollIntoViewIfNeeded();
  await expect(image).toHaveAttribute(
    "src",
    /\/_next\/image\?url=%2Fimages%2F/,
  );
}

test("loads optimized local photography for the selected services visuals", async ({
  page,
}) => {
  await page.goto("/tjenester", { waitUntil: "domcontentloaded" });

  await expectLoadedServiceImage(page.locator("#hero img"));
  await expectLoadedServiceImage(
    page.locator('section[aria-labelledby="services-approach-heading"] img'),
  );

  const editorialImages = page.locator(
    'section[aria-labelledby="services-images-heading"] img',
  );

  await expect(editorialImages).toHaveCount(3);
  for (const image of await editorialImages.all()) {
    await expectLoadedServiceImage(image);
  }
});
