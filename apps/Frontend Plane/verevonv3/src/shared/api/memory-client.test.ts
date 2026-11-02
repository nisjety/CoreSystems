import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  correctMemory,
  deleteMemory,
  getMemoryControls,
  listMemories,
  rememberMemory,
  resetMemory,
  setMemoryState,
} from './memory-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('listMemories', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('requests the memory list endpoint and normalizes snake_case entries', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        memories: [
          { memory_id: 'mem_1', topic: 'USER', content: 'prefers dark mode', updated_at: '2026-07-29T10:00:00Z' },
        ],
        degraded: false,
        degradation_reason: '',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await listMemories()

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory')
    expect(init?.method ?? 'GET').toBe('GET')
    expect(result).toEqual({
      memories: [
        {
          memoryId: 'mem_1',
          topic: 'USER',
          content: 'prefers dark mode',
          updatedAt: '2026-07-29T10:00:00Z',
          // No `provenance` in the payload -> 'unknown'. An older gateway that
          // does not send the field must NOT have its rows read as stated.
          provenance: 'unknown',
        },
      ],
      degraded: false,
      degradationReason: '',
    })
  })

  it('maps provenance, and refuses to guess for anything unrecognised', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        memories: [
          { memory_id: 'm1', provenance: 'inferred' },
          { memory_id: 'm2', provenance: 'stated' },
          { memory_id: 'm3', provenance: 'somethingNew' },
          { memory_id: 'm4' },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await listMemories()

    expect(result.memories.map((entry) => entry.provenance)).toEqual([
      'inferred',
      'stated',
      // A provenance this client does not know is 'unknown', never a real
      // value — the badge must not claim the user said something.
      'unknown',
      'unknown',
    ])
  })

  it('drops entries with no memory id and surfaces the degraded flag/reason', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        memories: [{ topic: 'USER', content: 'no id, dropped' }],
        degraded: true,
        degradation_reason: 'DEGRADED_LETTA_ZDR_READ_SUPPRESSED',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await listMemories()

    expect(result.memories).toEqual([])
    expect(result.degraded).toBe(true)
    expect(result.degradationReason).toBe('DEGRADED_LETTA_ZDR_READ_SUPPRESSED')
  })

  it('returns an empty, non-degraded result for a user with no memories yet', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ memories: [], degraded: false, degradation_reason: '' }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await listMemories()

    expect(result).toEqual({ memories: [], degraded: false, degradationReason: '' })
  })

  it('appends a limit query parameter when provided', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ memories: [] }))
    vi.stubGlobal('fetch', fetchMock)

    await listMemories(25)

    const [path] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory?limit=25')
  })
})

describe('deleteMemory', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends a DELETE against the encoded memory id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ deleted: true, degraded: false, degradation_reason: '' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await deleteMemory('mem 1')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory/mem%201')
    expect(init.method).toBe('DELETE')
    expect(result).toEqual({ deleted: true, degraded: false, degradationReason: '' })
  })

  it('propagates a not-found error rather than silently succeeding', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ error: { code: 'memory_not_found', message: 'memory not found' } }, 404),
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(deleteMemory('gone')).rejects.toThrow('memory not found')
  })
})

describe('correctMemory', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reports the original as removed only when the BFF says so', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ memory_id: 'mem_2', deleted: true, original: 'removed', degraded: false, degradation_reason: '' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await correctMemory('mem 1', 'thread-1', 'Ny tekst')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory/mem%201')
    expect(init.method).toBe('PATCH')
    expect(result).toEqual({ memoryId: 'mem_2', deleted: true, original: 'removed', degraded: false, degradationReason: '' })
  })

  it('keeps a not-found original as not_found, never as removed', async () => {
    // Session-core answers 404 for a memory that exists but is not the
    // caller's; the BFF reports that as `original: "not_found"`.
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      jsonResponse({ memory_id: 'mem_2', deleted: false, original: 'not_found', degraded: false, degradation_reason: '' }),
    ))

    const result = await correctMemory('org-mem', 'thread-1', 'Ny tekst')

    expect(result.deleted).toBe(false)
    expect(result.original).toBe('not_found')
  })

  it('reads an older BFF without `original` from `deleted` alone', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ memory_id: 'mem_2', deleted: false })))
    expect((await correctMemory('mem_1', 'thread-1', 'x')).original).toBe('not_removed')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ memory_id: 'mem_2', deleted: true })))
    expect((await correctMemory('mem_1', 'thread-1', 'x')).original).toBe('removed')
  })

  it('does not believe `removed` without the boolean that means the same', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ memory_id: 'mem_2', deleted: false, original: 'removed' })))
    const result = await correctMemory('mem_1', 'thread-1', 'x')
    expect(result).toMatchObject({ deleted: false, original: 'not_removed' })
  })
})

