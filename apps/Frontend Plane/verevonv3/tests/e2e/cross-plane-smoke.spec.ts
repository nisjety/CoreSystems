import { test, expect } from '@playwright/test'

/**
 * Authenticated cross-plane journey (audit proof-ladder L4): one signed-in,
 * org-scoped session proves Frontend → Gateway → Control / Model / Data /
 * Ingestion / Application. Reuses the storageState from auth.setup.ts, so
 * every request carries the verified session + active-org cookies.
 *
 * These are contract-level assertions (status + envelope shape), not a full
 * click-through — they catch the exact failure the audit flagged: protected
 * gateway routes returning 401 because no verified session could be minted.
 */

test('Control: session resolves for the signed-in user', async ({ page }) => {
  const res = await page.request.get('/api/v1/me')
  expect(res.status()).toBe(200)
  const body = await res.json()
  expect(body.user?.email).toBeTruthy()
})

test('Model: model catalog is live', async ({ page }) => {
  const res = await page.request.get('/api/v1/models')
  expect(res.status()).toBe(200)
  const body = await res.json()
  expect(Array.isArray(body.models)).toBeTruthy()
  expect(body.models.length).toBeGreaterThan(0)
})

test('Data + Ingestion + Application: knowledge workspace fans out', async ({ page }) => {
  const res = await page.request.get('/api/v1/knowledge/sources')
  expect(res.status()).toBe(200)
  const body = await res.json()
  const data = body.data ?? body
  // Each fan-out leg is present (empty is honest for a fresh org; the point is
  // the gateway assembled every plane's contribution without a whole-payload
  // failure).
  for (const leg of ['dataPlane', 'graph', 'finspo', 'integrations', 'diagnostics']) {
    expect(data, `knowledge payload missing "${leg}" leg`).toHaveProperty(leg)
  }
})

test('Control: insights overview responds (real or honest empty)', async ({ page }) => {
  const res = await page.request.get('/api/v1/insights/overview')
  // insight-core may report empty/unavailable, but the gateway route must not
  // 404 or 401 for an authenticated org-scoped caller.
  expect([200, 204]).toContain(res.status())
})

test('SPA shell loads for the authenticated session', async ({ page }) => {
  const resp = await page.goto('/')
  expect(resp?.status()).toBeLessThan(400)
  // The app mounts into #root (Solid) — assert the shell rendered, not a blank
  // error page.
  await expect(page.locator('#root')).toBeAttached()
})
