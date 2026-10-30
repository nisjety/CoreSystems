import { expect, test } from "@playwright/test";

test("the insight hub leads visitors to an indexable integration guide", async ({
  page,
}) => {
  await page.goto("/innsikt", { waitUntil: "domcontentloaded" });

  await expect(
    page.getByRole("heading", {
      level: 1,
      name: "Tydelige valg før dere bygger videre.",
    }),
  ).toBeVisible();
  await expect(page.locator('a[href="#main-content"]')).toHaveCount(1);
  await expect(page.locator("#main-content")).toHaveCount(1);

  await page
    .locator('a[href="/innsikt/systemintegrasjon-for-nettbutikk"]')
    .click();
  await expect(page).toHaveURL(/\/innsikt\/systemintegrasjon-for-nettbutikk$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Systemintegrasjon for nettbutikk.",
  );
  await expect(page.locator("#main-content")).toHaveCount(1);
  await expect(
    page.locator(
      'link[rel="canonical"][href="https://www.triodelab.no/innsikt/systemintegrasjon-for-nettbutikk"]',
    ),
  ).toHaveCount(1);
  const structuredData = await page
    .locator('script[type="application/ld+json"]')
    .allTextContents();
  expect(
    structuredData.some((data) => data.includes('"@type":"Article"')),
  ).toBe(true);
  await expect(
    page.getByRole("heading", { name: /ki-agenter skal bruke integrasjonen/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", {
      name: /videre lesing: Google Cloud: AI agent trends 2026/i,
    }),
  ).toHaveAttribute(
    "href",
    "https://cloud.google.com/resources/content/ai-agent-trends-2026",
  );
});

test("the integrations service links to its related decision guide", async ({
  page,
}) => {
  await page.goto("/tjenester/integrasjoner", {
    waitUntil: "domcontentloaded",
  });

  await page.getByRole("link", { name: /les guiden/i }).click();
  await expect(page).toHaveURL(/\/innsikt\/systemintegrasjon-for-nettbutikk$/);
});

test("the KI guide is indexable and connected to the KI service", async ({
  page,
}) => {
  await page.goto("/innsikt/ki-pilot-for-bedrifter", {
    waitUntil: "domcontentloaded",
  });

  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "KI-pilot for bedrifter.",
  );
  await expect(
    page.locator(
      'link[rel="canonical"][href="https://www.triodelab.no/innsikt/ki-pilot-for-bedrifter"]',
    ),
  ).toHaveCount(1);
  await expect(
    page.getByRole("heading", { name: /sjekkliste/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: /vanlige spørsmål/i }),
  ).toBeVisible();
  await expect(
    page.getByText("Hvilken oppgave er egnet som første KI-pilot?"),
  ).toBeVisible();

  await page.goto("/tjenester/ki-og-automatisering", {
    waitUntil: "domcontentloaded",
  });
  await expect(
    page.locator('a[href="/innsikt/ki-pilot-for-bedrifter"]'),
  ).toHaveCount(1);
});

test("the paid-media article is indexable and links to the ads service", async ({
  page,
}) => {
  await page.goto("/innsikt/google-meta-tiktok-snapchat-annonsering", {
    waitUntil: "domcontentloaded",
  });

  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Annonsering med mennesker, KI og målbar verdi.",
  );
  await expect(
    page.locator(
      'link[rel="canonical"][href="https://www.triodelab.no/innsikt/google-meta-tiktok-snapchat-annonsering"]',
    ),
  ).toHaveCount(1);
  await expect(
    page.getByRole("heading", { name: /KI-innhold må også tåle/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", {
      name: /kilde: Google: AI transparency labels in Ads/i,
    }),
  ).toHaveAttribute(
    "href",
    "https://blog.google/products/ads-commerce/google-ads-ai-transparency-labels/",
  );
  await expect(
    page.getByRole("link", {
      name: /kilde: TikTok: Symphony Creative Studio/i,
    }),
  ).toHaveAttribute(
    "href",
    "https://ads.tiktok.com/business/en-US/blog/tiktok-symphony-ai-creative-suite",
  );
  await expect(
    page.locator("article").locator('a[href="/tjenester/google-og-meta-ads"]'),
  ).toHaveCount(1);
});

test("the ads service links back to its related paid-media article", async ({
  page,
}) => {
  await page.goto("/tjenester/google-og-meta-ads", {
    waitUntil: "domcontentloaded",
  });

  await expect(
    page.locator('a[href="/innsikt/google-meta-tiktok-snapchat-annonsering"]'),
  ).toHaveCount(1);
});
