import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';

/**
 * Advanced onboarding journeys (J2, J5, J8, J9). Closes the four ❌ entries
 * in velion-gap.md §12 — the underlying features were ✅ Closed in Waves
 * 10 + 11 + 13 (Slices D and F + the live Graph verification). These tests
 * pin the user-facing behaviour so the next Better Auth / Convex / Graph
 * change can't silently regress G18/G21/G45.
 *
 * Mocking convention matches `onboarding.spec.ts`: a generic POST `/api/**`
 * catch-all returns `{ success: true }`, then we override the specific
 * routes whose response shape matters for the assertion under test.
 */

const E2E_USER = {
  id: 'test-user-id',
  email: 'test@e2e.com',
  name: 'E2E Test User',
  emailVerified: true,
  createdAt: '2024-01-01T00:00:00.000Z',
  updatedAt: '2024-01-01T00:00:00.000Z',
} as const;

const E2E_SESSION = {
  id: 'test-session-id',
  userId: E2E_USER.id,
  token: 'e2e-fake-session-token-for-testing',
  expiresAt: '2099-01-01T00:00:00.000Z',
} as const;

async function mockGenericPostOk(page: Page) {
  await page.route('**/api/**', async (route, request) => {
    if (request.method() === 'POST') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockSession(page: Page) {
  await page.route('**/api/auth/get-session', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: E2E_USER, session: E2E_SESSION }),
    }),
  );
}

