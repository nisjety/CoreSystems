// Read-only smoke check against an already running, authenticated stack.
// Browser plugin not available: use the repository's Playwright installation.
import { chromium, expect } from '@playwright/test'
import assert from 'node:assert/strict'
import path from 'node:path'
import os from 'node:os'

async function main() {
  const baseURL = process.env.INSIGHTS_BASE_URL || 'http://localhost:5173'
  const storageState = process.env.INSIGHTS_STORAGE_STATE || 'tests/e2e/.auth/local-state.json'
  const browser = await chromium.launch()
  try {
    const context = await browser.newContext({ baseURL, storageState, viewport: { width: 1440, height: 1000 } })
    const page = await context.newPage()
    const session = await context.request.get('/api/v1/session/current')
    assert.equal(session.status(), 200, 'An active session and ready authentication service are required')
    const errors = []
    const consoleErrors = []
    const reads = []
    page.on('pageerror', (error) => errors.push(error.message))
    page.on('console', (message) => { if (message.type() === 'error') consoleErrors.push(message.text().slice(0, 200)) })
    page.on('response', (response) => {
      const url = new URL(response.url())
      if (/^\/api\/v1\/(cost\/(summary|users|azure\/actual)|insights\/overview)$/.test(url.pathname)) {
        reads.push({ path: url.pathname, status: response.status(), scope: url.searchParams.get('scope') })
      }
    })
    const initialCost = page.waitForResponse((response) => new URL(response.url()).pathname === '/api/v1/cost/summary')
    void initialCost.catch(() => {})
    await page.goto('/insights')
    await expect(page.locator('.verevon-insights-page')).toBeVisible()
    const summaryResponse = await initialCost
    assert.equal(summaryResponse.status(), 200)
    const organizationSummary = (await summaryResponse.json()).data
    assert.equal(organizationSummary.scope, 'organization', 'Use an organization owner/admin session')
    assert.equal(organizationSummary.basisBreakdownAvailable, true)
    const reportURL = new URL(summaryResponse.url())
    reportURL.pathname = '/api/v1/insights/overview'
    const outcomeResponse = await context.request.get(reportURL.href)
    assert.equal(outcomeResponse.status(), 200)
    const outcomes = (await outcomeResponse.json()).data
    const verified = outcomes.scorecards.filter((card) => card.coverage === 'measured')
    assert.equal(verified.length, 3, 'Support totals must be verified from retained owner records')
    assert.ok(verified.every((card) => card.source === 'conversation-core' && card.aggregation === 'owner_record_count' && card.checked_at))
    reportURL.pathname = '/api/v1/cost/users'
    const usersResponse = await context.request.get(reportURL.href)
    assert.equal(usersResponse.status(), 200)
    const users = (await usersResponse.json()).data
    assert.equal(users.reduce((sum, row) => sum + row.entry_count, 0), organizationSummary.entryCount)
    assert.equal(users.reduce((sum, row) => sum + row.subscription_entries, 0), organizationSummary.subscriptionEntryCount)
    assert.equal(users.reduce((sum, row) => sum + row.unknown_basis_entries, 0), organizationSummary.unknownBasisEntryCount)
    await expect(page.locator('.verevon-insights-users table')).toBeVisible({ timeout: 30_000 })
    const refresh = page.locator('.verevon-insights-refresh button')
    await expect(refresh).toBeEnabled({ timeout: 30_000 })
    const checked = page.getByTestId('insights-checked-at')
    await expect(checked).toBeVisible()
    const before = await checked.getAttribute('datetime')
    await refresh.click()
    await expect(checked).not.toHaveAttribute('datetime', before, { timeout: 30_000 })
    await expect(refresh).toBeEnabled({ timeout: 30_000 })

    const myRead = page.waitForResponse((response) => {
      const url = new URL(response.url())
      return url.pathname === '/api/v1/cost/summary' && url.searchParams.get('scope') === 'me'
    })
    await page.locator('.verevon-insights-scope button').nth(1).click()
    const personalResponse = await myRead
    assert.equal(personalResponse.status(), 200)
    assert.equal((await personalResponse.json()).data.scope, 'me')
    await expect(page.locator('.verevon-insights-users')).toHaveCount(0)
    await expect(page.getByRole('heading', { name: /Organization charges from Azure|Organisasjonskostnader fra Azure/ })).toHaveCount(0)
    await expect(refresh).toBeEnabled({ timeout: 30_000 })

    const orgRead = page.waitForResponse((response) => {
      const url = new URL(response.url())
      return url.pathname === '/api/v1/cost/summary' && url.searchParams.get('scope') === 'organization'
    })
    await page.locator('.verevon-insights-scope button').nth(0).click()
    assert.equal((await orgRead).status(), 200)
    await expect(page.locator('.verevon-insights-users table')).toBeVisible({ timeout: 30_000 })
    await expect(refresh).toBeEnabled({ timeout: 30_000 })
    const forgedURL = new URL(summaryResponse.url())
    forgedURL.searchParams.set('org_id', 'another-org')
    forgedURL.searchParams.set('user_id', 'another-user')
    const forged = await context.request.get(forgedURL.href)
    assert.equal(forged.status(), 200)
    assert.equal((await forged.json()).data.entryCount, organizationSummary.entryCount)
    reportURL.searchParams.set('org_id', 'another-org')
    reportURL.searchParams.set('user_id', 'another-user')
    const forgedUsers = await context.request.get(reportURL.href)
    assert.equal(forgedUsers.status(), 200)
    assert.deepEqual((await forgedUsers.json()).data, users)
    const invalid = await context.request.get('/api/v1/cost/summary?scope=global')
    assert.equal(invalid.status(), 400)
    const azure = await context.request.get('/api/v1/cost/azure/actual')
    assert.equal(azure.status(), 200)
    assert.equal((await azure.json()).data.available, true)

    // Wait for a real timer-driven request; no clock simulation or API mocks.
    const lastCheck = await checked.getAttribute('datetime')
    await page.waitForResponse((response) => {
      const url = new URL(response.url())
      return url.pathname === '/api/v1/cost/summary' && response.status() === 200
    }, { timeout: 75_000 })
    await expect(checked).not.toHaveAttribute('datetime', lastCheck, { timeout: 30_000 })
    await expect(refresh).toBeEnabled({ timeout: 30_000 })
    assert.equal(await page.locator('vite-error-overlay').count(), 0)
    assert.deepEqual(errors, [])
    assert.deepEqual(consoleErrors, [])
    assert.ok(reads.every((read) => read.status === 200), JSON.stringify(reads))
    const screenshot = path.join(os.tmpdir(), 'verevon-insights-refresh-desktop.png')
    await page.screenshot({ path: screenshot, fullPage: false })
    await page.locator('.verevon-insights-users table').scrollIntoViewIfNeeded()
    const coverageBounds = await page.locator('.verevon-insights-coverage-note').boundingBox()
    assert.ok(coverageBounds && coverageBounds.height < 120, 'Coverage must be a compact notice, not a stretched metric column')
    await page.screenshot({ path: path.join(os.tmpdir(), 'verevon-insights-users-desktop.png'), fullPage: false })
    await refresh.scrollIntoViewIfNeeded()
    await page.setViewportSize({ width: 390, height: 844 })
    const closeNavigation = page.getByRole('button', { name: 'Close navigation' })
    if (await closeNavigation.isVisible()) {
      // The scrim covers the viewport beneath the drawer; click its exposed edge.
      const bounds = await closeNavigation.boundingBox()
      assert.ok(bounds)
      await closeNavigation.click({ position: { x: bounds.width - 12, y: bounds.height / 2 } })
    }
    await expect(refresh).toBeVisible()
    const refreshBounds = await refresh.boundingBox()
    assert.ok(refreshBounds && refreshBounds.x >= 0 && refreshBounds.x + refreshBounds.width <= 390)
    await page.screenshot({ path: path.join(os.tmpdir(), 'verevon-insights-refresh-mobile.png'), fullPage: false })
    await page.locator('.verevon-insights-users table').scrollIntoViewIfNeeded()
    const userBounds = await page.locator('.verevon-insights-users tbody th').first().boundingBox()
    assert.ok(userBounds && userBounds.width >= 180, 'User identifiers must remain readable on mobile')
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'The user table must scroll inside its card, not widen the page')
    await page.screenshot({ path: path.join(os.tmpdir(), 'verevon-insights-users-mobile.png'), fullPage: false })
    assert.deepEqual(errors, [])
    console.log(JSON.stringify({ url: page.url(), title: await page.title(), manualRefresh: 'passed', automaticRefresh: 'passed', scopeSwitch: 'passed', forgedScope: 'ignored', invalidScope: 400, azure: 200, relevantReads: reads.length, runtimeErrors: 0, screenshot }))
  } finally {
    await browser.close()
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1 })
