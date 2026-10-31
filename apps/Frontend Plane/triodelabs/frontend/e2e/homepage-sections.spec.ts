import { expect, test } from "@playwright/test";

test("centers the studio introduction headline above its divider", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const technologies = page.locator("#teknologier");
  const introduction = page.locator("#introduksjon");
  const headline = introduction.getByRole("heading", {
    name: "Dere setter retningen. Vi gjør den digital.",
  });
  const firstParagraph = introduction.getByText(
    "TriodeLab er et digitalt studio for virksomheter som trenger mer enn en pen overflate. Vi samler strategi, design og teknologi i løsninger som er tydelige å bruke og enkle å eie.",
  );
  const divider = introduction.locator("div.border-t").first();

  await expect(technologies).toBeVisible();
  await expect(introduction).toBeVisible();
  await expect(headline).toBeVisible();
  await expect(firstParagraph).toBeVisible();
  await expect(divider).toBeVisible();
  await expect(
    introduction.getByText("Introduksjon", { exact: true }),
  ).toHaveCount(0);
  await expect(
    technologies.evaluate((element) => element.nextElementSibling?.id),
  ).resolves.toBe("introduksjon");

  const [introductionBox, headlineBox, dividerBox] = await Promise.all([
    introduction.boundingBox(),
    headline.boundingBox(),
    divider.boundingBox(),
  ]);

  expect(introductionBox).not.toBeNull();
  expect(headlineBox).not.toBeNull();
  expect(dividerBox).not.toBeNull();

  if (!introductionBox || !headlineBox || !dividerBox) {
    throw new Error("The studio introduction content could not be measured.");
  }

  expect(
    Math.abs(
      headlineBox.x +
        headlineBox.width / 2 -
        (introductionBox.x + introductionBox.width / 2),
    ),
  ).toBeLessThan(1);
  expect(headlineBox.y + headlineBox.height).toBeLessThan(dividerBox.y);
});

test("uses the warm beige surface across every homepage section and footer", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const homepageSurfaces = [
    "#hero",
    "#teknologier",
    "#introduksjon",
    "#tjenester",
    "#prosjekter",
    "#faq",
    "section[aria-labelledby='project-start-heading']",
    "#footer",
  ];
  const colors = await Promise.all(
    homepageSurfaces.map((selector) =>
      page
        .locator(selector)
        .evaluate((element) => getComputedStyle(element).backgroundColor),
    ),
  );

  expect(colors).toEqual(
    Array.from({ length: homepageSurfaces.length }, () => "rgb(252, 252, 250)"),
  );
});

test("replaces the former lower service cards with the product showcase", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await expect(page.locator("#kundearbeid")).toHaveCount(0);
  await expect(
    page.locator("#prosjekter").getByRole("heading", {
      name: "Field of Dreams",
    }),
  ).toBeVisible();
});