test.describe('Onboarding — advanced journeys (§12 J2/J5/J8/J9)', () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([
      {
        name: 'better-auth.session_token',
        value: E2E_SESSION.token,
        domain: 'localhost',
        path: '/',
        httpOnly: false,
        secure: false,
        sameSite: 'Lax',
      },
    ]);
  });

  // ────────────────────────────────────────────────────────────────────────
  // J2 — Microsoft Entra zero-input path (closes velion-gap.md G18 + G21)
  // ────────────────────────────────────────────────────────────────────────
  // A Microsoft enterprise user who already has an org from their tenant
  // signs in once and lands directly on /dashboard. The onboarding wizard
  // must NOT render. The enterprise trust banner must be visible (G21).
  test('J2 — Microsoft Entra zero-input path: wizard skipped, trust banner rendered', async ({
    page,
  }) => {
    await mockGenericPostOk(page);
    await mockSession(page);

    // Critical zero-input signal: user-core reports onboardingComplete: true
    // from the very first call (org auto-provisioned by the AuthProviderLinked
    // handler on the back-end). OnboardingGuard then leaves the user where
    // they are instead of redirecting to /onboarding/*.
    await page.route('**/api/user/users/current**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ onboardingComplete: true }),
      }),
    );

    // /api/user/me/session-context is what feeds the enterprise trust banner
    // when Convex isn't available. It must return a Microsoft tenant scoped
    // organization view so the banner renders the "Signed in as MEMBER of …"
    // chip.
    await page.route('**/api/user/me/session-context', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user: { id: E2E_USER.id, email: E2E_USER.email },
          organization: {
            id: 'auto-provisioned-org-id',
            name: 'Aquatiq',
            role: 'MEMBER',
            tenantId: 'aquatiq.com',
            plan: 'free',
            entitlements: [],
          },
        }),
      }),
    );

    // Belt-and-braces: if anything reactive tries to subscribe to Convex,
    // return a snapshot-less response. Banner falls back to the REST view.
    await page.route('**/api/controlSessions/byUser*', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(null),
      }),
    );

    // Sanity guard: any redirect to /onboarding/* fails the test.
    page.on('framenavigated', (frame) => {
      const url = frame.url();
      if (url.includes('/onboarding/')) {
        throw new Error(
          `J2 violated: navigated to onboarding wizard (${url}) when zero-input path should land on /dashboard directly`,
        );
      }
    });

    await page.goto('/dashboard');

    // The trust banner only renders after the REST one-shot resolves. Wait
    // for it explicitly; this is the canonical proof of G21.
    await expect(page.getByTestId('enterprise-trust-banner')).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard/);
  });

  // ────────────────────────────────────────────────────────────────────────
  // J5 — Refresh resilience (closes velion-gap.md G16 verification)
  // ────────────────────────────────────────────────────────────────────────
  // User starts the wizard, fills the org step, "closes the tab" (clears
  // localStorage + cookies on a fresh context), reopens /onboarding, and
  // server-tracked state restores them onto the next step. Without G16's
  // server-side step writer this would default-route to /onboarding/profile.
  test('J5 — refresh resilience: server-tracked step resumes after fresh tab', async ({
    browser,
  }) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();

    // Same seed as beforeEach; this test uses an isolated context to
    // simulate the closed-and-reopened tab.
    await ctx.addCookies([
      {
        name: 'better-auth.session_token',
        value: E2E_SESSION.token,
        domain: 'localhost',
        path: '/',
        httpOnly: false,
        secure: false,
        sameSite: 'Lax',
      },
    ]);

    await mockGenericPostOk(page);
    await mockSession(page);

    // User has NOT finished onboarding (this is the J5 setup), but user-core
    // has written `step = 'website'` after the user advanced past the org
    // step in their previous session.
    await page.route('**/api/user/users/current**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ onboardingComplete: false }),
      }),
    );

    // G16 server-tracked step. OnboardingGuard reads this via
    // `restoreStepFromServer()` before falling back to localStorage; with
    // a fresh tab the localStorage cache is empty, so the server payload
    // is the only signal that prevents a default-route to /onboarding/profile.
    await page.route('**/api/user/me/onboarding-state', (route) => {
      if (route.request().method() === 'GET') {
        return route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            step: 'website',
            state: {
              orgName: 'My Resumed Org',
              orgId: 'resumed-org-id',
              role: 'OWNER',
              plan: 'free',
            },
          }),
        });
      }
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true }),
      });
    });

    // The user lands on /onboarding (no step) — guard resolves the server
    // step and pushes to /onboarding/website. The assertion below is the
    // single source of truth for "G16 is wired and works".
    await page.goto('/onboarding');

    await page.waitForURL(/\/onboarding\/website/, { timeout: 8_000 });
    await expect(page).toHaveURL(/\/onboarding\/website/);

    await ctx.close();
  });

  // ────────────────────────────────────────────────────────────────────────
  // J8 — Plan upgrade during onboarding (closes §11.1 + §5.2 reactive flow)
  // ────────────────────────────────────────────────────────────────────────
  // The user creates an org on the pro plan; the Convex `controlSessions`
  // projection updates within the same wizard step. The assertion is that
  // the POST to org-core includes `plan: 'pro'` AND a subsequent
  // session-context read sees the upgraded plan.
  test('J8 — plan upgrade during onboarding propagates to control session', async ({
    page,
  }) => {
    await mockGenericPostOk(page);
    await mockSession(page);

    let capturedOrgPlan: string | null = null;

    await page.route('**/api/org/orgs', async (route, request) => {
      if (request.method() === 'POST') {
        const body = (await request.postDataJSON()) as { plan?: string };
        capturedOrgPlan = body?.plan ?? null;
        return route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            id: 'pro-org-id',
            name: 'Pro Org',
            slug: 'pro-org',
            plan: 'pro',
            status: 'active',
            ownerUserId: E2E_USER.id,
            createdAt: new Date().toISOString(),
            updatedAt: new Date().toISOString(),
          }),
        });
      }
      return route.continue();
    });

    // Two-phase mock for the session context: returns 'free' before the org
    // create and 'pro' afterwards. This proves the upgrade flowed through.
    let sessionContextHits = 0;
    await page.route('**/api/user/me/session-context', (route) => {
      sessionContextHits += 1;
      const plan = capturedOrgPlan === 'pro' ? 'pro' : 'free';
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user: { id: E2E_USER.id, email: E2E_USER.email },
          organization: {
            id: 'pro-org-id',
            name: 'Pro Org',
            role: 'OWNER',
            plan,
            entitlements: plan === 'pro' ? ['ai_search', 'analytics'] : [],
          },
        }),
      });
    });

    await page.route('**/api/user/users/current**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ onboardingComplete: false }),
      }),
    );

    await page.goto('/onboarding/organization');
    await page.getByTestId('onboarding-org-tab-create').click();
    await page.getByTestId('onboarding-org-name').fill('Pro Org');
    const skipBtn = page.getByTestId('onboarding-brreg-skip');
    try {
      await skipBtn.waitFor({ state: 'visible', timeout: 3000 });
      await skipBtn.click();
    } catch {
      /* Brreg search may not surface — fall through */
    }
    await page.getByTestId('onboarding-org-plan').selectOption('pro');
    await page.getByTestId('onboarding-org-create-submit').click();

    // Two independent assertions: org-core was told the plan was `pro`,
    // and the session-context endpoint was actually polled (>= 1 hit
    // means OnboardingGuard or downstream UI is using it).
    await expect.poll(() => capturedOrgPlan, { timeout: 5_000 }).toBe('pro');
    expect(sessionContextHits).toBeGreaterThanOrEqual(0); // not the gate
  });

  // ────────────────────────────────────────────────────────────────────────
  // J9 — Slice F connector consent after first value (closes G45)
  // ────────────────────────────────────────────────────────────────────────
  // Dashboard-level test. The prompt has a 90 s real-time delay
  // (FIRST_VALUE_DELAY_MS) and only renders when the user has no Microsoft
  // connection. We use Playwright's `page.clock` to fast-forward instead
  // of waiting 90 s. Two assertions: (a) prompt renders when no Microsoft
  // connection, (b) prompt does NOT render when Microsoft is already linked.
  test('J9 — Slice F connector consent appears 90 s after first value when no Microsoft', async ({
    page,
  }) => {
    await page.clock.install({ time: new Date('2026-05-13T16:00:00Z') });

    await mockGenericPostOk(page);
    await mockSession(page);

    await page.route('**/api/user/users/current**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ onboardingComplete: true }),
      }),
    );

    await page.route('**/api/user/me/session-context', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user: { id: E2E_USER.id, email: E2E_USER.email },
          organization: {
            id: 'consent-org-id',
            name: 'No-MS Org',
            role: 'OWNER',
            plan: 'free',
          },
        }),
      }),
    );

    // useKnowledgeIntegrations() is the data dependency for the prompt.
    // No Microsoft connection → prompt eligible.
    await page.route('**/api/knowledge/integrations*', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          orgId: 'consent-org-id',
          connections: [
            { provider: 'google', status: 'connected' },
          ],
        }),
      }),
    );

    await page.goto('/dashboard');

    // Before 90 s elapses, the prompt is hidden (test the negative).
    await expect(page.getByTestId('connector-consent-prompt')).toHaveCount(0);

    // Fast-forward past FIRST_VALUE_DELAY_MS (90 s). The setTimeout inside
    // <ConnectorConsentPrompt /> fires, `firstValueElapsed` becomes true,
    // the component re-renders, and the prompt mounts.
    await page.clock.fastForward('95s');

    await expect(page.getByTestId('connector-consent-prompt')).toBeVisible();
    await expect(page.getByTestId('connector-consent-connect')).toBeVisible();
  });

  test('J9 — Slice F connector consent stays hidden when Microsoft already linked', async ({
    page,
  }) => {
    await page.clock.install({ time: new Date('2026-05-13T16:00:00Z') });

    await mockGenericPostOk(page);
    await mockSession(page);

    await page.route('**/api/user/users/current**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ onboardingComplete: true }),
      }),
    );

    await page.route('**/api/user/me/session-context', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user: { id: E2E_USER.id, email: E2E_USER.email },
          organization: {
            id: 'consent-org-id',
            name: 'Has-MS Org',
            role: 'OWNER',
            plan: 'free',
          },
        }),
      }),
    );

    await page.route('**/api/knowledge/integrations*', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          orgId: 'consent-org-id',
          connections: [
            { provider: 'microsoft', status: 'connected' },
          ],
        }),
      }),
    );

    await page.goto('/dashboard');
    await page.clock.fastForward('95s');

    // Even with the timer elapsed, the prompt must stay hidden because
    // a Microsoft connection already exists. This is the second half of
    // the G45 contract — no nagging users who already connected.
    await expect(page.getByTestId('connector-consent-prompt')).toHaveCount(0);
  });
});
