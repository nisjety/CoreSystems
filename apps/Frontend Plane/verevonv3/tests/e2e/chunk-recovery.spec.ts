import { expect, test, type Page } from '@playwright/test'

/**
 * Stale-build recovery against a PRODUCTION build (WS09 S5).
 *
 *   pnpm exec vite build
 *   pnpm exec vite preview --port 5176 --strictPort
 *   pnpm exec playwright test --project=chunk-recovery
 *
 * A stale tab is simulated by answering the sign-in page's lazy chunk with 404,
 * which is what a deploy that replaced the hashed files does to an old tab. No
 * backend is needed: a signed-out visit lands on the sign-in route, whose chunk
 * is the one that fails. Vite fires `vite:preloadError` in production builds only,
 * so this cannot run against `pnpm dev`.
 */

const RELOAD_KEY = 'verevon.chunk-reload.v1'
const NOTICE = '[data-testid="stale-build-notice"]'

/**
 * Count main-frame document loads from here on: the visit and every reload. `framenavigated` would also
 * count the router's same-document replace on boot (measured 2026-10-28: three events for two loads), so
 * the document requests are counted instead.
 */
const countNavigations = (page: Page) => {
  const counter = { count: 0 }
  page.on('request', (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) counter.count += 1
  })
  return counter
}

const blockSignInChunk = (page: Page) =>
  page.route('**/assets/AuthPage-*.js', (route) => route.fulfill({ status: 404, body: 'gone' }))

test.describe('stale chunk recovery', () => {
  test('reloads once, then shows the notice and never navigates a third time', async ({ page }) => {
    await blockSignInChunk(page)
    const navigations = countNavigations(page)

    await page.goto('/login')
    await expect(page.locator(NOTICE)).toBeVisible({ timeout: 20_000 })

    // The first navigation is the visit itself, the second is the automatic reload.
    expect(navigations.count).toBe(2)
    expect(await page.evaluate((key) => window.sessionStorage.getItem(key), RELOAD_KEY)).not.toBeNull()

    // Nothing else navigates once the notice is up.
    await page.waitForTimeout(3_000)
    expect(navigations.count).toBe(2)
  })

  test('the notice can be dismissed and offers a reload', async ({ page }) => {
    await blockSignInChunk(page)
    await page.goto('/login')
    const notice = page.locator(NOTICE)
    await expect(notice).toBeVisible({ timeout: 20_000 })
    await expect(notice.getByRole('button', { name: /reload|last på nytt/i })).toBeVisible()

    await notice.getByRole('button', { name: /dismiss|lukk/i }).click()
    await expect(notice).toBeHidden()
  })

  test('does not reload on its own when the reload marker is already set', async ({ page }) => {
    await blockSignInChunk(page)
    await page.addInitScript((key) => {
      window.sessionStorage.setItem(key, String(Date.now()))
    }, RELOAD_KEY)
    const navigations = countNavigations(page)

    await page.goto('/login')
    await expect(page.locator(NOTICE)).toBeVisible({ timeout: 20_000 })
    expect(navigations.count).toBe(1)
  })
})
