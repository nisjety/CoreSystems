import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import { listDirectoryRooms, searchDirectory, searchDirectoryPeople } from './directory'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url: String(url), init })
    return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
  }))
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const paramsOf = (call: FetchCall | undefined) => new URL(call?.url ?? '', 'http://localhost').searchParams

describe('searchDirectoryPeople (WS04-S3)', () => {
  it('GETs /api/v1/directory/people and sends only a trimmed q', async () => {
    const calls = stubFetch(200, { data: { people: [], next_cursor: null } })
    await searchDirectoryPeople('  kari  ')
    expect(calls).toHaveLength(1)
    expect(new URL(calls[0]?.url ?? '', 'http://localhost').pathname).toBe('/api/v1/directory/people')
    expect((calls[0]?.init?.method ?? 'GET').toUpperCase()).toBe('GET')
    expect([...paramsOf(calls[0]).keys()]).toEqual(['q'])
    expect(paramsOf(calls[0]).get('q')).toBe('kari')
  })

  it('omits q when it is empty or only whitespace, and sends no query string at all with no options', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    await searchDirectoryPeople('')
    await searchDirectoryPeople('   ')
    for (const call of calls) {
      expect(call.url).not.toContain('?')
      expect(call.url.endsWith('/api/v1/directory/people')).toBe(true)
    }
  })

  it('sends limit, cursor and exclude_member_of when given', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    await searchDirectoryPeople('ola', { limit: 20, cursor: 'opaque-cursor', excludeMemberOf: 'room_1' })
    const params = paramsOf(calls[0])
    expect([...params.keys()].sort()).toEqual(['cursor', 'exclude_member_of', 'limit', 'q'])
    expect(params.get('limit')).toBe('20')
    expect(params.get('cursor')).toBe('opaque-cursor')
    expect(params.get('exclude_member_of')).toBe('room_1')
  })

  it('leaves an empty or null cursor and an empty exclude room off the wire', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    await searchDirectoryPeople('', { cursor: null, excludeMemberOf: '' })
    await searchDirectoryPeople('', { cursor: '' })
    for (const call of calls) expect(call.url).not.toContain('?')
  })

  it('never sends an org or user id: those come from the session', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    await searchDirectoryPeople('x', { limit: 5, cursor: 'c', excludeMemberOf: 'room_1' })
    const keys = [...paramsOf(calls[0]).keys()]
    for (const forbidden of ['org_id', 'orgId', 'organization_id', 'user_id', 'userId', 'subject_id', 'tenant_id']) {
      expect(keys, forbidden).not.toContain(forbidden)
    }
  })

  it('percent-encodes a query that carries reserved characters', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    await searchDirectoryPeople('a&b=c d')
    expect(paramsOf(calls[0]).get('q')).toBe('a&b=c d')
    expect(calls[0]?.url).not.toContain('a&b')
  })

  it('maps subject_id, display_name, email and is_self', async () => {
    stubFetch(200, {
      data: {
        people: [
          { subject_id: 'user_2', display_name: 'Kari Nordmann', email: 'kari@example.test', is_self: false },
          { subject_id: 'user_1', display_name: 'Ola Nordmann', email: 'ola@example.test', is_self: true },
        ],
        next_cursor: 'next-1',
      },
    })
    const page = await searchDirectoryPeople('')
    expect(page.people).toEqual([
      { subjectId: 'user_2', displayName: 'Kari Nordmann', email: 'kari@example.test', isSelf: false },
      { subjectId: 'user_1', displayName: 'Ola Nordmann', email: 'ola@example.test', isSelf: true },
    ])
    expect(page.nextCursor).toBe('next-1')
  })

  it('drops rows without a subject_id, and defaults the other fields', async () => {
    stubFetch(200, {
      data: {
        people: [
          { display_name: 'Ingen id', email: 'x@example.test' },
          { subject_id: '', display_name: 'Tom id' },
          { subject_id: 42, display_name: 'Tall' },
          { subject_id: 'user_3' },
          { subject_id: 'user_4', display_name: 7, email: null, is_self: 'true' },
        ],
      },
    })
    const page = await searchDirectoryPeople('')
    expect(page.people).toEqual([
      { subjectId: 'user_3', displayName: '', email: '', isSelf: false },
      { subjectId: 'user_4', displayName: '', email: '', isSelf: false },
    ])
  })

  it('answers next_cursor null when it is empty, null or absent', async () => {
    stubFetch(200, { data: { people: [], next_cursor: '' } })
    expect((await searchDirectoryPeople('')).nextCursor).toBeNull()
    stubFetch(200, { data: { people: [], next_cursor: null } })
    expect((await searchDirectoryPeople('')).nextCursor).toBeNull()
    stubFetch(200, { data: { people: [] } })
    expect((await searchDirectoryPeople('')).nextCursor).toBeNull()
  })

  it('answers an empty list for a body with no people', async () => {
    stubFetch(200, { data: {} })
    expect(await searchDirectoryPeople('')).toEqual({ people: [], nextCursor: null })
  })

  it('hands the abort signal to the request', async () => {
    const calls = stubFetch(200, { data: { people: [] } })
    const controller = new AbortController()
    await searchDirectoryPeople('x', { signal: controller.signal })
    expect(calls[0]?.init?.signal).toBe(controller.signal)
  })

  it('surfaces a typed refusal as an ApiError with its code', async () => {
    for (const [status, code] of [
      [404, 'feature_disabled'],
      [403, 'active_membership_required'],
      [429, 'rate_limited'],
      [503, 'directory_unavailable'],
    ] as const) {
      stubFetch(status, { error: { code, message: code } })
      const error = await searchDirectoryPeople('x').catch((caught: unknown) => caught)
      expect(error, code).toBeInstanceOf(ApiError)
      expect(error, code).toMatchObject({ status, code })
    }
  })
})

