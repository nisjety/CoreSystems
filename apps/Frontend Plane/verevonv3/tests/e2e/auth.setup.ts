import { test as setup, expect } from '@playwright/test'

/**
 * Auth setup: sign the seeded verified account in through the gateway, ensure
 * it has an active organization, and persist the session as storageState so
 * every e2e spec starts authenticated + org-scoped.
 *
 * Uses `page.request` (not the standalone `request` fixture) so the cookies
 * land in the page context's jar that `storageState()` serializes.
 *
 * Prereq: the verified account must exist. Run once:
 *   bash "apps/Control Plane/scripts/seed-verified-test-user.sh"
 * (creates e2e@verevon.dev with email_verified=true).
 */
const EMAIL = process.env.E2E_EMAIL || 'e2e@verevon.dev'
const PASSWORD = process.env.E2E_PASSWORD || 'e2e-Verevon-Pass-123'
const STORAGE_STATE = 'tests/e2e/.auth/state.json'

setup('authenticate and ensure org', async ({ page, baseURL }) => {
  const origin = baseURL ?? 'http://localhost:5199'
  const api = page.request

  // 1. Sign in through the same-origin gateway (Better Auth needs Origin).
  const signin = await api.post('/api/v1/auth/sign-in', {
    headers: { 'content-type': 'application/json', origin },
    data: { email: EMAIL, password: PASSWORD },
  })
  expect(signin.ok(), `sign-in failed: ${signin.status()}`).toBeTruthy()

  // 2. Confirm the session resolves.
  const me = await api.get('/api/v1/me')
  expect(me.status(), 'protected /me should be 200 with a verified session').toBe(200)

  // 3. Ensure an org exists. Some local stacks return a duplicate error instead
  // of deduping, so accept the proven duplicate case and keep the authenticated
  // session state.
  const org = await api.post('/api/v1/onboarding/actions/create-organization', {
    headers: { 'content-type': 'application/json', origin },
    data: { name: 'Verevon E2E Org', plan: 'trial' },
  })
  if (![200, 201].includes(org.status())) {
    const orgBody = await org.json().catch(() => null) as { error?: { code?: string; message?: string } } | null
    expect(
      org.status() === 400 && orgBody?.error?.message === 'Organization already exists',
      `create-organization status ${org.status()}: ${JSON.stringify(orgBody)}`,
    ).toBeTruthy()
  }

  // 4. The normal product routes are correctly guarded behind onboarding. E2E
  // specs exercise those routes, so finish the real onboarding lifecycle once
  // for this dedicated fixture account instead of letting every browser journey
  // silently land on the wizard.
  const current = await api.get('/api/v1/session/current')
  expect(current.status(), 'active org should resolve after organization setup').toBe(200)
  const currentBody = await current.json() as { data?: { org?: { id?: string | null } | null } }
  const orgId = currentBody.data?.org?.id
  expect(orgId, 'active org id should be present before completing onboarding').toBeTruthy()

  const completed = await api.post('/api/v1/onboarding/complete', {
    headers: { 'content-type': 'application/json', origin },
    data: { orgId, plan: 'trial', source: 'verevon-e2e' },
  })
  expect(completed.status(), 'onboarding completion should be durable').toBe(200)

  const refreshedSession = await api.get('/api/v1/session/current')
  expect(refreshedSession.status(), 'session should resolve after onboarding completion').toBe(200)
  const refreshedBody = await refreshedSession.json() as { data?: { onboardingStatus?: string } }
  expect(refreshedBody.data?.onboardingStatus, 'completed onboarding must survive a fresh session read').toBe('COMPLETED')

  // 5. Persist cookies (session + active-org) for the e2e project.
  await page.context().storageState({ path: STORAGE_STATE })
})