describe('memory controls', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads the state and the decision taken from it', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        state: 'paused',
        reason: 'user_paused',
        use_allowed: false,
        generation_allowed: false,
        org_memory_policy: 'allow',
        hidden_rows: 0,
        updated_at: '2026-10-24T10:00:00Z',
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const controls = await getMemoryControls()

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe('/api/v1/memory/controls')
    expect(controls).toEqual({
      state: 'paused',
      reason: 'user_paused',
      useAllowed: false,
      generationAllowed: false,
      orgMemoryPolicy: 'allow',
      hiddenRows: 0,
      updatedAt: '2026-10-24T10:00:00Z',
    })
  })

  it('never reads an unknown state as on, or a missing flag as allowed', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ state: 'dormant', reason: 'new_reason' })))

    const controls = await getMemoryControls()

    expect(controls.state).toBeNull()
    expect(controls.reason).toBe('unknown')
    expect(controls.useAllowed).toBe(false)
    expect(controls.generationAllowed).toBe(false)
  })

  it('sends only the state with PUT', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ state: 'off', reason: 'user_off', hidden_rows: 3 }))
    vi.stubGlobal('fetch', fetchMock)

    const controls = await setMemoryState('off')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory/controls')
    expect(init.method).toBe('PUT')
    expect(JSON.parse(String(init.body))).toEqual({ state: 'off' })
    expect(controls).toMatchObject({ state: 'off', hiddenRows: 3 })
  })

  it('surfaces a refusal instead of pretending the state changed', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse({ error: { code: 'memory_controls_unavailable', message: 'zero data retention' } }, 409),
        ),
    )

    await expect(setMemoryState('paused')).rejects.toThrow('zero data retention')
  })
})

describe('resetMemory', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('always sends confirm: true and reports what was erased', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ durable_deleted: 4, semantic_unconfirmed: 1, degraded: true, degradation_reason: 'LETTA' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await resetMemory()

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory/reset')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ confirm: true })
    expect(result).toEqual({
      durableDeleted: 4,
      semanticUnconfirmed: 1,
      degraded: true,
      degradationReason: 'LETTA',
    })
  })
})

describe('rememberMemory', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts the thread and content and returns the new id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ memory_id: 'mem_9', degraded: false }))
    vi.stubGlobal('fetch', fetchMock)

    const saved = await rememberMemory('thread-1', 'Jeg heter Ima')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/memory')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ threadId: 'thread-1', content: 'Jeg heter Ima' })
    expect(saved).toEqual({ memoryId: 'mem_9' })
  })

  it('rejects while memory is paused', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse({ error: { code: 'memory_not_enabled', message: 'memory_not_enabled:user_paused' } }, 409),
        ),
    )

    await expect(rememberMemory('thread-1', 'x')).rejects.toThrow('memory_not_enabled')
  })
})

describe('listMemories withheld legacy rows (WS17-S2)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('maps withheld_reason and origin_space_ref from a withheld row', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({
          memories: [
            {
              memory_id: 'm1',
              topic: 'USER',
              content: 'learned in a room',
              withheld_reason: 'room_origin_unverified',
              origin_space_ref: 'space-room-1',
            },
          ],
        }),
      ),
    )

    const result = await listMemories()

    expect(result.memories[0]).toMatchObject({
      memoryId: 'm1',
      withheldReason: 'room_origin_unverified',
      originSpaceRef: 'space-room-1',
    })
  })

  it('reads an unrecognised withheld reason as unknown, never as a known value', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({
          memories: [{ memory_id: 'm1', withheld_reason: 'some_future_reason' }],
        }),
      ),
    )

    const result = await listMemories()

    expect(result.memories[0]?.withheldReason).toBe('unknown')
    // No Space was named, so none is invented.
    expect(result.memories[0]).not.toHaveProperty('originSpaceRef')
  })

  it('accepts the camelCase spelling too', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({
          memories: [{ memoryId: 'm1', withheldReason: 'room_origin_unverified', originSpaceRef: 'space-2' }],
        }),
      ),
    )

    const result = await listMemories()

    expect(result.memories[0]).toMatchObject({ withheldReason: 'room_origin_unverified', originSpaceRef: 'space-2' })
  })

  it('leaves both fields absent for an ordinary row and for an older gateway', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({
          memories: [
            { memory_id: 'old', topic: 'USER', content: 'from an older gateway' },
            { memory_id: 'blank', withheld_reason: '', origin_space_ref: '' },
          ],
        }),
      ),
    )

    const result = await listMemories()

    for (const entry of result.memories) {
      expect(entry).not.toHaveProperty('withheldReason')
      expect(entry).not.toHaveProperty('originSpaceRef')
    }
    expect(result.memories).toHaveLength(2)
  })
})
