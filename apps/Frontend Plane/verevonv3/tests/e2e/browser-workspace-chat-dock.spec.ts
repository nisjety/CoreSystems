import { expect, test, type Page } from '@playwright/test'

/**
 * The browser dock beside a live run (WS9.G), end to end.
 *
 * Runs in the `local` project (this file matches `browser-workspace-*.spec.ts`), against
 * the local dev stack, with the chat dock flag `VITE_CHAT_BROWSER_DOCK` on in the served
 * build. A deep-research turn is the run that opens the live panel.
 *
 * Code-only pass: this spec was written and is NOT run. Its selectors for the deep-research
 * control are unverified (see `startDeepResearchRun`). Selectors are classes and the dock's
 * own data attributes, never labels: the chrome is bilingual.
 *
 * Covers: right by default, bottom persisted across a reload, maximised, hidden, and the
 * restore from hidden. The stored record holds exactly four fields and no page content.
 */

const PAGE = '.verevon-chat-page'
const PANEL = '.verevon-chat-run-panel'
const PANEL_TOGGLE = '.verevon-chat-run-panel__toggle'
const DOCK_PICKER = '.browser-dock-controls'
const DOCK_KEY_PREFIX = 'verevon.chat.browser-dock.v1:'
const DOCK_FIELDS = ['chatPerTab', 'composerHidden', 'position', 'size']

/** A deep-research turn runs several rounds before it answers. */
const RUN_TIMEOUT = 180_000

/**
 * A fresh chat for this user. The init script runs on every navigation, so it clears the
 * dock record only once per tab (the sessionStorage guard); a reload must keep the record.
 */
async function openFreshChat(page: Page): Promise<void> {
  await page.addInitScript(() => {
    if (window.sessionStorage.getItem('dock-spec-initialised')) return
    window.sessionStorage.setItem('dock-spec-initialised', '1')
    window.localStorage.setItem('verevon.locale', 'en')
    for (const key of Object.keys(window.localStorage)) {
      if (key.startsWith('verevon.chat.browser-dock.v1:')) window.localStorage.removeItem(key)
    }
    window.localStorage.removeItem('verevon.chat.runPanel.collapsed.v1')
  })
  await page.goto('/chat')
  await expect(page.locator(PAGE)).toBeVisible()
}

/**
 * Starts a deep-research turn, which opens the live run panel beside the transcript.
 * UNVERIFIED: the control is matched by its bilingual label, which the code-only pass did not
 * check against the rendered composer. Adjust the locator when the spec is first run.
 */
async function startDeepResearchRun(page: Page): Promise<void> {
  await page.getByRole('button', { name: /Deep research|Dyp research/ }).first().click()
  await page.locator('form textarea').fill('Undersøk to kilder om rengjøring av kjøkkenvask og oppsummer kort.')
  await page.locator('form button[type="submit"]').click()
  await expect(page.locator(PANEL)).toBeVisible({ timeout: RUN_TIMEOUT })
}

/** The one dock record this account holds, or null. Keys are org and user ids, so match by prefix. */
async function readDockRecord(page: Page): Promise<{ key: string; value: string } | null> {
  return page.evaluate((prefix) => {
    const key = Object.keys(window.localStorage).find((candidate) => candidate.startsWith(prefix))
    if (!key) return null
    const value = window.localStorage.getItem(key)
    return value === null ? null : { key, value }
  }, DOCK_KEY_PREFIX)
}

async function pickLayout(page: Page, position: 'right' | 'bottom' | 'maximised' | 'hidden'): Promise<void> {
  await page.locator(`${DOCK_PICKER} [data-position="${position}"]`).click()
}

test.describe('browser dock beside a live run', () => {
  test('docks right by default, keeps bottom across a reload, maximises, hides and restores', async ({ page }) => {
    test.setTimeout(RUN_TIMEOUT + 120_000)
    await openFreshChat(page)
    await startDeepResearchRun(page)

    // Default layout: the browser docks to the right, as the rail always has.
    await expect(page.locator(PAGE)).toHaveClass(/verevon-chat-page--dock-right/)
    await expect(page.locator(`${DOCK_PICKER} [data-position="right"]`)).toHaveAttribute('aria-pressed', 'true')

    // Bottom: the class follows the choice, and the choice survives a reload.
    await pickLayout(page, 'bottom')
    await expect(page.locator(PAGE)).toHaveClass(/verevon-chat-page--dock-bottom/)
    await page.reload()
    await expect(page.locator(PAGE)).toBeVisible()
    await expect(page.locator(PANEL)).toBeVisible({ timeout: RUN_TIMEOUT })
    await expect(page.locator(PAGE)).toHaveClass(/verevon-chat-page--dock-bottom/)

    // The stored record: exactly four fields, the chosen layout, and no page or run content.
    const record = await readDockRecord(page)
    expect(record, 'the dock record must be stored under the per-account key').not.toBeNull()
    const stored = JSON.parse(record?.value ?? 'null') as Record<string, unknown>
    expect(Object.keys(stored).sort()).toEqual(DOCK_FIELDS)
    expect(stored.position).toBe('bottom')
    expect(stored.chatPerTab).toBe('keep')
    expect(stored.composerHidden).toBe(false)
    expect(record?.value ?? '').not.toMatch(/https?:|run_|session/i)

    // Maximised: the browser covers the page.
    await pickLayout(page, 'maximised')
    await expect(page.locator(PAGE)).toHaveClass(/verevon-chat-page--dock-maximised/)

    // Hidden: the rail folds to its collapsed strip, and no dock class applies.
    await pickLayout(page, 'hidden')
    await expect(page.locator(PANEL)).toHaveClass(/verevon-chat-run-panel--collapsed/)
    await expect(page.locator(PAGE)).not.toHaveClass(/verevon-chat-page--dock-/)

    // Restoring from the rail returns to the last visible layout this visit (maximised).
    await page.locator(PANEL_TOGGLE).click()
    await expect(page.locator(PAGE)).toHaveClass(/verevon-chat-page--dock-maximised/)
    const restored = await readDockRecord(page)
    expect(JSON.parse(restored?.value ?? 'null')).toMatchObject({ position: 'maximised' })
  })
})