describe('listDirectoryRooms (WS04-S8)', () => {
  it('GETs /api/v1/directory/rooms with only a trimmed q and the cursor', async () => {
    const calls = stubFetch(200, { data: { enabled: true, rooms: [], next_cursor: null } })
    await listDirectoryRooms('  lager  ', { cursor: 'c2' })
    expect(calls).toHaveLength(1)
    const url = new URL(calls[0]?.url ?? '', 'http://localhost')
    expect(url.pathname).toBe('/api/v1/directory/rooms')
    expect([...url.searchParams.keys()].sort()).toEqual(['cursor', 'q'])
    expect(url.searchParams.get('q')).toBe('lager')
    expect(url.searchParams.get('cursor')).toBe('c2')
    await listDirectoryRooms('   ')
    expect(calls[1]?.url.endsWith('/api/v1/directory/rooms')).toBe(true)
  })

  it('maps rooms and drops rows without a ref or a name', async () => {
    stubFetch(200, {
      data: {
        enabled: true,
        rooms: [
          { space_ref: 'room_1', name: 'Lager', topic: 'Innkjøp', member_count: 4, join_role: 'editor' },
          { space_ref: 'room_2', name: 'Ops', topic: '  ', member_count: 'many', join_role: 'manager' },
          { name: 'Ingen ref' },
          { space_ref: '', name: 'Tom ref' },
          { space_ref: 'room_3', name: '   ' },
          { space_ref: 'room_4', name: 7 },
        ],
        next_cursor: 'next-1',
      },
    })
    const page = await listDirectoryRooms('')
    expect(page).toEqual({
      enabled: true,
      rooms: [
        { spaceRef: 'room_1', name: 'Lager', topic: 'Innkjøp', memberCount: 4, joinRole: 'editor' },
        { spaceRef: 'room_2', name: 'Ops', topic: null, memberCount: 0, joinRole: 'viewer' },
      ],
      nextCursor: 'next-1',
    })
  })

  it('passes enabled:false through as its own answer, not as an empty directory', async () => {
    stubFetch(200, { data: { enabled: false, rooms: [] } })
    expect(await listDirectoryRooms('')).toEqual({ enabled: false, rooms: [], nextCursor: null })
  })

  it('carries feature_disabled and directory_unavailable as ApiError codes', async () => {
    for (const [status, code] of [
      [404, 'feature_disabled'],
      [503, 'directory_unavailable'],
    ] as const) {
      stubFetch(status, { error: { code, message: code } })
      const error = await listDirectoryRooms('x').catch((caught: unknown) => caught)
      expect(error, code).toBeInstanceOf(ApiError)
      expect(error, code).toMatchObject({ status, code })
    }
  })
})

describe('searchDirectory (WS04-S9)', () => {
  it('sends kinds person and agent by default, with the trimmed q, limit, cursor and exclude_member_of', async () => {
    const calls = stubFetch(200, { data: { results: [] } })
    await searchDirectory('  ola  ', { limit: 10, cursor: 'c2', excludeMemberOf: 'room_1' })
    const params = paramsOf(calls[0])
    expect([...params.keys()].sort()).toEqual(['cursor', 'exclude_member_of', 'kinds', 'limit', 'q'])
    expect(params.get('kinds')).toBe('person,agent')
    expect(params.get('q')).toBe('ola')
    expect(params.get('limit')).toBe('10')
    expect(params.get('cursor')).toBe('c2')
    expect(params.get('exclude_member_of')).toBe('room_1')
  })

  it('sends only the kinds asked for', async () => {
    const calls = stubFetch(200, { data: { results: [] } })
    await searchDirectory('', { kinds: ['agent'] })
    expect(paramsOf(calls[0]).get('kinds')).toBe('agent')
    expect(paramsOf(calls[0]).has('q')).toBe(false)
  })

  it('maps person and agent rows and drops rows without a subject_id or an agent_ref', async () => {
    stubFetch(200, {
      data: {
        results: [
          { kind: 'person', subject_id: 'user_2', display_name: 'Kari Nordmann', email: 'kari@example.test', is_self: false },
          {
            kind: 'agent',
            agent_ref: 'agent_ref_1',
            display_name: 'Lagerhjelpen',
            description: 'Hjelper',
            installed_in: ['room_1', '', 7],
          },
          { kind: 'person', display_name: 'Ingen id' },
          { kind: 'agent', display_name: 'Ingen ref' },
          { kind: 'agent', agent_ref: '' },
          { kind: 'person', subject_id: '' },
        ],
        next_cursor: 'n1',
      },
    })
    const page = await searchDirectory('')
    expect(page.results).toEqual([
      { kind: 'person', subjectId: 'user_2', displayName: 'Kari Nordmann', email: 'kari@example.test', isSelf: false },
      {
        kind: 'agent',
        agentRef: 'agent_ref_1',
        displayName: 'Lagerhjelpen',
        description: 'Hjelper',
        installedIn: ['room_1'],
      },
    ])
    expect(page.nextCursor).toBe('n1')
    expect(page.presenceOnly).toBe(true)
  })

  it('surfaces feature_disabled as an ApiError with its code', async () => {
    stubFetch(404, { error: { code: 'feature_disabled', message: 'off' } })
    const error = await searchDirectory('x').catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 404, code: 'feature_disabled' })
  })
})
