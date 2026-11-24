// @vitest-environment jsdom
// W3 D21g: "Shared to <room>, take back" in the memory list. The list is the person's own live shares
// (GET /api/v1/memory/space-shares), and a take-back runs `memory.retract_from_space`. Norwegian default copy.

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MemorySpaceShares } from '@/features/settings/components/MemorySpaceShares'
import { executeAction } from '@/shared/actions/action-client'
import { spaceMemorySharesFrom } from '@/shared/api/memory-client'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  executeAction: vi.fn(),
}))

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), { headers: { 'Content-Type': 'application/json' }, status })
}

const SHARE = {
  room_memory_id: 'room_mem_1',
  space_ref: 'space-room-1',
  source_memory_id: 'mem_1',
  shared_at: '2026-11-20T10:00:00.000Z',
}
const ROOMS = { data: { spaces: [{ space_ref: 'space-room-1', name: 'Design', kind: 'room', lifecycle: 'active' }] } }
const ACTOR = { type: 'human', userId: 'user_1', orgId: 'org_1' }
const TAKEN_BACK = { result: { memory_id: 'room_mem_1', space_ref: 'space-room-1', deleted: true } } as unknown as Awaited<
  ReturnType<typeof executeAction>
>

function stubBackend(shares: () => Response) {
  const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`
    if (key === 'GET /api/v1/memory/space-shares') return shares()
    if (key === 'GET /api/v1/spaces') return jsonResponse(ROOMS)
    return jsonResponse({}, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

beforeEach(() => {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
  vi.mocked(executeAction).mockReset()
})

afterEach(() => {
  cleanup()
  clearSession()
  flush()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('spaceMemorySharesFrom', () => {
  it('keeps complete rows and drops a row without a room copy or a room', () => {
    expect(
      spaceMemorySharesFrom({
        shares: [SHARE, { room_memory_id: '', space_ref: 'x' }, { room_memory_id: 'r', space_ref: '' }, 'junk'],
      }),
    ).toEqual([
      { roomMemoryId: 'room_mem_1', spaceRef: 'space-room-1', sourceMemoryId: 'mem_1', sharedAt: SHARE.shared_at },
    ])
    expect(spaceMemorySharesFrom(null)).toEqual([])
  })
})

describe('MemorySpaceShares', () => {
  it('lists a live share by room name and takes it back through the action', async () => {
    let listed = [SHARE]
    const fetchMock = stubBackend(() => jsonResponse({ data: { shares: listed } }))
    vi.mocked(executeAction).mockImplementation(async () => {
      listed = []
      return TAKEN_BACK
    })

    render(() => <MemorySpaceShares version={0} />)
    expect(await screen.findByText(/Delt i Design/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Ta tilbake fra Design' }))
    flush()

    await waitFor(() => {
      expect(executeAction).toHaveBeenCalledWith(
        'memory.retract_from_space',
        ACTOR,
        expect.objectContaining({ memoryId: 'room_mem_1', spaceRef: 'space-room-1' }),
        expect.objectContaining({ idempotencyKey: expect.any(String) }),
      )
    })
    await waitFor(() => {
      expect(screen.queryByText(/Delt i Design/)).toBeNull()
    })
    const reads = fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/memory/space-shares')
    expect(reads.length).toBeGreaterThanOrEqual(2)
  })

  it('treats a copy already gone as taken back', async () => {
    stubBackend(() => jsonResponse({ data: { shares: [SHARE] } }))
    const { ApiError } = await import('@/shared/api/api-error')
    vi.mocked(executeAction).mockRejectedValue(new ApiError('gone', 404, 'memory_not_found'))

    render(() => <MemorySpaceShares version={0} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Ta tilbake fra Design' }))
    flush()

    await waitFor(() => expect(executeAction).toHaveBeenCalled())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('reuses the request id for a retry after a failure and starts a new one once a take-back settled', async () => {
    stubBackend(() => jsonResponse({ data: { shares: [SHARE] } }))
    const { ApiError } = await import('@/shared/api/api-error')
    vi.mocked(executeAction)
      .mockRejectedValueOnce(new ApiError('down', 503, 'upstream_unavailable'))
      .mockResolvedValue(TAKEN_BACK)

    render(() => <MemorySpaceShares version={0} />)
    const keys: unknown[] = []
    for (let attempt = 1; attempt <= 3; attempt += 1) {
      fireEvent.click(await screen.findByRole('button', { name: 'Ta tilbake fra Design' }))
      flush()
      await waitFor(() => expect(executeAction).toHaveBeenCalledTimes(attempt))
      await waitFor(() =>
        expect(
          (screen.getByRole('button', { name: 'Ta tilbake fra Design' }) as HTMLButtonElement).disabled,
        ).toBe(false),
      )
      keys.push((vi.mocked(executeAction).mock.calls[attempt - 1]?.[3] as { idempotencyKey?: string })?.idempotencyKey)
    }
    expect(keys[0]).toEqual(expect.any(String))
    expect(keys[1]).toBe(keys[0])
    expect(keys[2]).not.toBe(keys[1])
  })

  it('shows nothing while sharing is off', async () => {
    const fetchMock = stubBackend(() =>
      jsonResponse({ error: { code: 'feature_disabled', message: 'off', details: {} } }, 404),
    )

    render(() => <MemorySpaceShares version={0} />)
    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/memory/space-shares')).toBe(true)
    })
    flush()
    expect(screen.queryByText('Delt i rom')).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
