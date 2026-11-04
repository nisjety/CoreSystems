import { expect, test } from '@playwright/test'

import { closeSessionViaApi, createBrowserSession, EXAMPLE_URL, resetHome, submitBrowserUrl } from './browser-workspace-helpers'

/**
 * WS9.A (browser omnibox search) API evidence. Not run in the W2 build; the test
 * phase runs it against the stack. The component behaviour (dropdown keys, the
 * history, the ZDR gate on the client) is covered by the unit tests in
 * src/features/browser/omnibox; this spec pins the gateway side:
 *   - a zero-data-retention caller gets an empty suggestion list, with no upstream call;
 *   - the two omnibox action ids dispatch for a signed-in user.
 */
test.describe('browser omnibox (WS9.A)', () => {
  test('suggestions are empty for a zero-data-retention request', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.get(`${origin}/api/v1/search/suggestions?q=norwegian&scope=queries&limit=6`, {
      headers: { 'x-zdr': '1' },
    })
    expect(response.status()).toBe(200)
    const body = await response.json()
    expect(body.data.suggestions).toEqual([])
  })

  test('the browser_omnibox.search action dispatches for a signed-in user', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.post(`${origin}/api/v1/actions/execute`, {
      data: {
        actionId: 'browser_omnibox.search',
        idempotencyKey: `omnibox-${Date.now()}`,
        input: { query: 'norwegian salmon exports', limit: 3 },
      },
    })
    expect(response.status()).toBe(200)
    const body = await response.json()
    expect(body.data.actionId).toBe('browser_omnibox.search')
    expect(body.data.status).toBe('completed')
  })

  test('an empty browser_omnibox.search query is refused as invalid input', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.post(`${origin}/api/v1/actions/execute`, {
      data: { actionId: 'browser_omnibox.search', idempotencyKey: `omnibox-bad-${Date.now()}`, input: { query: '   ' } },
    })
    expect(response.status()).toBe(400)
  })
})

/**
 * WS9.B (reload and the navigation error card), D2-133. Written in the W2 build and NOT run:
 * it needs the live stack (a Chromium Quarry edge, the gateway and the SPA, signed in as the
 * e2e user). It pins three things:
 *   - a navigation that does not load shows the typed error card (role alert, the URL that was
 *     asked for, a typed reason, a Norwegian sentence, never a raw server message), Retry loads
 *     that URL again and Lukk removes the card;
 *   - the reload request body on the wire: a hard reload posts exactly
 *     {type: 'reload', ignore_cache: true} with actor human, a soft reload is a plain navigate;
 *   - the gateway keeps its two refusals: an agent actor cannot reload (403
 *     browser_action_denied) and a reload that carries a url is refused as invalid.
 */
const UNRESOLVABLE_URL = 'https://this-host-does-not-resolve.invalid/'

type ActionRequestBody = { action?: Record<string, unknown>; actor?: string }

function isActionPost(request: { url(): string; method(): string }): boolean {
  return request.method() === 'POST' && /\/api\/v1\/browser\/sessions\/[^/]+\/actions$/.test(new URL(request.url()).pathname)
}

