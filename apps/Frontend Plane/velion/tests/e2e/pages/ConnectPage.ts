import type { Page } from '@playwright/test';

export class ConnectPage {
  constructor(private readonly page: Page) {}

  async skip() {
    await this.page.getByTestId('onboarding-connect-skip').click();
  }

  async connect(sourceKey: string) {
    await this.page.getByTestId(`onboarding-connect-source-${sourceKey}`).click();
    await this.page.getByTestId('onboarding-connect-connect').click();
  }

  async goBack() {
    await this.page.getByTestId('onboarding-connect-back').click();
  }
}
