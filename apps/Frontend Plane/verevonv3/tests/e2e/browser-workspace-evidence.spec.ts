import { expect, test } from '@playwright/test'

/**
 * WS9.D (evidence panels) API evidence. Not run in the W2 build; the test phase runs it
 * against the stack. The panel behaviour (ZDR gate, polling rules, redacted rendering) is
 * covered by the unit tests in src/features/browser/evidence. This spec pins the gateway side
 * the panels call:
 *   - the evidence routes are owner-gated: an unknown session is a 404, the same as a foreign one;
 *   - the devtools filter allowlist refuses an unknown level with a typed 400;
 *   - the procedure compile is a POST and is refused for an unknown session.
 */
test.describe('browser evidence panels (WS9.D)', () => {
  const unknownSession = '00000000-0000-4000-8000-000000000000'

  test('the egress and receipts reads are owner-gated', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    for (const path of ['egress-receipts', 'receipts']) {
      const response = await request.get(`${origin}/api/v1/browser/sessions/${unknownSession}/${path}`)
      expect(response.status()).toBe(404)
      const body = await response.json()
      expect(body.error.code).toBe('browser_session_not_found')
    }
  })

  test('the procedure compile is owner-gated', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.post(`${origin}/api/v1/browser/sessions/${unknownSession}/procedure`, {
      data: {},
    })
    expect(response.status()).toBe(404)
    const body = await response.json()
    expect(body.error.code).toBe('browser_session_not_found')
  })

  test('an unknown devtools level is refused as an invalid devtools query', async ({ request, baseURL }) => {
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.get(
      `${origin}/api/v1/browser/sessions/${unknownSession}/devtools?levels=not-a-level`,
    )
    expect(response.status()).toBe(400)
    const body = await response.json()
    expect(body.error.code).toBe('invalid_devtools_query')
  })
})
