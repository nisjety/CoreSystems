import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('../http', () => http)

const { getSpaceTimeline } = await import('./timeline')
const { getSpaceThreadTranscript } = await import('./core')

beforeEach(() => http.requestJson.mockReset())
afterEach(() => vi.restoreAllMocks())

function row(extra: Record<string, unknown> = {}) {
  return { kind: 'notice', id: 'rn_1', action: 'agent_bound', created_at: '2026-11-02T10:00:00Z', ...extra }
}

async function systemEntriesFor(rows: unknown) {
  http.requestJson.mockResolvedValue({ page: {}, threads: [], system_entries: rows })
  return (await getSpaceTimeline('space_1')).systemEntries
}

describe('getSpaceTimeline system rows (WS15-S12)', () => {
  it('parses a kind notice row as a system entry', async () => {
    const entries = await systemEntriesFor([
      row({ actor_id: 'alice', subject_id: 'agent-1' }),
    ])
    expect(entries).toEqual([
      {
        kind: 'system',
        id: 'rn_1',
        at: '2026-11-02T10:00:00.000Z',
        code: 'agent_bound',
        actorId: 'alice',
        subjectId: 'agent-1',
      },
    ])
  })

  it('parses every notice word the Application plane writes', async () => {
    const words = ['agent_bound', 'agent_paused', 'agent_resumed', 'agent_revoked', 'room_renamed', 'topic_changed']
    const entries = await systemEntriesFor(
      words.map((action, index) => row({ id: `rn_${index}`, action, created_at: `2026-11-02T10:00:0${index}Z` })),
    )
    expect(entries.map((entry) => entry.code)).toEqual(words)
    // A notice carries no role change and, here, no actor or subject.
    expect(entries.every((entry) => entry.fromRole === undefined && entry.toRole === undefined)).toBe(true)
  })

  it('keeps membership rows with their roles beside notices', async () => {
    const entries = await systemEntriesFor([
      {
        kind: 'membership',
        id: 'm_1',
        action: 'role_changed',
        created_at: '2026-11-02T09:00:00Z',
        actor_id: 'owner',
        subject_id: 'bob',
        from_role: 'viewer',
        to_role: 'editor',
      },
      row(),
    ])
    expect(entries.map((entry) => [entry.id, entry.code])).toEqual([
      ['m_1', 'role_changed'],
      ['rn_1', 'agent_bound'],
    ])
    expect(entries[0]).toMatchObject({ fromRole: 'viewer', toRole: 'editor', actorId: 'owner', subjectId: 'bob' })
  })

  it('reads them oldest first even though the gateway sends newest first, breaking ties by id', async () => {
    const entries = await systemEntriesFor([
      row({ id: 'b', created_at: '2026-11-02T10:00:05Z' }),
      row({ id: 'a2', created_at: '2026-11-02T10:00:01Z' }),
      row({ id: 'a1', created_at: '2026-11-02T10:00:01Z' }),
    ])
    expect(entries.map((entry) => entry.id)).toEqual(['a1', 'a2', 'b'])
  })

  it('drops rows it cannot place: no id, no word, no readable time, an unknown kind or not an object', async () => {
    const entries = await systemEntriesFor([
      null,
      'x',
      7,
      row({ id: '' }),
      row({ id: undefined }),
      row({ id: 'no-word', action: '  ' }),
      row({ id: 'no-time', created_at: undefined }),
      row({ id: 'bad-time', created_at: 'yesterday' }),
      row({ id: 'wrong-kind', kind: 'message' }),
      row({ id: 'no-kind', kind: undefined }),
      row({ id: 'ok' }),
    ])
    expect(entries.map((entry) => entry.id)).toEqual(['ok'])
  })

  it('treats anything but an array as no system rows, and reads the truncation flag', async () => {
    for (const value of [undefined, null, 'x', {}, 3]) {
      expect(await systemEntriesFor(value)).toEqual([])
    }
    http.requestJson.mockResolvedValue({ page: {}, threads: [], system_entries: [row()], system_truncated: true })
    expect((await getSpaceTimeline('space_1')).systemTruncated).toBe(true)
    http.requestJson.mockResolvedValue({ page: {}, threads: [], system_truncated: 'yes' })
    expect((await getSpaceTimeline('space_1')).systemTruncated).toBe(false)
  })

  it('asks the timeline route for the room with the paging parameters encoded', async () => {
    http.requestJson.mockResolvedValue({ page: {}, threads: [] })
    await getSpaceTimeline('space 1', { limit: 20, before: 'c1', refresh: ['t1', 't2'] })
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/space%201/timeline?limit=20&before=c1&refresh=t1%2Ct2')
  })
})

