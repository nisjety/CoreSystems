import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import type { AttentionRoom, SpacesAttention } from '@/shared/api/spaces-attention-client'

// WS11-S4: the shared attention store. The client is replaced with a fake, so these tests count the
// requests the store makes, not the ones a server would answer. Time is faked, and the tab is "visible"
// unless a test says otherwise. No BroadcastChannel and no Web Lock here, so the one tab polls for itself.

const attention = vi.hoisted(() => ({
  getSpacesAttention: vi.fn(),
  announce: vi.fn(),
}))

vi.mock('@/shared/api/spaces-attention-client', () => ({
  getSpacesAttention: attention.getSpacesAttention,
}))
vi.mock('@/shared/a11y/announcer', () => ({
  announce: attention.announce,
}))

const store = await import('./space-attention')

const visibility = { state: 'visible' as 'visible' | 'hidden' }

function room(spaceRef: string, needsYou: number, extra: Partial<AttentionRoom> = {}): AttentionRoom {
  return {
    spaceRef,
    unreadThreads: 0,
    unreadCapped: false,
    mentions: 0,
    needsYou,
    working: 0,
    lastActivityAt: null,
    lastReadAt: null,
    manualUnread: false,
    level: 'mentions',
    badge: needsYou > 0 ? { kind: 'count', n: needsYou } : { kind: 'none', n: 0 },
    gaps: [],
    ...extra,
  }
}

function answer(rooms: AttentionRoom[], badge = rooms.reduce((sum, item) => sum + item.needsYou, 0)): SpacesAttention {
  return {
    generatedAt: 1,
    mode: 'live',
    rooms,
    totals: { badge, unreadRooms: 0, otherOpen: 0 },
    truncated: false,
    staleSources: [],
  }
}

const tr = (_no: string, en: string) => en

