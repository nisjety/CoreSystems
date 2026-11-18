import { expect, test, type Locator, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS16-S5 UI baseline under emulated forced colors and more contrast (later tier: needs the seeded Space fixtures and
 * a needs-you attention item at critical salience for member A). The critical row border lives on the Attention page
 * and the Space Activity tab, not in the room list or the sidebar; the presence, unread and working dots sit in the
 * room and the sidebar. Whether this Chromium project emulates forced colors at all is unverified (U1), so each test
 * checks its media query first and skips (fails under E2E_REQUIRE_SPACE=1) when the emulation did not take.
 * Nothing here starts a model run.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_CRITICAL_ATTENTION=1   member A has a needs-you item at critical salience
 */

const criticalRow = (page: Page): Locator =>
  page.locator('li.verevon-attention-row[data-kind="needs_you"][data-salience="critical"]').first()
const dots = (page: Page): Locator => page.locator('.core-sidebar-working-dot, .core-sidebar-unread-dot, .verevon-space-presence__dot')

async function borderLeftPx(locator: Locator): Promise<number> {
  return locator.evaluate((element) => Number.parseFloat(getComputedStyle(element).borderLeftWidth))
}

/** A visible status dot keeps a size a sighted or low-vision reader can see. */
async function expectDotsKeepTheirSize(page: Page): Promise<void> {
  for (const dot of await dots(page).all()) {
    if (!(await dot.isVisible())) continue
    const box = await dot.boundingBox()
    expect(box?.width ?? 0, 'a status dot keeps a visible size').toBeGreaterThanOrEqual(6)
  }
}

test.describe('UI baseline under emulated accessibility modes', () => {
  test('forced colors: the critical row keeps a border and the status dots keep their size', async ({ browser, baseURL }, testInfo) => {
    const ref = requireSpaceRef(test)
    requireMembers(test, 'A')
    skipUnlessSpace(test, process.env.E2E_SPACE_CRITICAL_ATTENTION !== '1', 'E2E_SPACE_CRITICAL_ATTENTION=1 needs a critical needs-you item for member A')
    const a = await memberContext(browser, baseURL, 'A')
    try {
      const page = await a.newPage()
      await page.emulateMedia({ forcedColors: 'active' })
      const forced = await page.evaluate(() => window.matchMedia('(forced-colors: active)').matches)
      skipUnlessSpace(test, !forced, 'Chromium in this project did not emulate forced colors (U1)')

      await page.goto('/attention')
      await expect(criticalRow(page)).toBeVisible()
      expect(await borderLeftPx(criticalRow(page)), 'the critical border survives forced colors').toBeGreaterThanOrEqual(2)
      await testInfo.attach('attention-forced-colors', { body: await page.screenshot({ fullPage: true }), contentType: 'image/png' })

      await page.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
      await expectDotsKeepTheirSize(page)
      await testInfo.attach('room-forced-colors', { body: await page.screenshot({ fullPage: true }), contentType: 'image/png' })
    } finally {
      await a.close()
    }
  })

  test('more contrast: the critical row keeps a border and the status dots keep their size', async ({ browser, baseURL }, testInfo) => {
    const ref = requireSpaceRef(test)
    requireMembers(test, 'A')
    skipUnlessSpace(test, process.env.E2E_SPACE_CRITICAL_ATTENTION !== '1', 'E2E_SPACE_CRITICAL_ATTENTION=1 needs a critical needs-you item for member A')
    const a = await memberContext(browser, baseURL, 'A')
    try {
      const page = await a.newPage()
      await page.emulateMedia({ contrast: 'more' })
      const more = await page.evaluate(() => window.matchMedia('(prefers-contrast: more)').matches)
      skipUnlessSpace(test, !more, 'Chromium in this project did not emulate prefers-contrast: more (U1)')

      await page.goto('/attention')
      await expect(criticalRow(page)).toBeVisible()
      expect(await borderLeftPx(criticalRow(page)), 'the critical border survives more contrast').toBeGreaterThanOrEqual(2)
      await testInfo.attach('attention-more-contrast', { body: await page.screenshot({ fullPage: true }), contentType: 'image/png' })

      await page.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
      await expectDotsKeepTheirSize(page)
      await testInfo.attach('room-more-contrast', { body: await page.screenshot({ fullPage: true }), contentType: 'image/png' })
    } finally {
      await a.close()
    }
  })
})
