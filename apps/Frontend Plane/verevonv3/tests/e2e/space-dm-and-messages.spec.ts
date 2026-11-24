import { expect, test, type BrowserContext, type Locator, type Page, type Route } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * F-t1-016 (WS12): direct messages and group DMs in the Spaces sidebar. Member A's seeded storage
 * state signs the browser in; the shell features, the room listing, the people directory, the DM
 * open route, the attention aggregate and the notify-level route are answered by the spec, so the
 * case needs no seeded DM and runs no model. Envelopes are `{ data }` as the BFF sends them.
 * Selectors are classes and roles, with both locales' words where copy is needed.
 *
 * What it pins:
 *  - a DM is drawn in the "Direct messages" group under its participants' names;
 *  - D22r: at the DM default level `all` its unread count is a strong direct-message badge;
 *  - "New message" opens the DM for exactly the chosen people and lands on it;
 *  - D22r: muting a DM is level `off` and unmuting restores `all` (never the room default).
 *
 * The live half (a real DM opened by A, B's message reaching A's inbox through the attention
 * ingest, and a Control audience reconcile) is the M2-S5 flow on a seeded stack; it is not mocked
 * here.
 */

const SPACES = '/spaces'
// RegExps, not globs: Playwright anchors a glob at the end, which would skip the query string.
const FEATURES = /\/api\/v1\/spaces\/features$/
const LISTING = /\/api\/v1\/spaces(\?.*)?$/
const AGGREGATE = /\/api\/v1\/spaces\/attention(\?.*)?$/
const PEOPLE = /\/api\/v1\/directory\/people(\?.*)?$/
const OPEN_DM = /\/api\/v1\/spaces\/dm$/
const NOTIFY = /\/api\/v1\/spaces\/([^/]+)\/notify$/

const PEER = { subject_id: 'e2e-dm-peer', display_name: 'Annika DM-Peer', email: 'annika.dm-peer@example.test' }

const DM = {
  space_ref: 'space-e2e-dm',
  name: 'Direct message',
  kind: 'dm',
  lifecycle: 'active',
  is_organization_room: false,
  participants: [{ subject_id: PEER.subject_id, display_name: PEER.display_name }],
}

type Level = 'all' | 'mentions' | 'off'

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

function aggregate(level: Level, unread: number) {
  const badge = unread === 0 || level === 'off'
    ? { kind: 'none', n: 0 }
    : level === 'all'
      ? { kind: 'count', n: unread, muted: true }
      : { kind: 'dot', n: 0 }
  return {
    generated_at: Date.now(),
    mode: 'live',
    rooms: [
      {
        space_ref: DM.space_ref,
        unread_threads: unread,
        unread_capped: false,
        mentions: 0,
        needs_you: 0,
        working: 0,
        last_activity_at: Date.now(),
        last_read_at: null,
        manual_unread: false,
        level,
        badge,
        gaps: [],
      },
    ],
    totals: { badge: 0, unread_rooms: unread > 0 ? 1 : 0, other_open: 0 },
    truncated: false,
  }
}

type Served = {
  readonly openBodies: () => readonly unknown[]
  readonly notifyBodies: () => readonly { spaceRef: string; body: unknown }[]
}

async function serve(context: BrowserContext, options: { level: Level; unread: number }): Promise<Served> {
  let level = options.level
  const openBodies: unknown[] = []
  const notifyBodies: { spaceRef: string; body: unknown }[] = []
  await context.route(FEATURES, async (route) => {
    await json(route, {
      features: {
        message_search: false,
        entry_links: false,
        command_palette: false,
        room_pins: false,
        thread_fork: false,
        attention: true,
      },
    })
  })
  await context.route(LISTING, async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    await json(route, { spaces: [DM], features: { dm: true, room_directory: false } })
  })
  await context.route(AGGREGATE, async (route) => {
    await json(route, aggregate(level, options.unread))
  })
  await context.route(PEOPLE, async (route) => {
    await json(route, { people: [{ ...PEER, is_self: false }], next_cursor: null })
  })
  await context.route(OPEN_DM, async (route) => {
    openBodies.push(JSON.parse(route.request().postData() ?? 'null'))
    await json(route, { space: { ...DM, lifecycle: 'active' }, created: false })
  })
  await context.route(NOTIFY, async (route) => {
    const spaceRef = decodeURIComponent(NOTIFY.exec(new URL(route.request().url()).pathname)?.[1] ?? '')
    if (route.request().method() === 'PUT') {
      const body = JSON.parse(route.request().postData() ?? 'null') as { level?: Level }
      notifyBodies.push({ spaceRef, body })
      if (body?.level) level = body.level
    }
    await json(route, { level, hide_preview: false })
  })
  return { openBodies: () => openBodies, notifyBodies: () => notifyBodies }
}

