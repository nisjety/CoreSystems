import { expect, test } from "@playwright/test";

test("keeps six animated service cards in one row and changes their themed background", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1051, height: 946 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#tjenester");
  await section.scrollIntoViewIfNeeded();
  const tabs = section
    .getByRole("tablist", { name: "Tjenester" })
    .getByRole("tab");
  const heading = section.locator("#services-heading");

  await expect(tabs).toHaveCount(6);
  await expect(heading).toHaveText("Webløsninger");

  const boxes = await Promise.all(
    Array.from({ length: 6 }, (_, index) => tabs.nth(index).boundingBox()),
  );
  const headingBox = await heading.boundingBox();
  expect(boxes.every((box) => box !== null)).toBe(true);
  expect(headingBox).not.toBeNull();

  if (boxes.some((box) => box === null) || !headingBox) {
    throw new Error("Capability cards or heading could not be measured.");
  }

  const cardBoxes = boxes.filter(
    (box): box is NonNullable<typeof box> => box !== null,
  );
  const rowTop = cardBoxes[0].y;
  for (const [index, box] of cardBoxes.entries()) {
    expect(Math.abs(box.y - rowTop)).toBeLessThan(16);
    expect(box.y + box.height).toBeLessThan(headingBox.y);
    await expect(tabs.nth(index).locator("svg")).toHaveCount(1);

    if (index > 0) {
      expect(
        cardBoxes[index - 1].x + cardBoxes[index - 1].width,
      ).toBeLessThanOrEqual(box.x + 2);
    }
  }

  const commerce = tabs.nth(1);
  await expect
    .poll(
      async () => {
        if ((await commerce.getAttribute("aria-selected")) === "true") {
          return true;
        }

        await commerce.click();
        return false;
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  await expect(commerce).toHaveAttribute("aria-selected", "true");
  await expect(heading).toHaveText("Nettbutikk");
  await expect(
    section.locator('[data-service-background="Nettbutikk"]'),
  ).toHaveCSS("opacity", "1");
  await expect(
    section.getByRole("link", { name: "Utforsk tjenesten" }),
  ).toHaveAttribute("href", "/tjenester/nettbutikk");
  await tabs.nth(2).click();
  await expect(tabs.nth(2)).toHaveAttribute("aria-selected", "true");
});

test("keeps the selected service card visible when mobile arrows advance through the rail", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 600 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const section = page.locator("#tjenester");
  const rail = section.getByRole("tablist", { name: "Tjenester" });
  const tabs = rail.getByRole("tab");
  const next = section.getByRole("button", { name: "Neste tjeneste" });

  await section.scrollIntoViewIfNeeded();
  await expect(tabs).toHaveCount(6);
  await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
  await expect(next).toBeEnabled();
  await next.scrollIntoViewIfNeeded();
  let previousScroll = await page.evaluate(() => window.scrollY);
  let stableReadings = 0;
  await expect
    .poll(async () => {
      const currentScroll = await page.evaluate(() => window.scrollY);
      stableReadings =
        Math.abs(currentScroll - previousScroll) < 0.5 ? stableReadings + 1 : 0;
      previousScroll = currentScroll;
      return stableReadings;
    })
    .toBeGreaterThanOrEqual(3);
  const initialPageScroll = await page.evaluate(() => window.scrollY);

  for (let index = 1; index < 6; index += 1) {
    await expect
      .poll(async () => {
        if ((await tabs.nth(index).getAttribute("aria-selected")) === "true") {
          return true;
        }

        await next.click();
        return false;
      })
      .toBe(true);
    await expect(tabs.nth(index)).toHaveAttribute("aria-selected", "true");
  }

  await expect
    .poll(async () => {
      return rail.evaluate((element) => {
        const selected = element.querySelector<HTMLElement>(
          '[role="tab"][aria-selected="true"]',
        );
        if (!selected) return false;

        const viewport = element.getBoundingClientRect();
        const card = selected.getBoundingClientRect();
        return (
          card.left >= viewport.left - 2 && card.right <= viewport.right + 2
        );
      });
    })
    .toBe(true);
  await expect(tabs.last()).toHaveAttribute("aria-selected", "true");
  await expect(
    section.getByRole("link", { name: "Utforsk tjenesten" }),
  ).toHaveAttribute("href", "/tjenester/ki-og-automatisering");
  const finalPageScroll = await page.evaluate(() => window.scrollY);
  expect(Math.abs(finalPageScroll - initialPageScroll)).toBeLessThan(4);
});

test("removes small section eyebrow labels from the homepage", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const labels = [
    ["#teknologier", "Teknologi"],
    ["#tjenester", "01 / Kapabiliteter"],
    ["#prosjekter", "03 / Egne produkter"],
    ["#prosjekter", "01 / EGET PRODUKT"],
    ["#faq", "FAQ"],
    ["section[aria-labelledby='project-start-heading']", "Neste steg"],
  ] as const;

  for (const [section, label] of labels) {
    await expect(
      page.locator(section).getByText(label, { exact: true }),
    ).toHaveCount(0);
  }
});

test("answers common buyer questions in FAQ and keeps a focused prefooter CTA", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const faq = page.locator("#faq");
  const prefooter = page.locator(
    "section[aria-labelledby='project-start-heading']",
  );
  const prompts = [
    "Hva koster en nettside eller nettbutikk?",
    "Hvor lang tid tar det å lage en digital løsning?",
    "Kan dere hjelpe oss å bli mer synlige på nett?",
  ];

  for (const prompt of prompts) {
    const item = faq.locator("details").filter({ hasText: prompt });
    await expect(item).toHaveCount(1);
    await expect(item.locator("summary > span").first()).toHaveText(prompt);
    await item.locator("summary").click();
    await expect(item).toHaveAttribute("open", "");
    await expect(item.locator("p")).not.toBeEmpty();
  }

  await expect(
    prefooter.getByRole("heading", { name: /Har dere noe/ }),
  ).toBeVisible();
  await expect(
    prefooter.getByRole("link", { name: /Fortell oss om det/i }),
  ).toHaveAttribute("href", "/kontakt");
  await expect(prefooter.locator("ol li")).toHaveCount(0);
  await expect(
    page
      .locator("main#main-content")
      .evaluate((element) => element.nextElementSibling?.id),
  ).resolves.toBe("footer");
});

test("opens a preaddressed email from the envelope in the navigation", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const envelope = page.locator(
    'header[data-site-nav] a[aria-label="Kontakt"]',
  );
  await expect(envelope).toHaveAttribute(
    "href",
    "mailto:post@triodelab.no?subject=Foresp%C3%B8rsel%20fra%20triodelab.no&body=Hei%20TriodeLab%2C%0A%0A",
  );
});
