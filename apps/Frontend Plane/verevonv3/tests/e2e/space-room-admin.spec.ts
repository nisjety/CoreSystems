import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS04-S2, S6 and S7 room administration, end to end (later tier: needs the seeded Space fixtures and the room-admin
 * routes on). Member A owns the named room, B is an editor of it, and E is a same-org outsider who is not in it
 * (scripts/space-e2e-seed.sh; space-fixtures.ts). Nothing here starts a model run.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_REF_ORG   optional: the organization room of the fixture organization. When unset, the room member A's
 *                       Space list marks `is_organization_room` is used, so a seeded suite needs no extra producer.
 *
 * The sole-owner refusal (WS04-S4), the directory join refusal (WS04-S8, SD-23) and the history-window gate (WS04-S11,
 * D18) are checked through the API with the same fixtures. Each is a refusal, so a run changes nothing; a route whose
 * flag is off answers feature_disabled, which counts as the same "never succeeds".
 */

const roomUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`
const composer = (page: Page) => page.getByPlaceholder(/Skriv (i|til) rommet/)
const archivedBanner = (page: Page) => page.locator('.verevon-space-archived-banner')

/** The organization room: E2E_SPACE_REF_ORG, or the row of the member's own Space list marked as that room. */
async function requireOrgRoomRef(target: Parameters<typeof skipUnlessSpace>[0], page: Page): Promise<string> {
  let ref = process.env.E2E_SPACE_REF_ORG?.trim() ?? ''
  if (!ref) {
    const listing = await page.request.get('/api/v1/spaces')
    const body = listing.ok() ? ((await listing.json()) as { data?: { spaces?: unknown } }) : {}
    const rows = Array.isArray(body.data?.spaces) ? (body.data.spaces as Record<string, unknown>[]) : []
    const row = rows.find((space) => space.is_organization_room === true && space.archived !== true)
    ref = typeof row?.space_ref === 'string' ? row.space_ref : ''
  }
  skipUnlessSpace(target, !ref, 'no active organization room: set E2E_SPACE_REF_ORG or seed the organization room')
  return ref
}

type ErrorBody = { error?: { code?: string } }
type Membership = { data?: { subject_id?: unknown; role?: unknown } }
type Roster = { data?: { members?: unknown } }

const errorCode = async (response: { json: () => Promise<unknown> }): Promise<string | undefined> =>
  ((await response.json().catch(() => ({}))) as ErrorBody).error?.code

async function openRoomDetails(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Romdetaljer' }).click()
  await expect(page.getByRole('dialog', { name: 'Romdetaljer' })).toBeVisible()
}

test('the owner renames a named room, and the sidebar shows the new name without a reload', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    await page.goto(roomUrl(ref))
    const heading = page.getByRole('heading', { level: 1 })
    await expect(heading).toBeVisible()
    const name = `E2E ${Date.now().toString(36)}`
    await openRoomDetails(page)
    await page.getByRole('button', { name: 'Rediger navn og emne' }).click()
    await page.getByLabel('Navn').fill(name)
    const saved = page.waitForResponse(
      (response) => response.request().method() === 'PATCH' && response.url().includes(`/api/v1/spaces/${encodeURIComponent(ref)}`),
    )
    await page.getByRole('button', { name: 'Lagre' }).click()
    expect((await saved).status()).toBeLessThan(300)
    await expect(heading).toHaveText(name)
    // DEPTH-WS04-S2-1: the sidebar listing reads the room again after the save.
    await expect(
      page.getByRole('navigation', { name: /^(Romnavigasjon|Spaces navigation)$/ }).getByText(name),
    ).toBeVisible({ timeout: 15_000 })
  } finally {
    await owner.close()
  }
})

test('the organization room offers no rename, no topic and no archive, even to its owner', async ({ browser, baseURL }) => {
  requireMembers(test, 'A')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    const orgRef = await requireOrgRoomRef(test, page)
    await page.goto(roomUrl(orgRef))
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
    // D22e: the header shows no topic for the organization room, whatever is stored.
    await expect(page.locator('.verevon-space-header__topic')).toHaveCount(0)
    await openRoomDetails(page)
    await expect(page.locator('.verevon-space-details__topic')).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Rediger navn og emne' })).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Arkiver rommet' })).toHaveCount(0)

    // The server refuses too, not only the UI: a rename answers space_kind_not_editable (or the route is off), and an
    // archive answers room_not_archivable (or the route is off). Neither ever succeeds.
    const rename = await page.request.patch(`/api/v1/spaces/${encodeURIComponent(orgRef)}`, { data: { name: 'E2E rename' } })
    expect(rename.status()).toBeGreaterThanOrEqual(400)
    const renameCode = ((await rename.json().catch(() => ({}))) as { error?: { code?: string } }).error?.code
    expect(['space_kind_not_editable', 'room_role_forbidden', 'feature_disabled']).toContain(renameCode)
    const archive = await page.request.post(`/api/v1/spaces/${encodeURIComponent(orgRef)}/archive`, { data: {} })
    expect(archive.status()).toBeGreaterThanOrEqual(400)
    const archiveCode = ((await archive.json().catch(() => ({}))) as { error?: { code?: string } }).error?.code
    expect(['room_not_archivable', 'feature_disabled']).toContain(archiveCode)
  } finally {
    await owner.close()
  }
})

test('an editor reads the room but has none of its administration controls', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'B')
  const editor = await memberContext(browser, baseURL, 'B')
  try {
    const page = await editor.newPage()
    await page.goto(roomUrl(ref))
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
    await openRoomDetails(page)
    await expect(page.getByRole('button', { name: 'Rediger navn og emne' })).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Arkiver rommet' })).toHaveCount(0)
  } finally {
    await editor.close()
  }
})

test('the owner archives through the action registry; an editor reads the archived room and cannot write', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const owner = await memberContext(browser, baseURL, 'A')
  const editor = await memberContext(browser, baseURL, 'B')
  try {
    const ownerPage = await owner.newPage()
    await ownerPage.goto(roomUrl(ref))
    await openRoomDetails(ownerPage)
    await ownerPage.getByRole('button', { name: 'Arkiver rommet' }).click()
    const executed = ownerPage.waitForRequest(
      (request) => request.url().includes('/api/v1/actions/execute') && (request.postData() ?? '').includes('"spaces.archive_room"'),
    )
    await ownerPage.getByRole('button', { name: 'Bekreft arkivering av rommet' }).click()
    expect((await executed).method()).toBe('POST')
    await expect(archivedBanner(ownerPage)).toBeVisible({ timeout: 15_000 })

    const editorPage = await editor.newPage()
    await editorPage.goto(roomUrl(ref))
    await expect(archivedBanner(editorPage)).toBeVisible({ timeout: 15_000 })
    await expect(composer(editorPage)).toHaveCount(0)

    // Bringing the room back restores the composer for the owner.
    await archivedBanner(ownerPage).getByRole('button', { name: 'Fjern arkivering' }).click()
    await expect(archivedBanner(ownerPage)).toHaveCount(0, { timeout: 15_000 })
    await expect(composer(ownerPage)).toBeVisible()
  } finally {
    await owner.close()
    await editor.close()
  }
})

test('a same-org outsider gets the same answer for a private room as for a room that does not exist', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'E')
  const outsider = await memberContext(browser, baseURL, 'E')
  try {
    const page = await outsider.newPage()
    const membership = (value: string) => `/api/v1/spaces/${encodeURIComponent(value)}/membership`
    const real = await page.request.get(membership(ref))
    const missing = await page.request.get(membership(`space_missing_${Date.now().toString(36)}`))
    expect(real.status()).toBe(404)
    expect(missing.status()).toBe(404)
    expect(await real.text()).toBe(await missing.text())
  } finally {
    await outsider.close()
  }
})

test('the sole owner cannot step down: the role change is refused and the roster is unchanged (WS04-S4)', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    const base = `/api/v1/spaces/${encodeURIComponent(ref)}`
    const membership = (await (await page.request.get(`${base}/membership`)).json()) as Membership
    const self = typeof membership.data?.subject_id === 'string' ? membership.data.subject_id : ''
    const rosterBody = (await (await page.request.get(`${base}/roster`)).json()) as Roster
    const members = Array.isArray(rosterBody.data?.members) ? (rosterBody.data.members as Record<string, unknown>[]) : []
    const owners = members.filter((member) => member.subject_type === 'user' && member.role === 'owner')
    // Only a run where A is the one owner proves the guard, and only then is the call sure to change nothing.
    test.skip(
      membership.data?.role !== 'owner' || owners.length !== 1 || owners[0]?.subject_id !== self,
      'member A must be the only owner of E2E_SPACE_REF',
    )
    const stepDown = await page.request.patch(`${base}/members/me`, { data: { role: 'editor' } })
    expect(stepDown.status()).toBeGreaterThanOrEqual(400)
    expect(['room_sole_owner', 'feature_disabled']).toContain(await errorCode(stepDown))
    const after = (await (await page.request.get(`${base}/membership`)).json()) as Membership
    expect(after.data?.role).toBe('owner')
  } finally {
    await owner.close()
  }
})

test('a same-org outsider cannot join a private room from the directory (WS04-S8, SD-23)', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'E')
  const outsider = await memberContext(browser, baseURL, 'E')
  try {
    const page = await outsider.newPage()
    const join = (value: string) => page.request.post(`/api/v1/spaces/${encodeURIComponent(value)}/join`, { data: {} })
    const real = await join(ref)
    const missing = await join(`space_missing_${Date.now().toString(36)}`)
    expect(real.status()).toBeGreaterThanOrEqual(400)
    // A private room answers exactly like a room that does not exist, so a join probes nothing.
    expect(real.status()).toBe(missing.status())
    expect(await errorCode(real)).toBe(await errorCode(missing))
    // And the outsider is still not in it.
    expect((await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/membership`)).status()).toBe(404)
  } finally {
    await outsider.close()
  }
})

test("an editor cannot set a member's history window; only an owner or manager may (WS04-S11, D18)", async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'B')
  const editor = await memberContext(browser, baseURL, 'B')
  try {
    const page = await editor.newPage()
    const base = `/api/v1/spaces/${encodeURIComponent(ref)}`
    const membership = (await (await page.request.get(`${base}/membership`)).json()) as Membership
    const self = typeof membership.data?.subject_id === 'string' ? membership.data.subject_id : ''
    test.skip(membership.data?.role !== 'editor' || !self, 'member B must be an editor of E2E_SPACE_REF')
    const put = await page.request.put(`${base}/members/${encodeURIComponent(self)}/history-window`, {
      data: { mode: 'since_join' },
    })
    expect(put.status()).toBeGreaterThanOrEqual(400)
    expect(['room_role_forbidden', 'member_history_window_unavailable', 'feature_disabled']).toContain(
      await errorCode(put),
    )
  } finally {
    await editor.close()
  }
})
