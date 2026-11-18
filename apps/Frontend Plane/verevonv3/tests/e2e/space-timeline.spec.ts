import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * WS15-S1 and WS15-S8 timeline of a long room, windowed, in a mock. This needs only the Vite server at E2E_BASE_URL:
 * every Space call is answered by page.route, so there is no backend and no model. A member's own posts are the only
 * writes, and the mock does not start a run.
 *
 * The context turns on timeline_windows and timeline_anchor, because a feature absent from `features` is off and both
 * the windowed listing and the jump pill depend on them. The payloads follow timeline.ts (page, threads, membership)
 * and SpaceThread in core.ts. They are not yet checked against a live response, so the first run may need the mock's
 * context or session fields adjusted. The session is answered by /api/v1/me.
 */

const REF = 'mock-room'
const MEMBER = 'mock-a'
const PAGE_SIZE = 50
const EARLIER_PAGE_DELAY_MS = 800

type Room = { total: number; timelineReads: number; beforeReads: number; transcripts: number; inFlight: number; maxInFlight: number }

const CONTEXT = {
  space: { id: REF, space_id: REF, name: 'Mock-rommet', lifecycle: 'active' },
  membership: { role: 'owner', subject_id: MEMBER, org_id: 'mock-org' },
  features: { human_posts: true, thread_pane: true, timeline_windows: true, timeline_anchor: true },
  message_policy: null,
  limits: {},
}

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

const box = (page: Page) => page.getByRole('textbox').first()

function post(index: number) {
  return {
    thread_id: `thread-${String(index).padStart(5, '0')}`,
    space_id: REF,
    title: `Innlegg ${index}`,
    preview: `Forhåndsvisning ${index}`,
    updated_at: new Date(Date.UTC(2026, 0, 1, 0, 0, index)).toISOString(),
    last_message_seq: index + 1,
    turn_count: 1,
    reply_count: 0,
    owner_subject_id: MEMBER,
    last_author_subject_id: MEMBER,
    pinned: false,
  }
}

/** One window of the room, oldest first. `before` is the cursor the previous page returned (c:<lowest index>). */
function roomWindow(room: Room, before: string | null) {
  const upper = before ? Number(before.slice(2)) : room.total
  const lower = Math.max(0, upper - PAGE_SIZE)
  return {
    page: {
      windowed: true,
      has_more: lower > 0,
      next_before: lower > 0 ? `c:${lower}` : null,
      next_after: null,
      newest: `c:${room.total}`,
      ...(before ? {} : { total_posts: room.total }),
    },
    threads: Array.from({ length: upper - lower }, (_, offset) => post(lower + offset)),
    pinned: [],
    refreshed: [],
    system_entries: [],
    system_truncated: false,
    membership: { role: 'owner', subject_id: MEMBER, org_id: 'mock-org' },
    pins: [],
    read_marker: null,
    shared_read: null,
    agent_work: null,
    unavailable: [],
  }
}

