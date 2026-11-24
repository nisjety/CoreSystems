import { expect, test, type BrowserContext, type Page, type Route } from '@playwright/test'
import { memberContext, requireMembers, skipUnlessSpace } from './space-fixtures'

/**
 * A-REC-02 / WS04-S8: the room directory end to end. Two halves.
 *
 * Mocked (member A's storage state signs the browser in; the listing, the directory, the join and the membership
 * read are answered by the spec, so it needs no seeded room and no directory policy):
 *  - with `features.room_directory` on, the "Browse rooms" link is offered and leads to /spaces/directory;
 *  - with it off, the link is not offered;
 *  - the directory lists a room, and Join sends one idempotent self-join and lands on the room.
 *
 * Live (later tier: needs SPACE_ROOM_DIRECTORY_API on, the organization's directory policy turned on by an operator
 * (D22av: operator-only), and members A and E of space-fixtures.ts). Member A creates a fresh named room, opens it to
 * the organization through the room details (the visibility settings PATCH), member E finds it in the directory and
 * joins it, and A closes and archives it again afterwards, so the shared fixture room is never touched.
 */

const LISTING = /\/api\/v1\/spaces(\?.*)?$/
const ORG_ROOM_ENSURE = /\/api\/v1\/spaces\/organization-room$/
const DIRECTORY = /\/api\/v1\/directory\/rooms(\?.*)?$/
const BROWSE_ROOMS = /^(Finn rom|Browse rooms)$/
const JOIN = /^(Bli med|Join)$/

const OPEN_ROOM = {
  space_ref: 'space-e2e-open-room',
  name: 'E2E Open Room',
  topic: 'Anyone in the organization may join',
  member_count: 3,
  join_role: 'viewer',
}

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify({ data: body }) })
}

type Served = {
  readonly joinBodies: () => readonly unknown[]
}

/** The listing answers no Space (so /spaces stays on its index) and the given directory feature. */
async function serve(context: BrowserContext, options: { roomDirectory: boolean }): Promise<Served> {
  const joinBodies: unknown[] = []
  // Any create from the index page (the organization-room ensure, a personal Space) is refused, so a mocked run never
  // creates anything on the stack behind it.
  const refuse = (route: Route) =>
    route.fulfill({
      status: 503,
      contentType: 'application/json',
      body: JSON.stringify({ error: { code: 'unavailable', message: 'mocked', details: {} } }),
    })
  await context.route(ORG_ROOM_ENSURE, refuse)
  await context.route(LISTING, async (route) => {
    if (route.request().method() === 'GET') {
      await json(route, { spaces: [], features: { dm: false, room_directory: options.roomDirectory } })
      return
    }
    await refuse(route)
  })
  await context.route(DIRECTORY, async (route) => {
    await json(route, { enabled: true, rooms: [OPEN_ROOM], next_cursor: null })
  })
  const ref = encodeURIComponent(OPEN_ROOM.space_ref)
  await context.route(new RegExp(`/api/v1/spaces/${ref}/join$`), async (route) => {
    joinBodies.push(JSON.parse(route.request().postData() ?? 'null'))
    await json(route, { role: 'viewer', changed: true, already_member: false, recipient_ready: false })
  })
  await context.route(new RegExp(`/api/v1/spaces/${ref}/membership$`), async (route) => {
    await json(route, { subject_id: 'e2e-self', role: 'viewer', recipient_ready: true })
  })
  return { joinBodies: () => joinBodies }
}

test('with the directory on, Browse rooms leads to the directory; Join sends one self-join and opens the room', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const served = await serve(a, { roomDirectory: true })
    const page = await a.newPage()
    await page.goto('/spaces')

    const link = page.getByRole('link', { name: BROWSE_ROOMS }).first()
    await expect(link).toBeVisible({ timeout: 15_000 })
    await link.click()
    await expect(page).toHaveURL(/\/spaces\/directory$/)
    await expect(page.getByRole('heading', { level: 1, name: BROWSE_ROOMS })).toBeVisible()

    const row = page.locator('.verevon-space-directory__row', { hasText: OPEN_ROOM.name })
    await expect(row).toBeVisible()
    await expect(row).toContainText(OPEN_ROOM.topic)
    await expect(row).toContainText(/3 (medlemmer|members)/)

    await row.getByRole('button', { name: JOIN }).click()
    await expect(page).toHaveURL(new RegExp(`/spaces/${encodeURIComponent(OPEN_ROOM.space_ref)}$`), { timeout: 15_000 })
    // One join, carrying an idempotency key and nothing that names a role: the room decides the role.
    expect(served.joinBodies()).toHaveLength(1)
    const body = served.joinBodies()[0] as Record<string, unknown>
    expect(Object.keys(body)).toEqual(['idempotency_key'])
    expect(typeof body.idempotency_key).toBe('string')
  } finally {
    await a.close()
  }
})

