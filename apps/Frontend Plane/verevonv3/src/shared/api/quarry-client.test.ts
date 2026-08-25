import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  checkChange,
  getChangeLatest,
  getTeamCreditUsage,
  listQuarrySnapshots,
  promoteChangeToSnapshot,
  QUARRY_API_BASE,
  scheduleRefreshRun,
} from './quarry-client'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

/** Wire envelope Quarry emits: { data, meta, error } with snake_case fields. */
function envelope(data: unknown) {
  return jsonResponse({ data, meta: { request_id: 'req_test' }, error: null })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('quarry-client routing', () => {
  it('targets the same-origin /api/quarry base path', async () => {
    const fetchMock = vi.fn().mockResolvedValue(envelope({ items: [] }))
    vi.stubGlobal('fetch', fetchMock)

    await listQuarrySnapshots()

    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')
    expect(calledUrl).toContain(`${QUARRY_API_BASE}/v1/snapshots`)
    // gatewayBaseUrl() defaults to '' (same-origin) in tests — no host prefix.
    expect(calledUrl.startsWith('/api/quarry/')).toBe(true)
  })
})

describe('quarry-client request shapes', () => {
  it('changeCheck POSTs { url } and decodes the snake_case wire to camelCase data', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({
        source_url: 'https://x/',
        org_id: 'org_a',
        status: 'changed',
        diff_id: 'diff_1',
        checked_at: '2026-08-25T00:00:00Z',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const record = await checkChange('https://x/')
    const [calledUrl, init] = fetchMock.mock.calls[0] as [string, RequestInit]

    expect(calledUrl).toContain('/v1/change/check')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ url: 'https://x/' })
    // Envelope unwrapped + camelCase mapping applied:
    expect(record?.sourceUrl).toBe('https://x/')
    expect(record?.status).toBe('changed')
    expect(record?.diffId).toBe('diff_1')
  })

  it('scheduleRefreshRun passes optional priority through and unwraps the ack', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({ request_id: 'chg_01ABC', accepted: true }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const ack = await scheduleRefreshRun('https://x/', 'high')

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ url: 'https://x/', priority: 'high' })
    expect(ack?.requestId).toBe('chg_01ABC')
    expect(ack?.accepted).toBe(true)
  })

  it('promoteTrackedResultToSnapshot uses the url query parameter', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({
        snapshot_id: 'snp_1',
        org_id: 'org_a',
        url: 'https://x/',
        fingerprint: 'blake3:abc',
        change_status: 'modified',
        captured_at: '2026-08-25T00:00:00Z',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const snapshot = await promoteChangeToSnapshot('https://x/')
    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')

    expect(calledUrl).toContain('/v1/change/snapshot?')
    expect(calledUrl).toContain(encodeURIComponent('https://x/'))
    expect(snapshot?.snapshotId).toBe('snp_1')
    expect(snapshot?.changeStatus).toBe('modified')
  })

  it('changeLatest GETs with a required url query param', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({
        baseline_id: 'bln_1',
        org_id: 'org_a',
        source_url: 'https://x/',
        fingerprint: 'blake3:abc',
        captured_at: '2026-08-25T00:00:00Z',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const baseline = await getChangeLatest('https://x/')
    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')

    expect(calledUrl).toContain('/v1/change/latest?url=')
    expect(baseline?.baselineId).toBe('bln_1')
  })

  it('teamCreditUsage forwards the period window', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({ org_id: 'org_a', period: '7d', credits_used: 12.5, utilization_percent: 4.2 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const usage = await getTeamCreditUsage('30d')
    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')

    expect(calledUrl).toContain('/v1/team/credit-usage')
    expect(calledUrl).toContain('period=30d')
    expect(usage?.creditsUsed).toBe(12.5)
  })
})

describe('quarry-client response decoding', () => {
  it('decodes paginated snapshot rows; omitted optionals stay undefined', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      envelope({
        items: [
          {
            snapshot_id: 'snp_1',
            org_id: 'org_a',
            url: 'https://x/',
            fingerprint: 'blake3:a',
            change_status: 'new',
            captured_at: '2026-08-25T00:00:00Z',
            // source_id / prev_fingerprint / artifact_id omitted — serde skip_serializing_if
          },
        ],
        next_cursor: null,
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const page = await listQuarrySnapshots()
    const row = page?.items?.[0]

    expect(row?.snapshotId).toBe('snp_1')
    expect(row?.sourceId).toBeUndefined()
    expect(row?.prevFingerprint).toBeUndefined()
  })
})