test.describe('browser navigation error card and reload (WS9.B, D2-133)', () => {
  let sessionId: string | null = null

  test.afterEach(async ({ request }) => {
    await closeSessionViaApi(request, sessionId)
    sessionId = null
  })

  test('a navigation that does not load shows the error card, Prøv igjen retries that URL, Lukk removes it', async ({ page }) => {
    test.setTimeout(120_000)
    await resetHome(page)
    sessionId = await submitBrowserUrl(page, EXAMPLE_URL)

    const addressInput = page.getByLabel('Nettleseradresse')
    await addressInput.fill(UNRESOLVABLE_URL)
    const [failed] = await Promise.all([
      page.waitForResponse((res) => isActionPost(res.request())),
      page.getByLabel('Naviger til adresse').click(),
    ])
    expect(failed.status()).toBeGreaterThanOrEqual(400)

    const card = page.locator('.browser-navigation-error')
    await expect(card).toBeVisible({ timeout: 20_000 })
    await expect(card).toHaveAttribute('role', 'alert')
    await expect(card).toHaveAttribute('data-reason', /^(invalid_url|blocked|unreachable|failed)$/)
    await expect(card.locator('.browser-navigation-error__title')).toHaveText('Siden kunne ikke åpnes')
    await expect(card.locator('.browser-navigation-error__url')).toHaveText(UNRESOLVABLE_URL)
    // The card carries a typed sentence only: no raw error code, stack or upstream message.
    const text = (await card.innerText()).toLowerCase()
    for (const raw of ['err_', 'error_code', 'chromium', 'quarry', 'stack', 'undefined']) {
      expect(text, `card text must not contain "${raw}"`).not.toContain(raw)
    }

    // Prøv igjen sends a navigate action for the same URL, not a reload.
    const [retry] = await Promise.all([
      page.waitForRequest((request) => isActionPost(request)),
      card.getByRole('button', { name: 'Prøv igjen' }).click(),
    ])
    const retryBody = retry.postDataJSON() as ActionRequestBody
    expect(retryBody.action).toEqual({ type: 'navigate', url: UNRESOLVABLE_URL })
    expect(retryBody.actor).toBe('human')

    // It cannot load, so the card is still there; Lukk removes it.
    await expect(card).toBeVisible({ timeout: 20_000 })
    await card.getByRole('button', { name: 'Lukk' }).click()
    await expect(card).toHaveCount(0)
  })

  test('the reload buttons post the documented request body', async ({ page }) => {
    test.setTimeout(120_000)
    await resetHome(page)
    sessionId = await submitBrowserUrl(page, EXAMPLE_URL)

    // A soft reload is a GET of the session URL: a navigate action, never a reload action.
    const [soft] = await Promise.all([
      page.waitForRequest((request) => isActionPost(request)),
      page.getByRole('button', { name: 'Last nettlesersiden på nytt', exact: true }).click(),
    ])
    const softBody = soft.postDataJSON() as ActionRequestBody
    expect(softBody.action?.type).toBe('navigate')
    expect(softBody.actor).toBe('human')

    // The hard reload only exists when the server advertises hard_reload.
    const hard = page.getByRole('button', { name: 'Last nettlesersiden på nytt uten hurtigbuffer' })
    test.skip((await hard.count()) === 0, 'the Quarry edge in this stack does not advertise hard_reload')
    const [hardRequest, hardResponse] = await Promise.all([
      page.waitForRequest((request) => isActionPost(request)),
      page.waitForResponse((res) => isActionPost(res.request())),
      hard.click(),
    ])
    const hardBody = hardRequest.postDataJSON() as ActionRequestBody
    expect(hardBody.action).toEqual({ type: 'reload', ignore_cache: true })
    expect(hardBody.actor).toBe('human')
    expect(hardResponse.status(), await hardResponse.text()).toBe(200)
  })

  test('the gateway refuses an agent reload and a reload that carries a url', async ({ request, baseURL }) => {
    test.setTimeout(90_000)
    const origin = baseURL ?? 'http://localhost:5173'
    const created = await createBrowserSession(request, origin, { url: EXAMPLE_URL })
    sessionId = created.session.id
    const actionsUrl = `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/actions`

    const agent = await request.post(actionsUrl, {
      headers: { 'content-type': 'application/json', origin },
      data: { action: { type: 'reload', ignore_cache: false }, actor: 'agent' },
    })
    expect(agent.status()).toBe(403)
    expect(((await agent.json()) as { error?: { code?: string } }).error?.code).toBe('browser_action_denied')

    const withUrl = await request.post(actionsUrl, {
      headers: { 'content-type': 'application/json', origin },
      data: { action: { type: 'reload', url: 'https://example.com/' }, actor: 'human' },
    })
    expect(withUrl.status()).toBe(400)
    expect(((await withUrl.json()) as { error?: { code?: string } }).error?.code).toBe('invalid_browser_action')
  })
})
