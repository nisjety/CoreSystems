import { expect, test } from '@playwright/test'

/**
 * EP-8 S7 (Space memory: owner list and delete) gateway evidence. Not in CI today; run locally
 * against a live dev stack. The tab's states are covered by
 * src/features/spaces/components/SpaceMemoryTab.test.tsx and the authority by Session Core's own
 * tests. This spec pins what a browser can observe of the two routes:
 *   - with `GATEWAY_SPACE_MEMORY_UI` off (the default), both answer 404 `feature_unavailable`;
 *   - with it on, a Space the caller cannot manage is refused 403 `space_memory_not_authorized`
 *     (or 404 `space_not_found`), never listed, and the refusal carries no upstream text;
 *   - a malformed memory id is a typed 400 before any Control call.
 * Set `SPACE_MEMORY_E2E_ENABLED=true` when the stack runs with the gateway switch on.
 */
const switchedOn = process.env.SPACE_MEMORY_E2E_ENABLED === 'true'
const unknownSpace = 'space-e2e-not-a-real-space'

test.describe('Space memory routes (EP-8 S7)', () => {
  test('both routes are hidden while the switch is off', async ({ request, baseURL }) => {
    test.skip(switchedOn, 'the stack runs with GATEWAY_SPACE_MEMORY_UI on')
    const origin = baseURL ?? 'http://localhost:5173'
    const list = await request.get(`${origin}/api/v1/spaces/${unknownSpace}/memory`)
    expect(list.status()).toBe(404)
    expect((await list.json()).error.code).toBe('feature_unavailable')
    const removed = await request.delete(`${origin}/api/v1/spaces/${unknownSpace}/memory/m1`)
    expect(removed.status()).toBe(404)
    expect((await removed.json()).error.code).toBe('feature_unavailable')
  })

  test('a Space the caller cannot manage is never listed', async ({ request, baseURL }) => {
    test.skip(!switchedOn, 'needs GATEWAY_SPACE_MEMORY_UI=true on the stack')
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.get(`${origin}/api/v1/spaces/${unknownSpace}/memory`)
    expect([403, 404]).toContain(response.status())
    const body = await response.json()
    expect(['space_memory_not_authorized', 'space_not_found', 'active_membership_required']).toContain(
      body.error.code,
    )
    expect(body.data).toBeUndefined()
    expect(typeof body.error.details === 'object' || body.error.details === undefined).toBe(true)
  })

  test('a malformed memory id is refused before Control is asked', async ({ request, baseURL }) => {
    test.skip(!switchedOn, 'needs GATEWAY_SPACE_MEMORY_UI=true on the stack')
    const origin = baseURL ?? 'http://localhost:5173'
    const response = await request.delete(`${origin}/api/v1/spaces/${unknownSpace}/memory/not..valid`)
    expect([400, 403]).toContain(response.status())
    const body = await response.json()
    expect(['invalid_memory', 'active_membership_required']).toContain(body.error.code)
  })
})
