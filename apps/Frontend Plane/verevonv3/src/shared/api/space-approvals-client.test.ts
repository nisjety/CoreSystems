import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  decideSpaceApproval,
  listSpaceStandingApprovals,
  newApprovalIdempotencyKey,
  offerableScopes,
  readSpaceApprovalPolicy,
  revokeSpaceStandingApproval,
  type SpaceApprovalPolicy,
} from './space-approvals-client'

function ok(data: unknown = {}, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function failure(status: number, code: string): Response {
  return new Response(JSON.stringify({ error: { code, message: code } }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function policy(maxGrantScope: SpaceApprovalPolicy['maxGrantScope']): SpaceApprovalPolicy {
  return { maxGrantScope, defaultAddresseeKind: '', quarantineMode: '', revision: 1 }
}

function sentOf(fetchMock: ReturnType<typeof vi.fn>, index = 0): { path: string; init: RequestInit } {
  const [path, init] = fetchMock.mock.calls[index] as unknown as [string, RequestInit]
  return { path, init }
}

afterEach(() => vi.unstubAllGlobals())

describe('offerableScopes', () => {
  const mcp = 'mcp__github__create_issue'

  it('offers once only without a policy, whatever the role and tool', () => {
    expect(offerableScopes({ policy: undefined, viewerRole: 'owner', toolKey: mcp })).toEqual(['once'])
  })

  it('offers once only to a non-manager', () => {
    for (const viewerRole of ['member', 'viewer', undefined, '']) {
      expect(offerableScopes({ policy: policy('always'), viewerRole, toolKey: mcp })).toEqual(['once'])
    }
  })

  it('offers once only for a tool that is not an MCP tool', () => {
    for (const toolKey of ['web_search', 'mcpx__a__b', '', undefined]) {
      expect(offerableScopes({ policy: policy('always'), viewerRole: 'manager', toolKey })).toEqual(['once'])
    }
  })

  it('offers once only when the Space ceiling is below always', () => {
    for (const ceiling of ['once', 'thread'] as const) {
      expect(offerableScopes({ policy: policy(ceiling), viewerRole: 'owner', toolKey: mcp })).toEqual(['once'])
    }
  })

  it('offers always for a manager or owner, a ceiling of always and an MCP tool', () => {
    for (const viewerRole of ['manager', 'owner']) {
      expect(offerableScopes({ policy: policy('always'), viewerRole, toolKey: mcp })).toEqual(['once', 'always'])
    }
  })

  it('never offers thread: it has no store yet', () => {
    const offered = offerableScopes({ policy: policy('always'), viewerRole: 'owner', toolKey: mcp })
    expect(offered).not.toContain('thread')
  })
})

describe('newApprovalIdempotencyKey', () => {
  it('is a fresh key of an allowed length for each decision', () => {
    const a = newApprovalIdempotencyKey()
    const b = newApprovalIdempotencyKey()
    expect(a).not.toBe(b)
    expect(a).toMatch(/^appr-[0-9a-f]{24}$/)
    expect(a.length).toBeGreaterThanOrEqual(8)
    expect(a.length).toBeLessThanOrEqual(200)
  })
})

describe('decideSpaceApproval', () => {
  it('omits the scope for once and sends no idempotency key', async () => {
    const fetchMock = vi.fn(async () => ok({ approval: { id: 'ap-1', status: 'GRANTED' }, grant: { status: 'none' } }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await decideSpaceApproval('space 1', 'ap/1', { decision: 'approve', scope: 'once' })

    const { path, init } = sentOf(fetchMock)
    expect(path).toBe('/api/v1/spaces/space%201/approvals/ap%2F1/decide')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ decision: 'approve' })
    expect(result.grant).toEqual({ status: 'none' })
    expect(result.approval?.id).toBe('ap-1')
  })

  it('sends the scope, the reason and a generated idempotency key for always', async () => {
    const fetchMock = vi.fn(async () => ok({ approval: { id: 'ap-1' }, grant: { status: 'none' } }))
    vi.stubGlobal('fetch', fetchMock)

    await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'always', reason: 'ok' })

    const body = JSON.parse(String(sentOf(fetchMock).init.body)) as Record<string, unknown>
    expect(body).toMatchObject({ decision: 'approve', scope: 'always', reason: 'ok' })
    expect(String(body.idempotency_key)).toMatch(/^appr-/)
  })

  it('reuses the caller key so a retry of the same decision dedupes', async () => {
    const fetchMock = vi.fn(async () => ok({ approval: { id: 'ap-1' }, grant: { status: 'none' } }))
    vi.stubGlobal('fetch', fetchMock)

    await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'always', idempotencyKey: 'key-aaaaaaaa' })
    await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'always', idempotencyKey: 'key-aaaaaaaa' })

    const first = JSON.parse(String(sentOf(fetchMock, 0).init.body)) as Record<string, unknown>
    const second = JSON.parse(String(sentOf(fetchMock, 1).init.body)) as Record<string, unknown>
    expect(first.idempotency_key).toBe('key-aaaaaaaa')
    expect(second.idempotency_key).toBe('key-aaaaaaaa')
  })

  it('never sends a scope for a reject and no key unless the caller gave one', async () => {
    const fetchMock = vi.fn(async () => ok({ approval: { id: 'ap-1', status: 'DENIED' }, grant: { status: 'none' } }))
    vi.stubGlobal('fetch', fetchMock)

    await decideSpaceApproval('space-1', 'ap-1', { decision: 'reject' })

    expect(JSON.parse(String(sentOf(fetchMock).init.body))).toEqual({ decision: 'reject' })
  })

  it('maps a recorded grant with its rule', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        ok({
          approval: { id: 'ap-1', status: 'GRANTED' },
          grant: {
            status: 'recorded',
            rule: {
              rule_id: 'rule-1',
              agent_subject_id: 'agent-1',
              tool_key: 'mcp__a__b',
              action_key: 'mcp__a__b',
              granted_by_role: 'manager',
              use_count: 2,
              revoked_at: null,
            },
          },
        }),
      ),
    )

    const result = await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'always' })

    expect(result.grant.status).toBe('recorded')
    if (result.grant.status !== 'recorded') throw new Error('unreachable')
    expect(result.grant.rule).toMatchObject({
      ruleId: 'rule-1',
      agentSubjectId: 'agent-1',
      toolKey: 'mcp__a__b',
      grantedByRole: 'manager',
      useCount: 2,
      revokedAt: undefined,
    })
  })

  it('maps a not_recorded grant with its reason, and an unknown or absent grant to none', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        ok({ approval: { id: 'ap-1' }, grant: { status: 'not_recorded', reason: 'grant_scope_exceeds_ceiling' } }),
      ),
    )
    const notRecorded = await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'always' })
    expect(notRecorded.grant).toEqual({ status: 'not_recorded', reason: 'grant_scope_exceeds_ceiling' })

    vi.stubGlobal('fetch', vi.fn(async () => ok({ approval: { id: 'ap-1' }, grant: { status: 'something_new' } })))
    expect((await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve' })).grant).toEqual({ status: 'none' })

    vi.stubGlobal('fetch', vi.fn(async () => ok({ approval: { id: 'ap-1' } })))
    expect((await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve' })).grant).toEqual({ status: 'none' })

    // No approval in the answer is a null approval, not a crash.
    vi.stubGlobal('fetch', vi.fn(async () => ok({ grant: { status: 'none' } })))
    expect((await decideSpaceApproval('space-1', 'ap-1', { decision: 'approve' })).approval).toBeNull()
  })

  it('throws the typed ApiError for a refusal so the caller can map the status', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => failure(409, 'approval_already_decided')))
    await expect(decideSpaceApproval('space-1', 'ap-1', { decision: 'approve' })).rejects.toMatchObject({
      status: 409,
      code: 'approval_already_decided',
    })
    vi.stubGlobal('fetch', vi.fn(async () => failure(403, 'approval_scope_not_permitted')))
    await expect(
      decideSpaceApproval('space-1', 'ap-1', { decision: 'approve', scope: 'thread' }),
    ).rejects.toMatchObject({ status: 403, code: 'approval_scope_not_permitted' })
  })
})

