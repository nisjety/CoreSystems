import { expect, test, type Page, type Request } from '@playwright/test'
import { execSync } from 'node:child_process'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS02-S5 realtime room, end to end (later tier: needs the seeded Space fixtures, the Space event
 * stream route on, and human posts on so a post needs no model). Member A owns the shared room, B
 * is an editor in it (scripts/space-e2e-seed.sh). No call here starts a model run and nothing
 * selects a Verevon, budget or genius model: the traffic is members' own posts.
 */

const spaceUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`

const box = (page: Page) => page.getByRole('textbox').first()
const sendButton = (page: Page) => page.getByRole('button', { name: /^(Send|Send til rommet)$/ })
const liveIndicator = (page: Page) => page.locator('.verevon-space-live[data-state="live"]')
const degradedIndicator = (page: Page) => page.locator('.verevon-space-live[data-state="degraded"]')

const isThreadsListing = (request: Request) =>
  request.method() === 'GET' && /\/api\/v1\/spaces\/[^/]+\/threads(\?|$)/.test(request.url())
const isEventsStream = (request: Request) =>
  request.method() === 'GET' && /\/api\/v1\/spaces\/[^/]+\/events(\?|$)/.test(request.url())

function p95(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1)] ?? Number.POSITIVE_INFINITY
}

test('B sees A’s post within 1.5 s (p95) and the room runs no 6 s listing timer while live', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    await pageA.goto(spaceUrl(ref))
    await pageB.goto(spaceUrl(ref))
    await expect(liveIndicator(pageB)).toBeVisible({ timeout: 15_000 })

    // Count B's listing reads while nothing happens: a live room does not poll every 6 s.
    const listingReads: number[] = []
    pageB.on('request', (request) => {
      if (isThreadsListing(request)) listingReads.push(Date.now())
    })
    await pageB.waitForTimeout(20_000)
    expect(listingReads.length, 'a live idle room re-reads its listing only on events and visibility').toBeLessThanOrEqual(2)

    const marker = `rt-${Date.now().toString(36)}`
    const latencies: number[] = []
    for (let i = 0; i < 6; i += 1) {
      const text = `Sanntid ${marker} ${i}`
      await box(pageA).fill(text)
      const sentAt = Date.now()
      await sendButton(pageA).click()
      await expect(pageB.getByText(text)).toBeVisible({ timeout: 10_000 })
      latencies.push(Date.now() - sentAt)
      // Keep each sample clear of the controller's 1.5 s re-read spacing.
      await pageB.waitForTimeout(1_600)
    }
    expect(p95(latencies), `latencies ${latencies.join(', ')} ms`).toBeLessThanOrEqual(1_500)
  } finally {
    await a.close()
    await b.close()
  }
})

test('a dropped connection resumes by Last-Event-ID and the room shows no gap', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    const streams: Array<{ lastEventId: string | undefined }> = []
    pageB.on('request', (request) => {
      if (isEventsStream(request)) streams.push({ lastEventId: request.headers()['last-event-id'] })
    })
    await pageA.goto(spaceUrl(ref))
    await pageB.goto(spaceUrl(ref))
    await expect(liveIndicator(pageB)).toBeVisible({ timeout: 15_000 })

    // B drops off the network; A posts meanwhile.
    await b.setOffline(true)
    const marker = `gap-${Date.now().toString(36)}`
    await box(pageA).fill(`Mens B var borte ${marker}`)
    await sendButton(pageA).click()

    await b.setOffline(false)
    await expect(pageB.getByText(`Mens B var borte ${marker}`)).toBeVisible({ timeout: 40_000 })
    // The second attempt names its position: <generation>.<seq>.
    expect(streams.length).toBeGreaterThanOrEqual(2)
    expect(streams[streams.length - 1]?.lastEventId).toMatch(/^[^.]+\.\d+$/)
  } finally {
    await a.close()
    await b.close()
  }
})

test('stopping session-core degrades to the 6 s poll within 35 s, and the room recovers', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  const stopCommand = process.env.E2E_SPACE_STOP_SESSION_CORE_CMD?.trim()
  const startCommand = process.env.E2E_SPACE_START_SESSION_CORE_CMD?.trim()
  skipUnlessSpace(test, !stopCommand || !startCommand, 'E2E_SPACE_STOP_SESSION_CORE_CMD and E2E_SPACE_START_SESSION_CORE_CMD must stop and start session-core on the test stack')
  requireMembers(test, 'B')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const page = await b.newPage()
    await page.goto(spaceUrl(ref))
    await expect(liveIndicator(page)).toBeVisible({ timeout: 15_000 })

    const listingReads: number[] = []
    page.on('request', (request) => {
      if (isThreadsListing(request)) listingReads.push(Date.now())
    })

    execSync(stopCommand ?? '', { stdio: 'ignore' })
    try {
      await expect(degradedIndicator(page)).toBeVisible({ timeout: 35_000 })
      // Degraded means today's behaviour: the listing is read on a ~6 s timer again.
      const before = listingReads.length
      await page.waitForTimeout(14_000)
      expect(listingReads.length - before).toBeGreaterThanOrEqual(2)
    } finally {
      execSync(startCommand ?? '', { stdio: 'ignore' })
    }
    await expect(liveIndicator(page)).toBeVisible({ timeout: 90_000 })
  } finally {
    await b.close()
  }
})

test('a zero-retention organisation stays on the poll: no live indicator and the listing is read on a timer', async ({ browser, baseURL }) => {
  const zdrRef = process.env.E2E_SPACE_REF_ZDR?.trim()
  skipUnlessSpace(test, !zdrRef, 'E2E_SPACE_REF_ZDR must name a Space in the ZDR fixture organisation')
  requireMembers(test, 'F')
  const f = await memberContext(browser, baseURL, 'F')
  try {
    const page = await f.newPage()
    const listingReads: number[] = []
    page.on('request', (request) => {
      if (isThreadsListing(request)) listingReads.push(Date.now())
    })
    await page.goto(spaceUrl(zdrRef ?? ''))
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
    await page.waitForTimeout(15_000)
    await expect(page.locator('.verevon-space-live')).toHaveCount(0)
    // The first read plus at least two timer reads in 15 s.
    expect(listingReads.length).toBeGreaterThanOrEqual(3)
  } finally {
    await f.close()
  }
})
