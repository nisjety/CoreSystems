import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  AUDIT_MAX_LIMIT,
  aggregateToolActions,
  aggregateWorkspaceActivity,
  listAuditEvents,
  listAuditPage,
  listOwnAuditEvents,
  listToolActionEvents,
  zeroDataRetention,
  type AuditEvent,
} from '@/shared/api/audit-client'
import { ApiError } from '@/shared/api/http'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('listAuditEvents', () => {
  it('normalizes the live Audit Core snake_case row at the API boundary', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: [{
        id: 17,
        event: 'member.removed',
        occurred_at: '2026-07-14T18:00:00Z',
        user_id: 'user_admin',
        actor_role: 'admin',
        resource_id: 'user_member',
        outcome: 'ok',
        request_id: 'req_audit_17',
      }],
      meta: { count: 1 },
      error: null,
    }), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    })))

    await expect(listAuditEvents({ limit: 25 })).resolves.toEqual([{
      id: '17',
      event: 'member.removed',
      occurredAt: '2026-07-14T18:00:00Z',
      userId: 'user_admin',
      actorRole: 'admin',
      resource: 'user_member',
      outcome: 'ok',
      requestId: 'req_audit_17',
    }])
  })
})

describe('zeroDataRetention', () => {
  it('reads the flag top-level or from details, defaulting to false', () => {
    expect(zeroDataRetention({ zdr: true })).toBe(true)
    expect(zeroDataRetention({ details: { zdr: true } })).toBe(true)
    expect(zeroDataRetention({ details: { zero_data_retention: 'true' } })).toBe(true)
    expect(zeroDataRetention({ details: { tool: 'x' } })).toBe(false)
    expect(zeroDataRetention({})).toBe(false)
  })
})

describe('aggregateWorkspaceActivity', () => {
  it('rolls up tool-action events per data category with ZDR coverage', () => {
    const events: AuditEvent[] = [
      { event: 'tool_action', details: { tool: 'search', data_category: 'customer', source: 'hubspot', zdr: true } },
      { event: 'tool_action', details: { tool: 'search', data_category: 'customer', source: 'hubspot' } },
      { event: 'tool_action', details: { tool: 'lookup', data_category: 'billing', source: 'stripe' } },
    ]
    const activity = aggregateWorkspaceActivity(events)
    expect(activity.totalEvents).toBe(3)
    expect(activity.zdrEvents).toBe(1)
    expect(activity.tools).toEqual(expect.arrayContaining(['search', 'lookup']))

    const customer = activity.categories.find((c) => c.category === 'customer')
    expect(customer).toEqual({ category: 'customer', count: 2, zdrCount: 1 })
    // Most-active category sorts first.
    expect(activity.categories[0]?.category).toBe('customer')
  })

  it('files events with no declared category under "Uncategorized"', () => {
    const activity = aggregateWorkspaceActivity([{ event: 'tool_action', details: { tool: 'x' } }])
    expect(activity.categories).toEqual([{ category: 'Uncategorized', count: 1, zdrCount: 0 }])
  })
})