describe('getSpaceTimeline paging and head (WS15-S8)', () => {
  it('sends the after cursor and caps the refresh list at 30 thread ids', async () => {
    http.requestJson.mockResolvedValue({ page: {}, threads: [] })
    await getSpaceTimeline('room', { after: 'c9' })
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room/timeline?after=c9')
    await getSpaceTimeline('room', { refresh: Array.from({ length: 35 }, (_, index) => `t${index}`) })
    const url = String(http.requestJson.mock.lastCall?.[0])
    const refresh = new URL(url, 'http://localhost').searchParams.get('refresh')?.split(',') ?? []
    expect(refresh).toHaveLength(30)
    expect(refresh.at(-1)).toBe('t29')
  })

  it('parses the page cursors, the windowed flag and the total, and reads a legacy page as one complete page', async () => {
    http.requestJson.mockResolvedValueOnce({
      page: { windowed: true, has_more: true, next_before: ' c1 ', next_after: '', newest: 'c9', total_posts: 42.7 },
      threads: [],
    })
    expect((await getSpaceTimeline('room')).page).toEqual({
      windowed: true,
      hasMore: true,
      nextBefore: 'c1',
      nextAfter: null,
      newest: 'c9',
      totalPosts: 42,
    })
    http.requestJson.mockResolvedValueOnce({ page: { windowed: false, has_more: true }, threads: [] })
    expect((await getSpaceTimeline('room')).page).toMatchObject({ windowed: false, hasMore: false, totalPosts: null })
  })

  it('reads the legacy head only when the answer carries a membership', async () => {
    http.requestJson.mockResolvedValueOnce({ page: { windowed: true }, threads: [] })
    expect((await getSpaceTimeline('room')).head).toBeUndefined()
    const membership = {
      space_ref: 'room',
      org_id: 'org_1',
      subject_id: 'user_1',
      kind: 'shared',
      role: 'editor',
      revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
    }
    http.requestJson.mockResolvedValueOnce({
      page: { windowed: true },
      threads: [],
      membership,
      space: { space_ref: 'room', name: 'Lager', kind: 'shared', lifecycle: 'active' },
    })
    const withHead = await getSpaceTimeline('room')
    expect(withHead.head?.membership).toEqual(membership)
    expect(withHead.head?.space.name).toBe('Lager')
  })
})

describe('getSpaceThreadTranscript window (WS15-S8)', () => {
  it('sends the tail as tail alone, with before, or with include_first, and parses the window', async () => {
    http.requestJson.mockResolvedValue({
      transcript: { threadId: 'thr_1', turns: [], window: { omitted_before: 7, has_more_before: true } },
    })
    const tail = await getSpaceThreadTranscript('room', 'thr 1', { tail: 20 })
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room/threads/thr%201/transcript?tail=20')
    expect(tail.window).toEqual({ omittedBefore: 7, hasMoreBefore: true })
    await getSpaceThreadTranscript('room', 'thr_1', { tail: 20, before: 'msg_9', includeFirst: true })
    expect(http.requestJson).toHaveBeenLastCalledWith(
      '/api/v1/spaces/room/threads/thr_1/transcript?tail=20&before=msg_9',
    )
    await getSpaceThreadTranscript('room', 'thr_1', { tail: 20, includeFirst: true })
    expect(http.requestJson).toHaveBeenLastCalledWith(
      '/api/v1/spaces/room/threads/thr_1/transcript?tail=20&include_first=1',
    )
  })

  it('reads no window when none was asked for, and reads the turns', async () => {
    http.requestJson.mockResolvedValue({
      transcript: {
        threadId: 'thr_1',
        turns: [{ role: 'user', content: 'hi', message_id: 'msg_1' }],
        window: { omitted_before: 7, has_more_before: true },
      },
    })
    const plain = await getSpaceThreadTranscript('room', 'thr_1')
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room/threads/thr_1/transcript')
    expect(plain.window).toBeUndefined()
    expect(plain.turns).toEqual([{ role: 'user', content: 'hi', messageId: 'msg_1' }])
  })
})
