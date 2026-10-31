import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import {
  createSiemDestination,
  deleteSiemDestination,
  exportComplianceEvents,
  getComplianceExportStatus,
  getComplianceManifest,
  listComplianceEvents,
  listComplianceKeys,
  listSiemDestinations,
  updateSiemDestination,
  verifyComplianceChain,
} from '@/shared/api/audit-compliance-client'

type Call = { url: string; init: RequestInit | undefined }

function stubFetch(...responses: Array<Response | (() => Response)>): Call[] {
  const calls: Call[] = []
  const queue = [...responses]
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, init })
    const next = queue.shift()
    if (!next) throw new Error('unexpected extra request')
    return typeof next === 'function' ? next() : next
  }))
  return calls
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function at(calls: Call[], index: number): Call {
  const call = calls[index]
  if (!call) throw new Error(`no request #${index}`)
  return call
}

function pathOf(call: Call): string {
  return call.url.replace(/^https?:\/\/[^/]+/, '')
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('listComplianceEvents', () => {
  it('sends only the filters it was given, never an org id, and reads the page meta', async () => {
    const calls = stubFetch(json({
      data: [{ chain_seq: 4, event: 'tool_action', row_hash: 'ab' }],
      meta: { count: 1, next_cursor: 'cur-2', limit: 200, head_seq: 9 },
      error: null,
    }))

    const page = await listComplianceEvents({
      event: 'tool_action',
      userId: 'user_1',
      since: '2026-01-01T00:00:00Z',
      limit: 200,
    })

    expect(pathOf(at(calls, 0))).toBe(
      '/api/v1/audit/compliance/events?event=tool_action&user_id=user_1&since=2026-01-01T00%3A00%3A00Z&limit=200',
    )
    expect(at(calls, 0).url).not.toContain('org_id')
    expect(at(calls, 0).init?.credentials).toBe('include')
    expect(page.nextCursor).toBe('cur-2')
    expect(page.headSeq).toBe(9)
    expect(page.limit).toBe(200)
    expect(page.events).toHaveLength(1)
  })

  it('has no next cursor on the last page', async () => {
    stubFetch(json({ data: [], meta: { count: 0, next_cursor: null, limit: 200, head_seq: 0 }, error: null }))
    const page = await listComplianceEvents()
    expect(page.nextCursor).toBeNull()
    expect(page.events).toEqual([])
  })

  it('throws the typed 403 for a member', async () => {
    stubFetch(json({
      error: { code: 'forbidden', message: 'Audit compliance requires an organization owner or admin role.' },
    }, 403))

    const failure = await listComplianceEvents().catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).status).toBe(403)
    expect((failure as ApiError).code).toBe('forbidden')
  })
})

describe('exportComplianceEvents', () => {
  it('follows the cursor until the range is exhausted', async () => {
    const calls = stubFetch(
      json({ data: [{ chain_seq: 1 }], meta: { next_cursor: 'c1', limit: 1, head_seq: 2 } }),
      json({ data: [{ chain_seq: 2 }], meta: { next_cursor: null, limit: 1, head_seq: 2 } }),
    )

    const seqs: number[] = []
    for await (const page of exportComplianceEvents({ limit: 1 })) {
      seqs.push(...page.events.map((event) => event.chain_seq))
    }

    expect(seqs).toEqual([1, 2])
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/compliance/events?limit=1')
    expect(pathOf(at(calls, 1))).toBe('/api/v1/audit/compliance/events?limit=1&cursor=c1')
  })
})

