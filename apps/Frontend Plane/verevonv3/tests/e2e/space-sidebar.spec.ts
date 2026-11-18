import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS16-S9 Spaces sidebar, end to end (later tier: needs the seeded Space fixtures and the sidebar route on). Member A
 * uses two browser contexts (two devices) and two tabs in one context. The ZDR organisation (fixture F) keeps its
 * layout in this browser only and sends no layout write. Nothing here starts a model run.
 *
 * Owed: two contexts see the same sections after a reload; two tabs rebase a stale write (409) and keep both edits;
 * a ZDR organisation stays local.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_REF_ZDR   a Space in the ZDR fixture organisation (as in space-realtime.spec.ts)
 */

const nav = (page: Page) => page.getByRole('navigation', { name: /^(Romnavigasjon|Spaces navigation)$/ })
const sectionNamed = (page: Page, name: string) => page.locator('section.space-sidebar-section', { hasText: name })
const layoutWrite = (response: { request: () => { method: () => string }; url: () => string }) =>
  response.request().method() === 'PUT' && response.url().includes('/api/v1/spaces/sidebar')

async function openSidebar(page: Page, ref: string): Promise<void> {
  await page.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
  await expect(nav(page)).toBeVisible()
}

async function createSection(page: Page, name: string): Promise<void> {
  await page.getByRole('button', { name: /^(Ny gruppe|New section)$/ }).click()
  await page.getByLabel(/^(Gruppenavn|Section name)$/).fill(name)
  await page.getByRole('button', { name: /^(Opprett|Create)$/ }).click()
  await expect(sectionNamed(page, name)).toBeVisible({ timeout: 15_000 })
}

test('two browser contexts of one member see the same sections after a reload', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const first = await memberContext(browser, baseURL, 'A')
  const second = await memberContext(browser, baseURL, 'A')
  try {
    const name = `E2E ${Date.now().toString(36)}`
    const pageOne = await first.newPage()
    await openSidebar(pageOne, ref)
    const saved = pageOne.waitForResponse(layoutWrite)
    await createSection(pageOne, name)
    expect((await saved).status()).toBeLessThan(300)
    await pageOne.reload()

    const pageTwo = await second.newPage()
    await openSidebar(pageTwo, ref)
    await expect(sectionNamed(pageOne, name)).toBeVisible()
    await expect(sectionNamed(pageTwo, name)).toBeVisible()
  } finally {
    await first.close()
    await second.close()
  }
})

test('two tabs of one member both keep their section edits when one write is stale', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const tabOne = await a.newPage()
    const tabTwo = await a.newPage()
    await openSidebar(tabOne, ref)
    // Tab two loads before tab one writes, so its next write carries an older revision.
    await openSidebar(tabTwo, ref)

    const marker = Date.now().toString(36)
    const first = `Tab en ${marker}`
    const second = `Tab to ${marker}`
    const saved = tabOne.waitForResponse(layoutWrite)
    await createSection(tabOne, first)
    expect((await saved).status()).toBeLessThan(300)

    // A stale write answers 409; the tab rebases its edit on the server copy and saves again, not as a failure.
    await createSection(tabTwo, second)
    await expect(tabTwo.locator('.space-sidebar-notice[role="alert"]')).toHaveCount(0)

    await tabOne.reload()
    await tabTwo.reload()
    for (const tab of [tabOne, tabTwo]) {
      await expect(sectionNamed(tab, first)).toBeVisible()
      await expect(sectionNamed(tab, second)).toBeVisible()
    }
  } finally {
    await a.close()
  }
})

test('a zero-retention organisation keeps its sections in this browser only and sends no layout write', async ({ browser, baseURL }) => {
  const zdrRef = process.env.E2E_SPACE_REF_ZDR?.trim() ?? ''
  skipUnlessSpace(test, !zdrRef, 'E2E_SPACE_REF_ZDR must name a Space in the ZDR fixture organisation')
  requireMembers(test, 'F')
  const f = await memberContext(browser, baseURL, 'F')
  try {
    const page = await f.newPage()
    const writes: string[] = []
    page.on('request', (request) => {
      if (request.method() === 'PUT' && request.url().includes('/api/v1/spaces/sidebar')) writes.push(request.url())
    })
    await openSidebar(page, zdrRef)
    await expect(page.locator('.space-sidebar-local-note')).toContainText(/Lagret bare i denne nettleseren|Saved in this browser only/)
    await createSection(page, `ZDR ${Date.now().toString(36)}`)
    // Longer than the 400 ms save debounce, so a server write would have gone out by now.
    await page.waitForTimeout(2_000)
    expect(writes).toHaveLength(0)
  } finally {
    await f.close()
  }
})
