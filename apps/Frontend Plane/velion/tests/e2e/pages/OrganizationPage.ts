import type { Page } from '@playwright/test';

export class OrganizationPage {
  constructor(private readonly page: Page) {}

  async selectTab(tab: 'create' | 'join') {
    await this.page.getByTestId(`onboarding-org-tab-${tab}`).click();
  }

  async dismissBrregSearch() {
    const skipBtn = this.page.getByTestId('onboarding-brreg-skip');
    try {
      await skipBtn.waitFor({ state: 'visible', timeout: 3000 });
      await skipBtn.click();
    } catch {
      // BrregSearch not visible, continue
    }
  }

  async fillCreateForm(name: string, plan = 'free') {
    await this.page.getByTestId('onboarding-org-name').fill(name);
    await this.dismissBrregSearch();
    await this.page.getByTestId('onboarding-org-plan').selectOption(plan);
  }

  async submitCreate() {
    await this.page.getByTestId('onboarding-org-create-submit').click();
  }

  async fillJoinForm(inviteCode: string) {
    await this.page.getByTestId('onboarding-org-invite-code').fill(inviteCode);
  }

  async submitJoin() {
    await this.page.getByTestId('onboarding-org-join-submit').click();
  }

  async goBack() {
    await this.page.getByTestId('onboarding-org-join-back').click();
  }
}
