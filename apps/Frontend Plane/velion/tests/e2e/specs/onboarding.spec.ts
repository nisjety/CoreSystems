import { test, expect } from '@playwright/test';
import { OrganizationPage } from '../pages/OrganizationPage';
import { WebsitePage } from '../pages/WebsitePage';
import { ConnectPage } from '../pages/ConnectPage';
import { TeamPage } from '../pages/TeamPage';
import { CompletePage } from '../pages/CompletePage';

test.describe('Onboarding flow', () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([{
      name: 'better-auth.session_token',
      value: 'e2e-fake-session-token-for-testing',
      domain: 'localhost',
      path: '/',
      httpOnly: false,
      secure: false,
      sameSite: 'Lax',
    }]);
  });

  test('happy path — skip optional steps and reach dashboard', async ({ page }) => {
    await page.route('**/api/**', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ success: true }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/auth/get-session', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ user: { id: 'test-user-id', email: 'test@e2e.com', name: 'E2E Test User', emailVerified: true, createdAt: '2024-01-01T00:00:00.000Z', updatedAt: '2024-01-01T00:00:00.000Z' }, session: { id: 'test-session-id', userId: 'test-user-id', token: 'e2e-fake-session-token-for-testing', expiresAt: '2099-01-01T00:00:00.000Z' } })
    }));
    await page.route('**/api/org/orgs', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ id: 'test-org-id', name: 'E2E Test Org', slug: 'e2e-test-org', plan: 'free', status: 'active', ownerUserId: 'test-user-id', createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/user/users/current**', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ onboardingComplete: true })
    }));
    await page.goto('/onboarding/organization');
    const org = new OrganizationPage(page);
    await org.selectTab('create');
    await org.fillCreateForm('Test Org');
    await org.submitCreate();
    const website = new WebsitePage(page);
    await website.skip();
    const connect = new ConnectPage(page);
    await connect.skip();
    const team = new TeamPage(page);
    await team.skip();
    const complete = new CompletePage(page);
    await complete.goToDashboard();
    await expect(page).toHaveURL(/\/dashboard/);
  });

  test('join flow — join existing org by invite code', async ({ page }) => {
    await page.route('**/api/**', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ success: true }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/auth/get-session', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ user: { id: 'test-user-id', email: 'test@e2e.com', name: 'E2E Test User', emailVerified: true, createdAt: '2024-01-01T00:00:00.000Z', updatedAt: '2024-01-01T00:00:00.000Z' }, session: { id: 'test-session-id', userId: 'test-user-id', token: 'e2e-fake-session-token-for-testing', expiresAt: '2099-01-01T00:00:00.000Z' } })
    }));
    await page.route('**/api/org/orgs', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ id: 'test-org-id', name: 'E2E Test Org', slug: 'e2e-test-org', plan: 'free', status: 'active', ownerUserId: 'test-user-id', createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/user/users/current**', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ onboardingComplete: true })
    }));
    await page.route('**/api/auth/organization/accept-invitation**', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ organizationId: 'test-org-id' }) });
      } else { await route.continue(); }
    });
    await page.goto('/onboarding/organization');
    const org = new OrganizationPage(page);
    await org.selectTab('join');
    await org.fillJoinForm('TEST-INVITE-CODE');
    await org.submitJoin();
    const website = new WebsitePage(page);
    await website.skip();
    const connect = new ConnectPage(page);
    await connect.skip();
    const team = new TeamPage(page);
    await team.skip();
    const complete = new CompletePage(page);
    await complete.goToDashboard();
    await expect(page).toHaveURL(/\/dashboard/);
  });

  test('team invite — add a team member before completing', async ({ page }) => {
    await page.route('**/api/**', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ success: true }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/auth/get-session', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ user: { id: 'test-user-id', email: 'test@e2e.com', name: 'E2E Test User', emailVerified: true, createdAt: '2024-01-01T00:00:00.000Z', updatedAt: '2024-01-01T00:00:00.000Z' }, session: { id: 'test-session-id', userId: 'test-user-id', token: 'e2e-fake-session-token-for-testing', expiresAt: '2099-01-01T00:00:00.000Z' } })
    }));
    await page.route('**/api/org/orgs', async (route, request) => {
      if (request.method() === 'POST') {
        await route.fulfill({ status: 200, contentType: 'application/json',
          body: JSON.stringify({ id: 'test-org-id', name: 'E2E Test Org', slug: 'e2e-test-org', plan: 'free', status: 'active', ownerUserId: 'test-user-id', createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }) });
      } else { await route.continue(); }
    });
    await page.route('**/api/user/users/current**', route => route.fulfill({
      status: 200, contentType: 'application/json',
      body: JSON.stringify({ onboardingComplete: true })
    }));
    await page.goto('/onboarding/organization');
    const org = new OrganizationPage(page);
    await org.selectTab('create');
    await org.fillCreateForm('My Team');
    await org.submitCreate();
    const website = new WebsitePage(page);
    await website.skip();
    const connect = new ConnectPage(page);
    await connect.skip();
    const team = new TeamPage(page);
    await team.addMember('bob@example.com', 'admin');
    await team.submit();
    const complete = new CompletePage(page);
    await complete.goToDashboard();
    await expect(page).toHaveURL(/\/dashboard/);
  });
});