const DM_GROUP = /^(Direktemeldinger|Direct messages)$/
const PEER_NAME = new RegExp(PEER.display_name)

test('a DM is listed under Direct messages and its unread count at level all is a strong DM badge', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    await serve(a, { level: 'all', unread: 2 })
    const page = await a.newPage()
    await page.goto(SPACES)

    await expect(page.getByText(DM_GROUP).first()).toBeVisible()
    const row = page.getByRole('link', { name: PEER_NAME }).first()
    await expect(row).toBeVisible()

    const badge = page.locator('.space-attention-badge[data-kind="count"]', { hasText: /^2$/ }).first()
    await expect(badge).toBeVisible()
    await expect(badge).toHaveAttribute('aria-label', /2 (uleste i direktemeldingen|unread in this direct message)/)
    await expect(badge).toHaveAttribute('data-strong', 'true')
  } finally {
    await a.close()
  }
})

test('New message opens the DM for exactly the chosen person and lands on it', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const served = await serve(a, { level: 'all', unread: 0 })
    const page = await a.newPage()
    await page.goto(SPACES)

    await page.getByRole('button', { name: /^(Ny melding|New message)$/ }).click()
    const dialog = page.getByRole('dialog', { name: /^(Ny melding|New message)$/ })
    await expect(dialog).toBeVisible()
    await dialog.getByLabel(/^(Søk etter personer|Search people)$/).fill('Annika')
    await dialog.getByRole('checkbox', { name: PEER_NAME }).check()
    await dialog.getByRole('button', { name: /^(Start melding|Start message)$/ }).click()

    await expect.poll(() => served.openBodies().length).toBe(1)
    expect(served.openBodies()[0]).toMatchObject({ participants: [PEER.subject_id] })
    await expect(page).toHaveURL(new RegExp(`/spaces/${DM.space_ref}$`))
  } finally {
    await a.close()
  }
})

test('muting a DM sends level off and unmuting restores all, not the room default', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const served = await serve(a, { level: 'all', unread: 1 })
    const page = await a.newPage()
    await page.goto(SPACES)

    const options = page.getByLabel(new RegExp(`^(Valg for|Options for) ${PEER.display_name}$`)).first()
    const muteItem = () => page.getByRole('menuitemcheckbox', { name: /(Demp samtale|Mute conversation)/ }).first()
    /** The item, with the row menu opened if it is closed. */
    const openMuteItem = async () => {
      if (!(await muteItem().isVisible())) await options.click()
      return muteItem()
    }
    await expect(options).toBeVisible()
    await (await openMuteItem()).click()
    await expect.poll(() => served.notifyBodies().length).toBe(1)
    expect(served.notifyBodies()[0]).toEqual({ spaceRef: DM.space_ref, body: { level: 'off' } })

    // The store asks again; once its answer reports `off`, the item is checked and now unmutes.
    await expect.poll(async () => (await openMuteItem()).getAttribute('aria-checked'), { timeout: 25_000 }).toBe('true')
    await (await openMuteItem()).click()
    await expect.poll(() => served.notifyBodies().length).toBe(2)
    expect(served.notifyBodies()[1]).toEqual({ spaceRef: DM.space_ref, body: { level: 'all' } })
  } finally {
    await a.close()
  }
})

// ---------------------------------------------------------------------------------------------
// F-t1-017 (WS12-S4..S8): the message lifecycle on the seeded Space (scripts/space-e2e-seed.sh:
// A owns E2E_SPACE_REF, B is an editor), with the message revise and reactions features on. Real
// routes, no mocks, and no model run: every message here is a person's own post. A missing fixture
// skips these on a laptop and fails them under E2E_REQUIRE_SPACE=1.
//
// What they pin:
//  - Edit replaces the text for everyone, marked edited; B's open room re-reads it live from the
//    content-free `thread.updated` signal (DEPTH-WS12-S4-1), with no reload;
//  - Delete leaves the tombstone for everyone, and the text is gone;
//  - the toolbar's quick reaction and Copy link (DEPTH-WS12-S7-1).
// ---------------------------------------------------------------------------------------------

