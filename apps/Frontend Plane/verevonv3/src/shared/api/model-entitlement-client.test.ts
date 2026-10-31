import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  explainModelEntitlement,
  getModelEntitlement,
  putModelEntitlement,
} from './model-entitlement-client'
import { ApiError } from './http'

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('model entitlement client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('reads the switch-off answer as enabled false', async () => {
    const fetchMock = vi.fn(async () => json({ enabled: false }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(getModelEntitlement('org_1')).resolves.toEqual({ enabled: false })

    const [path] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/model-entitlement')
  })

  it('replaces the whole set with the revision it was edited from', async () => {
    const fetchMock = vi.fn(async () => json({ organization_id: 'org_1', revision: 4 }))
    vi.stubGlobal('fetch', fetchMock)

    await putModelEntitlement('org_1', {
      expected_revision: 3,
      roles: { member: { allow_mode: 'listed', allow: ['gpt-5'] } },
    })

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/model-entitlement')
    expect(init.method).toBe('PUT')
    expect(JSON.parse(String(init.body))).toEqual({
      expected_revision: 3,
      roles: { member: { allow_mode: 'listed', allow: ['gpt-5'] } },
    })
  })

  it('surfaces a stale revision and a ZDR refusal as typed errors', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        json({ error: 'the model policy changed since it was read', code: 'revision_conflict' }, 409),
      ),
    )
    await expect(putModelEntitlement('org_1', { expected_revision: 1 })).rejects.toMatchObject({
      status: 409,
      code: 'revision_conflict',
    })

    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        json({ error: { code: 'model_policy_leaves_no_zdr_model', message: 'no attested model' } }, 409),
      ),
    )
    const refusal = await putModelEntitlement('org_1', { expected_revision: 1 }).catch(
      (error: unknown) => error,
    )
    expect(refusal).toBeInstanceOf(ApiError)
    expect((refusal as ApiError).code).toBe('model_policy_leaves_no_zdr_model')
  })

  it('asks for another member only when one is named', async () => {
    const fetchMock = vi.fn(async () => json({ state: 'none' }))
    vi.stubGlobal('fetch', fetchMock)

    await explainModelEntitlement('org_1')
    await explainModelEntitlement('org_1', 'user 2')

    const paths = fetchMock.mock.calls.map((call) => (call as unknown as [string])[0])
    expect(paths).toEqual([
      '/api/v1/orgs/org_1/model-entitlement/explain',
      '/api/v1/orgs/org_1/model-entitlement/explain?user_id=user%202',
    ])
  })
})
