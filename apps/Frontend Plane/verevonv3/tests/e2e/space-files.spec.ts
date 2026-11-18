import { expect, test, type Locator, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * WS14-S9 room files, end to end (later tier: needs the seeded Space fixtures with room_files.files_tab on and the
 * file-text pipeline running). Owner A uploads a text file, editor B lists and previews it, and viewer C has no add
 * control. Removing a file is for its uploader only, not for a role: B cannot remove A's file, A removes it, and B no
 * longer lists it. No call here starts a model run: a text file is extracted, not answered.
 */

const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
const filesTab = (page: Page) => page.getByRole('tab', { name: /^(Filer|Files)$/ })
const panel = (page: Page) => page.locator('section.verevon-space-files')
const rowFor = (page: Page, name: string): Locator => panel(page).locator('li.verevon-space-files__row', { hasText: name })

async function openFiles(page: Page, ref: string): Promise<void> {
  await page.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
  await filesTab(page).click()
  await expect(panel(page)).toBeVisible()
}

test('the owner uploads a file, an editor lists and previews it but cannot remove it, and the owner removes it', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    const marker = Date.now().toString(36)
    const name = `e2e-files-${marker}.txt`

    await openFiles(pageA, ref)
    await panel(pageA).locator('input[type="file"]').setInputFiles({ name, mimeType: 'text/plain', buffer: Buffer.from(`Filinnhold ${marker}`, 'utf8') })
    // Processing runs in the background; the row appears once the file is listed.
    await expect(rowFor(pageA, name)).toBeVisible({ timeout: 180_000 })

    await openFiles(pageB, ref)
    const rowB = rowFor(pageB, name)
    await expect(rowB).toBeVisible({ timeout: 30_000 })
    await rowB.getByRole('button', { name: new RegExp(`^(Forhåndsvis|Preview) ${escapeRegExp(name)}$`) }).click()
    const dialog = pageB.getByRole('dialog')
    await expect(dialog.locator('pre.verevon-space-files__text')).toContainText(marker)
    await pageB.keyboard.press('Escape')
    await expect(dialog).toHaveCount(0)
    // Removing is for the uploader only: editor B sees no remove control on A's file.
    await expect(rowB.getByRole('button', { name: /^(Fjern|Remove)$/ })).toHaveCount(0)

    // A removes the file: remove, then confirm.
    const rowA = rowFor(pageA, name)
    await rowA.getByRole('button', { name: /^(Fjern|Remove)$/ }).click()
    await rowA.getByRole('button', { name: new RegExp(`^(Bekreft fjerning av|Confirm removing) ${escapeRegExp(name)}$`) }).click()
    await expect(rowFor(pageA, name)).toHaveCount(0, { timeout: 30_000 })

    // B no longer lists it after a reload.
    await pageB.reload()
    await openFiles(pageB, ref)
    await expect(rowFor(pageB, name)).toHaveCount(0)
  } finally {
    await a.close()
    await b.close()
  }
})

test('a viewer has no add control and no drop zone, and is told why', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'C')
  const c = await memberContext(browser, baseURL, 'C')
  try {
    const page = await c.newPage()
    await openFiles(page, ref)
    await expect(panel(page).locator('.verevon-space-files__add')).toHaveCount(0)
    await expect(panel(page).locator('.verevon-space-files__drop')).toHaveCount(0)
    await expect(page.getByText(/Rollen din i rommet tillater ikke å legge til filer\.|Your role in this room does not allow adding files\./)).toBeVisible()
  } finally {
    await c.close()
  }
})
