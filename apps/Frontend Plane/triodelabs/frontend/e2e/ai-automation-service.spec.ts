import { expect, test } from "@playwright/test";

test("the homepage exposes KI and automation as the sixth selectable service", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const serviceTab = page.getByRole("tab", {
    name: /ki og automatisering/i,
  });
  await serviceTab.click();
  await expect(serviceTab).toHaveAttribute("aria-selected", "true");
  await page.getByRole("link", { name: /utforsk tjenesten/i }).click();

  await expect(page).toHaveURL(/\/tjenester\/ki-og-automatisering$/);
});

test("the sixth service is discoverable and opens its own KI page", async ({
  page,
}) => {
  await page.goto("/tjenester", { waitUntil: "domcontentloaded" });

  const serviceLink = page.getByRole("link", {
    name: /les om ki og automatisering/i,
  });
  await expect(serviceLink).toBeVisible();
  await serviceLink.click();

  await expect(page).toHaveURL(/\/tjenester\/ki-og-automatisering$/);
  await expect(page.getByRole("heading", { level: 1 })).toContainText("KI som");
  await expect(
    page.locator(
      'link[rel="canonical"][href="https://www.triodelab.no/tjenester/ki-og-automatisering"]',
    ),
  ).toHaveCount(1);
  await expect(
    page.getByText("Hva kan KI og automatisering brukes til i en bedrift?"),
  ).toBeVisible();
});

test("the services menu keeps the sixth item reachable on a narrow screen", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 780 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const menuTrigger = page.getByRole("button", { name: /meny/i }).first();
  await menuTrigger.click();

  const serviceLink = page
    .getByRole("link", { name: /ki og automatisering/i })
    .first();
  await expect(serviceLink).toBeVisible();
  await serviceLink.click();
  await expect(page).toHaveURL(/\/tjenester\/ki-og-automatisering$/);
});
