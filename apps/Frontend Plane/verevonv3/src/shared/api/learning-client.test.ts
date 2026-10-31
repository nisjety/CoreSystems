import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  decideLearningProposal,
  listLearningLedger,
  snapshotContent,
  undoLearningEntry,
} from './learning-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('listLearningLedger', () => {
  it('requests the ledger and normalizes snake_case entries', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        entries: [
          {
            entry_id: 'led_2',
            kind: 'skill',
            action: 'proposed',
            skill_id: 'skill_1',
            skill_name: 'Utrullingsrutine',
            revision: 2,
            actor_kind: 'system',
            thread_id: 'thr_1',
            reason: 'Gjentatt i tre samtaler',
            before_json: '',
            after_json: '{"content":"Kjør testene"}',
            undone: false,
            undoable: true,
            created_at: '2026-10-25T10:00:00Z',
          },
        ],
        next_cursor: 'cursor-2',
        memory_skip_reason: '',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const page = await listLearningLedger()

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/learning/ledger')
    expect(init?.method ?? 'GET').toBe('GET')
    expect(page).toEqual({
      entries: [
        {
          entryId: 'led_2',
          kind: 'skill',
          action: 'proposed',
          skillId: 'skill_1',
          skillName: 'Utrullingsrutine',
          revision: 2,
          actorKind: 'system',
          threadId: 'thr_1',
          reason: 'Gjentatt i tre samtaler',
          beforeJson: '',
          afterJson: '{"content":"Kjør testene"}',
          undone: false,
          undoable: true,
          createdAt: '2026-10-25T10:00:00Z',
        },
      ],
      nextCursor: 'cursor-2',
      memorySkipReason: '',
    })
  })

  it('sends the cursor and page size only when they are given', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ entries: [], next_cursor: '' }))
    vi.stubGlobal('fetch', fetchMock)

    await listLearningLedger({ cursor: 'abc def', limit: 20 })

    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/learning/ledger?cursor=abc+def&limit=20')
  })

  it('drops an entry of an unknown kind and reads an unknown actor as unattributed', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        entries: [
          { entry_id: 'x1', kind: 'workspace', action: 'created' },
          { entry_id: 'm1', kind: 'memory', action: 'learned', actor_kind: 'robot', after_json: '{"content":"Ima"}' },
          { kind: 'skill', action: 'created' },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const page = await listLearningLedger()

    expect(page.entries.map((entry) => entry.entryId)).toEqual(['m1'])
    expect(page.entries[0]?.actorKind).toBe('unattributed')
    expect(page.entries[0]?.createdAt).toBeNull()
  })
})

describe('decideLearningProposal', () => {
  it('posts only the decision word to the proposal path', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ review_state: 'accepted', enabled: true, revision: 2 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await decideLearningProposal('skill_1', 'accept')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/learning/proposals/skill_1/decision')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ decision: 'accept' })
    expect(result).toEqual({ reviewState: 'accepted', enabled: true, revision: 2 })
  })

  it('reports a rejected proposal as not enabled', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ review_state: 'rejected', enabled: false, revision: 3 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await decideLearningProposal('skill_9', 'reject')

    expect(JSON.parse(String((fetchMock.mock.calls[0] as [string, RequestInit])[1].body))).toEqual({
      decision: 'reject',
    })
    expect(result.enabled).toBe(false)
    expect(result.reviewState).toBe('rejected')
  })

  it('surfaces a refused decision as an error, never as a result', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({ error: { code: 'not_proposed', message: 'not_proposed' } }, 409),
      ),
    )

    await expect(decideLearningProposal('skill_1', 'accept')).rejects.toMatchObject({ status: 409 })
  })
})

describe('undoLearningEntry', () => {
  it('posts to the entry undo path and reads what the undo did', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ action: 'restored', skill_id: 'skill_1', revision: 3 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await undoLearningEntry('led_2')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/learning/ledger/led_2/undo')
    expect(init.method).toBe('POST')
    expect(result).toEqual({ action: 'restored', skillId: 'skill_1', revision: 3 })
  })
})

describe('snapshotContent', () => {
  it('reads the content of a snapshot', () => {
    expect(snapshotContent('{"content":"Kjør testene","name":"x"}')).toBe('Kjør testene')
  })

  it('treats an absent, malformed or contentless snapshot as no content', () => {
    expect(snapshotContent('')).toBeNull()
    expect(snapshotContent('{not json')).toBeNull()
    expect(snapshotContent('"just a string"')).toBeNull()
    expect(snapshotContent('{"content":42}')).toBeNull()
  })
})
