// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { I18nProvider } from '@/shared/i18n'
import { SpaceAttentionReadAll, SpaceRoomAttentionBadge, SpaceTotalAttentionBadge } from './SpaceAttentionBadge'

// WS11-S4: the sidebar's attention badges and "Mark all read". The shared store and the client are
// replaced, so each test states exactly what the server said and checks what the sidebar draws.

const attention = vi.hoisted(() => ({
  room: undefined as unknown,
  totals: undefined as unknown,
  stale: false,
  refresh: vi.fn(),
  markAllSpacesRead: vi.fn(),
  announce: vi.fn(),
}))

vi.mock('@/features/spaces/lib/space-attention', () => ({
  retainSpaceAttention: () => () => undefined,
  refreshSpaceAttention: attention.refresh,
  spaceAttentionRoom: () => attention.room,
  spaceAttentionTotals: () => attention.totals,
  spaceAttentionStale: () => attention.stale,
  spaceAttentionAvailable: () => true,
}))
vi.mock('@/shared/api/spaces-attention-client', () => ({
  markAllSpacesRead: attention.markAllSpacesRead,
}))
vi.mock('@/shared/a11y/announcer', () => ({
  announce: attention.announce,
}))

/** One room as the gateway sends it, after the client has read it. */
function roomWith(overrides: Record<string, unknown>) {
  return {
    spaceRef: 'room-a',
    unreadThreads: 0,
    unreadCapped: false,
    mentions: 0,
    needsYou: 0,
    working: 0,
    lastActivityAt: null,
    lastReadAt: null,
    manualUnread: false,
    level: 'mentions',
    badge: { kind: 'none', n: 0 },
    gaps: [],
    ...overrides,
  }
}

beforeEach(() => {
  attention.room = undefined
  attention.totals = undefined
  attention.stale = false
  attention.refresh.mockReset()
  attention.announce.mockReset()
  attention.markAllSpacesRead.mockReset().mockResolvedValue({ roomsMarked: 1, itemsResolved: 0, markedAt: 1 })
})

afterEach(() => cleanup())

describe('SpaceRoomAttentionBadge', () => {
  it('shows the server count for a room that needs the reader, as a strong count', () => {
    attention.room = roomWith({ needsYou: 1, mentions: 1, badge: { kind: 'count', n: 2 } })
    render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    const badge = screen.getByRole('img', { name: /2 (trenger deg|need you)/ })
    expect(badge.getAttribute('data-kind')).toBe('count')
    expect(badge.getAttribute('data-strong')).toBe('true')
    expect(badge.textContent).toBe('2')
  })

  it('shows a quiet count for unread posts that nobody is waiting on', () => {
    attention.room = roomWith({ unreadThreads: 4, level: 'all', badge: { kind: 'count', n: 4, muted: true } })
    render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    const badge = screen.getByRole('img', { name: /4 (uleste|unread)/ })
    expect(badge.getAttribute('data-kind')).toBe('count')
    expect(badge.getAttribute('data-strong')).toBeNull()
  })

  it('shows a dot for new activity at the mentions level, with no number', () => {
    attention.room = roomWith({ unreadThreads: 1, badge: { kind: 'dot', n: 0 } })
    render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    const badge = screen.getByRole('img', { name: /(Ny aktivitet|New activity)/ })
    expect(badge.getAttribute('data-kind')).toBe('dot')
    expect(badge.textContent).toBe('')
  })

  it('draws nothing for a room the answer does not name', () => {
    attention.room = undefined
    const { container } = render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    expect(container.querySelector('.space-attention-badge')).toBeNull()
  })

  it('draws nothing for a room whose badge is none, because unknown is not zero and nothing is waiting', () => {
    attention.room = roomWith({ badge: { kind: 'none', n: 0 } })
    const { container } = render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    expect(container.querySelector('.space-attention-badge')).toBeNull()
  })

  it('says the count may be out of date once the answer is stale', () => {
    attention.room = roomWith({ needsYou: 1, badge: { kind: 'count', n: 1 } })
    attention.stale = true
    render(() => (
      <I18nProvider>
        <SpaceRoomAttentionBadge spaceRef="room-a" />
      </I18nProvider>
    ))
    const badge = screen.getByRole('img', { name: /(kan være utdatert|may be out of date)/ })
    expect(badge.getAttribute('data-stale')).toBe('true')
  })
})

describe('SpaceTotalAttentionBadge', () => {
  it('draws nothing while no room needs the reader, even when some rooms are unread', () => {
    attention.totals = { badge: 0, unreadRooms: 3, otherOpen: 0 }
    const { container } = render(() => (
      <I18nProvider>
        <SpaceTotalAttentionBadge />
      </I18nProvider>
    ))
    expect(container.querySelector('.space-attention-badge')).toBeNull()
  })

  it('draws the total the server gave, and says what it counts', () => {
    attention.totals = { badge: 3, unreadRooms: 1, otherOpen: 0 }
    render(() => (
      <I18nProvider>
        <SpaceTotalAttentionBadge />
      </I18nProvider>
    ))
    const badge = screen.getByRole('img', { name: /3 (trenger deg i rom|need you in rooms)/ })
    expect(badge.textContent).toBe('3')
  })
})

describe('SpaceAttentionReadAll', () => {
  it('is not offered while nothing is counted', () => {
    attention.totals = { badge: 0, unreadRooms: 0, otherOpen: 0 }
    const { container } = render(() => (
      <I18nProvider>
        <SpaceAttentionReadAll />
      </I18nProvider>
    ))
    expect(screen.queryByRole('button', { name: /(Merk alle som lest|Mark all read)/ })).toBeNull()
    expect(container.querySelector('.space-attention-readall')).toBeNull()
  })

  it('marks every room read, says so, and asks for a fresh answer', async () => {
    attention.totals = { badge: 0, unreadRooms: 2, otherOpen: 0 }
    render(() => (
      <I18nProvider>
        <SpaceAttentionReadAll />
      </I18nProvider>
    ))
    fireEvent.click(screen.getByRole('button', { name: /(Merk alle som lest|Mark all read)/ }))
    await waitFor(() => expect(attention.refresh).toHaveBeenCalledTimes(1))
    expect(attention.markAllSpacesRead).toHaveBeenCalledWith()
    expect(attention.announce).toHaveBeenCalledWith(
      expect.stringMatching(/(Alle rom er merket som lest|All rooms marked as read)/),
      { key: 'space-attention-read-all' },
    )
  })

  it('says why it failed, and offers the button again', async () => {
    attention.totals = { badge: 1, unreadRooms: 1, otherOpen: 0 }
    attention.markAllSpacesRead.mockRejectedValue(new Error('offline'))
    render(() => (
      <I18nProvider>
        <SpaceAttentionReadAll />
      </I18nProvider>
    ))
    fireEvent.click(screen.getByRole('button', { name: /(Merk alle som lest|Mark all read)/ }))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(attention.refresh).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: /(Merk alle som lest|Mark all read)/ }).hasAttribute('disabled')).toBe(false)
  })

  it('says the counts may be out of date once the answer is stale', () => {
    attention.totals = undefined
    attention.stale = true
    render(() => (
      <I18nProvider>
        <SpaceAttentionReadAll />
      </I18nProvider>
    ))
    expect(screen.getByText(/(Tellingen kan være utdatert|Counts may be out of date)/)).toBeTruthy()
  })
})
