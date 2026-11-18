import { afterEach, describe, expect, it, vi } from 'vitest'

import { getOwnSpaceStatus, setOwnSpaceStatus, spaceStatusFrom } from './status'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init?.body)) as Record<string, unknown>

describe('spaceStatusFrom (WS11-S10)', () => {
  it('reads a well-formed status', () => {
    expect(spaceStatusFrom({ state: 'dnd', text: 'In a meeting', expires_at: 1760000000000 })).toEqual({
      state: 'dnd',
      text: 'In a meeting',
      expiresAt: 1760000000000,
    })
  })

  it('reads null, a non-object and an unknown state as no status', () => {
    for (const raw of [null, undefined, 'dnd', 4, ['dnd'], {}, { state: 'busy' }, { state: null, text: 'x' }]) {
      expect(spaceStatusFrom(raw)).toBeNull()
    }
  })

  it('reads a blank text and a non-number expires_at as null', () => {
    expect(spaceStatusFrom({ state: 'away', text: '   ', expires_at: 1760000000000 })).toEqual({
      state: 'away',
      text: null,
      expiresAt: 1760000000000,
    })
    expect(spaceStatusFrom({ state: 'away', text: 42, expires_at: '1760000000000' })).toEqual({
      state: 'away',
      text: null,
      expiresAt: null,
    })
    expect(spaceStatusFrom({ state: 'away', text: 'Out', expires_at: Number.POSITIVE_INFINITY })).toEqual({
      state: 'away',
      text: 'Out',
      expiresAt: null,
    })
  })
})

describe('setOwnSpaceStatus (WS11-S10)', () => {
  it('PUTs the state alone when no text or expiry is given', async () => {
    const calls = stubFetch(200, { data: { state: 'away', text: null, expires_at: null } })
    const saved = await setOwnSpaceStatus({ state: 'away' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/status')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PUT')
    expect(new Headers(calls[0]?.init?.headers).get('Content-Type')).toBe('application/json')
    expect(sentBody(calls[0])).toEqual({ state: 'away' })
    expect(saved).toEqual({ state: 'away', text: null, expiresAt: null })
  })

  it('trims the text and includes the expiry', async () => {
    const calls = stubFetch(200, { data: { state: 'dnd', text: 'Lunch', expires_at: 1760000000000 } })
    await setOwnSpaceStatus({ state: 'dnd', text: '  Lunch  ', expiresAt: 1760000000000 })
    expect(sentBody(calls[0])).toEqual({ state: 'dnd', text: 'Lunch', expires_at: 1760000000000 })
  })

  it('omits blank text', async () => {
    const calls = stubFetch(200, { data: { state: 'available', text: null, expires_at: null } })
    await setOwnSpaceStatus({ state: 'available', text: '   ' })
    expect(sentBody(calls[0])).toEqual({ state: 'available' })
  })
})

describe('getOwnSpaceStatus (WS11-S10)', () => {
  it('GETs the own status and reads a status with no state as null', async () => {
    const calls = stubFetch(200, { data: { state: null, text: null, expires_at: null } })
    expect(await getOwnSpaceStatus()).toBeNull()
    expect(calls[0]?.url.endsWith('/api/v1/spaces/status')).toBe(true)
    expect((calls[0]?.init?.method ?? 'GET').toUpperCase()).toBe('GET')

    stubFetch(200, { data: { state: 'offline', text: null, expires_at: 1760000000000 } })
    expect(await getOwnSpaceStatus()).toEqual({ state: 'offline', text: null, expiresAt: 1760000000000 })
  })
})
