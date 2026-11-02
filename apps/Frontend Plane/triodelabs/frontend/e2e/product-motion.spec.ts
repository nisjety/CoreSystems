import { expect, test, type Page } from "@playwright/test";

async function openProductShowcase(page: Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#prosjekter");
  await section.scrollIntoViewIfNeeded();
  await expect(section).toBeVisible();
  await expect(
    section.getByRole("tab", { name: "Agenci", exact: true }),
  ).toBeEnabled({ timeout: 20_000 });

  return section;
}

async function selectProduct(
  section: ReturnType<Page["locator"]>,
  name: "Agenci" | "Verevon",
) {
  const tab = section.getByRole("tab", { name, exact: true });

  await expect
    .poll(
      async () => {
        if ((await tab.getAttribute("aria-selected")) === "true") {
          return true;
        }

        await tab.click();
        return false;
      },
      { timeout: 15_000 },
    )
    .toBe(true);
}

test("shows an orbit card and swaps its artwork with the selected product", async ({
  page,
}) => {
  const section = await openProductShowcase(page);
  const stage = section.getByTestId("product-orbit-stage");
  const foregroundCard = stage.getByTestId("product-orbit-card");

  await stage.scrollIntoViewIfNeeded();
  await expect(stage).toBeVisible();
  await expect(foregroundCard).toBeVisible();
  const trail = stage.getByTestId("product-orbit-trail").first();
  await expect(trail).toBeVisible();
  await expect
    .poll(
      () =>
        trail.evaluate(
          (element) => getComputedStyle(element).animationPlayState,
        ),
      { timeout: 10_000 },
    )
    .toBe("running");
  const initialTrailTransform = await trail.evaluate(
    (element) => getComputedStyle(element).transform,
  );
  await expect
    .poll(() =>
      trail.evaluate((element) => getComputedStyle(element).transform),
    )
    .not.toBe(initialTrailTransform);
  await expect(foregroundCard.locator("img")).toHaveAttribute(
    "src",
    /agenci-macbook-mockup\.jpg/,
  );

  await selectProduct(section, "Verevon");

  await expect(section.getByRole("tab", { name: "Verevon" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(foregroundCard.locator("img")).toHaveAttribute(
    "src",
    /verevon-hover-mockup\.jpg/,
  );
  await expect(
    section.getByRole("link", { name: "Utforsk Verevon" }),
  ).toBeVisible();

  await selectProduct(section, "Agenci");
  await expect(foregroundCard.locator("img")).toHaveAttribute(
    "src",
    /agenci-macbook-mockup\.jpg/,
  );
});

test("stops image movement when reduced motion is requested", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const section = await openProductShowcase(page);
  const stage = section.getByTestId("product-orbit-stage");

  await expect(stage.getByTestId("product-orbit-card")).toBeVisible();
  await selectProduct(section, "Verevon");
  await expect(
    stage.getByTestId("product-orbit-card").locator("img"),
  ).toHaveAttribute("src", /verevon-hover-mockup\.jpg/);

  await expect
    .poll(() =>
      stage.evaluate(
        (element) =>
          element
            .getAnimations({ subtree: true })
            .filter((animation) => animation.playState === "running").length,
      ),
    )
    .toBe(0);
});
