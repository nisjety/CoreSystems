import { expect, test } from "@playwright/test";

test("consultancy is discoverable and keeps the contact intent", async ({
  page,
}) => {
  await page.goto("/tjenester");
  await page
    .getByRole("link", {
      name: "Rådgivning og konsulentbistand for deres team",
    })
    .click();
  await expect(page).toHaveURL(/\/tjenester\/radgivning-og-konsulentbistand$/);
  await expect(page.getByRole("heading", { level: 1 })).toContainText(
    "Rådgivning",
  );
  await expect(
    page.getByRole("heading", { name: "Dette kan inngå i leveransen." }),
  ).toBeVisible();
  await expect(
    page.locator(
      "link[rel='canonical'][href='https://www.triodelab.no/tjenester/radgivning-og-konsulentbistand']",
    ),
  ).toHaveCount(1);
  await expect(
    page
      .locator("article")
      .getByRole("link", { name: "Kontakt oss", exact: true }),
  ).toHaveAttribute("href", "/kontakt?tjeneste=radgivning-og-konsulentbistand");
  await page
    .getByText("Kan dere jobbe sammen med vårt eksisterende utviklingsteam?", {
      exact: true,
    })
    .click();
  await expect(
    page.getByText("Ja. Vi kan gå inn i et avtalt arbeidsområde", {
      exact: false,
    }),
  ).toBeVisible();
});

test("KI explains an illustrative workflow and links to related services and products", async ({
  page,
}) => {
  await page.goto("/tjenester/ki-og-automatisering");
  await expect(
    page.getByRole("heading", {
      name: "En arbeidsflyt med mennesker i beslutningen.",
    }),
  ).toBeVisible();
  await expect(
    page.getByText("Dette er et eksempel på hvordan en pilot kan avgrenses.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Utforsk Agenci", exact: true }),
  ).toHaveAttribute("href", "https://agenci.no/");
  await expect(
    page.getByRole("link", { name: "Utforsk Verevon", exact: true }),
  ).toHaveAttribute("href", "https://www.verevon.com/");
  await page
    .locator("article")
    .getByRole("link", { name: "systemintegrasjoner", exact: true })
    .click();
  await expect(page).toHaveURL(/\/tjenester\/integrasjoner$/);
  await expect(
    page.getByRole("heading", { name: "Dette kan inngå i leveransen." }),
  ).toBeVisible();
});