describe('manifest, keys and export status', () => {
  it('reads the manifest for a range', async () => {
    const calls = stubFetch(json({ data: { org_id: 'o', first_seq: 1, last_seq: 3, count: 3 }, error: null }))
    const manifest = await getComplianceManifest({ since: '2026-01-01T00:00:00Z' })
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/compliance/manifest?since=2026-01-01T00%3A00%3A00Z')
    expect(manifest.count).toBe(3)
  })

  it('lists the public keys', async () => {
    stubFetch(json({ data: [{ key_id: 'k1', algorithm: 'ed25519', public_key: 'AAAA', status: 'active' }] }))
    await expect(listComplianceKeys()).resolves.toEqual([
      { key_id: 'k1', algorithm: 'ed25519', public_key: 'AAAA', status: 'active' },
    ])
  })

  it('reports the exporter as off while nothing is configured', async () => {
    stubFetch(json({ data: { exporter_enabled: false, destinations: [] }, error: null }))
    await expect(getComplianceExportStatus()).resolves.toEqual({ exporter_enabled: false, destinations: [] })
  })
})

describe('verifyComplianceChain', () => {
  it('posts only the fields it was given, in the wire names', async () => {
    const calls = stubFetch(json({ data: { status: 'verified', complete: true, rows_verified: 5 }, error: null }))
    const report = await verifyComplianceChain({ fromSeq: 10, maxRows: 500 })
    expect(at(calls, 0).init?.method).toBe('POST')
    expect(JSON.parse(at(calls, 0).init?.body as string)).toEqual({ from_seq: 10, max_rows: 500 })
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/compliance/verify')
    expect(report.status).toBe('verified')
  })

  it('surfaces a broken verdict with its first broken position, not an error', async () => {
    stubFetch(json({ data: { status: 'broken', first_broken_seq: 12, reason: 'row_hash_mismatch' }, error: null }))
    const report = await verifyComplianceChain()
    expect(report.status).toBe('broken')
    expect(report.first_broken_seq).toBe(12)
  })
})

describe('SIEM destinations', () => {
  it('lists destinations, which only say whether a secret is set', async () => {
    const calls = stubFetch(json({
      data: [{ id: 'd-1', kind: 'ndjson_webhook', url: 'https://siem.example.com/h', secret_set: true }],
      meta: { count: 1, max: 3 },
    }))
    const destinations = await listSiemDestinations()
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/siem-destinations')
    expect(destinations[0]?.secret_set).toBe(true)
    expect(JSON.stringify(destinations)).not.toContain('secret"')
  })

  it('creates with the wire field names and no org id', async () => {
    const calls = stubFetch(json({ data: { id: 'd-2', secret_set: true } }, 201))
    await createSiemDestination({
      kind: 'splunk_hec',
      url: 'https://splunk.example.com/services/collector',
      secret: 'token',
      eventFilter: { planes: ['control'] },
    })
    expect(at(calls, 0).init?.method).toBe('POST')
    expect(JSON.parse(at(calls, 0).init?.body as string)).toEqual({
      kind: 'splunk_hec',
      url: 'https://splunk.example.com/services/collector',
      secret: 'token',
      event_filter: { planes: ['control'] },
    })
  })

  it('updates by id with the revision it read, sending only what changes', async () => {
    const calls = stubFetch(json({ data: { id: 'd/1', enabled: false } }))
    await updateSiemDestination('d/1', { expectedRevision: 3, enabled: false })
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/siem-destinations/d%2F1')
    expect(at(calls, 0).init?.method).toBe('PATCH')
    expect(JSON.parse(at(calls, 0).init?.body as string)).toEqual({ expected_revision: 3, enabled: false })
  })

  it('surfaces a revision conflict as a typed error', async () => {
    stubFetch(json({ error: { code: 'revision_conflict', message: 'The destination changed since it was read.' } }, 409))
    const failure = await updateSiemDestination('d-1', { expectedRevision: 1, enabled: true }).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).code).toBe('revision_conflict')
  })

  it('deletes on an empty 204', async () => {
    const calls = stubFetch(new Response(null, { status: 204 }))
    await expect(deleteSiemDestination('d-1')).resolves.toBeUndefined()
    expect(at(calls, 0).init?.method).toBe('DELETE')
    expect(pathOf(at(calls, 0))).toBe('/api/v1/audit/siem-destinations/d-1')
  })
})