describe('readSpaceApprovalPolicy', () => {
  it('reads the ceiling and its revision', async () => {
    const fetchMock = vi.fn(async () =>
      ok({ policy: { max_grant_scope: 'always', default_addressee_kind: 'run_owner', quarantine_mode: 'hold', revision: 3 } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const read = await readSpaceApprovalPolicy('space-1')

    expect(sentOf(fetchMock).path).toBe('/api/v1/spaces/space-1/approval-policy')
    expect(read).toEqual({
      maxGrantScope: 'always',
      defaultAddresseeKind: 'run_owner',
      quarantineMode: 'hold',
      revision: 3,
    })
  })

  it('treats an unknown ceiling as once and a missing policy as undefined', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ok({ policy: { max_grant_scope: 'forever' } })))
    expect((await readSpaceApprovalPolicy('space-1'))?.maxGrantScope).toBe('once')
    vi.stubGlobal('fetch', vi.fn(async () => ok({ policy: null })))
    expect(await readSpaceApprovalPolicy('space-1')).toBeUndefined()
  })

  it('returns undefined on any error, so the picker offers once only', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => failure(404, 'feature_disabled')))
    expect(await readSpaceApprovalPolicy('space-1')).toBeUndefined()
    vi.stubGlobal('fetch', vi.fn(async () => failure(503, 'approval_authority_unavailable')))
    expect(await readSpaceApprovalPolicy('space-1')).toBeUndefined()
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('network down'))))
    expect(await readSpaceApprovalPolicy('space-1')).toBeUndefined()
  })
})

describe('standing approvals', () => {
  it('lists live rules only and drops anything that is not a rule', async () => {
    const fetchMock = vi.fn(async () =>
      ok({
        grants: [
          { rule_id: 'rule-1', tool_key: 'mcp__a__b' },
          { rule_id: 'rule-2', tool_key: 'mcp__a__c', revoked_at: '2026-11-01T10:00:00Z' },
          { not_a_rule: true },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const rules = await listSpaceStandingApprovals('space-1')

    expect(sentOf(fetchMock).path).toBe('/api/v1/spaces/space-1/approval-grants')
    expect(rules.map((rule) => rule.ruleId)).toEqual(['rule-1'])
    expect(rules[0]).toMatchObject({ scope: 'always', useCount: 0, revokedAt: undefined })
  })

  it('throws the typed refusal for a non-manager', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => failure(403, 'role_forbidden')))
    await expect(listSpaceStandingApprovals('space-1')).rejects.toMatchObject({ status: 403 })
  })

  it('revokes with a DELETE on the encoded grant id', async () => {
    const fetchMock = vi.fn(async () => ok({ grant: { id: 'g/1', status: 'revoked' } }))
    vi.stubGlobal('fetch', fetchMock)

    await revokeSpaceStandingApproval('space-1', 'g/1')

    const { path, init } = sentOf(fetchMock)
    expect(path).toBe('/api/v1/spaces/space-1/approval-grants/g%2F1')
    expect(init.method).toBe('DELETE')
  })
})