test('with the directory off, no Browse rooms link is offered', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    await serve(a, { roomDirectory: false })
    const page = await a.newPage()
    const listed = page.waitForResponse((response) => LISTING.test(new URL(response.url()).pathname) && response.request().method() === 'GET')
    await page.goto('/spaces')
    await listed
    await expect(page.getByRole('link', { name: BROWSE_ROOMS })).toHaveCount(0)
  } finally {
    await a.close()
  }
})

// ---- Live half -------------------------------------------------------------------------------------------------

type Listing = { data?: { spaces?: unknown; features?: { room_directory?: unknown } } }
type ErrorBody = { error?: { code?: string } }

const errorCode = async (response: { json: () => Promise<unknown> }): Promise<string | undefined> =>
  ((await response.json().catch(() => ({}))) as ErrorBody).error?.code

/** Polls the caller's listing until the room is active (Control registers a new room asynchronously). */
async function waitUntilActive(page: Page, ref: string): Promise<boolean> {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    const body = (await (await page.request.get('/api/v1/spaces')).json().catch(() => ({}))) as Listing
    const rows = Array.isArray(body.data?.spaces) ? (body.data.spaces as Record<string, unknown>[]) : []
    if (rows.some((row) => row.space_ref === ref && row.lifecycle === 'active')) return true
    await page.waitForTimeout(2_000)
  }
  return false
}

test('an owner opens a new room to the organization, and a same-org member finds and joins it (live)', async ({ browser, baseURL }) => {
  test.setTimeout(180_000)
  requireMembers(test, 'A', 'E')
  const owner = await memberContext(browser, baseURL, 'A')
  const member = await memberContext(browser, baseURL, 'E')
  let ref = ''
  try {
    const ownerPage = await owner.newPage()
    const listing = (await (await ownerPage.request.get('/api/v1/spaces')).json().catch(() => ({}))) as Listing
    skipUnlessSpace(test, listing.data?.features?.room_directory !== true, 'SPACE_ROOM_DIRECTORY_API is off on this stack')

    const name = `E2E Directory ${Date.now().toString(36)}`
    const created = await ownerPage.request.post('/api/v1/spaces', { data: { kind: 'room', name } })
    expect(created.status(), 'member A creates a named room').toBeLessThan(300)
    ref = ((await created.json()) as { data?: { space?: { space_ref?: string } } }).data?.space?.space_ref ?? ''
    expect(ref).not.toBe('')
    expect(await waitUntilActive(ownerPage, ref), 'Control registers the new room').toBe(true)

    // Before it is opened, E cannot see it in the directory nor join it.
    const memberPage = await member.newPage()
    const before = await memberPage.request.post(`/api/v1/spaces/${encodeURIComponent(ref)}/join`, { data: {} })
    expect(before.status()).toBeGreaterThanOrEqual(400)

    // The settings path: A opens the room to the organization from the room details.
    await ownerPage.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
    await ownerPage.getByRole('button', { name: 'Romdetaljer' }).click()
    const details = ownerPage.getByRole('dialog', { name: 'Romdetaljer' })
    await expect(details).toBeVisible()
    const form = details.locator('form.verevon-space-room-visibility')
    await expect(form).toBeVisible()
    await form.getByLabel(/^(Åpent for organisasjonen|Open to the organization)/).check()
    const saved = ownerPage.waitForResponse(
      (response) =>
        response.request().method() === 'PATCH' && response.url().includes(`/api/v1/spaces/${encodeURIComponent(ref)}/settings`),
    )
    await form.getByRole('button', { name: /^(Lagre|Save)$/ }).click()
    const settings = await saved
    if (settings.status() === 403 && (await errorCode(settings)) === 'directory_disabled') {
      skipUnlessSpace(test, true, "the organization's directory policy is off (operator-only, D22av)")
    }
    expect(settings.status(), 'the visibility settings PATCH').toBeLessThan(300)
    await expect(form.getByRole('status')).toHaveText(/^(Lagret\.|Saved\.)$/)

    // The directory: E finds the room by name and joins it.
    await memberPage.goto('/spaces/directory')
    await memberPage.getByRole('searchbox').fill(name)
    const row = memberPage.locator('.verevon-space-directory__row', { hasText: name })
    await expect(row).toBeVisible({ timeout: 15_000 })
    await row.getByRole('button', { name: JOIN }).click()
    await expect(memberPage).toHaveURL(new RegExp(`/spaces/${encodeURIComponent(ref)}`), { timeout: 60_000 })
    const membership = await memberPage.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/membership`)
    expect(membership.status()).toBe(200)
    expect(((await membership.json()) as { data?: { role?: unknown } }).data?.role).toBe('viewer')
    await expect(memberPage.getByRole('heading', { level: 1 })).toHaveText(name, { timeout: 30_000 })
  } finally {
    if (ref) {
      // Close and archive the room again, so a rerun never lists it. Best effort: a failure here is not the test.
      const cleanup = await owner.newPage()
      const base = `/api/v1/spaces/${encodeURIComponent(ref)}`
      await cleanup.request.patch(`${base}/settings`, { data: { visibility: 'private' } }).catch(() => undefined)
      await cleanup.request.post(`${base}/archive`, { data: {} }).catch(() => undefined)
    }
    await owner.close()
    await member.close()
  }
})
