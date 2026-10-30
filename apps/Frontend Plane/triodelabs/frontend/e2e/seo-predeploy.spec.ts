import { expect, test } from "@playwright/test";

const legalPaths = ["/personvern", "/cookies", "/vilkar", "/salgsvilkar"];

test("all sitemap pages respond with unique metadata and canonical URLs", async ({
  request,
}) => {
  const sitemap = await request.get("/sitemap.xml");
  expect(sitemap.ok()).toBeTruthy();
  const urls = [...(await sitemap.text()).matchAll(/<loc>(.*?)<\/loc>/g)].map(
    (match) => match[1],
  );
  expect(urls).toHaveLength(36);
  expect(urls).toContain("https://www.triodelab.no/innsikt");
  for (const slug of [
    "systemintegrasjon-for-nettbutikk",
    "ki-pilot-for-bedrifter",
    "google-meta-tiktok-snapchat-annonsering",
  ]) {
    expect(urls).toContain(`https://www.triodelab.no/innsikt/${slug}`);
  }
  const titles = new Set<string>();
  for (const url of urls) {
    const response = await request.get(new URL(url).pathname);
    expect(response.status(), url).toBe(200);
    const html = await response.text();
    expect(html, url).toContain(`rel="canonical" href="${url}"`);
    expect(html, url).toMatch(/<meta name="description" content="[^"]+"/);
    expect(html, url).not.toMatch(/name="robots" content="[^"]*noindex/);
    const title = html.match(/<title>(.*?)<\/title>/)?.[1];
    expect(title, url).toBeTruthy();
    expect(titles.has(title!), url).toBe(false);
    titles.add(title!);
    expect((html.match(/<h1(?:\s|>)/g) ?? []).length, url).toBe(1);
  }
  const robots = await request.get("/robots.txt");
  expect(await robots.text()).toContain("https://www.triodelab.no/sitemap.xml");
  const missing = await request.get("/side-som-ikke-finnes-seo-test");
  expect(missing.status()).toBe(404);
  expect(await missing.text()).toContain('content="noindex"');
});

test("legal routes and consultancy remain reachable from the mobile footer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.locator("#tjenester").getByRole("tab")).toHaveCount(6);
  const legalNavigation = page.getByRole("navigation", {
    name: "Personvern og vilkår",
  });
  for (const path of legalPaths) {
    await legalNavigation.locator(`a[href="${path}"]`).click();
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth + 1,
      ),
    ).toBeTruthy();
  }
  await page
    .locator('footer a[href="/tjenester/radgivning-og-konsulentbistand"]')
    .click();
  await expect(page.getByRole("heading", { level: 1 })).toContainText(
    "Rådgivning",
  );
  await page.locator('footer a[href="/innsikt"]').click();
  await expect(page).toHaveURL(/\/innsikt$/);
  await expect(page.getByText("Innsikt / Fagblogg")).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toContainText(
    "Tydelige valg",
  );
});

test("third-party chat is requested only after explicit activation", async ({
  page,
}) => {
  let requests = 0;
  await page.route("https://widget.agenci.no/**", async (route) => {
    requests += 1;
    await route.abort();
  });
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Aktiver chat fra Agenci" }),
  ).toBeVisible();
  expect(requests).toBe(0);
  await page.getByRole("button", { name: "Aktiver chat fra Agenci" }).click();
  const chatError = page
    .getByRole("alert")
    .filter({ hasText: "Chatten kunne ikke lastes" });
  await expect(chatError).toContainText("Chatten kunne ikke lastes");
  expect(requests).toBe(1);
  await expect(
    page.getByRole("button", { name: "Aktiver chat fra Agenci" }),
  ).toHaveCount(0);
  await expect(chatError.getByRole("link")).toHaveAttribute(
    "href",
    "mailto:post@triodelab.no",
  );
});

test("contact keeps an honest email fallback when server configuration is absent", async ({
  page,
}) => {
  test.skip(
    process.env.PLAYWRIGHT_CONTACT_ENABLED === "1",
    "Configured form is covered separately",
  );
  await page.goto("/kontakt?tjeneste=ki-og-automatisering");
  await expect(
    page.getByRole("link", { name: "Send e-post til post@triodelab.no" }),
  ).toHaveAttribute("href", "mailto:post@triodelab.no");
  await expect(
    page.getByRole("button", { name: "Send henvendelse" }),
  ).toHaveCount(0);
});

test("contact validates, preserves failed input and confirms only accepted submissions", async ({
  page,
}) => {
  test.skip(
    process.env.PLAYWRIGHT_CONTACT_ENABLED !== "1",
    "Needs configured test server with dummy credentials and mocked API",
  );
  const bodies: Record<string, string>[] = [];
  let accepted = false;
  await page.route("**/api/contact", async (route) => {
    bodies.push(route.request().postDataJSON());
    await route.fulfill({
      status: accepted ? 200 : 502,
      contentType: "application/json",
      body: JSON.stringify({ success: accepted }),
    });
  });
  await page.goto("/kontakt?tjeneste=ki-og-automatisering");
  await expect(
    page.locator(
      'link[rel="canonical"][href="https://www.triodelab.no/kontakt"]',
    ),
  ).toHaveCount(1);
  await expect(page.getByLabel("Hva gjelder det?")).toHaveValue("ki");
  await expect(page.getByLabel("Bedrift", { exact: true })).toHaveAttribute(
    "maxlength",
    "200",
  );
  await page
    .getByRole("button", { name: "Send henvendelse", exact: true })
    .click();
  await expect(page.getByText("Navn må være minst 2 tegn")).toBeVisible();
  expect(bodies).toHaveLength(0);
  await page
    .getByRole("textbox", { name: "Navn", exact: true })
    .fill("QA Test");
  await page
    .getByRole("textbox", { name: "E-post", exact: true })
    .fill("qa@example.test");
  await page
    .getByLabel("Fortell kort om hvor dere vil")
    .fill("Dette er en simulert innsending for å teste skjemaet.");
  await page
    .getByRole("button", { name: "Send henvendelse", exact: true })
    .click();
  await expect(page.locator("form").getByRole("alert")).toContainText(
    "Vi kunne ikke bekrefte innsendingen",
  );
  await expect(
    page.getByRole("textbox", { name: "Navn", exact: true }),
  ).toHaveValue("QA Test");
  await expect(page.getByRole("status")).toHaveCount(0);
  accepted = true;
  await page
    .getByRole("button", { name: "Send henvendelse", exact: true })
    .click();
  await expect(page.getByRole("status")).toContainText(
    "Takk! Henvendelsen er sendt",
  );
  expect(bodies).toHaveLength(2);
  expect(bodies[0].source).toBe("ki-og-automatisering");
  expect(bodies[1].submissionId).toBe(bodies[0].submissionId);
  await expect(
    page.getByRole("textbox", { name: "Navn", exact: true }),
  ).toBeEmpty();
});