async function mockRoom(page: Page, total: number): Promise<Room> {
  const room: Room = { total, timelineReads: 0, beforeReads: 0, transcripts: 0, inFlight: 0, maxInFlight: 0 }
  // Registered first, so it is the lowest priority: anything not mocked below answers 404 and nothing hangs.
  await page.route(/\/api\/v1\//, (route) => json(route, { error: { code: 'not_mocked' } }, 404))
  await page.route(/\/api\/v1\/me$/, (route) =>
    json(route, { data: { id: MEMBER, subject_id: MEMBER, org_id: 'mock-org', email: 'mock-a@example.test' } }),
  )
  await page.route(new RegExp(`/api/v1/spaces/${REF}/context`), (route) => json(route, { data: CONTEXT }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/timeline`), async (route) => {
    const before = new URL(route.request().url()).searchParams.get('before')
    if (before) {
      room.beforeReads += 1
      // A slow earlier page, so the reader's anchor can be read before the prepend lands.
      await new Promise((resolve) => setTimeout(resolve, EARLIER_PAGE_DELAY_MS))
    } else {
      room.timelineReads += 1
    }
    await json(route, { data: roomWindow(room, before) })
  })
  await page.route(new RegExp(`/api/v1/spaces/${REF}/threads/[^/]+/transcript`), async (route) => {
    room.transcripts += 1
    room.inFlight += 1
    room.maxInFlight = Math.max(room.maxInFlight, room.inFlight)
    await new Promise((resolve) => setTimeout(resolve, 250))
    room.inFlight -= 1
    await json(route, { data: { transcript: { threadId: '', turns: [], window: { omitted_before: 0, has_more_before: false } } } })
  })
  await page.route(new RegExp(`/api/v1/spaces/${REF}/threads$`), async (route) => {
    if (route.request().method() !== 'POST') return json(route, { data: { threads: [] } })
    room.total += 1
    return json(route, { data: { thread_id: `thread-${String(room.total - 1).padStart(5, '0')}`, message_id: `m-${room.total}`, sequence: room.total, deduplicated: false } }, 201)
  })
  // No live stream in this mock: the room falls back to its 6 s poll, which is how a new post arrives here.
  await page.route(new RegExp(`/api/v1/spaces/${REF}/events`), (route) => route.abort())
  await page.route(new RegExp(`/api/v1/spaces/${REF}/presence`), (route) => json(route, { data: { present: [], ttl_seconds: 30 } }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/read`), (route) => json(route, { data: { last_read_at: null, manual_unread: false } }))
  await page.route(/\/api\/v1\/spaces\/attention/, (route) => json(route, { data: { rooms: [], totals: { badge: 0 } }, meta: { stale_sources: [] } }))
  return room
}

/** The scrolling ancestor of the room list, read as the reader sees it. */
const scrollTopOfRoom = (): number => {
  let node: HTMLElement | null = document.querySelector<HTMLElement>('ol.verevon-room-timeline')
  while (node) {
    const { overflowY } = getComputedStyle(node)
    if (node.scrollHeight > node.clientHeight && (overflowY === 'auto' || overflowY === 'scroll')) return node.scrollTop
    node = node.parentElement
  }
  return window.scrollY
}

test('a 1,500-post room opens on 50 posts with one timeline read, pages up once, and holds the reader position on prepend', async ({ page }) => {
  const room = await mockRoom(page, 1500)
  await page.goto(`/spaces/${REF}#chat`)
  const posts = page.locator('li.verevon-room-post')
  await expect(posts).toHaveCount(PAGE_SIZE, { timeout: 15_000 })
  expect(room.timelineReads, 'one timeline read on open').toBe(1)

  // Scroll to the top: the sentinel asks for the earlier page exactly once.
  await page.locator('ol.verevon-room-timeline').hover()
  await page.mouse.wheel(0, -20_000)
  await expect.poll(() => room.beforeReads, { timeout: 15_000 }).toBe(1)
  const anchor = posts.first()
  const anchorId = await anchor.getAttribute('id')
  const yBefore = (await anchor.boundingBox())?.y ?? 0
  await expect(posts).toHaveCount(PAGE_SIZE * 2, { timeout: 15_000 })
  const yAfter = (await page.locator(`[id="${anchorId}"]`).boundingBox())?.y ?? 0
  expect(Math.abs(yAfter - yBefore), 'the reader keeps their post in place on prepend').toBeLessThanOrEqual(4)

  expect(room.beforeReads, 'scroll-up pages once').toBe(1)
  expect(room.maxInFlight, 'at most four transcript requests in flight').toBeLessThanOrEqual(4)
  expect(await posts.count(), 'DOM posts stay under 450').toBeLessThan(450)
})

for (const width of [390, 1280]) {
  test(`a 2,000-post room at ${width} px: a new post while scrolled up shows the jump pill and moves nothing; an own send goes to the bottom`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    const room = await mockRoom(page, 2000)
    await page.goto(`/spaces/${REF}#chat`)
    const posts = page.locator('li.verevon-room-post')
    await expect(posts).toHaveCount(PAGE_SIZE, { timeout: 15_000 })

    await page.locator('ol.verevon-room-timeline').hover()
    await page.mouse.wheel(0, -3_000)
    const readerAt = await page.evaluate(scrollTopOfRoom)

    // A post arrives on the next poll while the reader is scrolled up.
    room.total += 1
    await expect(page.locator('li.verevon-room-jump-item button.verevon-room-jump[data-new="true"]')).toBeVisible({ timeout: 20_000 })
    expect(Math.abs((await page.evaluate(scrollTopOfRoom)) - readerAt), 'a scrolled-up reader is not moved').toBeLessThanOrEqual(2)

    // The reader's own send goes to the bottom and clears the pill.
    await box(page).fill(`Min melding ${width}`)
    await page.getByRole('button', { name: /^(Send|Send til rommet)$/ }).click()
    await expect(posts.last()).toBeInViewport({ timeout: 15_000 })
    await expect(page.locator('li.verevon-room-jump-item button.verevon-room-jump')).toHaveCount(0)
  })
}
