import { expect, test } from "@playwright/test";

for (const reducedMotion of ["no-preference", "reduce"] as const) {
  test.describe(`homepage hydration (${reducedMotion})`, () => {
    test.use({ reducedMotion });

    test("preserves the server-rendered content when JavaScript becomes interactive", async ({
      page,
    }) => {
      const pageErrors: string[] = [];
      page.on("pageerror", (error) => pageErrors.push(error.message));

      let releaseScripts: () => void = () => {};
      const scriptGate = new Promise<void>((resolve) => {
        releaseScripts = resolve;
      });

      // Observe the actual server DOM before hydration, even on fast machines.
      await page.route("**/_next/static/chunks/**", async (route) => {
        await scriptGate;
        await route.continue();
      });

      try {
        await page.goto("/", { waitUntil: "commit" });
        const initialMain = await page
          .locator("main#main-content")
          .elementHandle();
        expect(initialMain).not.toBeNull();

        releaseScripts();
        await expect(
          page.getByRole("tab", { name: "01 Webløsninger" }),
        ).toBeEnabled();

        expect(pageErrors).toEqual([]);
        expect(
          await initialMain!.evaluate((element) => element.isConnected),
        ).toBe(true);
        await expect(page.locator(".hero-copy p").first()).toContainText(
          "Vi hjelper team med rådgivning, utvikling",
        );
      } finally {
        releaseScripts();
      }
    });
  });
}
