import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import { archiveSpaceRoom, unarchiveSpaceRoom, updateSpaceRoomDetails } from './room-details'

type FetchCall = { url: string; init: RequestInit }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url: String(url), init })
    return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
  }))
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init.body)) as Record<string, unknown>

describe('updateSpaceRoomDetails (WS04-S2)', () => {
  it('PATCHes /api/v1/spaces/{ref} with only the fields it was given', async () => {
    const calls = stubFetch(200, { data: { space_ref: 'room', name: 'Lager', topic: 'Innkjøp' } })
    await updateSpaceRoomDetails('room', { name: 'Lager' })
    expect(calls).toHaveLength(1)
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room')).toBe(true)
    expect(calls[0]?.init.method).toBe('PATCH')
    expect(sentBody(calls[0])).toEqual({ name: 'Lager' })
    expect(Object.keys(sentBody(calls[0]))).not.toContain('topic')
  })

  it('sends a topic-only edit without the name', async () => {
    const calls = stubFetch(200, { data: { space_ref: 'room', name: 'Lager', topic: 'Innkjøp' } })
    await updateSpaceRoomDetails('room', { topic: 'Innkjøp' })
    expect(sentBody(calls[0])).toEqual({ topic: 'Innkjøp' })
  })

  it('sends both when both are given', async () => {
    const calls = stubFetch(200, { data: { space_ref: 'room', name: 'Ny', topic: 'Emne' } })
    await updateSpaceRoomDetails('room', { name: 'Ny', topic: 'Emne' })
    expect(sentBody(calls[0])).toEqual({ name: 'Ny', topic: 'Emne' })
  })

  it('sends an empty topic as an empty string (the clear), and maps the stored empty topic to null', async () => {
    const calls = stubFetch(200, { data: { space_ref: 'room', name: 'Lager', topic: '' } })
    const saved = await updateSpaceRoomDetails('room', { topic: '' })
    expect(sentBody(calls[0])).toEqual({ topic: '' })
    expect(saved).toEqual({ spaceRef: 'room', name: 'Lager', topic: null })
  })

  it('maps a missing or null topic in the answer to null, and a set one to the string', async () => {
    stubFetch(200, { data: { space_ref: 'room', name: 'Lager', topic: null } })
    expect((await updateSpaceRoomDetails('room', { name: 'Lager' })).topic).toBeNull()
    stubFetch(200, { data: { space_ref: 'room', name: 'Lager' } })
    expect((await updateSpaceRoomDetails('room', { name: 'Lager' })).topic).toBeNull()
    stubFetch(200, { data: { space_ref: 'room', name: 'Lager', topic: 'Innkjøp' } })
    expect((await updateSpaceRoomDetails('room', { name: 'Lager' })).topic).toBe('Innkjøp')
  })

  it('falls back to the requested ref when the answer names none, and encodes the ref in the path', async () => {
    const calls = stubFetch(200, { data: { name: 'Lager' } })
    const saved = await updateSpaceRoomDetails('a b/c', { name: 'Lager' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/a%20b%2Fc')).toBe(true)
    expect(saved.spaceRef).toBe('a b/c')
  })

  it('carries a typed refusal as an ApiError with its code and details', async () => {
    stubFetch(400, {
      error: { code: 'room_details_invalid', message: 'bad', details: { field: 'name', reason: 'name_required' } },
    })
    const error = await updateSpaceRoomDetails('room', { name: '' }).catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 400, code: 'room_details_invalid' })
    expect((error as ApiError).details).toMatchObject({ field: 'name', reason: 'name_required' })
  })

  it('carries the role refusal and the disabled flag by code', async () => {
    stubFetch(403, { error: { code: 'room_role_forbidden', message: 'no' } })
    await expect(updateSpaceRoomDetails('room', { name: 'x' })).rejects.toMatchObject({ status: 403, code: 'room_role_forbidden' })
    stubFetch(404, { error: { code: 'feature_disabled', message: 'off' } })
    await expect(updateSpaceRoomDetails('room', { name: 'x' })).rejects.toMatchObject({ status: 404, code: 'feature_disabled' })
  })
})

describe('archiveSpaceRoom and unarchiveSpaceRoom', () => {
  it('POSTs /archive with no body when no key is given, and maps the answer', async () => {
    const calls = stubFetch(200, { data: { state: 'archived', changed: true, archived_at: '2026-10-31T09:30:00Z' } })
    const result = await archiveSpaceRoom('room')
    expect(calls).toHaveLength(1)
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/archive')).toBe(true)
    expect(calls[0]?.init.method).toBe('POST')
    expect(calls[0]?.init.body).toBeUndefined()
    expect(result).toEqual({ state: 'archived', changed: true, archivedAt: '2026-10-31T09:30:00Z' })
  })

  it('sends the idempotency key as the only body field', async () => {
    const calls = stubFetch(200, { data: { state: 'archived', changed: false, archived_at: '2026-10-31T09:30:00Z' } })
    const result = await archiveSpaceRoom('room', { idempotencyKey: 'space-archive:1' })
    expect(sentBody(calls[0])).toEqual({ idempotency_key: 'space-archive:1' })
    expect(result.changed).toBe(false)
  })

  it('POSTs /unarchive and maps a null archived_at', async () => {
    const calls = stubFetch(200, { data: { state: 'active', changed: true, archived_at: null } })
    const result = await unarchiveSpaceRoom('a b/c')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/a%20b%2Fc/unarchive')).toBe(true)
    expect(result).toEqual({ state: 'active', changed: true, archivedAt: null })
  })

  it('falls back to the state the call asked for when the answer names none', async () => {
    stubFetch(200, { data: {} })
    expect((await archiveSpaceRoom('room')).state).toBe('archived')
    stubFetch(200, { data: {} })
    expect((await unarchiveSpaceRoom('room')).state).toBe('active')
  })

  it.each([
    [403, 'room_role_forbidden'],
    [404, 'feature_disabled'],
    [409, 'room_not_archivable'],
    [409, 'room_state_conflict'],
    [503, 'room_admin_unavailable'],
  ])('carries %i %s as an ApiError with its code', async (status, code) => {
    stubFetch(status, { error: { code, message: 'no' } })
    await expect(archiveSpaceRoom('room')).rejects.toMatchObject({ status, code })
  })

  it('carries the 429 retry hint in details', async () => {
    stubFetch(429, { error: { code: 'rate_limited', message: 'slow', details: { retry_after_seconds: 12 } } })
    const error = await archiveSpaceRoom('room').catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ApiError)
    expect((error as ApiError).details).toMatchObject({ retry_after_seconds: 12 })
  })
})
