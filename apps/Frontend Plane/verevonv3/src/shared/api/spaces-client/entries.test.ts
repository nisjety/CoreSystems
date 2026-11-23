import { afterEach, describe, expect, it, vi } from 'vitest'

import { getCommandPaletteFeature, getSpaceFeatures, resolveSpaceEntry } from './entries'

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

describe('resolveSpaceEntry (WS13-S4a)', () => {
  it('maps a message entry to the thread that holds it', async () => {
    const calls = stubFetch(200, {
      data: { kind: 'message', space_ref: 'room', thread_id: 'thr_1', message_id: 'msg_1' },
    })
    await expect(resolveSpaceEntry('room', 'msg_1')).resolves.toEqual({
      kind: 'message',
      spaceRef: 'room',
      threadId: 'thr_1',
      messageId: 'msg_1',
    })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/entries/msg_1')).toBe(true)
  })

  it('throws on a malformed body: another kind, a blank thread, no thread or no body', async () => {
    for (const body of [
      { kind: 'thread', thread_id: 'thr_1', message_id: 'msg_1' },
      { kind: 'message', thread_id: '  ', message_id: 'msg_1' },
      { kind: 'message', thread_id: 'thr_1' },
      {},
    ]) {
      stubFetch(200, { data: body })
      await expect(resolveSpaceEntry('room', 'msg_1')).rejects.toThrow('invalid_space_entry_response')
    }
  })
})

describe('getSpaceFeatures (WS13-S4a)', () => {
  it('reads a flag as on only when it is exactly true', async () => {
    const calls = stubFetch(200, {
      data: {
        features: { message_search: true, entry_links: 'true', command_palette: 1, room_pins: false, thread_fork: true },
      },
    })
    expect(await getSpaceFeatures()).toEqual({
      message_search: true,
      entry_links: false,
      command_palette: false,
      room_pins: false,
      thread_fork: true,
    })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/features')).toBe(true)
  })
})

describe('getCommandPaletteFeature (DEPTH-WS13-S6-1)', () => {
  it('reports the gateway answer for command_palette, true or false', async () => {
    stubFetch(200, { data: { features: { command_palette: true } } })
    await expect(getCommandPaletteFeature()).resolves.toBe(true)
    stubFetch(200, { data: { features: { command_palette: false } } })
    await expect(getCommandPaletteFeature()).resolves.toBe(false)
  })

  it('answers undefined when the gateway names no command_palette key, so the build default stays', async () => {
    stubFetch(200, { data: { features: { message_search: true } } })
    await expect(getCommandPaletteFeature()).resolves.toBeUndefined()
  })

  it('answers undefined when the read fails, so the build default stays', async () => {
    stubFetch(503, { error: { code: 'search_unavailable', message: 'unavailable', details: {} } })
    await expect(getCommandPaletteFeature()).resolves.toBeUndefined()
  })
})
