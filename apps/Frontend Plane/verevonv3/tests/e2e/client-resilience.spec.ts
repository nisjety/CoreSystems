import { expect, test, type Page } from '@playwright/test'

/**
 * Client resilience against a PRODUCTION build (WS07-S1..S3; M1-S7 browser cases).
 *
 *   pnpm exec vite build
 *   pnpm exec vite preview --port 5176 --strictPort
 *   CLIENT_RESILIENCE_E2E=1 CHUNK_E2E_BASE_URL=http://localhost:5176 pnpm exec playwright test tests/e2e/client-resilience.spec.ts
 *
 * Needs no backend and no live stack. Each case blocks one lazy chunk, or answers the
 * client-config call, and checks what the client itself does: one automatic reload, the
 * stale-deploy card in the region that failed (and only there), and a non-blocking banner
 * for a stale contract that never reloads the page on its own.
 *
 * Opt-in: the project that runs this file (a production-preview project, like chunk-recovery's)
 * is not wired in playwright.config.ts yet, so the default `e2e` project would otherwise run it
 * against the dev stack, where Vite never fires `vite:preloadError`.
 */

const BASE_URL = process.env.CHUNK_E2E_BASE_URL || 'http://localhost:5176'
const ENABLED = process.env.CLIENT_RESILIENCE_E2E === '1'
const NOTICE = '[data-testid="stale-build-notice"]'
const CONTRACT_BANNER = '[data-testid="client-contract-banner"]'
const STALE_DEPLOY_CARD = '[role="alert"][data-failure-class="stale_deploy"]'
const CLIENT_CONFIG = '**/api/v1/client-config'

const SKIP_REASON = 'Set CLIENT_RESILIENCE_E2E=1 and point CHUNK_E2E_BASE_URL at a production preview.'
test.use({ baseURL: BASE_URL })

/** Main-frame document loads from here on: the visit and every automatic reload. */
const countDocumentLoads = (page: Page) => {
  const counter = { count: 0 }
  page.on('request', (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) counter.count += 1
  })
  return counter
}

/** Answers the client-config call with a contract the bundle may or may not meet. */
const answerClientConfig = (page: Page, minContract: number, policy: 'banner' | 'enforce') =>
  page.route(CLIENT_CONFIG, (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        data: {
          contract: 1,
          min_contract: minContract,
          policy,
          revision: 'e2e-client-resilience',
          build_date: '2026-11-23',
          environment: 'development',
        },
      }),
    }),
  )

test.describe('lazy chunk failure inside the shell', () => {
  test.skip(!ENABLED, SKIP_REASON)

  test('one automatic reload, then the stale-deploy card in the outlet only, and the shell stays', async ({ page }) => {
    // A route inside the shell (not a standalone surface), so its failure reaches the outlet's boundary.
    await page.route('**/assets/NotFoundPage-*.js', (route) => route.fulfill({ status: 404, body: 'gone' }))
    const loads = countDocumentLoads(page)

    await page.goto('/client-resilience-missing-surface')

    const card = page.locator(STALE_DEPLOY_CARD)
    await expect(card).toBeVisible({ timeout: 20_000 })
    await expect(card).toHaveAttribute('data-failure-region', 'outlet')
    // The visit and the one automatic reload; nothing navigates after the card is up.
    expect(loads.count).toBe(2)
    await expect(page.locator(NOTICE)).toBeVisible()

    // The failure is contained: the shell around the outlet is still on screen, and only one card is drawn.
    await expect(page.locator('main.core-main')).toBeVisible()
    await expect(page.locator(STALE_DEPLOY_CARD)).toHaveCount(1)

    // A stale build cannot be retried into health, so the card offers Reload and no Retry.
    await expect(card.getByRole('button', { name: /last (inn )?på nytt|reload/i })).toBeVisible()
    await expect(card.getByRole('button', { name: /prøv igjen|retry/i })).toHaveCount(0)

    await page.waitForTimeout(3_000)
    expect(loads.count).toBe(2)
  })
})

test.describe('stale client contract', () => {
  test.skip(!ENABLED, SKIP_REASON)

  test('a contract below the gateway minimum shows the non-blocking banner and never reloads', async ({ page }) => {
    await answerClientConfig(page, 2, 'banner')
    const loads = countDocumentLoads(page)

    await page.goto('/login')

    const banner = page.locator(CONTRACT_BANNER)
    await expect(banner).toBeVisible({ timeout: 20_000 })
    await expect(banner).toHaveAttribute('role', 'status')
    await expect(banner.getByRole('button', { name: /last (inn )?på nytt|reload/i })).toBeVisible()

    await page.waitForTimeout(3_000)
    expect(loads.count).toBe(1)
  })

  test('policy enforce with no unsaved-work probe registered still only shows the banner', async ({ page }) => {
    // The sign-in page registers no unsaved-work probe, so `enforce` cannot judge a reload safe and must not reload.
    await answerClientConfig(page, 2, 'enforce')
    const loads = countDocumentLoads(page)

    await page.goto('/login')

    await expect(page.locator(CONTRACT_BANNER)).toBeVisible({ timeout: 20_000 })
    await page.waitForTimeout(3_000)
    expect(loads.count).toBe(1)
  })

  test('a contract the gateway still serves shows no banner at all', async ({ page }) => {
    await answerClientConfig(page, 1, 'banner')
    const configRequest = page.waitForRequest(CLIENT_CONFIG)

    await page.goto('/login')
    await configRequest
    await page.waitForTimeout(1_000)

    await expect(page.locator(CONTRACT_BANNER)).toHaveCount(0)
  })
})
