import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('../http', () => http)

const { getSpaceTimeline } = await import('./timeline')

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
