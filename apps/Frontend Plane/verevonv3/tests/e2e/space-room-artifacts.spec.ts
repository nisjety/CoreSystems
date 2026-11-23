import { expect, test, type Locator, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * WS07-S11 and S12, and the artifacts case of M1-S7, end to end: the agent artifacts of a room as its
 * members read them. Member A opens a chip under an agent turn and the per-thread sheet (Tab stays in
 * it, the page behind it is inert, Escape gives focus back), then the room's Artifacts tab and the
 * version stepper of a revised artifact. Member B, a second member of the same Space, sees the same
 * artifacts in that tab and can open one. Nothing here starts a model run.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_REF            a Space whose conversation holds an agent artifact with at least two versions,
 *                            with the room artifact surfaces on (features.room_artifacts)
 *   E2E_STORAGE_STATE_B      a second member of that Space whose history window includes the conversation
 *
 * A missing fixture skips these tests on a laptop and fails them under E2E_REQUIRE_SPACE=1.
 */

/** The room and its data load over the gateway; a cold start can take a while. */
const LOAD_WAIT = 30_000

async function openRoom(page: Page, ref: string): Promise<void> {
  await page.goto(`/spaces/${encodeURIComponent(ref)}`)
  await expect(page.getByRole('tablist', { name: /^(Romvisninger|Space views)$/ })).toBeVisible({ timeout: LOAD_WAIT })
}

async function openArtifactsTab(page: Page, ref: string): Promise<void> {
  await openRoom(page, ref)
  await page.getByRole('tab', { name: /^(Artefakter|Artifacts)$/ }).click()
  await expect(page.locator('.verevon-space-artifacts__entry').first()).toBeVisible({ timeout: LOAD_WAIT })
}

/** The room artifact titles the Artifacts tab lists, in the order it lists them. */
async function artifactTitles(page: Page, ref: string): Promise<string[]> {
  await openArtifactsTab(page, ref)
  const titles = await page.locator('.verevon-space-artifacts__title').allTextContents()
  return titles.map((title) => title.trim())
}

/** The per-thread sheet, named by its heading ("Artefakter i ..." / "Artifacts in ..."). */
const sheet = (page: Page) => page.getByRole('dialog', { name: /^(Artefakter i|Artifacts in)/ })

/** True when the focus is on the sheet or somewhere inside it. */
const focusInside = (dialog: Locator) => dialog.evaluate((element) => element.contains(document.activeElement))

test('an artifact chip opens the sheet, Tab stays inside it, and Escape gives focus back to the chip', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await openRoom(page, ref)

    const chip = page.locator('.verevon-space-artifact-chip').first()
    await expect(chip).toBeVisible({ timeout: LOAD_WAIT })
    await expect(chip).toHaveAccessibleName(/^(Åpne artefakt|Open artifact) .+, (versjon|version) \d+$/)
    await chip.click()

    const dialog = sheet(page)
    await expect(dialog).toBeVisible()
    await expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(await focusInside(dialog)).toBe(true)
    // The chip that opened the sheet sits behind it, so it is inert while the sheet is open.
    expect(await chip.evaluate((element) => element.closest('[inert]') !== null)).toBe(true)

    // Tab from the last stop wraps to the first, so focus never leaves the sheet, however far it goes.
    for (let step = 0; step < 40; step += 1) {
      await page.keyboard.press('Tab')
      expect(await focusInside(dialog), `focus left the sheet after ${step + 1} Tab presses`).toBe(true)
    }
    await page.keyboard.press('Shift+Tab')
    expect(await focusInside(dialog)).toBe(true)

    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
    await expect(chip).toBeFocused()
    expect(await chip.evaluate((element) => element.closest('[inert]') === null)).toBe(true)
  } finally {
    await a.close()
  }
})

test('the Artifacts tab lists the room artifacts, and the version stepper walks a revised artifact', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await openArtifactsTab(page, ref)
    await expect(page.getByRole('heading', { level: 2, name: /^(Artefakter|Artifacts)$/ })).toBeVisible()

    // The fixture's revised artifact: its entry says how many versions it has.
    const revised = page.locator('.verevon-space-artifacts__entry').filter({ hasText: /\d+ (versjoner|versions)/ }).first()
    await expect(revised).toBeVisible()
    await revised.click()

    const dialog = sheet(page)
    await expect(dialog).toBeVisible()
    const stepper = dialog.locator('.verevon-chat-artifact-versions')
    const previous = stepper.getByRole('button', { name: /^(Forrige versjon|Previous version)$/ })
    const next = stepper.getByRole('button', { name: /^(Neste versjon|Next version)$/ })
    const label = stepper.locator(':scope > span')

    // The sheet opens on the newest revision: Next is off, Previous steps back.
    await expect(next).toBeDisabled()
    const newest = ((await label.textContent()) ?? '').trim()
    expect(newest).not.toBe('')
    await previous.click()
    await expect(label).not.toHaveText(newest)
    await expect(next).toBeEnabled()

    await next.click()
    await expect(label).toHaveText(newest)
    await expect(next).toBeDisabled()
  } finally {
    await a.close()
  }
})

test('a second member of the Space sees the same artifacts in the Artifacts tab and can open one', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    const titlesA = await artifactTitles(pageA, ref)
    expect(titlesA.length, 'the fixture Space lists at least one artifact to A').toBeGreaterThan(0)

    const titlesB = await artifactTitles(pageB, ref)
    expect(titlesB, 'B lists the artifact A lists').toContain(titlesA[0])

    await pageB.locator('.verevon-space-artifacts__entry').filter({ hasText: titlesA[0]! }).first().click()
    const dialog = sheet(pageB)
    await expect(dialog).toBeVisible({ timeout: LOAD_WAIT })
    await expect(dialog.locator('.verevon-chat-artifact-view')).toBeVisible()
  } finally {
    await a.close()
    await b.close()
  }
})
