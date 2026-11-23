import { expect, test, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'

/**
 * Client resilience against a PRODUCTION build (WS07-S1..S3; M1-S7 browser cases).
 *
 *   SOURCE_REVISION=e2e-local pnpm exec vite build
 *   pnpm exec vite preview --port 5176 --strictPort
 *   pnpm exec playwright test --project=client-resilience
 *
 * The `client-resilience` project in playwright.config.ts points at CHUNK_E2E_BASE_URL (default
 * http://localhost:5176, shared with chunk-recovery). The plan's E2E_BASE_URL names the signed-in
 * stack (:5199) in this config, so it is not reused here.
 *
 * The build needs a SOURCE_REVISION: without one the build id is `dev`, and the stale-tab and contract
 * checks never run in a dev build, so the banner cases would wait for a request that is never sent.
 *
 * Needs no backend, no sign-in and no live stack (the project has no setup dependency). Each case blocks
 * one lazy chunk, or answers `/version.json` or the client-config call, and checks what the client itself
 * does: one automatic reload, the stale-deploy card in the region that failed (and only there), and
 * non-blocking banners for a new build or a stale contract that never reload the page on their own.
 *
 * The M1-S7 cases that need Space data or a signed-in room (a throw in one tab of a real room, the
 * restricted-read notice, a `javascript:` link rendered as text, the artifact turn and the ZDR storage
 * checks) are live-stack proofs; their unit coverage is SpaceCockpit.test.tsx, SpaceRoomTimeline.test.tsx
 * and chat-markdown.test.ts.
 */

const NOTICE = '[data-testid="stale-build-notice"]'
const NEW_VERSION_BANNER = '[data-testid="new-version-banner"]'
const CONTRACT_BANNER = '[data-testid="client-contract-banner"]'
const STALE_DEPLOY_CARD = '[role="alert"][data-failure-class="stale_deploy"]'
const CLIENT_CONFIG = '**/api/v1/client-config'
const VERSION_JSON = '**/version.json'

/**
 * This bundle's contract, read from the source so a contract bump keeps the stale cases stale
 * (one above it) and the current case current (equal to it).
 */
const CLIENT_CONTRACT = (() => {
  const source = readFileSync(new URL('../../src/shared/build/client-contract.ts', import.meta.url), 'utf8')
  const match = /export const CLIENT_CONTRACT = (\d+)/.exec(source)
  if (!match) throw new Error('CLIENT_CONTRACT not found in src/shared/build/client-contract.ts')
  return Number(match[1])
})()

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
          contract: Math.max(CLIENT_CONTRACT, minContract),
          min_contract: minContract,
          policy,
          revision: 'e2e-client-resilience',
          build_date: '2026-11-23',
          environment: 'development',
        },
      }),
    }),
  )

/** Two animation frames: whatever the last response set has rendered. */
const settleRender = (page: Page) =>
  page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))))

/**
 * Re-run the stale-tab and contract checks the way a returning tab does: move the page clock past the
 * 30 s throttle and fire `online`, one of the triggers `startVersionCheck` listens to.
 */
const triggerRecheck = async (page: Page) => {
  await page.clock.fastForward('00:31')
  await page.evaluate(() => window.dispatchEvent(new Event('online')))
}

test.describe('lazy chunk failure inside the shell', () => {
  test('one automatic reload, then the stale-deploy card in the outlet only, and the shell stays', async ({ page }) => {
    // A route inside the shell, so its failure reaches the outlet's boundary. A standalone surface such as /login
    // draws the stale-build notice only (AppShell's RoutedFailure hides chunk errors), which chunk-recovery.spec covers.
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

test.describe('a deploy under an open tab', () => {
  test('the new-version banner appears, the page never reloads, and typed input is kept', async ({ page }) => {
    await page.clock.install()
    // The first check reaches the preview server, which serves this build's own version.json: no banner.
    let liveBuild: string | null = null
    await page.route(VERSION_JSON, (route) =>
      liveBuild === null
        ? route.fallback()
        : route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ build_id: liveBuild }) }),
    )
    const firstCheck = page.waitForResponse(VERSION_JSON)
    const loads = countDocumentLoads(page)

    await page.goto('/login')
    await firstCheck
    const email = page.locator('input[type="email"]').first()
    await email.fill('draft@example.com')
    await settleRender(page)
    await expect(page.locator(NEW_VERSION_BANNER)).toHaveCount(0)

    // The deploy: the server now names another build. The open tab learns it on its next check.
    liveBuild = 'e2e-client-resilience-next'
    const swapCheck = page.waitForResponse(VERSION_JSON)
    await triggerRecheck(page)
    await swapCheck

    const banner = page.locator(NEW_VERSION_BANNER)
    await expect(banner).toBeVisible({ timeout: 10_000 })
    await expect(banner).toHaveAttribute('role', 'status')
    await expect(banner.getByRole('button', { name: /last (inn )?på nytt|reload/i })).toBeVisible()

    // The user decides when to reload: the page stays, and what they typed is still there.
    await page.clock.runFor(5_000)
    expect(loads.count).toBe(1)
    await expect(email).toHaveValue('draft@example.com')
  })
})

test.describe('stale client contract', () => {
  test('a contract below the gateway minimum shows the non-blocking banner and never reloads', async ({ page }) => {
    await answerClientConfig(page, CLIENT_CONTRACT + 1, 'banner')
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
    await answerClientConfig(page, CLIENT_CONTRACT + 1, 'enforce')
    const loads = countDocumentLoads(page)

    await page.goto('/login')

    await expect(page.locator(CONTRACT_BANNER)).toBeVisible({ timeout: 20_000 })
    await page.waitForTimeout(3_000)
    expect(loads.count).toBe(1)
  })

  test('a contract the gateway still serves shows no banner, and the same page shows one once it is stale', async ({
    page,
  }) => {
    await page.clock.install()
    let minContract = CLIENT_CONTRACT
    await page.route(CLIENT_CONFIG, (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          data: { contract: CLIENT_CONTRACT + 1, min_contract: minContract, policy: 'banner', revision: 'e2e', build_date: '2026-11-23' },
        }),
      }),
    )
    const firstConfig = page.waitForResponse(CLIENT_CONFIG)

    await page.goto('/login')
    await (await firstConfig).finished()
    await settleRender(page)
    await expect(page.locator(CONTRACT_BANNER)).toHaveCount(0)

    // Control: the very same check path on the very same page draws the banner once the minimum moves past
    // this bundle, so the absence above is the answer to a served contract, not a check that never ran.
    minContract = CLIENT_CONTRACT + 1
    const staleConfig = page.waitForResponse(CLIENT_CONFIG)
    await triggerRecheck(page)
    await staleConfig
    await expect(page.locator(CONTRACT_BANNER)).toBeVisible({ timeout: 10_000 })
  })
})
