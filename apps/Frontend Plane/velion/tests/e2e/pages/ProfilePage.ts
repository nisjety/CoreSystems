import type { Page } from '@playwright/test';

export class ProfilePage {
  constructor(private readonly page: Page) {}

  async fill(firstName: string, lastName: string, displayName?: string, jobTitle?: string) {
    await this.page.getByTestId('onboarding-profile-firstName').fill(firstName);
    await this.page.getByTestId('onboarding-profile-lastName').fill(lastName);
    if (displayName !== undefined) {
      await this.page.getByTestId('onboarding-profile-displayName').fill(displayName);
    }
    if (jobTitle !== undefined) {
      await this.page.getByTestId('onboarding-profile-jobTitle').fill(jobTitle);
    }
  }

  async submit() {
    await this.page.getByTestId('onboarding-profile-submit').click();
  }
}