/** Let the pending poll resolve and the store's writes commit. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0)
  flush()
}

function setVisibility(state: 'visible' | 'hidden'): void {
  visibility.state = state
  document.dispatchEvent(new Event('visibilitychange'))
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('BroadcastChannel', undefined)
  visibility.state = 'visible'
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility.state })
  attention.getSpacesAttention.mockReset()
  attention.announce.mockReset()
  store.resetSpaceAttentionForTests()
})

afterEach(() => {
  store.resetSpaceAttentionForTests()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  delete (document as unknown as { visibilityState?: unknown }).visibilityState
})

describe('the shared attention store: what it shows', () => {
  it('reports nothing known before the first answer, rather than zero', async () => {
    attention.getSpacesAttention.mockReturnValue(new Promise(() => undefined))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    expect(store.spaceAttentionRoom('room-a')).toBeUndefined()
    expect(store.spaceAttentionTotals()).toBeUndefined()
    expect(store.spaceAttentionStale()).toBe(false)
    release()
  })

  it('reports nothing for an organization with zero retention, even when it answers', async () => {
    attention.getSpacesAttention.mockResolvedValue({ ...answer([room('room-a', 3)]), mode: 'zdr_empty' })
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    expect(store.spaceAttentionRoom('room-a')).toBeUndefined()
    expect(store.spaceAttentionTotals()).toBeUndefined()
    release()
  })

  it('polls again after a stop even when the last request never came back', async () => {
    attention.getSpacesAttention.mockReturnValueOnce(new Promise(() => undefined))
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const first = store.retainSpaceAttention(tr)
    await settle()
    first()
    const second = store.retainSpaceAttention(tr)
    // The spacing gap still applies to the new session: the request is due ten seconds after the last one.
    await vi.advanceTimersByTimeAsync(10_000)
    flush()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    expect(store.spaceAttentionRoom('room-a')?.badge).toEqual({ kind: 'count', n: 1 })
    second()
  })

  it('renders the server badge as given, and never adds the rooms up itself', async () => {
    attention.getSpacesAttention.mockResolvedValue(
      answer([room('room-a', 2), room('room-b', 0, { unreadThreads: 4, level: 'all', badge: { kind: 'count', n: 4 } })], 9),
    )
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(store.spaceAttentionRoom('room-a')?.badge).toEqual({ kind: 'count', n: 2 })
    expect(store.spaceAttentionRoom('room-b')?.badge).toEqual({ kind: 'count', n: 4 })
    expect(store.spaceAttentionRoom('room-c')).toBeUndefined()
    expect(store.spaceAttentionTotals()).toEqual({ badge: 9, unreadRooms: 0, otherOpen: 0 })
    release()
  })

  it('marks the last answer stale once it is older than 90 seconds, and keeps showing it', async () => {
    attention.getSpacesAttention
      .mockResolvedValueOnce(answer([room('room-a', 1)]))
      .mockRejectedValue(new ApiError('Unavailable', 503, 'space_index_unavailable'))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(store.spaceAttentionStale()).toBe(false)

    await vi.advanceTimersByTimeAsync(60_000)
    flush()
    expect(store.spaceAttentionStale()).toBe(false)
    expect(store.spaceAttentionRoom('room-a')?.badge).toEqual({ kind: 'count', n: 1 })

    await vi.advanceTimersByTimeAsync(50_000)
    flush()
    expect(store.spaceAttentionStale()).toBe(true)
    expect(store.spaceAttentionRoom('room-a')?.badge).toEqual({ kind: 'count', n: 1 })
    release()
  })

  it('announces a rise in the number waiting, politely, and never a fall', async () => {
    attention.getSpacesAttention
      .mockResolvedValueOnce(answer([room('room-a', 1)]))
      .mockResolvedValueOnce(answer([room('room-a', 3)]))
      .mockResolvedValueOnce(answer([room('room-a', 1)]))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.announce).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(30_000)
    expect(attention.announce).toHaveBeenCalledTimes(1)
    expect(attention.announce).toHaveBeenLastCalledWith('2 new alerts in rooms. 3 waiting for you.', {
      key: 'space-attention-increase',
      coalesceMs: 10_000,
      kind: 'status',
    })

    await vi.advanceTimersByTimeAsync(30_000)
    expect(attention.announce).toHaveBeenCalledTimes(1)
    release()
  })
})

describe('the shared attention store: how often it asks', () => {
  it('asks once per browser, however many components retain it', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const releases = [store.retainSpaceAttention(tr), store.retainSpaceAttention(tr), store.retainSpaceAttention(tr)]
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    releases.forEach((release) => release())
  })

  it('stops asking when the last component lets go', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const first = store.retainSpaceAttention(tr)
    const second = store.retainSpaceAttention(tr)
    await settle()
    first()
    second()
    await vi.advanceTimersByTimeAsync(600_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
  })

  it('polls every thirty seconds while the answers keep coming', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const release = store.retainSpaceAttention(tr)
    await settle()
    await vi.advanceTimersByTimeAsync(29_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    release()
  })

  it('backs off after each failure, doubling the wait up to five minutes', async () => {
    attention.getSpacesAttention.mockRejectedValue(new ApiError('Busy', 503, 'search_busy'))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(59_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(2_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)

    await vi.advanceTimersByTimeAsync(118_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(2_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(3)
    release()
  })

  it('waits at least as long as the gateway asks in Retry-After', async () => {
    attention.getSpacesAttention.mockRejectedValue(new ApiError('Too often', 429, 'rate_limited', { retryAfterSeconds: 200 }))
    const release = store.retainSpaceAttention(tr)
    await settle()
    await vi.advanceTimersByTimeAsync(199_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(2_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    release()
  })

  it('stops for the session when the gateway says the surface is off', async () => {
    attention.getSpacesAttention.mockRejectedValue(new ApiError('Off', 404, 'space_attention_disabled'))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(store.spaceAttentionAvailable()).toBe(false)
    await vi.advanceTimersByTimeAsync(600_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    release()
  })

  it('asks for a fresh answer after a read, but no more often than every ten seconds', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const release = store.retainSpaceAttention(tr)
    await settle()
    store.refreshSpaceAttention()
    store.refreshSpaceAttention()
    await vi.advanceTimersByTimeAsync(9_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)

    store.refreshSpaceAttention()
    await vi.advanceTimersByTimeAsync(9_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(3)
    release()
  })

  it('does not drop a request made while a poll is still on its way', async () => {
    let finishFirst: (value: SpacesAttention) => void = () => undefined
    attention.getSpacesAttention
      .mockReturnValueOnce(
        new Promise<SpacesAttention>((resolve) => {
          finishFirst = resolve
        }),
      )
      .mockResolvedValue(answer([room('room-a', 2)]))
    const release = store.retainSpaceAttention(tr)
    // The first poll has been out for longer than the gap, so a request now is due at once.
    await vi.advanceTimersByTimeAsync(11_000)
    store.refreshSpaceAttention()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)

    finishFirst(answer([room('room-a', 1)]))
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)
    expect(store.spaceAttentionRoom('room-a')?.badge).toEqual({ kind: 'count', n: 2 })
    release()
  })

  it('does nothing for a refresh when no component is watching', () => {
    store.refreshSpaceAttention()
    expect(attention.getSpacesAttention).not.toHaveBeenCalled()
  })
})

describe('the shared attention store: visibility', () => {
  it('polls only while the tab is visible, and asks at once when it is seen again', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)

    setVisibility('hidden')
    await vi.advanceTimersByTimeAsync(300_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)

    setVisibility('visible')
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(2)

    await vi.advanceTimersByTimeAsync(30_000)
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(3)
    release()
  })

  it('never asks from a tab that starts hidden, until the tab is seen', async () => {
    attention.getSpacesAttention.mockResolvedValue(answer([room('room-a', 1)]))
    visibility.state = 'hidden'
    const release = store.retainSpaceAttention(tr)
    await settle()
    expect(attention.getSpacesAttention).not.toHaveBeenCalled()

    setVisibility('visible')
    await settle()
    expect(attention.getSpacesAttention).toHaveBeenCalledTimes(1)
    release()
  })
})
