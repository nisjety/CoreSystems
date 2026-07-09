import { expect, test } from '@playwright/test'

import { closeSessionViaApi, EXAMPLE_URL, resetHome, submitBrowserUrl } from './browser-workspace-helpers'

/**
 * Phase 6 capability 3: live/near-live frame streaming and human takeover
 * (Phase 4), driven through the real Home UI. `browser-live.spec.ts` already
 * proves the underlying WS/poll transport at the API level as `e2e@velion.dev`
 * — this spec proves the SPA control surface built on top of it actually
 * works end to end as `local@velion.dev`: the rendered frame visibly
 * refreshes on its own, and the human/agent control toggle is a real round
 * trip to the backend, not just client-side UI state.
 */
test.describe('browser workspace: live frame streaming and human takeover', () => {
  let sessionId: string | null = null

  test.afterEach(async ({ request }) => {
    await closeSessionViaApi(request, sessionId)
    sessionId = null
  })

  test('the rendered frame keeps refreshing on its own, and toggling control mode is a real backend round trip', async ({ page }) => {
    test.setTimeout(90_000)

    // `BrowserChrome.tsx` sends the control toggle over the live frame
    // WebSocket when one is open (`sendBrowserWsControl`), falling back to a
    // REST `POST .../control` only when it is not — both are genuine
    // round trips to the backend, so this spec accepts either as proof,
    // rather than assuming one specific transport is always active.
    const sentControlFrames: string[] = []
    page.on('websocket', (ws) => {
      ws.on('framesent', (frame) => {
        if (typeof frame.payload === 'string' && frame.payload.includes('"type":"control"')) {
          sentControlFrames.push(frame.payload)
        }
      })
    })

    await resetHome(page)
    sessionId = await submitBrowserUrl(page, EXAMPLE_URL)

    const frameImage = page.getByAltText(/Gjengitt nettleserside for .+/)
    await expect(frameImage).toBeVisible({ timeout: 20_000 })

    // Near-live streaming: the rendered frame's `src` (a polling URL with a
    // cache-busting tick, a live-frame-stream data URL, or a WS-delivered
    // data URL, depending on which transport is active for this session)
    // must actually change between two samples taken far enough apart —
    // proving a live feed is really updating the page, not a single static
    // screenshot.
    const firstSrc = await frameImage.getAttribute('src')
    await page.waitForTimeout(2_000)
    await expect
      .poll(async () => frameImage.getAttribute('src'), { timeout: 10_000 })
      .not.toBe(firstSrc)

    // Human takeover: capture whichever control-chip state the session
    // started in, click it, and confirm the state actually flips AND the
    // click produced a real backend round trip (a WS control frame or a
    // `POST .../control` request) — not just a client-side label swap.
    const controlChip = page.locator('.knowledge-browser-control-chip')
    const before = await controlChip.getAttribute('aria-pressed')

    const restControlWait = page
      .waitForResponse((res) => res.url().includes('/control') && res.request().method() === 'POST', { timeout: 5_000 })
      .catch(() => null)
    await controlChip.click()
    await expect(controlChip).toHaveAttribute('aria-pressed', before === 'true' ? 'false' : 'true')
    const restResponse = await restControlWait
    expect(
      restResponse !== null || sentControlFrames.length > 0,
      'toggling control mode must be a real backend round trip (WS control frame or REST POST), not just client-side state',
    ).toBeTruthy()
    if (restResponse) expect(restResponse.status(), await restResponse.text()).toBe(200)

    // Toggling back returns the chip to its original state — a full round
    // trip in both directions, not a one-way client flag.
    sentControlFrames.length = 0
    const restControlWaitBack = page
      .waitForResponse((res) => res.url().includes('/control') && res.request().method() === 'POST', { timeout: 5_000 })
      .catch(() => null)
    await controlChip.click()
    await expect(controlChip).toHaveAttribute('aria-pressed', before ?? 'false')
    const restResponseBack = await restControlWaitBack
    expect(restResponseBack !== null || sentControlFrames.length > 0).toBeTruthy()
  })
})
