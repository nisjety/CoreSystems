import type { Page } from '@playwright/test';

export class TeamPage {
  constructor(private readonly page: Page) {}

  async skip() {
    await this.page.getByTestId('onboarding-team-skip').click();
  }

  async addMember(email: string, role = 'member') {
    await this.page.getByTestId('onboarding-team-email').fill(email);
    await this.page.getByTestId('onboarding-team-role').selectOption(role);
    await this.page.getByTestId('onboarding-team-add-member').click();
  }

  async removeMember(index: number) {
    await this.page.getByTestId('onboarding-team-remove-member').nth(index).click();
  }

  async submit() {
    await this.page.getByTestId('onboarding-team-submit').click();
  }

  async goBack() {
    await this.page.getByTestId('onboarding-team-back').click();
  }
}