describe('honesty invariant: no-source events never become a per-connection claim', () => {
  it('counts a source-less event in the workspace rollup but in no per-source bucket', () => {
    const events: AuditEvent[] = [
      // No `source`/`provider`/`resource` ⇒ unattributable to any connection.
      { event: 'tool_action', details: { tool: 'web_search', data_category: 'public' } },
      { event: 'tool_action', details: { tool: 'lookup', data_category: 'customer', source: 'hubspot' } },
    ]

    // Workspace rollup includes BOTH events.
    const activity = aggregateWorkspaceActivity(events)
    expect(activity.totalEvents).toBe(2)
    expect(activity.categories.find((c) => c.category === 'public')?.count).toBe(1)

    // Per-source aggregation includes ONLY the attributable event — the
    // source-less one must never create or inflate a per-connection bucket.
    const perSource = aggregateToolActions(events)
    expect(perSource.size).toBe(1)
    expect(perSource.has('hubspot')).toBe(true)
    const totalAttributed = [...perSource.values()].reduce((sum, s) => sum + s.count, 0)
    expect(totalAttributed).toBe(1)
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('role-gated audit reads (WS05-S1)', () => {
  it('listAuditPage keeps meta.scope and ignores a scope it does not know', async () => {
    for (const scope of ['org', 'self', 'org_redacted'] as const) {
      vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
        data: [{ id: 1, event: 'tool_action' }],
        meta: { count: 1, scope },
        error: null,
      })))
      const page = await listAuditPage({ limit: 10 })
      expect(page.scope).toBe(scope)
      expect(page.events).toHaveLength(1)
    }

    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ data: [], meta: { scope: 'galaxy' } })))
    const unknown = await listAuditPage()
    expect(unknown.scope).toBeUndefined()
    expect('scope' in unknown).toBe(false)

    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ data: [] })))
    expect((await listAuditPage()).scope).toBeUndefined()
  })

  it('listAuditPage throws the typed ApiError on a 403 audit_scope_denied', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: {
        code: 'audit_scope_denied',
        message: 'You can only read your own audit events.',
        details: { scope: 'self' },
      },
    }, 403)))
    const failure = await listAuditPage({ userId: 'someone-else' }).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    const apiError = failure as ApiError
    expect(apiError.status).toBe(403)
    expect(apiError.code).toBe('audit_scope_denied')
    expect(apiError.message).toBe('You can only read your own audit events.')
  })

  it('listAuditPage sends the filters as query parameters with credentials', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => jsonResponse({ data: [] }))
    vi.stubGlobal('fetch', fetchMock)
    await listAuditPage({ event: 'tool_action', userId: 'u1', since: '2026-01-01T00:00:00Z', limit: 7 })
    const [url, init] = fetchMock.mock.calls[0]!
    const parsed = new URL(String(url), 'http://localhost')
    expect(parsed.pathname).toBe('/api/v1/audit')
    expect(parsed.searchParams.get('event')).toBe('tool_action')
    expect(parsed.searchParams.get('user_id')).toBe('u1')
    expect(parsed.searchParams.get('since')).toBe('2026-01-01T00:00:00Z')
    expect(parsed.searchParams.get('limit')).toBe('7')
    expect(parsed.searchParams.has('org_id')).toBe(false)
    expect(init?.credentials).toBe('include')
  })

  it('listOwnAuditEvents sends the caller user_id and a default limit of 25', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => jsonResponse({
      data: [{ id: 3, event: 'login', user_id: 'user-a' }],
      meta: { scope: 'self' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    const page = await listOwnAuditEvents('user-a')
    const parsed = new URL(String(fetchMock.mock.calls[0]![0]), 'http://localhost')
    expect(parsed.searchParams.get('user_id')).toBe('user-a')
    expect(parsed.searchParams.get('limit')).toBe('25')
    expect(page.scope).toBe('self')
    expect(page.events[0]?.userId).toBe('user-a')
  })

  it('listToolActionEvents defaults to the gateway page ceiling of 200', async () => {
    expect(AUDIT_MAX_LIMIT).toBe(200)
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => jsonResponse({ data: [] }))
    vi.stubGlobal('fetch', fetchMock)
    await listToolActionEvents()
    const parsed = new URL(String(fetchMock.mock.calls[0]![0]), 'http://localhost')
    expect(parsed.searchParams.get('event')).toBe('tool_action')
    expect(parsed.searchParams.get('limit')).toBe('200')
    expect(parsed.searchParams.has('user_id')).toBe(false)
  })

  it('a redacted org-wide row (no user, subject or resource) still aggregates into the workspace rollup', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      data: [
        { id: 1, occurred_at: '2026-10-01T00:00:00Z', plane: 'model', event: 'tool_action', outcome: 'ok', details: { tool: 'web_search', data_category: 'public', zdr: true, phase: 'complete' } },
        { id: 2, occurred_at: '2026-10-01T00:01:00Z', plane: 'model', event: 'tool_action', outcome: 'ok', details: { tool: 'lookup', data_category: 'customer', zdr: false } },
      ],
      meta: { count: 2, scope: 'org_redacted' },
    })))
    const page = await listAuditPage({ event: 'tool_action', limit: 200 })
    expect(page.scope).toBe('org_redacted')
    for (const event of page.events) {
      expect(event.userId).toBeUndefined()
      expect(event.resource).toBeUndefined()
    }
    const activity = aggregateWorkspaceActivity(page.events)
    expect(activity.totalEvents).toBe(2)
    expect(activity.zdrEvents).toBe(1)
    expect(activity.categories).toEqual(expect.arrayContaining([
      { category: 'public', count: 1, zdrCount: 1 },
      { category: 'customer', count: 1, zdrCount: 0 },
    ]))
    expect(activity.tools).toEqual(expect.arrayContaining(['web_search', 'lookup']))
    // No source on a redacted row, so no per-connection bucket is invented.
    expect(aggregateToolActions(page.events).size).toBe(0)
  })
})
