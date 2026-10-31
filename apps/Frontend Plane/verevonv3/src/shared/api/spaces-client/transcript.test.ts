import { afterEach, describe, expect, it, vi } from 'vitest'

import { getSpaceThreads, getSpaceThreadTranscript, loadSpaceTranscript } from './index'

type FetchCall = { url: string }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      calls.push({ url: String(url) })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const transcript = (turns: unknown[]) => ({ data: { transcript: { threadId: 't1', turns } } })

const art = (over: Record<string, unknown> = {}) => ({
  id: 'a1',
  kind: 'document',
  title: 'Plan',
  content: 'body',
  version: 1,
  ...over,
})

describe('getSpaceThreadTranscript turn mapping (WS07-S11)', () => {
  it('keeps an artifact-only assistant turn and maps message_id', async () => {
    stubFetch(200, transcript([{ role: 'assistant', content: '', message_id: 'm7', artifacts: [art()] }]))
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns).toHaveLength(1)
    expect(turns[0]).toMatchObject({ role: 'assistant', content: '', messageId: 'm7' })
    expect(turns[0]?.artifacts?.map((a) => a.id)).toEqual(['a1'])
  })

  it('drops an empty user turn and an empty assistant turn without artifacts', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'user', content: '   ' },
        { role: 'assistant', content: '' },
        { role: 'assistant', content: 'hei' },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns.map((t) => t.content)).toEqual(['hei'])
  })

  it('does not let a user turn carry artifacts, so a user turn with only artifacts is dropped', async () => {
    stubFetch(200, transcript([{ role: 'user', content: '', artifacts: [art()] }]))
    expect((await getSpaceThreadTranscript('room', 't1')).turns).toEqual([])
  })

  it('gates artifacts by kind: pdf, image and binary are dropped and an all-bad turn vanishes', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'assistant', content: '', artifacts: [art({ kind: 'pdf' }), art({ id: 'i', kind: 'image' })] },
        {
          role: 'assistant',
          content: 'her',
          artifacts: [art({ id: 'b', kind: 'binary' }), art({ id: 'c', kind: 'code' }), art({ id: 'h', kind: 'html' })],
        },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns).toHaveLength(1)
    expect(turns[0]?.artifacts?.map((a) => a.kind)).toEqual(['code', 'html'])
  })

  it('accepts the camelCase messageId and omits artifacts when none survive', async () => {
    stubFetch(200, transcript([{ role: 'assistant', content: 'ok', messageId: 'm1', artifacts: [art({ version: 0 })] }]))
    const [turn] = (await getSpaceThreadTranscript('room', 't1')).turns
    expect(turn?.messageId).toBe('m1')
    expect(turn).not.toHaveProperty('artifacts')
  })
})

describe('getSpaceThreadTranscript turn time', () => {
  it('maps the server created_at to a normalized ISO createdAt', async () => {
    stubFetch(
      200,
      transcript([{ role: 'user', content: 'hei', created_at: '2026-10-31T09:30:15.250+00:00' }]),
    )
    const [turn] = (await getSpaceThreadTranscript('room', 't1')).turns
    expect(turn?.createdAt).toBe('2026-10-31T09:30:15.250Z')
  })

  it('accepts the camelCase createdAt and normalizes an offset to UTC', async () => {
    stubFetch(200, transcript([{ role: 'assistant', content: 'ok', createdAt: '2026-10-31T11:30:15+02:00' }]))
    const [turn] = (await getSpaceThreadTranscript('room', 't1')).turns
    expect(turn?.createdAt).toBe('2026-10-31T09:30:15.000Z')
  })

  it('leaves createdAt off when it is missing or not a time, and keeps the turn', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'user', content: 'a' },
        { role: 'user', content: 'b', created_at: 'yesterday-ish' },
        { role: 'user', content: 'c', created_at: '' },
        { role: 'user', content: 'd', created_at: 1793439015 },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns.map((t) => t.content)).toEqual(['a', 'b', 'c', 'd'])
    for (const turn of turns) expect(turn).not.toHaveProperty('createdAt')
  })

  it('prefers created_at over createdAt when both parse', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'user', content: 'x', created_at: '2026-10-31T09:00:00Z', createdAt: '2026-10-31T10:00:00Z' },
      ]),
    )
    expect((await getSpaceThreadTranscript('room', 't1')).turns[0]?.createdAt).toBe('2026-10-31T09:00:00.000Z')
  })
})

describe('getSpaceThreads shared_read (WS07-S4)', () => {
  const threads = (extra: Record<string, unknown>) => ({
    data: { space: { space_id: 's' }, membership: { role: 'owner' }, threads: [], ...extra },
  })

  it('defaults to available when shared_read is absent or malformed', async () => {
    stubFetch(200, threads({}))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
    stubFetch(200, threads({ shared_read: null }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
    stubFetch(200, threads({ shared_read: 'no' }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
    stubFetch(200, threads({ shared_read: { available: 'false' } }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
    stubFetch(200, threads({ shared_read: {} }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
  })

  it('is unavailable only on an explicit false, and available on an explicit true', async () => {
    stubFetch(200, threads({ shared_read: { available: false } }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: false })
    stubFetch(200, threads({ shared_read: { available: true } }))
    expect((await getSpaceThreads('room')).shared_read).toEqual({ available: true })
  })
})

describe('loadSpaceTranscript (WS07-S4)', () => {
  it('maps a 200 to ok with the thread id and turns', async () => {
    stubFetch(200, transcript([{ role: 'user', content: 'hei' }]))
    const load = await loadSpaceTranscript('room', 't1')
    expect(load).toMatchObject({ state: 'ok', threadId: 't1' })
    expect(load.state === 'ok' && load.turns.map((t) => t.content)).toEqual(['hei'])
  })

  it('maps 403 to restricted, 404 to missing and everything else to error', async () => {
    stubFetch(403, { error: { code: 'space_read_not_authorized', message: 'no' } })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'restricted' })
    stubFetch(404, { error: { code: 'not_found', message: 'gone' } })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'missing' })
    stubFetch(500, { error: { code: 'internal', message: 'boom' } })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'error' })
    stubFetch(502, { error: { code: 'bad_gateway', message: 'x' } })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'error' })
  })

  it('maps a network failure to error, never to restricted', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('network down'))))
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'error' })
  })

  it('requests the transcript route with both ids encoded', async () => {
    const calls = stubFetch(200, transcript([]))
    await loadSpaceTranscript('room a', 't/1')
    expect(calls[0]?.url).toContain('/api/v1/spaces/room%20a/threads/t%2F1/transcript')
  })
})
