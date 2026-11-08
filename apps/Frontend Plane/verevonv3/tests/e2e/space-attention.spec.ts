import { expect, test, type BrowserContext, type Route } from '@playwright/test'
import { memberContext, requireMembers } from './space-fixtures'

/**
 * WS11-S4 cross-room attention in the Spaces sidebar. Member A's seeded storage state signs the
 * browser in; the attention routes, the shell features and the room listing are answered by the
 * spec, so the case needs no seeded unread state and runs no model. Envelopes are `{ data }` as the
 * BFF sends them. Selectors are classes and roles, with both locales' words where copy is needed.
 *
 * The badge is the server's: the spec sends `badge` and checks it is drawn as given. A gateway
 * whose shell flag `attention` is off is never asked for the aggregate.
 */

const SPACES = '/spaces'
// RegExps, not globs: Playwright anchors a glob at the end, which would skip the query string.
const FEATURES = /\/api\/v1\/spaces\/features$/
const LISTING = /\/api\/v1\/spaces(\?.*)?$/
const AGGREGATE = /\/api\/v1\/spaces\/attention(\?.*)?$/
const READ_ALL = /\/api\/v1\/spaces\/attention\/read-all$/

const ROOM = {
  space_ref: 'space-e2e-attention',
  name: 'Attention fixture room',
  kind: 'room',
  lifecycle: 'active',
  is_organization_room: false,
}

type Aggregate = { readonly needsYou: number; readonly unread: number }

async function json(route: Route, body: unknown) {
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

function aggregate({ needsYou, unread }: Aggregate) {
  return {
    generated_at: Date.now(),
    mode: 'live',
    rooms: [
      {
        space_ref: ROOM.space_ref,
        unread_threads: unread,
        unread_capped: false,
        mentions: 0,
        needs_you: needsYou,
        working: 0,
        last_activity_at: Date.now(),
        last_read_at: null,
        manual_unread: false,
        level: 'mentions',
        badge: needsYou > 0 ? { kind: 'count', n: needsYou } : { kind: 'none', n: 0 },
        gaps: [],
      },
    ],
    totals: { badge: needsYou, unread_rooms: unread > 0 ? 1 : 0, other_open: 0 },
    truncated: false,
  }
}

type Served = {
  /** Every aggregate request the page made (GET `/spaces/attention`, never read-all). */
  readonly aggregateCalls: () => number
  /** The bodies of the read-all POSTs. */
  readonly readAllBodies: () => readonly string[]
}

/** Answer the shell features, the room listing and the attention routes for one context. */
async function serve(context: BrowserContext, options: { attention: boolean; first: Aggregate }): Promise<Served> {
  let calls = 0
  let current: Aggregate = options.first
  const bodies: string[] = []
  await context.route(FEATURES, async (route) => {
    await json(route, {
      features: {
        message_search: false,
        entry_links: false,
        command_palette: false,
        room_pins: false,
        thread_fork: false,
        attention: options.attention,
      },
    })
  })
  await context.route(LISTING, async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    await json(route, { spaces: [ROOM], features: { dm: false, room_directory: false } })
  })
  // The listing pattern ends at `/spaces`, so it never catches the attention paths below.
  await context.route(AGGREGATE, async (route) => {
    calls += 1
    if (!options.attention) {
      await route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: JSON.stringify({ error: { code: 'space_attention_disabled', message: 'off', details: {} } }),
      })
      return
    }
    await json(route, aggregate(current))
  })
  await context.route(READ_ALL, async (route) => {
    bodies.push(route.request().postData() ?? '')
    // Read-all advances the markers; "needs you" is the server's rule and this fixture clears it too.
    current = { needsYou: 0, unread: 0 }
    await json(route, { rooms_marked: 1, items_resolved: 1, marked_at: Date.now() })
  })
  return { aggregateCalls: () => calls, readAllBodies: () => bodies }
}

const markAllButton = /^(Merk alle som lest|Mark all read)$/

test('the room badge shows the server count and mark-all-read clears it', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const served = await serve(a, { attention: true, first: { needsYou: 3, unread: 1 } })
    const page = await a.newPage()
    await page.goto(SPACES)

    await expect(page.getByText(ROOM.name).first()).toBeVisible()

    // The badge is the gateway's `badge.n`, drawn as given.
    const badge = page.locator('.space-attention-badge[data-kind="count"]', { hasText: /^3$/ }).first()
    await expect(badge).toBeVisible()
    await expect(badge).toHaveAttribute('aria-label', /3 (trenger deg|need you)/)
    expect(served.aggregateCalls()).toBeGreaterThan(0)

    const readAll = page.locator('.space-attention-readall').getByRole('button', { name: markAllButton })
    await expect(readAll).toBeVisible()
    await readAll.click()
    await expect.poll(() => served.readAllBodies().length).toBe(1)

    // The store asks again once the spacing gap (10 s) allows; the next answer has nothing waiting.
    await expect(page.locator('.space-attention-badge')).toHaveCount(0, { timeout: 25_000 })
    await expect(page.locator('.space-attention-readall').getByRole('button', { name: markAllButton })).toHaveCount(0)
  } finally {
    await a.close()
  }
})

test('with the attention flag off the aggregate is never requested and no badge is drawn', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const served = await serve(a, { attention: false, first: { needsYou: 3, unread: 1 } })
    const page = await a.newPage()
    const featuresAsked = page.waitForRequest(FEATURES)
    await page.goto(SPACES)

    await expect(page.getByText(ROOM.name).first()).toBeVisible()
    await featuresAsked
    // Longer than one poll would take to fire; the store must have decided from the flag alone.
    await page.waitForTimeout(3_000)

    expect(served.aggregateCalls()).toBe(0)
    await expect(page.locator('.space-attention-badge')).toHaveCount(0)
    await expect(page.locator('.space-attention-readall')).toHaveCount(0)
  } finally {
    await a.close()
  }
})
