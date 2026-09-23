import { test as setup, expect } from '@playwright/test'

/**
 * Local-stack auth setup: signs the pre-provisioned `local@verevon.dev`
 * account in through the gateway and persists the session as storageState,
 * so the `local` project's specs — which target the dockerized v3 dev stack
 * on :5173 (bind-mounted Vite, HMR), not the :5199 stack the `e2e` project
 * targets — start authenticated. Mirrors auth.setup.ts's e2e@verevon.dev flow.
 *
 * Unlike e2e@verevon.dev, this account is seeded with its own "Verevon" org by
 * build-verevon-services.sh's seed_dev_account step, so it should already be
 * org-scoped; the create-organization call below is only a defensive no-op
 * (tolerates "already exists") in case a fresh stack hasn't seeded it yet.
 *
 * Prereq: the account must exist — it's created automatically by
 *   bash "apps/Frontend Plane/verevonv3/build-verevon-services.sh"
 * (opt in with SEED_DEV_ACCOUNT=1). Override via LOCAL_E2E_EMAIL/LOCAL_E2E_PASSWORD
 * if the local stack's seed differs from the default.
 */
const EMAIL = process.env.LOCAL_E2E_EMAIL || 'local@verevon.dev'
const PASSWORD = process.env.LOCAL_E2E_PASSWORD || 'Verevon-Admin-2026!'
const STORAGE_STATE = 'tests/e2e/.auth/local-state.json'

setup('authenticate local@verevon.dev', async ({ page, baseURL }) => {
  const origin = baseURL ?? 'http://localhost:5173'
  const api = page.request

  // A TCP listener can open before Vite's proxy answers after a cold rebuild.
  // Check the same-origin HTTP path before attempting a stateful sign-in.
  await expect.poll(async () => {
    try { return (await api.get('/health', { timeout: 3_000 })).status() }
    catch { return 0 }
  }, { timeout: 30_000, message: 'same-origin gateway must answer before sign-in' }).toBe(200)

  // 1. Sign in through the same-origin gateway (Better Auth needs Origin).
  const signin = await api.post('/api/v1/auth/sign-in', {
    headers: { 'content-type': 'application/json', origin },
    data: { email: EMAIL, password: PASSWORD },
  })
  expect(signin.ok(), `sign-in failed: ${signin.status()}`).toBeTruthy()

  // 2. Confirm the session resolves.
  const me = await api.get('/api/v1/me')
  expect(me.status(), 'protected /me should be 200 with a verified session').toBe(200)

  // 3. Defensive org bootstrap — this account should already own "Verevon",
  // but accept the duplicate case the same way auth.setup.ts does for a
  // stack where the dev seed hasn't run yet.
  const org = await api.post('/api/v1/onboarding/actions/create-organization', {
    headers: { 'content-type': 'application/json', origin },
    data: { name: 'Verevon', plan: 'trial' },
  })
  if (![200, 201].includes(org.status())) {
    const orgBody = await org.json().catch(() => null) as { error?: { code?: string; message?: string } } | null
    expect(
      org.status() === 400 && orgBody?.error?.message === 'Organization already exists',
      `create-organization status ${org.status()}: ${JSON.stringify(orgBody)}`,
    ).toBeTruthy()
  }

  // Login and membership do not finish onboarding. Use the same owner-backed
  // lifecycle as auth.setup.ts; otherwise every chat test lands on the wizard.
  const current = await api.get('/api/v1/session/current')
  expect(current.status(), 'active organization should resolve').toBe(200)
  const currentBody = await current.json() as { data?: { org?: { id?: string | null } | null } }
  const orgId = currentBody.data?.org?.id
  expect(orgId, 'an active organization is required for acceptance tests').toBeTruthy()
  const completed = await api.post('/api/v1/onboarding/complete', {
    headers: { 'content-type': 'application/json', origin },
    data: { orgId, plan: 'trial', source: 'verevon-local-e2e' },
  })
  expect(completed.status(), 'onboarding completion should be durable').toBe(200)
  const refreshed = await api.get('/api/v1/session/current')
  expect(refreshed.status()).toBe(200)
  const refreshedBody = await refreshed.json() as { data?: { onboardingStatus?: string } }
  expect(refreshedBody.data?.onboardingStatus).toBe('COMPLETED')

  // Prove the fixture reaches the intended product before running dependent tests.
  await page.goto('/chat')
  await expect(page.locator('.verevon-chat-page')).toBeVisible()

  // Persist cookies (session + active-org) for the local and chat projects.
  await page.context().storageState({ path: STORAGE_STATE })
})
