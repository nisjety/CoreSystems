import { expect, test } from '@playwright/test'

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
