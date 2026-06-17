import type { Page } from '@playwright/test';

export class CompletePage {
  constructor(private readonly page: Page) {}

  async goToDashboard() {
    await this.page.getByTestId('onboarding-complete-dashboard').click();
  }

  async goBack() {
    await this.page.getByTestId('onboarding-complete-back').click();
  }
}
