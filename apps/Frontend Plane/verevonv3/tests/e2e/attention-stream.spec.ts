import { expect, test, type BrowserContext, type Route } from '@playwright/test'

/**
 * Attention stream (WS16 S5): one browser profile holds one connection to the per-person attention stream,
 * however many tabs it has open, and a refused stream leaves the shell working. The BFF routes are answered
 * by the spec, so the case needs no model and no live Session Core; the envelope is `{ data }` as the BFF
 * sends it.
 */

const CHAT = '/chat'
// RegExps, not globs: Playwright anchors a glob at the end, and the listing and the stream are sibling paths.
const ATTENTION = /\/api\/v1\/attention$/
const STREAM = /\/api\/v1\/attention\/stream$/

const LISTING = {
  enabled: true,
  items: [],
  counts: { needsYou: 0, working: 0, unseen: 0 },
  capabilities: { stream: true, push: false },
}

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

/** Serves the listing with the stream on, and holds each stream request open so the first tab keeps the lock. */
async function serveHeldStream(context: BrowserContext, hits: { count: number }) {
  await context.route(ATTENTION, async (route) => {
    await json(route, LISTING)
  })
  await context.route(STREAM, async () => {
    hits.count += 1
    await new Promise(() => undefined)
  })
}

test.describe('attention stream', () => {
  test('two tabs of one browser profile share one stream connection', async ({ browser }) => {
    const context = await browser.newContext()
    const hits = { count: 0 }
    await serveHeldStream(context, hits)
    try {
      const first = await context.newPage()
      await first.goto(CHAT)
      await expect.poll(() => hits.count).toBe(1)

      const second = await context.newPage()
      await second.goto(CHAT)
      // The second tab waits for the lock the first tab holds, so it opens no connection of its own.
      await second.waitForTimeout(1_500)
      expect(hits.count).toBe(1)
    } finally {
      await context.close()
    }
  })

  test('a refused stream leaves the shell and its rail working', async ({ page }) => {
    await page.route(ATTENTION, async (route) => {
      await json(route, LISTING)
    })
    await page.route(STREAM, async (route) => {
      await route.fulfill({
        status: 412,
        contentType: 'application/json',
        body: JSON.stringify({ error: { code: 'thread_persistence_not_permitted', message: 'attention_stream_disabled' } }),
      })
    })

    await page.goto(CHAT)

    await expect(page.locator('#core-workspace-sidebar')).toBeVisible()
  })
})
