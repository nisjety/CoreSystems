import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * Chat history archive surface (WS17 S2): a thread archived from its row menu
 * leaves the ordinary list, appears under "Show archived", and comes back
 * through Restore. The BFF routes are answered by the spec, so the case needs
 * no model and no live Session Core; the envelope is `{ data }` as the BFF
 * sends it. Selectors are roles and classes, never copy, so the spec holds for
 * either locale; the thread titles are what the rows must show.
 */

const CHAT = '/chat'
// A RegExp, not a glob: Playwright anchors a glob at the end, which would skip
// the archived listing's `?archived=` query string.
const THREADS = /\/api\/v1\/chat\/threads(\?.*)?$/
const SEARCH = /\/api\/v1\/chat\/threads\/search(\?.*)?$/
const archiveRoute = (threadId: string, verb: 'archive' | 'unarchive') =>
  `**/api/v1/chat/threads/${threadId}/${verb}`

type Thread = { threadId: string; title: string; updatedAt: string; archivedAt?: string }

const KEEP: Thread = { threadId: 'thread-keep', title: 'Keep this chat', updatedAt: '2026-10-20T10:00:00Z' }
const ARCHIVE: Thread = { threadId: 'thread-archive', title: 'Archive this chat', updatedAt: '2026-10-19T10:00:00Z' }

// The row's title span: the button's accessible name also carries the date and
// the row's action names, so the title is matched on its own element.
function rowFor(page: Page, title: string) {
  return page.locator('span.verevon-sidebar-row-strong', { hasText: title })
}

async function json(route: Route, body: unknown) {
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

async function serveHistory(page: Page, state: { active: Thread[]; archived: Thread[] }) {
  await page.route(THREADS, async (route) => {
    const url = new URL(route.request().url())
    const archived = url.searchParams.get('archived')
    const list = archived === 'archived' ? state.archived : state.active
    await json(route, { sessions: list, nextCursor: null })
  })
  await page.route(archiveRoute(ARCHIVE.threadId, 'archive'), async (route) => {
    state.archived = [{ ...ARCHIVE, archivedAt: '2026-10-24T09:00:00Z' }]
    state.active = state.active.filter((item) => item.threadId !== ARCHIVE.threadId)
    await json(route, { session: state.archived[0], transcript: null })
  })
  await page.route(archiveRoute(ARCHIVE.threadId, 'unarchive'), async (route) => {
    state.active = [...state.active, ARCHIVE]
    state.archived = state.archived.filter((item) => item.threadId !== ARCHIVE.threadId)
    await json(route, { session: ARCHIVE, transcript: null })
  })
}

test.describe('chat history archive surface', () => {
  test('archive hides a thread, show archived lists it, and restore brings it back', async ({ page }) => {
    const state = { active: [KEEP, ARCHIVE], archived: [] as Thread[] }
    await serveHistory(page, state)
    await page.goto(CHAT)

    await page.getByRole('button', { name: `More options for ${ARCHIVE.title}` }).click()
    await page.getByRole('menuitem', { name: 'Archive' }).click()
    await expect(rowFor(page, ARCHIVE.title)).toHaveCount(0)

    await page.getByRole('button', { name: 'Show archived' }).click()
    await expect(rowFor(page, ARCHIVE.title)).toBeVisible()

    await page.getByRole('button', { name: `Restore ${ARCHIVE.title}` }).click()
    await page.getByRole('button', { name: 'Show active chats' }).click()
    await expect(rowFor(page, ARCHIVE.title)).toBeVisible()
  })

  test('search finds a chat beyond the loaded page by a word in its body', async ({ page }) => {
    // Not in the first page of the list, so only the server search can find it.
    const OLD: Thread = { threadId: 'thread-old', title: 'Quarterly billing', updatedAt: '2025-01-03T10:00:00Z' }
    const state = { active: [KEEP], archived: [] as Thread[] }
    await serveHistory(page, state)
    const searched: string[] = []
    await page.route(SEARCH, async (route) => {
      searched.push(new URL(route.request().url()).searchParams.get('q') ?? '')
      await json(route, {
        hits: [{
          threadId: OLD.threadId,
          messageId: 'message-4',
          role: 'assistant',
          snippet: 'the kvittering was sent',
          createdAt: OLD.updatedAt,
          threadTitle: OLD.title,
          archived: false,
        }],
      })
    })
    await page.goto(CHAT)

    await page.getByRole('button', { name: 'Search chats' }).click()
    await page.getByRole('combobox', { name: 'Search chats' }).fill('kvittering')

    const option = page.getByRole('option', { name: /Quarterly billing/ })
    await expect(option).toBeVisible({ timeout: 1000 })
    await expect(option.locator('mark')).toHaveText('kvittering')
    expect(searched).toEqual(['kvittering'])
  })
})
