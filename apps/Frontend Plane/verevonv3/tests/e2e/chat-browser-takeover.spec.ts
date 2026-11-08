import { expect, test, type Page } from '@playwright/test'

/**
 * Take over and give back of a chat Do run's browser (EP-1 S1), end to end.
 *
 * Needs a stack with Quarry's owner of record (Q-1) and `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` (Q-4),
 * the WS21-S6 `browser_session_opened` binding, and the switches on: `EXECUTION_DO_BROWSER_ENABLED`,
 * `EXECUTION_BROWSER_TAKEOVER_PAUSE`, `BROWSER_DO_ENABLED` and `VITE_CHAT_BROWSER_TAKEOVER` in the
 * served build. Skipped unless `E2E_CHAT_BROWSER_TAKEOVER=1` says that stack is up.
 *
 * Code-only pass: this spec was written and is NOT run. Selectors are classes and the control
 * chip's ARIA state, never labels: the chrome is bilingual.
 */

const PAGE = '.verevon-chat-page'
const PANEL = '.verevon-chat-run-panel'
const CONTROL_CHIP = `${PANEL} .knowledge-browser-control-chip`

/** A Do turn opens the page, observes it, and answers; the takeover waits inside it. */
const RUN_TIMEOUT = 180_000

async function openFreshChat(page: Page): Promise<void> {
  await page.addInitScript(() => window.localStorage.setItem('verevon.locale', 'en'))
  await page.goto('/chat')
  await expect(page.locator(PAGE)).toBeVisible()
}

/** Starts a Do turn whose first browser call opens example.com, so the live panel shows its frame. */
async function startBrowsingTurn(page: Page): Promise<void> {
  await page.locator('form textarea').fill('Open https://example.com and tell me the title of the page.')
  await page.locator('form button[type="submit"]').click()
  await expect(page.locator(CONTROL_CHIP)).toBeEnabled({ timeout: RUN_TIMEOUT })
}

/** The control route's answer for the next flip, so the spec reads Quarry's epoch from the wire. */
async function flip(page: Page): Promise<{ mode: string; epoch: number | null }> {
  const [response] = await Promise.all([
    page.waitForResponse((res) => /\/api\/v1\/browser\/sessions\/[^/]+\/control$/.test(new URL(res.url()).pathname)
      && res.request().method() === 'POST'),
    page.locator(CONTROL_CHIP).click(),
  ])
  expect(response.status(), await response.text()).toBe(200)
  const body = (await response.json()) as { data: { session: { control?: { mode: string; epoch?: number | null } } } }
  return { mode: body.data.session.control?.mode ?? '', epoch: body.data.session.control?.epoch ?? null }
}

test.describe('chat live panel: take over and give back the agent browser', () => {
  test.skip(process.env.E2E_CHAT_BROWSER_TAKEOVER !== '1', 'needs the EP-1 stack (Q-1, Q-4, WS21-S6)')

  test('the person takes the browser, the run waits, and continues after the give-back', async ({ page }) => {
    test.setTimeout(RUN_TIMEOUT + 120_000)
    await openFreshChat(page)
    await startBrowsingTurn(page)

    const takenOver = await flip(page)
    expect(takenOver.mode).toBe('human_takeover')
    await expect(page.locator(CONTROL_CHIP)).toHaveAttribute('aria-pressed', 'true')

    const givenBack = await flip(page)
    expect(givenBack.mode).toBe('agent_control')
    expect(givenBack.epoch).toBe((takenOver.epoch ?? 0) + 1)
    await expect(page.locator(CONTROL_CHIP)).toHaveAttribute('aria-pressed', 'false')

    // The run goes on after the give-back and answers the turn.
    await expect(page.locator('.verevon-chat-page [data-role="assistant"]').last()).toContainText(/Example Domain/i, {
      timeout: RUN_TIMEOUT,
    })
  })
})
