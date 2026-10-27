import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import { updateSpaceRoomDetails } from './room-details'

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
