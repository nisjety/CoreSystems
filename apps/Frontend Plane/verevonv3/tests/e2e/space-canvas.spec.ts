import { expect, test, type APIResponse, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS19-S6 and WS19-S7 canvas (the room's shared Markdown page), end to end (later tier: needs the seeded Space
 * fixtures with features.canvas on). Member A owns the Space and edits the canvas. "Someone else" is a second writer:
 * the same member's request context saves a newer version (page.request), which moves the head under A's editor.
 * Nothing here starts a model run.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_REF_NO_CANVAS   a Space with no canvas yet (the create path)
 * The restore test needs E2E_SPACE_REF's canvas to hold at least two versions.
 */

const canvasSection = (page: Page) => page.locator('section.verevon-space-canvas')
const draft = (page: Page) => page.locator('#space-canvas-draft')
const notice = (page: Page) => page.locator('.verevon-space-canvas__notice[role="status"]')
const conflictBanner = (page: Page) => page.locator('.verevon-space-canvas__banner[role="alert"]')

async function openCanvas(page: Page, ref: string): Promise<void> {
  await page.goto(`/spaces/${encodeURIComponent(ref)}#lerret`)
  await expect(canvasSection(page)).toBeVisible()
}

/** The room's current canvas head, read the way the panel reads it. */
async function headOf(page: Page, ref: string): Promise<{ id: string; no: number }> {
  const response = await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/canvas`)
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data?: { canvas?: { head_version_id?: string; head_version_no?: number } } }
  return { id: body.data?.canvas?.head_version_id ?? '', no: body.data?.canvas?.head_version_no ?? 0 }
}

/** A second writer saves a newer version from the same session. */
function saveAsOtherWriter(page: Page, ref: string, expectedVersionId: string, content: string, marker: string): Promise<APIResponse> {
  return page.request.post(`/api/v1/spaces/${encodeURIComponent(ref)}/canvas/versions`, {
    data: { expected_version_id: expectedVersionId, content, client_save_id: `canvas-save-other-${marker}` },
  })
}

test('a save refused because someone saved first keeps the draft and shows the conflict banner', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    const marker = Date.now().toString(36)
    await openCanvas(page, ref)
    const loaded = await headOf(page, ref)

    await page.getByRole('button', { name: /^(Rediger|Edit)$/ }).click()
    await draft(page).fill(`Mitt utkast ${marker}`)
    // The other writer saves between this editor's load and its save.
    const other = await saveAsOtherWriter(page, ref, loaded.id, `Annen skriver ${marker}`, marker)
    expect(other.status()).toBeLessThan(300)

    await page.getByRole('button', { name: /^(Lagre|Save)$/ }).click()
    await expect(conflictBanner(page)).toContainText(/Noen andre har lagret en nyere versjon|Someone else saved a newer version/, { timeout: 30_000 })
    // The draft survives, and the banner offers to save it on top of the latest version.
    await expect(draft(page)).toHaveValue(`Mitt utkast ${marker}`)
    await expect(page.getByRole('button', { name: /^(Lagre min tekst som ny versjon|Save my text as the new version)$/ })).toBeVisible()
  } finally {
    await a.close()
  }
})

test('a create whose response is dropped is retried with the same save id and shows Saved, not a failure', async ({ browser, baseURL }) => {
  const emptyRef = process.env.E2E_SPACE_REF_NO_CANVAS?.trim() ?? ''
  skipUnlessSpace(test, !emptyRef, 'E2E_SPACE_REF_NO_CANVAS must name a Space that has no canvas yet')
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await openCanvas(page, emptyRef)
    await page.getByRole('button', { name: /^(Opprett lerret|Create canvas)$/ }).click()
    await draft(page).fill(`Første versjon ${Date.now().toString(36)}`)

    // The first save reaches the server; its response is dropped on the way back to the page.
    let dropped = false
    await page.route(/\/api\/v1\/actions\/execute/, async (route) => {
      const isCanvasSave = route.request().method() === 'POST' && (route.request().postData() ?? '').includes('spaces.save_canvas')
      if (!isCanvasSave || dropped) return route.continue()
      dropped = true
      await route.fetch()
      return route.abort('connectionreset')
    })
    await page.getByRole('button', { name: /^(Lagre|Save)$/ }).click()
    await expect.poll(() => dropped, { timeout: 15_000 }).toBe(true)

    // The retry carries the same save id, so the server answers the version it already stored.
    if (!(await notice(page).isVisible({ timeout: 5_000 }).catch(() => false))) {
      await page.getByRole('button', { name: /^(Lagre|Save|Prøv igjen|Try again)$/ }).first().click()
    }
    await expect(notice(page)).toContainText(/Lagret som versjon 1\.|Saved as version 1\./, { timeout: 30_000 })
    await expect(conflictBanner(page)).toHaveCount(0)
    await expect(page.locator('.verevon-space-canvas__error[role="alert"]')).toHaveCount(0)
  } finally {
    await a.close()
  }
})

test('restoring version 1 shows the restored notice and moves the head to a new version', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await openCanvas(page, ref)
    const before = await headOf(page, ref)
    expect(before.no, 'the fixture canvas holds at least two versions').toBeGreaterThan(1)

    await page.getByRole('button', { name: /^(Historikk|History)$/ }).click()
    const versionOne = page.getByRole('listitem').filter({ hasText: /(Versjon|Version) 1\b/ }).first()
    await versionOne.getByRole('button', { name: /^(Gjenopprett|Restore)$/ }).click()
    await expect(notice(page)).toContainText(/Versjon 1 er gjenopprettet som versjon \d+|Restored version 1 as version \d+/, { timeout: 30_000 })

    const after = await headOf(page, ref)
    expect(after.no).toBe(before.no + 1)
  } finally {
    await a.close()
  }
})
