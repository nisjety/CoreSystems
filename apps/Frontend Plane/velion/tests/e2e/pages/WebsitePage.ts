import type { Page } from '@playwright/test';

export class WebsitePage {
  constructor(private readonly page: Page) {}

  async skip() {
    await this.page.getByTestId('onboarding-website-skip').click();
  }

  async submitUrl(url: string) {
    await this.page.getByTestId('onboarding-website-url').fill(url);
    await this.page.getByTestId('onboarding-website-submit').click();
  }

  async continueAfterCrawl() {
    await this.page.getByTestId('onboarding-website-continue').click();
  }

  async crawlSkip() {
    await this.page.getByTestId('onboarding-website-crawl-skip').click();
  }
}
