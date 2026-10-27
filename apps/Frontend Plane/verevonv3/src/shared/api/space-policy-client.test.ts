import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './http'
import {
  SPACE_POLICY_ADMIN_DISABLED,
  getSpacePolicy,
  isSpacePolicyAdminDisabled,
  listSpacePolicyEvents,
  newSpacePolicyIdempotencyKey,
  probeSpacePolicyAdminAvailable,
  setSpacePolicyEntitlements,
} from './space-policy-client'

// WS03-S6: the org-admin Space policy client. The org is the path segment, a write carries exactly
// one of preset_id or entitlements plus the caller's idempotency key and the expected revision, and
// never an org, actor or role.

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } })
}

function lastCall(fetchMock: ReturnType<typeof vi.fn>): { url: string; init: RequestInit } {
  const call = fetchMock.mock.calls.at(-1) as [string, RequestInit]
  return { url: call[0], init: call[1] }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('paths', () => {
  it('GET encodes the org id as one path segment', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ data: { org_id: 'o' } }))
    vi.stubGlobal('fetch', fetchMock)
    await getSpacePolicy('org/with space?x=1')
    const { url, init } = lastCall(fetchMock)
    expect(url).toContain('/api/v1/orgs/org%2Fwith%20space%3Fx%3D1/space-policy')
    expect(url).not.toContain('org/with')
    expect(init.method ?? 'GET').toBe('GET')
    expect(init.credentials).toBe('include')
  })

  it('events encode the org id and the limit', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ data: [] }))
    vi.stubGlobal('fetch', fetchMock)
    await listSpacePolicyEvents('org/1', 5)
    expect(lastCall(fetchMock).url).toContain('/api/v1/orgs/org%2F1/space-policy/events?limit=5')
    await listSpacePolicyEvents('org_1')
    expect(lastCall(fetchMock).url).toContain('/events?limit=20')
  })

  it('PUT goes to the entitlements path of the encoded org', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ data: { changed: true } }))
    vi.stubGlobal('fetch', fetchMock)
    await setSpacePolicyEntitlements('org/1', {
      idempotencyKey: 'space-policy-aaaaaaaa',
      expectedEntitlementRevision: 3,
      change: { preset_id: 'standard' },
    })
    const { url, init } = lastCall(fetchMock)
    expect(url).toContain('/api/v1/orgs/org%2F1/space-policy/entitlements')
    expect(init.method).toBe('PUT')
  })
})

describe('PUT body', () => {
  const all = {
    thread_create: true,
    thread_read: true,
    retrieval_read: false,
    import_write: false,
    agent_action: false,
    schedule_fire: false,
    sandbox_capability: false,
    process_registry: false,
  }

  async function bodyOf(change: Parameters<typeof setSpacePolicyEntitlements>[1]['change']) {
    const fetchMock = vi.fn(async () => jsonResponse({ data: { changed: true } }))
    vi.stubGlobal('fetch', fetchMock)
    await setSpacePolicyEntitlements('org_1', {
      idempotencyKey: 'space-policy-bbbbbbbb',
      expectedEntitlementRevision: 7,
      change,
    })
    return JSON.parse(String(lastCall(fetchMock).init.body)) as Record<string, unknown>
  }

  it('sends a preset with the key and the expected revision, and no entitlements', async () => {
    const body = await bodyOf({ preset_id: 'conversation_only' })
    expect(body).toEqual({
      idempotency_key: 'space-policy-bbbbbbbb',
      expected_entitlement_revision: 7,
      preset_id: 'conversation_only',
    })
    expect(body).not.toHaveProperty('entitlements')
  })

  it('sends the eight keys with the key and the expected revision, and no preset', async () => {
    const body = await bodyOf({ entitlements: all })
    expect(body).toEqual({
      idempotency_key: 'space-policy-bbbbbbbb',
      expected_entitlement_revision: 7,
      entitlements: all,
    })
    expect(body).not.toHaveProperty('preset_id')
  })

  it('never carries an org, actor or role', async () => {
    for (const change of [{ preset_id: 'standard' }, { entitlements: all }] as const) {
      const body = await bodyOf(change)
      for (const key of ['org_id', 'org', 'actor', 'actor_id', 'role', 'subject_id', 'user_id']) {
        expect(body).not.toHaveProperty(key)
      }
    }
  })

  it('uses the caller-supplied idempotency key verbatim, so a retry reuses it', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ data: { changed: true } }))
    vi.stubGlobal('fetch', fetchMock)
    const input = { idempotencyKey: 'space-policy-retry-0001', expectedEntitlementRevision: 2, change: { preset_id: 'standard' } }
    await setSpacePolicyEntitlements('org_1', input)
    await setSpacePolicyEntitlements('org_1', input)
    const keys = fetchMock.mock.calls.map((call) => (JSON.parse(String((call as unknown as [string, RequestInit])[1].body)) as { idempotency_key: string }).idempotency_key)
    expect(keys).toEqual(['space-policy-retry-0001', 'space-policy-retry-0001'])
  })
})

describe('newSpacePolicyIdempotencyKey', () => {
  it('is 8 to 128 characters and differs between calls', () => {
    const a = newSpacePolicyIdempotencyKey()
    const b = newSpacePolicyIdempotencyKey()
    expect(a).not.toBe(b)
    for (const key of [a, b]) {
      expect(key.length).toBeGreaterThanOrEqual(8)
      expect(key.length).toBeLessThanOrEqual(128)
    }
  })
})

describe('isSpacePolicyAdminDisabled', () => {
  it('is true only for a 404 space_policy_admin_disabled ApiError', () => {
    expect(isSpacePolicyAdminDisabled(new ApiError('off', 404, SPACE_POLICY_ADMIN_DISABLED))).toBe(true)
    expect(isSpacePolicyAdminDisabled(new ApiError('off', 404, 'not_found'))).toBe(false)
    expect(isSpacePolicyAdminDisabled(new ApiError('off', 403, SPACE_POLICY_ADMIN_DISABLED))).toBe(false)
    expect(isSpacePolicyAdminDisabled(new ApiError('off', 404, null))).toBe(false)
    expect(isSpacePolicyAdminDisabled(new Error(SPACE_POLICY_ADMIN_DISABLED))).toBe(false)
    expect(isSpacePolicyAdminDisabled({ status: 404, code: SPACE_POLICY_ADMIN_DISABLED })).toBe(false)
    expect(isSpacePolicyAdminDisabled(null)).toBe(false)
  })
})

describe('probeSpacePolicyAdminAvailable', () => {
  it('is true when the policy loads', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ data: { org_id: 'org_1', state: 'provisioned' } })))
    await expect(probeSpacePolicyAdminAvailable('org_1')).resolves.toBe(true)
  })

  it.each([
    [404, { error: { code: SPACE_POLICY_ADMIN_DISABLED, message: 'off' } }],
    [403, { error: { code: 'forbidden', message: 'no' } }],
    [500, { error: { code: 'internal', message: 'boom' } }],
  ])('is false on a %d answer', async (status, body) => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(body, status)))
    await expect(probeSpacePolicyAdminAvailable('org_1')).resolves.toBe(false)
  })

  it('is false when the request itself fails, and never throws', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch') }))
    await expect(probeSpacePolicyAdminAvailable('org_1')).resolves.toBe(false)
  })
})
