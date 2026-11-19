import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import {
  getSpaceMemberHistoryWindow,
  isMemberHistoryMode,
  memberHistoryWindowFrom,
  ownHistoryWindowFrom,
  setSpaceMemberHistoryWindow,
} from './member-history-window'

// D18: the per-member history window calls of the spaces client against the gateway routes
// `GET` and `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`.

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

const memberWindow = (over: Record<string, unknown> = {}) => ({
  source: 'member',
  mode: 'since_join',
  since_at: null,
  revision: 3,
  ...over,
})

describe('getSpaceMemberHistoryWindow (D18)', () => {
  it('GETs the member window path with both ids encoded, and never from a cache', async () => {
    const calls = stubFetch(200, {
      data: { space_ref: 'room 1', subject_id: 'user/2', history_window: memberWindow() },
    })
    await getSpaceMemberHistoryWindow('room 1', 'user/2')
    expect(calls).toHaveLength(1)
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/members/user%2F2/history-window')).toBe(true)
    expect(calls[0]?.init?.method).toBe('GET')
    expect(calls[0]?.init?.cache).toBe('no-store')
    expect(calls[0]?.init?.body).toBeUndefined()
  })

  it('reads the four fields of the window, and a member who follows the room', async () => {
    stubFetch(200, {
      data: {
        space_ref: 'room',
        subject_id: 'user-2',
        history_window: memberWindow({ source: 'member', mode: 'since_date', since_at: '2026-11-01T08:00:00Z', revision: 5 }),
      },
    })
    await expect(getSpaceMemberHistoryWindow('room', 'user-2')).resolves.toEqual({
      spaceRef: 'room',
      subjectId: 'user-2',
      window: { source: 'member', mode: 'since_date', sinceAt: '2026-11-01T08:00:00Z', revision: 5 },
    })
    stubFetch(200, {
      data: { space_ref: 'room', subject_id: 'user-3', history_window: memberWindow({ source: 'room', mode: 'full', revision: 0 }) },
    })
    expect((await getSpaceMemberHistoryWindow('room', 'user-3')).window).toEqual({
      source: 'room',
      mode: 'full',
      sinceAt: null,
      revision: 0,
    })
  })

  it('falls back to the requested ids when the answer names none', async () => {
    stubFetch(200, { data: { history_window: memberWindow() } })
    const read = await getSpaceMemberHistoryWindow('room', 'user-2')
    expect(read.spaceRef).toBe('room')
    expect(read.subjectId).toBe('user-2')
  })

  it('refuses an answer that is not a window as the upstream failure it is, and never makes one up', async () => {
    for (const data of [
      {},
      { history_window: null },
      { history_window: 'full' },
      { history_window: { mode: 'full' } },
      { history_window: { source: 'member' } },
      { history_window: memberWindow({ since_at: 'not a time' }) },
      { history_window: memberWindow({ revision: -1 }) },
      { history_window: memberWindow({ revision: 1.5 }) },
      { history_window: memberWindow({ revision: '2' }) },
      { history_window: memberWindow({ mode: 'Not A Word' }) },
    ]) {
      stubFetch(200, { data })
      const failure = await getSpaceMemberHistoryWindow('room', 'user-2').catch((error: unknown) => error)
      expect(failure, JSON.stringify(data)).toBeInstanceOf(ApiError)
      expect((failure as ApiError).status).toBe(502)
      expect((failure as ApiError).code).toBe('upstream_unavailable')
    }
  })

  it('carries a typed refusal as an ApiError with its code and details', async () => {
    stubFetch(403, { error: { code: 'room_role_forbidden', message: 'forbidden', details: {} } })
    const failure = await getSpaceMemberHistoryWindow('room', 'user-2').catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).status).toBe(403)
    expect((failure as ApiError).code).toBe('room_role_forbidden')
  })
})

