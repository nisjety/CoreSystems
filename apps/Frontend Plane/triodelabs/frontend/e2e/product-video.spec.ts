import { expect, test, type Page } from "@playwright/test";

async function openVerevon(page: Page, rejectPlayback = false) {
  await page.addInitScript((reject) => {
    HTMLMediaElement.prototype.play = function () {
      this.dataset.playCalls = String(Number(this.dataset.playCalls ?? 0) + 1);
      return reject
        ? Promise.reject(new DOMException("Playback denied", "NotAllowedError"))
        : Promise.resolve();
    };
    HTMLMediaElement.prototype.pause = function () {
      this.dataset.pauseCalls = String(
        Number(this.dataset.pauseCalls ?? 0) + 1,
      );
    };
  }, rejectPlayback);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const section = page.locator("#prosjekter");
  const tab = section.getByRole("tab", { name: "Verevon", exact: true });
  await expect
    .poll(
      async () => {
        if ((await tab.getAttribute("aria-selected")) === "true") return true;
        await tab.click();
        return false;
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  const card = section.getByRole("link", {
    name: "Verevon Se produktet",
    exact: true,
  });
  await expect(card).toBeVisible();
  await card.scrollIntoViewIfNeeded();
  await page.mouse.move(0, 0);
  return { card, video: card.locator("video"), section };
}

test("plays Verevon on hover and keyboard focus, then restores the still", async ({
  page,
}) => {
  const { card, video, section } = await openVerevon(page);
  await expect(video).toHaveAttribute("preload", "none");
  await expect(video).toHaveAttribute("data-playing", "false");
  await expect(video).not.toHaveAttribute("data-play-calls", /.+/);
  await card.hover();
  await expect(video).toHaveAttribute("data-playing", "true");
  await expect(video).toHaveJSProperty("muted", true);
  await page.mouse.move(0, 0);
  await expect(video).toHaveAttribute("data-playing", "false");
  await expect(video).toHaveJSProperty("currentTime", 0);

  await page.keyboard.press("Tab");
  await card.focus();
  await expect(video).toHaveAttribute("data-playing", "true");
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(video).toHaveAttribute("data-playing", "false");
  await card.scrollIntoViewIfNeeded();
  await expect(video).toHaveAttribute("data-playing", "true");
  await section.getByRole("tab", { name: "Verevon", exact: true }).focus();
  await expect(video).toHaveAttribute("data-playing", "false");

  await card.hover();
  await expect(video).toHaveAttribute("data-playing", "true");
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect(video).toHaveAttribute("data-playing", "false");
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await section.getByRole("tab", { name: "Agenci", exact: true }).click();
  await expect(section.locator("video")).toHaveCount(0);
});

test("keeps the poster when playback is rejected or the pointer is touch", async ({
  page,
}) => {
  const { card, video } = await openVerevon(page, true);
  await card.dispatchEvent("pointerenter", { pointerType: "touch" });
  await expect(video).not.toHaveAttribute("data-play-calls", /.+/);
  await card.hover();
  await expect(video).toHaveAttribute("data-play-calls", "1");
  await expect(video).toHaveAttribute("data-playing", "false");
  await expect(card.locator("img")).toBeVisible();
});

test("does not start video with reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const { card, video } = await openVerevon(page);
  await card.hover();
  await page.keyboard.press("Tab");
  await card.focus();
  await expect(video).toHaveAttribute("data-playing", "false");
  await expect(video).not.toHaveAttribute("data-play-calls", /.+/);
});
