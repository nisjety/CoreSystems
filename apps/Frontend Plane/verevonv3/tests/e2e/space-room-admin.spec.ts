import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS04-S2, S6 and S7 room administration, end to end (later tier: needs the seeded Space fixtures and the room-admin
 * routes on). Member A owns the named room, B is an editor of it, and E is a same-org outsider who is not in it
 * (scripts/space-e2e-seed.sh; space-fixtures.ts). Nothing here starts a model run.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_REF_ORG   the organization room of the fixture organization (an active Space, owner A)
 *
 * Owed, each with its own fixture: the sole-owner refusals (WS04-S4), the directory join (WS04-S8, SD-23) and the
 * history window (WS04-S11, D18).
 */

const roomUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`
const composer = (page: Page) => page.getByPlaceholder(/Skriv (i|til) rommet/)
const archivedBanner = (page: Page) => page.locator('.verevon-space-archived-banner')

function requireOrgRoomRef(target: Parameters<typeof skipUnlessSpace>[0]): string {
  const ref = process.env.E2E_SPACE_REF_ORG?.trim() ?? ''
  skipUnlessSpace(target, !ref, 'E2E_SPACE_REF_ORG must name the active organization room of the fixture organization')
  return ref
}

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
  const orgRef = requireOrgRoomRef(test)
  requireMembers(test, 'A')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    await page.goto(roomUrl(orgRef))
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
    await openRoomDetails(page)
    await expect(page.getByRole('button', { name: 'Rediger navn og emne' })).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Arkiver rommet' })).toHaveCount(0)
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
