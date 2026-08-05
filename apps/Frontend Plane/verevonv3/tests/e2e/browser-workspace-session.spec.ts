import { expect, test } from '@playwright/test'

import { closeSessionViaApi, closeSessionViaUi, EXAMPLE_URL, resetHome, submitBrowserUrl } from './browser-workspace-helpers'

/**
 * Phase 6 capability 1: open the browser workspace, create a session,
 * navigate, and confirm the unified chrome (Phase 1) renders real page
 * state — driven through the actual Home → "Innhent" UI, not the raw
 * gateway API (see `browser-live.spec.ts` for the API-level equivalent,
 * run as `e2e@verevon.dev`).
 */
test.describe('browser workspace: session creation and unified chrome', () => {
  let sessionId: string | null = null

  test.afterEach(async ({ request }) => {
    await closeSessionViaApi(request, sessionId)
    sessionId = null
  })

  test('creating a session renders the unified chrome with real page state, and the address bar drives real navigation', async ({ page }) => {
    test.setTimeout(90_000)
    await resetHome(page)

    sessionId = await submitBrowserUrl(page, EXAMPLE_URL)

    // Unified chrome: status badge, render-mode badge, address bar, and a
    // real rendered page image all reflect the actual live Quarry session.
    await expect(page.locator('.knowledge-browser-status--live')).toHaveText('Live')
    await expect(page.locator('.knowledge-browser-mode-badge')).toHaveText('Chromium')
    const addressInput = page.getByLabel('Nettleseradresse')
    await expect(addressInput).toHaveValue(EXAMPLE_URL)
    await expect(page.getByAltText(/Gjengitt nettleserside for .+/)).toBeVisible({ timeout: 20_000 })

    // Drive real navigation through the address bar (not the API) and
    // confirm the chrome picks up the new page's real state. IANA's own
    // "example domains" help page (not `httpbin.org`, which is a free,
    // occasionally-503ing community service unsuitable for a deterministic
    // regression check) — stable, IANA-maintained, and its title/content are
    // unambiguously distinct from the initial `example.com` page.
    const secondUrl = 'https://www.iana.org/help/example-domains'
    await addressInput.fill(secondUrl)
    await page.getByLabel('Naviger til adresse').click()
    await expect(addressInput).toHaveValue(secondUrl, { timeout: 20_000 })
    await expect(page.getByAltText(/Gjengitt nettleserside for .*Example Domains.*/i)).toBeVisible({ timeout: 20_000 })

    // Closing through the UI performs a real DELETE — verify it actually
    // leaves the composer's "live" state (not just a client-side illusion).
    await closeSessionViaUi(page)
    await expect(page.locator('.knowledge-browser-status--live')).toHaveCount(0)
  })
})
