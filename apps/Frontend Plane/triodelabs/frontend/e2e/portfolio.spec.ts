import { expect, test, type Page } from "@playwright/test";

async function openProductPreFooter(page: Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#prosjekter");
  await section.scrollIntoViewIfNeeded();
  await expect(section).toBeVisible();

  return section;
}

async function selectProduct(page: Page, name: "Agenci" | "Verevon") {
  const tab = page.getByRole("tab", { name, exact: true });

  await expect(tab).toBeVisible();
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

  return tab;
}

test("shows the Field of Dreams product showcase with interactive product panels", async ({
  page,
}) => {
  const section = await openProductPreFooter(page);
  await expect(
    section.getByRole("heading", {
      name: "Field of Dreams",
    }),
  ).toBeVisible();
  await expect(section.getByRole("heading", { level: 2 })).toHaveCount(1);
  await expect(
    section.getByRole("heading", {
      name: "Egne produkter for arbeid som må flyte.",
    }),
  ).toHaveCount(0);

  const tabList = section.getByRole("tablist", { name: "Velg produkt" });
  const agenciTab = page.getByRole("tab", { name: "Agenci", exact: true });
  const verevonTab = page.getByRole("tab", { name: "Verevon", exact: true });

  await expect(tabList).toBeVisible();
  await expect(tabList.getByRole("tab")).toHaveCount(2);
  await expect(agenciTab).toHaveAttribute("aria-selected", "true");
  await expect(verevonTab).toHaveAttribute("aria-selected", "false");

  const agenciPanel = page.locator("#owned-product-panel-0");
  const verevonPanel = page.locator("#owned-product-panel-1");
  const orbitImage = section.getByTestId("product-orbit-card").locator("img");
  await expect(agenciPanel).toHaveCount(1);
  await expect(agenciPanel).toBeVisible();
  await expect(verevonPanel).toHaveCount(1);
  await expect(verevonPanel).toBeHidden();
  await expect(agenciPanel).toContainText(
    "En KI-chatassistent som gjør virksomhetens egen kunnskap tilgjengelig hele døgnet.",
  );
  await expect(orbitImage).toHaveAttribute("src", /agenci-macbook-mockup\.jpg/);
  await expect
    .poll(
      () =>
        orbitImage.evaluate(
          (image: HTMLImageElement) => image.complete && image.naturalWidth > 0,
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
  await expect(
    agenciPanel.getByRole("link", { name: "Utforsk Agenci" }),
  ).toHaveAttribute("href", "https://agenci.no/");

  await selectProduct(page, "Verevon");

  const action = verevonPanel.getByRole("link", { name: "Utforsk Verevon" });

  await expect(verevonPanel).toHaveCount(1);
  await expect(verevonPanel).toBeVisible();
  await expect(agenciPanel).toHaveCount(1);
  await expect(agenciPanel).toBeHidden();
  await expect(verevonPanel).toContainText(
    "En KI-arbeidsflyt som gjør signaler om til tydelige, kontrollerte handlinger.",
  );
  await expect(orbitImage).toHaveAttribute("src", /verevon-hover-mockup\.jpg/);
  await expect
    .poll(
      () =>
        orbitImage.evaluate(
          (image: HTMLImageElement) => image.complete && image.naturalWidth > 0,
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
  await expect(action).toHaveAttribute("href", "https://www.verevon.com/");
  await expect(action).toHaveAttribute("target", "_blank");
  await expect(action).toHaveAttribute("rel", "noreferrer");
});

test("offers an accessible expandable FAQ on the homepage", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const faq = page.locator("section[aria-labelledby='faq-heading']");
  await expect(faq).toBeVisible({ timeout: 5_000 });
  await faq.scrollIntoViewIfNeeded();
  await expect(faq.locator("#faq-heading")).toBeVisible();

  const questions = faq.locator("details");
  await expect(questions).toHaveCount(7);

  const firstQuestion = questions.first();
  const firstSummary = firstQuestion.locator("summary");

  await expect(firstSummary).toBeVisible();
  await expect(firstQuestion).not.toHaveAttribute("open", "");

  await firstSummary.click();

  await expect(firstQuestion).toHaveAttribute("open", "");
});

test("changes the selected feature with keyboard navigation", async ({
  page,
}) => {
  await openProductPreFooter(page);
  const verevonTab = await selectProduct(page, "Verevon");
  const agenciTab = page.getByRole("tab", { name: "Agenci", exact: true });

  await verevonTab.focus();
  await verevonTab.press("ArrowRight");

  await expect(agenciTab).toBeFocused();
  await expect(agenciTab).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tabpanel", { name: "Agenci" })).toBeVisible();

  await agenciTab.press("End");

  await expect(verevonTab).toBeFocused();
  await expect(verevonTab).toHaveAttribute("aria-selected", "true");
});
