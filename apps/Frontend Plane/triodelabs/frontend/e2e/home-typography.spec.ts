import { expect, test } from "@playwright/test";

test("uses the shared home-page typography scale", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const sectionHeadings = [
    page.locator("#studio-introduction-heading"),
    page.locator("#services-heading"),
    page.locator("#projects-heading"),
    page.locator("#faq-heading"),
  ];
  const sectionHeadingStyles = await Promise.all(
    sectionHeadings.map((heading) =>
      heading.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          fontFamily: styles.fontFamily,
          fontSize: styles.fontSize,
          fontWeight: styles.fontWeight,
          lineHeight: styles.lineHeight,
        };
      }),
    ),
  );

  expect(sectionHeadingStyles).toEqual(
    Array.from({ length: sectionHeadings.length }, () =>
      expect.objectContaining({
        fontFamily: expect.stringContaining("Manrope"),
        fontSize: "48.96px",
        fontWeight: "300",
        lineHeight: "45.0432px",
      }),
    ),
  );

  const homeBodyStyles = await page.locator(".home-body").evaluateAll(
    (elements) =>
      elements.map((element) => {
        const styles = getComputedStyle(element);
        return {
          fontFamily: styles.fontFamily,
          fontSize: styles.fontSize,
          fontWeight: styles.fontWeight,
          lineHeight: styles.lineHeight,
        };
      }),
  );

  expect(homeBodyStyles.length).toBeGreaterThan(5);
  expect(homeBodyStyles).toEqual(
    Array.from({ length: homeBodyStyles.length }, () =>
      expect.objectContaining({
        fontFamily: expect.stringContaining("Manrope"),
        fontSize: "18px",
        fontWeight: "400",
        lineHeight: "27px",
      }),
    ),
  );
});