describe('setSpaceMemberHistoryWindow (D18)', () => {
  const answer = (window: Record<string, unknown>, changed = true) => ({
    data: { space_ref: 'room', subject_id: 'user-2', changed, history_window: window },
  })

  it('PUTs the mode and the revision it read, and nothing else, for full history', async () => {
    const calls = stubFetch(200, answer(memberWindow({ mode: 'full', revision: 4 })))
    const result = await setSpaceMemberHistoryWindow('room 1', 'user/2', { mode: 'full', expectedRevision: 3 })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/members/user%2F2/history-window')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PUT')
    expect(sentBody(calls[0])).toEqual({ mode: 'full', expected_revision: 3 })
    expect(result).toEqual({
      spaceRef: 'room',
      subjectId: 'user-2',
      changed: true,
      window: { source: 'member', mode: 'full', sinceAt: null, revision: 4 },
    })
  })

  it('sends the instant with since_date, and the revision only when it has one', async () => {
    const calls = stubFetch(200, answer(memberWindow({ mode: 'since_date', since_at: '2026-11-01T00:00:00.000Z', revision: 1 })))
    await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'since_date', sinceAt: '2026-11-01T00:00:00.000Z' })
    expect(sentBody(calls[0])).toEqual({ mode: 'since_date', since_at: '2026-11-01T00:00:00.000Z' })
    expect(Object.keys(sentBody(calls[0]))).not.toContain('expected_revision')
    await setSpaceMemberHistoryWindow('room', 'user-2', {
      mode: 'since_date',
      sinceAt: '2026-11-01T00:00:00.000Z',
      expectedRevision: 0,
    })
    // Revision 0 is a revision (a member with no row yet), not an absent one.
    expect(sentBody(calls[1])).toEqual({
      mode: 'since_date',
      since_at: '2026-11-01T00:00:00.000Z',
      expected_revision: 0,
    })
  })

  it('never sends an instant with full or since_join, whatever the caller passed', async () => {
    const calls = stubFetch(200, answer(memberWindow()))
    await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'since_join', sinceAt: '2026-11-01T00:00:00.000Z', expectedRevision: 2 })
    await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'full', sinceAt: '2026-11-01T00:00:00.000Z' })
    expect(sentBody(calls[0])).toEqual({ mode: 'since_join', expected_revision: 2 })
    expect(sentBody(calls[1])).toEqual({ mode: 'full' })
  })

  it('sends since_date with no instant as it is, so Control answers its own refusal', async () => {
    const calls = stubFetch(200, answer(memberWindow()))
    await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'since_date' })
    expect(sentBody(calls[0])).toEqual({ mode: 'since_date' })
  })

  it('refuses a mode an admin may not set before any request is made (room is written only by an invite)', async () => {
    const calls = stubFetch(200, answer(memberWindow()))
    for (const mode of ['room', 'all', '', 'FULL']) {
      await expect(
        setSpaceMemberHistoryWindow('room', 'user-2', { mode: mode as never }),
        mode,
      ).rejects.toBeInstanceOf(TypeError)
    }
    expect(calls).toHaveLength(0)
  })

  it('reports an unchanged window as changed false, and a missing flag as false', async () => {
    stubFetch(200, answer(memberWindow({ mode: 'full' }), false))
    expect((await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'full' })).changed).toBe(false)
    stubFetch(200, { data: { history_window: memberWindow({ mode: 'full' }) } })
    expect((await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'full' })).changed).toBe(false)
  })

  it('does not claim a change from an answer that is not a window', async () => {
    stubFetch(200, { data: { changed: true } })
    const failure = await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'full' }).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).status).toBe(502)
  })

  it('carries each typed refusal with its status, code and details', async () => {
    const cases: Array<[number, string, Record<string, unknown>]> = [
      [409, 'room_member_changed', { current_revision: 7 }],
      [422, 'member_history_window_invalid', { field: 'since_at' }],
      [422, 'member_history_window_invalid', { field: 'mode' }],
      [503, 'member_history_window_unavailable', {}],
      [409, 'room_roster_derived', {}],
      [409, 'room_archived', {}],
      [404, 'member_not_found', {}],
      [403, 'room_delegation_required', {}],
    ]
    for (const [status, code, details] of cases) {
      stubFetch(status, { error: { code, message: 'refused', details } })
      const failure = await setSpaceMemberHistoryWindow('room', 'user-2', { mode: 'full' }).catch((error: unknown) => error)
      expect(failure, code).toBeInstanceOf(ApiError)
      expect((failure as ApiError).status).toBe(status)
      expect((failure as ApiError).code).toBe(code)
      expect((failure as ApiError).details).toEqual(details)
    }
  })
})

describe('the window parsers (D18)', () => {
  it('reads a member window, takes a missing revision as 0 and a missing instant as none', () => {
    expect(memberHistoryWindowFrom({ source: 'room', mode: 'since_join' })).toEqual({
      source: 'room',
      mode: 'since_join',
      sinceAt: null,
      revision: 0,
    })
    expect(memberHistoryWindowFrom(memberWindow({ since_at: ' 2026-11-01T08:00:00Z ' }))?.sinceAt).toBe('2026-11-01T08:00:00Z')
  })

  it('keeps a mode this build does not know, so a newer Control is not an outage', () => {
    expect(memberHistoryWindowFrom(memberWindow({ mode: 'since_message' }))?.mode).toBe('since_message')
    expect(isMemberHistoryMode('since_message')).toBe(false)
  })

  it('knows exactly the three modes an admin may set', () => {
    for (const mode of ['full', 'since_join', 'since_date']) expect(isMemberHistoryMode(mode), mode).toBe(true)
    for (const mode of ['room', 'all', '', 'FULL', null, undefined, 3]) expect(isMemberHistoryMode(mode), String(mode)).toBe(false)
  })

  it('reads the caller’s own window only when it is well formed with a boolean hidden', () => {
    expect(ownHistoryWindowFrom({ source: 'member', mode: 'since_date', since_at: '2026-11-01T08:00:00Z', hidden: true })).toEqual({
      source: 'member',
      mode: 'since_date',
      sinceAt: '2026-11-01T08:00:00Z',
      hidden: true,
    })
    expect(ownHistoryWindowFrom({ source: 'room', mode: 'full', since_at: null, hidden: false })?.hidden).toBe(false)
    for (const raw of [
      undefined,
      null,
      'hidden',
      [],
      {},
      { source: 'member', mode: 'full', since_at: null },
      { source: 'member', mode: 'full', since_at: null, hidden: 'true' },
      { source: 'member', mode: 'full', since_at: null, hidden: 1 },
      { source: 'member', mode: 'since_date', since_at: 'yesterday', hidden: true },
      { source: '', mode: 'full', since_at: null, hidden: true },
      { source: 'member', mode: 7, since_at: null, hidden: true },
    ]) {
      expect(ownHistoryWindowFrom(raw), JSON.stringify(raw)).toBeUndefined()
    }
  })
})
