import { afterEach, describe, expect, it, vi } from 'vitest'

import { joinSpaceRoom, updateSpaceRoomSettings } from './membership-writer'

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

describe('joinSpaceRoom (WS04-S8)', () => {
  it('POSTs an empty JSON object when no idempotency key is given', async () => {
    const calls = stubFetch(200, { data: { role: 'viewer', changed: true } })
    await joinSpaceRoom('room 1')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room%201/join')).toBe(true)
    expect(calls[0]?.init?.method).toBe('POST')
    expect(sentBody(calls[0])).toEqual({})
  })

  it('POSTs the idempotency key as the only field when one is given', async () => {
    const calls = stubFetch(200, { data: { role: 'viewer', changed: true } })
    await joinSpaceRoom('room', 'join-key-0001')
    expect(sentBody(calls[0])).toEqual({ idempotency_key: 'join-key-0001' })
  })
})

describe('updateSpaceRoomSettings (WS04-S8)', () => {
  it('PATCHes only the visibility when only the visibility changes', async () => {
    const calls = stubFetch(200, { data: { visibility: 'org_public', changed: true } })
    await updateSpaceRoomSettings('room', { visibility: 'org_public' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/settings')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PATCH')
    expect(sentBody(calls[0])).toEqual({ visibility: 'org_public' })
  })

  it('PATCHes only the join role when only the join role changes', async () => {
    const calls = stubFetch(200, { data: { join_role: 'editor', changed: true } })
    await updateSpaceRoomSettings('room', { joinRole: 'editor' })
    expect(sentBody(calls[0])).toEqual({ join_role: 'editor' })
  })

  it('sends both changed fields and the key when both change', async () => {
    const calls = stubFetch(200, { data: { visibility: 'private', join_role: 'viewer', changed: true } })
    await updateSpaceRoomSettings('room', { visibility: 'private', joinRole: 'viewer' }, 'settings-key-0001')
    expect(sentBody(calls[0])).toEqual({ visibility: 'private', join_role: 'viewer', idempotency_key: 'settings-key-0001' })
  })
})
