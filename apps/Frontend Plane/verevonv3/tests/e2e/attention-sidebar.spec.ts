import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * Attention rail (WS16 S2): a thread that waits on the person sits in the Waiting section
 * above the chat list, the tab title counts it, and the rail stays out of the way when
 * nothing waits. The BFF routes are answered by the spec, so the case needs no model and no
 * live Session Core; the envelope is `{ data }` as the BFF sends it. Selectors are classes
 * and roles, never copy, so the spec holds for either locale.
 */

const CHAT = '/chat'
// A RegExp, not a glob: Playwright anchors a glob at the end, which would skip the query string.
const THREADS = /\/api\/v1\/chat\/threads(\?.*)?$/
const ATTENTION = /\/api\/v1\/attention$/

type Thread = { threadId: string; title: string; updatedAt: string; latestRunStatus?: string }

const WAITING: Thread = {
  threadId: 'thread-waiting',
  title: 'Approve the export',
  updatedAt: '2026-10-30T09:00:00Z',
  latestRunStatus: 'awaiting_approval',
}
const PLAIN: Thread = { threadId: 'thread-plain', title: 'Plain question', updatedAt: '2026-10-30T08:00:00Z' }

async function json(route: Route, body: unknown) {
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

async function serveAttention(page: Page, waiting: boolean) {
  await page.route(THREADS, async (route) => {
    await json(route, { sessions: [WAITING, PLAIN], nextCursor: null })
  })
  await page.route(ATTENTION, async (route) => {
    const items = waiting
      ? [{ threadId: WAITING.threadId, state: 'needs_approval', reason: 'approval', since: '2026-10-30T09:00:00Z', unseen: false, runId: 'run-1' }]
      : []
    await json(route, {
      enabled: true,
      items,
      counts: { needsYou: items.length, working: 0, unseen: 0 },
      capabilities: { stream: false, push: false },
    })
  })
}

test.describe('attention rail', () => {
  test('a thread waiting on an approval sits above the list and counts in the tab title', async ({ page }) => {
    await serveAttention(page, true)
    await page.goto(CHAT)

    const rail = page.locator('.chat-attention')
    await expect(rail).toBeVisible()
    await expect(rail.locator('.core-chat-session', { hasText: WAITING.title })).toBeVisible()
    await expect(page).toHaveTitle(/^\(1\) /)

    // The rail comes before the first row of the list it sits above.
    const firstListRow = page.locator('.core-chat-session', { hasText: PLAIN.title }).first()
    const railBox = await rail.boundingBox()
    const listBox = await firstListRow.boundingBox()
    expect(railBox && listBox && railBox.y < listBox.y).toBe(true)
  })

  test('no section and no count appear when nothing waits', async ({ page }) => {
    await serveAttention(page, false)
    await page.goto(CHAT)

    await expect(page.locator('.core-chat-session', { hasText: PLAIN.title })).toBeVisible()
    await expect(page.locator('.chat-attention')).toHaveCount(0)
    await expect(page).not.toHaveTitle(/^\(\d+\) /)
  })
})