const LOAD_WAIT = 30_000
const roomUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`
const composer = (page: Page) => page.getByRole('textbox').first()
const sendButton = (page: Page) => page.getByRole('button', { name: /^(Send|Send til rommet)$/ })

async function post(page: Page, text: string): Promise<Locator> {
  await composer(page).fill(text)
  await sendButton(page).click()
  const row = page.locator('.verevon-room-turn', { hasText: text }).first()
  await expect(row).toBeVisible({ timeout: LOAD_WAIT })
  // The settled row carries the stored message id; only then does it offer the toolbar.
  await expect(row).toHaveAttribute('data-message-id', /.+/, { timeout: LOAD_WAIT })
  return row
}

/** The row's toolbar, shown on hover (a touch screen draws it always). */
async function toolbarOf(row: Locator): Promise<Locator> {
  await row.hover()
  const toolbar = row.getByRole('toolbar', { name: /^(Meldingshandlinger|Message actions)$/ })
  await expect(toolbar).toBeVisible()
  return toolbar
}

test('an edit reaches the author and another member live, marked edited', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const marker = `edit-${Date.now().toString(36)}`
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    await pageA.goto(roomUrl(ref))
    const row = await post(pageA, `Original ${marker}`)

    await pageB.goto(roomUrl(ref))
    await expect(pageB.getByText(`Original ${marker}`)).toBeVisible({ timeout: LOAD_WAIT })

    await (await toolbarOf(row)).getByRole('button', { name: /^(Rediger|Edit)$/ }).click()
    const editor = pageA.getByRole('textbox', { name: /^(Rediger melding|Edit message)$/ })
    await editor.fill(`Rettet ${marker}`)
    await pageA.getByRole('button', { name: /^(Lagre|Save)$/ }).click()

    await expect(pageA.getByText(`Rettet ${marker}`)).toBeVisible({ timeout: LOAD_WAIT })
    await expect(pageA.locator('.verevon-room-turn', { hasText: `Rettet ${marker}` }).getByText(/^\((redigert|edited)\)$/)).toBeVisible()
    // B never reloads: the stream's thread.updated signal re-reads the transcript.
    await expect(pageB.getByText(`Rettet ${marker}`)).toBeVisible({ timeout: LOAD_WAIT })
    await expect(pageB.getByText(`Original ${marker}`)).toHaveCount(0)
  } finally {
    await a.close()
    await b.close()
  }
})

test('a delete leaves the tombstone for every member and the text is gone', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const marker = `delete-${Date.now().toString(36)}`
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    await pageA.goto(roomUrl(ref))
    const row = await post(pageA, `Slettes ${marker}`)
    const messageId = await row.getAttribute('data-message-id')
    await pageB.goto(roomUrl(ref))
    await expect(pageB.getByText(`Slettes ${marker}`)).toBeVisible({ timeout: LOAD_WAIT })

    await (await toolbarOf(row)).getByRole('button', { name: /^(Slett|Delete)$/ }).click()
    await pageA.getByRole('button', { name: /^(Bekreft sletting av meldingen|Confirm deleting the message)$/ }).click()

    const tombstone = /^(Meldingen er slettet\.|This message was deleted\.)/
    await expect(pageA.locator(`.verevon-room-turn[data-message-id="${messageId}"]`).getByText(tombstone)).toBeVisible({ timeout: LOAD_WAIT })
    await expect(pageA.getByText(`Slettes ${marker}`)).toHaveCount(0)
    await expect(pageB.locator(`.verevon-room-turn[data-message-id="${messageId}"]`).getByText(tombstone)).toBeVisible({ timeout: LOAD_WAIT })
    await expect(pageB.getByText(`Slettes ${marker}`)).toHaveCount(0)
  } finally {
    await a.close()
    await b.close()
  }
})

test('the toolbar reacts with one press and copies the canonical message link', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    await a.grantPermissions(['clipboard-read', 'clipboard-write'])
    const marker = `toolbar-${Date.now().toString(36)}`
    const page = await a.newPage()
    await page.goto(roomUrl(ref))
    const row = await post(page, `Verktøylinje ${marker}`)
    const messageId = (await row.getAttribute('data-message-id')) ?? ''

    const toolbar = await toolbarOf(row)
    await toolbar.getByRole('button', { name: /^(Reager med|React with) \u{1F44D}$/u }).click()
    await expect(row.locator('.verevon-reaction-chip[aria-pressed="true"]', { hasText: '\u{1F44D}' })).toBeVisible({ timeout: LOAD_WAIT })

    await (await toolbarOf(row)).getByRole('button', { name: /^(Kopier lenke|Copy link)$/ }).click()
    await expect(page.getByText(/^(Lenken er kopiert\.|Link copied\.)$/)).toBeVisible()
    const copied = await page.evaluate(() => navigator.clipboard.readText())
    const link = new URL(copied)
    expect(link.pathname).toBe(`/spaces/${encodeURIComponent(ref)}`)
    expect(link.searchParams.get('m')).toBe(messageId)
    expect(link.searchParams.get('thread_id')).toBeTruthy()
  } finally {
    await a.close()
  }
})
