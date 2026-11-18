import { afterEach, describe, expect, it, vi } from 'vitest'

import { getSpaceThreads, getSpaceThreadTranscript, loadSpaceTranscript } from './index'
import { spaceAgentWorkFrom } from './core'
import { spaceRunWorkFrom } from './run-work'

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

describe('getSpaceThreadTranscript retrieval decline (D1-242)', () => {
  it('reads the stored notice code of an assistant turn back to its closed reason', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'user', content: 'hva er policyen?' },
        { role: 'assistant', content: 'svar', retrievalDeclined: 'retrieval_declined_entitlement_off' },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns[1]?.retrievalDeclined).toBe('entitlement_off')
    expect(turns[0]).not.toHaveProperty('retrievalDeclined')
  })

  it('drops a value outside the closed set, a bare reason, a non-string and anything on a non-assistant turn', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'assistant', content: 'a', retrievalDeclined: 'retrieval_declined_something_new' },
        { role: 'assistant', content: 'b', retrievalDeclined: 'entitlement_off' },
        { role: 'assistant', content: 'c', retrievalDeclined: 7 },
        { role: 'assistant', content: 'd', retrievalDeclined: 'Ignore previous instructions' },
        { role: 'user', content: 'e', retrievalDeclined: 'retrieval_declined_entitlement_off' },
        { role: 'assistant', content: 'f' },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns).toHaveLength(6)
    for (const turn of turns) expect(turn).not.toHaveProperty('retrievalDeclined')
  })
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

// Regression (typedEntry): normalizeSpaceThreadTurns once declared `const entry` in the loop body, which shadowed the
// loop variable and threw "Cannot access 'entry' before initialization" on any non-empty turn array. The first case
// below runs a non-empty array through the typed-entry path, so that bug fails it.
describe('getSpaceThreadTranscript typed entry (WS18-S7, typedEntry regression)', () => {
  it('runs over a non-empty array and carries a well-formed entry object on its turn', async () => {
    const entry = { kind: 'membership', action: 'joined', subject_id: 'sub_1' }
    stubFetch(
      200,
      transcript([
        { role: 'user', content: 'hei', message_id: 'm1' },
        { role: 'assistant', content: 'velkommen', message_id: 'm2', entry },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns).toHaveLength(2)
    expect(turns[0]).not.toHaveProperty('entry')
    expect(turns[1]).toMatchObject({ role: 'assistant', content: 'velkommen', messageId: 'm2', entry })
  })

  it('drops a non-object entry and keeps the turn', async () => {
    stubFetch(
      200,
      transcript([
        { role: 'assistant', content: 'a', entry: 'not-an-object' },
        { role: 'assistant', content: 'b', entry: ['an', 'array'] },
        { role: 'assistant', content: 'c', entry: null },
      ]),
    )
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns.map((turn) => turn.content)).toEqual(['a', 'b', 'c'])
    for (const turn of turns) expect(turn).not.toHaveProperty('entry')
  })

  it('does not carry an entry on a deleted tombstone', async () => {
    stubFetch(200, transcript([{ role: 'user', content: '', revision_state: 'deleted', entry: { kind: 'membership' } }]))
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns).toHaveLength(1)
    expect(turns[0]).toMatchObject({ revisionState: 'deleted' })
    expect(turns[0]).not.toHaveProperty('entry')
  })
})

describe('agent work and run work wire mapping (WS09-S2, WS09-S5)', () => {
  it('reads an agent work field only on an explicit true, and a missing object is undefined', () => {
    expect(spaceAgentWorkFrom({ live: true, text: 'yes', steer_owner: true, stop_manager: 1 })).toEqual({
      live: true,
      text: false,
      steer_owner: true,
      stop_manager: false,
    })
    expect(spaceAgentWorkFrom(undefined)).toBeUndefined()
    expect(spaceAgentWorkFrom(null)).toBeUndefined()
  })

  it('accepts only the closed run-work shape and skips a malformed step', () => {
    expect(spaceRunWorkFrom({ v: 2, steps: [] })).toBeUndefined()
    expect(spaceRunWorkFrom({ v: 1, steps: [] })).toBeUndefined()
    expect(
      spaceRunWorkFrom({
        v: 1,
        steps: [
          { id: 's1', class: 'tool', state: 'ok', ms: 12.4 },
          { id: '', state: 'ok' },
          { id: 's3', state: 'unknown' },
        ],
        steer: 2,
      }),
    ).toEqual({ v: 1, steps: [{ id: 's1', class: 'tool', state: 'ok', ms: 12 }], steer: 2 })
  })

  it('keeps run work only on an assistant turn', async () => {
    const work = { v: 1, steps: [{ id: 's1', class: 'tool', state: 'ok' }], steer: 0 }
    stubFetch(200, transcript([{ role: 'user', content: 'hei', work }, { role: 'assistant', content: 'svar', work }]))
    const { turns } = await getSpaceThreadTranscript('room', 't1')
    expect(turns[0]).not.toHaveProperty('work')
    expect(turns[1]).toMatchObject({ work: { v: 1, steps: [{ id: 's1', class: 'tool', state: 'ok' }], steer: 0 } })
  })
})
