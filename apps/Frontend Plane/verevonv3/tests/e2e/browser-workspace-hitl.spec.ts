import { expect, test } from '@playwright/test'

import { closeSessionViaApi, resetHome, submitBrowserUrl } from './browser-workspace-helpers'

/**
 * Phase 6 capability 4: the Phase 5 HITL (human-in-the-loop) browser-action
 * approval gate, end to end through the ACTUAL UI — not the raw gateway API
 * (`verify_phase5_final.py`, referenced in Phase 5's close-out docs, proved
 * this at the API level; this spec is the first time the approval card /
 * evidence-drawer rendering has been exercised in a real browser).
 *
 * `https://example.com/login` deterministically classifies as a `login`-risk
 * action (`browser_risk.rs`'s `LOGIN_MARKERS` matches "/login" in the URL,
 * independent of the LLM planner) on the very first action of a durable
 * browser-agent run — the same target the Phase 6 security fast-follow stage
 * used to live-verify the interruptible-approval-wait fix, chosen again here
 * for the same determinism.
 */
const LOGIN_URL = 'https://example.com/login'

/**
 * `BrowserChrome.tsx` auto-opens the evidence drawer itself the moment a new
 * pending approval appears (Phase 5's own "surface the gate immediately"
 * design), so by the time a test wants to inspect the "Godkjenninger"
 * section after deciding, the drawer may already be open — a blind click on
 * the open/close toggle would then CLOSE it instead. Check the toggle's own
 * `aria-expanded` state first and only click if it is currently closed.
 */
async function ensureEvidenceDrawerOpen(page: import('@playwright/test').Page): Promise<void> {
  const toggle = page.getByLabel('Vis eller skjul tidslinje og evidens')
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') {
    await toggle.click()
  }
  await expect(toggle).toHaveAttribute('aria-expanded', 'true')
}

test.describe('browser workspace: HITL browser-action approval', () => {
  let sessionId: string | null = null

  test.afterEach(async ({ request }) => {
    await closeSessionViaApi(request, sessionId)
    sessionId = null
  })

  test('approving a gated action lets it proceed', async ({ page }) => {
    test.setTimeout(120_000)
    await resetHome(page)
    sessionId = await submitBrowserUrl(page, LOGIN_URL)

    // Start the durable, multi-step AI run ("AI-loop") — the default goal
    // ("Capture useful evidence from this page") is fine; the gate fires on
    // the server-derived start_url, not on goal content.
    const aiLoopButton = page.getByLabel('Kjør flere AI-foreslåtte nettlesersteg')
    await expect(aiLoopButton).toBeEnabled({ timeout: 15_000 })
    await aiLoopButton.click()

    // The approval bubble renders over the live page with the real risk
    // category and reason from execution-core's classifier.
    const approvalAlert = page.getByRole('alert', { name: 'Venter på godkjenning' })
    await expect(approvalAlert).toBeVisible({ timeout: 60_000 })
    await expect(approvalAlert).toContainText('Innlogging')
    await expect(approvalAlert.locator('.knowledge-browser-ai-bubble__body p').first()).not.toBeEmpty()

    // Approve it — a real POST to the generic orchestration decide route.
    const [decideResponse] = await Promise.all([
      page.waitForResponse((res) => res.url().includes('/api/v1/orchestration/approvals/') && res.url().endsWith('/decide') && res.request().method() === 'POST'),
      approvalAlert.getByRole('button', { name: 'Godkjenn' }).click(),
    ])
    expect(decideResponse.status(), await decideResponse.text()).toBe(200)

    // The bubble clears once decided, and the evidence drawer's
    // "Godkjenninger" section records a "Godkjent" (granted) entry — the
    // action was actually allowed to proceed, not silently dropped.
    await expect(approvalAlert).toHaveCount(0, { timeout: 15_000 })
    await ensureEvidenceDrawerOpen(page)
    const approvalsSection = page.locator('.knowledge-browser-evidence__approvals')
    await expect(approvalsSection).toBeVisible()
    await expect(approvalsSection.locator('.verevon-run-approval__badge')).toContainText('Godkjent', { timeout: 15_000 })
  })

  test('rejecting a gated action blocks it from ever executing', async ({ page }) => {
    test.setTimeout(120_000)
    await resetHome(page)
    sessionId = await submitBrowserUrl(page, LOGIN_URL)

    const aiLoopButton = page.getByLabel('Kjør flere AI-foreslåtte nettlesersteg')
    await expect(aiLoopButton).toBeEnabled({ timeout: 15_000 })
    await aiLoopButton.click()

    const approvalAlert = page.getByRole('alert', { name: 'Venter på godkjenning' })
    await expect(approvalAlert).toBeVisible({ timeout: 60_000 })
    await expect(approvalAlert).toContainText('Innlogging')

    const [decideResponse] = await Promise.all([
      page.waitForResponse((res) => res.url().includes('/api/v1/orchestration/approvals/') && res.url().endsWith('/decide') && res.request().method() === 'POST'),
      approvalAlert.getByRole('button', { name: 'Avslå' }).click(),
    ])
    expect(decideResponse.status(), await decideResponse.text()).toBe(200)

    await expect(approvalAlert).toHaveCount(0, { timeout: 15_000 })
    await ensureEvidenceDrawerOpen(page)
    const approvalsSection = page.locator('.knowledge-browser-evidence__approvals')
    await expect(approvalsSection).toBeVisible()
    await expect(approvalsSection.locator('.verevon-run-approval__badge')).toContainText('Avslått', { timeout: 15_000 })

    // The denied entry renders through the distinct denial marker
    // (`verevon-run-approval--denied`), never the normal completed/failed
    // timeline styling — this is the Phase 5 evidence-drawer commit's own
    // stated design intent, verified here for the first time in a real
    // browser.
    await expect(approvalsSection.locator('.verevon-run-approval--denied')).toBeVisible()

    // The AI-loop must have aborted, not merely skipped the gated action and
    // continued: a denial ends the run server-side, and the client-side loop
    // status (`AI-loopstatus`) mirrors that as "Stoppet", never "Utfører"
    // (acting).
    await expect(page.getByLabel('AI-loopstatus')).toHaveText(/Stoppet/, { timeout: 15_000 })
  })
})
