import { afterEach, describe, expect, it, vi } from 'vitest'
import { SESSION_EXPIRED_EVENT } from '@/shared/api/http'
import { anbudAuditBody, anbudMatchesBody, anbudRetendersBody, anbudValidationErrorBody } from '@/test/anbud-fixtures'
import { fetchRetenders, fetchTenderAudit, searchTenderCatalogue, searchTenderMatches } from './anbud-client'

vi.mock('./anbud-matching-context', () => ({ loadTenderMatchingContext: async () => ({ companyKeywords: [], userKeywords: [] }) }))

function answer(status: number, body: string): Response {
  return new Response(body, { status, headers: { 'Content-Type': 'application/json' } })
}

describe('anbud API client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the profile in the core\'s own field names, through the gateway, with no organisation header', async () => {
    const fetchMock = vi.fn().mockResolvedValue(answer(200, anbudMatchesBody))
    vi.stubGlobal('fetch', fetchMock)

    await searchTenderMatches({
      profile: {
        orgNumber: '974760673',
        cpv: ['72212000'],
        nuts: ['NO08'],
        keywords: [],
        valueRange: { currency: 'NOK', min: 200000 },
      },
      strictness: 'balanced',
    })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/anbud/matches')
    expect(init.method).toBe('POST')
    expect(init.credentials).toBe('include')
    expect(JSON.parse(String(init.body))).toEqual({
      profile: {
        org_number: '974760673',
        cpv: ['72212000'],
        nuts: ['NO08'],
        keywords: [],
        value_range: { currency: 'NOK', min: 200000 },
      },
      strictness: 'balanced',
      show: 'relevant',
    })
    expect((init.headers as Headers).get('x-verevon-org-id')).toBeNull()
  })

  it('reads the matches into camelCase, with the gate and the fit of each tender', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(answer(200, anbudMatchesBody)))

    const matches = await searchTenderMatches({
      profile: { cpv: ['72212000'], nuts: ['NO08'], keywords: [] },
      strictness: 'balanced',
    })

    expect(matches.funnel.matched).toBe(1)
    expect(matches.gainedByWidening).toEqual({ wide: 0 })
    expect(matches.results[0]?.noticeId).toBe('2026-100001')
    expect(matches.results[0]?.deadline?.daysLeft).toBe(20)
    expect(matches.results[0]?.gate[0]?.cite.lot).toBe('LOT-0001')
    expect(matches.results[0]?.fit.map((check) => check.rule)).toEqual(['cpv', 'region', 'value', 'keywords'])
  })

  it('asks for one notice by its id, and reads its audit sheet', async () => {
    const fetchMock = vi.fn().mockResolvedValue(answer(200, anbudAuditBody))
    vi.stubGlobal('fetch', fetchMock)

    const audit = await fetchTenderAudit('2026-100001', {
      profile: { cpv: ['72212000'], nuts: [], keywords: [] },
      strictness: 'wide',
    })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/anbud/matches/2026-100001')
    expect(JSON.parse(String(init.body))).toEqual({
      profile: { cpv: ['72212000'], nuts: [], keywords: [] },
      strictness: 'wide',
    })
    expect(audit.evidence.fetchedAt).toBe('2026-11-09T08:00:00Z')
    expect(audit.parse.sdkVersion).toBe('1.14')
    expect(audit.incumbencyStatus?.winners).toBe('not_configured')
  })

  it('asks the radar for twelve months unless told otherwise, and reads its widening steps', async () => {
    const fetchMock = vi.fn().mockResolvedValue(answer(200, anbudRetendersBody))
    vi.stubGlobal('fetch', fetchMock)

    const radar = await fetchRetenders({ profile: { cpv: ['72000000'], nuts: [], keywords: [] }, strictness: 'balanced' })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/anbud/retenders')
    expect(JSON.parse(String(init.body)).horizon_months).toBe(12)
    expect(radar.matching).toBe(0)
    expect(radar.widening[0]).toEqual({
      to: 'wide',
      adds: 0,
      dials: ['cpv_division', 'nuts1_region'],
    })
    expect(radar.results).toEqual([])
  })

  it('raises a refusal of the core as a typed error with its code, and leaves the session alone', async () => {
    const refusal = '{"error":{"code":"rate_limited","message":"too many scans are running; try again shortly"}}'
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(answer(429, refusal)))
    const signedOut = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, signedOut)

    try {
      await expect(
        searchTenderMatches({ profile: { cpv: ['72212000'], nuts: [], keywords: [] }, strictness: 'balanced' }),
      ).rejects.toMatchObject({ status: 429, code: 'rate_limited' })
      expect(signedOut).not.toHaveBeenCalled()
    } finally {
      window.removeEventListener(SESSION_EXPIRED_EVENT, signedOut)
    }
  })

  it('raises a validation refusal with its code', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(answer(400, anbudValidationErrorBody)))

    await expect(
      searchTenderMatches({ profile: { cpv: ['72212000'], nuts: [], keywords: [] }, strictness: 'balanced' }),
    ).rejects.toMatchObject({ status: 400, code: 'validation_failed' })
  })
  it('browses the catalogue without sending a profile and preserves cursor metadata', async () => {
    const fetchMock = vi.fn().mockResolvedValue(answer(200, JSON.stringify({ data: { results: [], next_cursor: '42', total_stored: 125, scanned: 50 } })))
    vi.stubGlobal('fetch', fetchMock)
    const result = await searchTenderCatalogue({ cursor: '100', query: 'drift' })
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/anbud/notices/search')
    expect(JSON.parse(fetchMock.mock.calls[0]?.[1].body)).toEqual({ cursor: '100', query: 'drift' })
    expect(result).toEqual({ results: [], nextCursor: '42', totalStored: 125, scanned: 50 })
  })

})
