import { afterEach, describe, expect, it, vi } from 'vitest'
import { deleteMemory, listMemories } from './memory-client'

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
