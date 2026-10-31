import { expect, test } from "@playwright/test";

const desktopWidths = [1024, 1440];

for (const width of desktopWidths) {
  test(`keeps the header and services dropdown on one surface at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "domcontentloaded" });

    const servicesTrigger = page.getByRole("button", {
      name: "Tjenester",
      exact: true,
    });
    await expect(servicesTrigger).toBeVisible();
    await expect
      .poll(
        async () => {
          if (
            (await servicesTrigger.getAttribute("aria-expanded")) === "true"
          ) {
            return true;
          }

          await servicesTrigger.click();
          return false;
        },
        { timeout: 15_000 },
      )
      .toBe(true);

    const header = page.locator("header[data-site-nav]");
    const dropdown = page.getByRole("region", { name: "Tjenester-meny" });

    await expect(dropdown).toBeVisible();
    await expect(
      dropdown.locator('a[href="/tjenester/integrasjoner"] img'),
    ).toHaveAttribute(
      "src",
      /images%2FIntegrasjoner\.jpg/,
    );

    await expect
      .poll(async () => {
        const [headerBackground, dropdownBackground] = await Promise.all([
          header.evaluate(
            (element) => getComputedStyle(element).backgroundColor,
          ),
          dropdown.evaluate(
            (element) => getComputedStyle(element).backgroundColor,
          ),
        ]);

        return headerBackground === dropdownBackground;
      })
      .toBe(true);

    const headerBackground = await header.evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    );
    expect(headerBackground).toBe("rgb(252, 252, 250)");

    const [
      headerBorder,
      headerShadow,
      dropdownTopBorder,
      headerBottom,
      dropdownTop,
    ] = await Promise.all([
      header.evaluate((element) => getComputedStyle(element).borderBottomWidth),
      header.evaluate((element) => getComputedStyle(element).boxShadow),
      dropdown.evaluate((element) => getComputedStyle(element).borderTopWidth),
      header.evaluate((element) => element.getBoundingClientRect().bottom),
      dropdown.evaluate((element) => element.getBoundingClientRect().top),
    ]);

    expect(headerBorder).toBe("0px");
    expect(headerShadow).toBe("none");
    expect(dropdownTopBorder).toBe("0px");
    expect(dropdownTop).toBeCloseTo(headerBottom, 3);
  });
}

test("switches to the compact menu below the desktop navigation breakpoint", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1023, height: 900 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const header = page.locator("header[data-site-nav]");
  await expect(
    header.getByRole("navigation", { name: "Hovednavigasjon" }),
  ).toBeHidden();
  await expect(header.getByRole("button", { name: "Åpne meny" })).toBeVisible();
});

test("exposes consulting directly, groups products, and opens a prepared email", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const navigation = page.getByRole("navigation", {
    name: "Hovednavigasjon",
  });
  await expect(
    navigation.getByRole("link", { name: "Rådgivning" }),
  ).toHaveAttribute("href", "/tjenester/radgivning-og-konsulentbistand");
  await expect(
    navigation.getByRole("button", { name: "Produkter" }),
  ).toBeVisible();
  await expect(
    page.getByLabel("Kontakt", { exact: true }),
  ).toHaveAttribute(
    "href",
    "mailto:post@triodelab.no?subject=Foresp%C3%B8rsel%20fra%20triodelab.no&body=Hei%20TriodeLab%2C%0A%0A",
  );
});
