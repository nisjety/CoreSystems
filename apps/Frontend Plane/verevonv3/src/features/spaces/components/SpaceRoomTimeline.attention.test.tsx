import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS11-S4: inside the room, "Jump to unread" moves focus to the first new post, and "Mark unread" on a
// post moves the room's read position to just before that post. The divider itself is covered by
// SpaceRoomTimeline.reading.test.tsx. The clients are mocked: no request is made.

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
}))
const attention = vi.hoisted(() => ({
  room: undefined as unknown,
  refresh: vi.fn(),
  markSpaceUnread: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/shared/api/spaces-attention-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-attention-client')>()),
  markSpaceUnread: attention.markSpaceUnread,
}))
vi.mock('@/features/spaces/lib/space-attention', () => ({
  retainSpaceAttention: () => () => undefined,
  refreshSpaceAttention: attention.refresh,
  spaceAttentionRoom: () => attention.room,
  spaceAttentionTotals: () => undefined,
  spaceAttentionStale: () => false,
  spaceAttentionAvailable: () => true,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')

const roster = [{ subject_id: 'u1', display_name: 'Kari Nordmann' }]
const agents: unknown[] = []

function renderRoom(threads: readonly unknown[], extra: Record<string, unknown> = {}) {
  return render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => threads as never}
      roster={() => roster as never}
      agents={() => agents as never}
      {...(extra as object)}
    />
  ))
}

const older = { thread_id: 'old', space_id: 'room-1', title: 'Gammel', updated_at: '2026-10-31T08:00:00Z', latest_run_status: 'completed' }
const fresh1 = { thread_id: 'n1', space_id: 'room-1', title: 'Ny en', updated_at: '2026-10-31T09:00:00Z', latest_run_status: 'completed' }
const fresh2 = { thread_id: 'n2', space_id: 'room-1', title: 'Ny to', updated_at: '2026-10-31T10:00:00Z', latest_run_status: 'completed' }

const JUMP = /(Hopp til første uleste|Jump to unread)/
const MARK_UNREAD = /(Merk som ulest|Mark unread)/

const originalScrollIntoView = Element.prototype.scrollIntoView

beforeEach(() => {
  attention.room = undefined
  attention.refresh.mockReset()
  attention.markSpaceUnread.mockReset().mockResolvedValue({ lastReadAt: 1, manualUnread: true })
  spacesClient.loadSpaceTranscript.mockReset().mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
  spacesClient.updateSpaceThreadPresentation.mockReset()
  // jsdom has no layout, so it has no scrollIntoView to call.
  Element.prototype.scrollIntoView = vi.fn()
})

afterEach(() => {
  cleanup()
  Element.prototype.scrollIntoView = originalScrollIntoView
})

describe('SpaceRoomTimeline: jump to unread', () => {
  it('offers the jump while something is new, and moves focus onto the first new post', async () => {
    renderRoom([fresh2, older, fresh1], { unreadThreadIds: () => new Set(['n1', 'n2']) })
    await screen.findByText('Ny en')
    fireEvent.click(screen.getByRole('button', { name: JUMP }))
    expect(document.activeElement?.id).toBe('post-n1')
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledTimes(1)
  })

  it('draws no jump control when nothing is new', async () => {
    renderRoom([older, fresh1], { unreadThreadIds: () => new Set<string>() })
    await screen.findByText('Ny en')
    expect(screen.queryByRole('button', { name: JUMP })).toBeNull()
  })

  it('draws no jump control on a visit that gave no derivation of what is new', async () => {
    renderRoom([older, fresh1])
    await screen.findByText('Ny en')
    expect(screen.queryByRole('button', { name: JUMP })).toBeNull()
  })
})

describe('SpaceRoomTimeline: mark unread from a post', () => {
  it('is not offered until the attention answer knows this room', async () => {
    renderRoom([fresh1])
    await screen.findByText('Ny en')
    expect(screen.queryByRole('button', { name: MARK_UNREAD })).toBeNull()
  })

  it('moves the read position to one millisecond before the post, and refreshes the badges', async () => {
    attention.room = { spaceRef: 'room-1' }
    renderRoom([fresh1])
    await screen.findByText('Ny en')
    const post = document.getElementById('post-n1')
    const button = screen.getAllByRole('button', { name: MARK_UNREAD }).find((item) => post?.contains(item))
    expect(button).toBeTruthy()
    fireEvent.click(button as HTMLElement)

    await waitFor(() => expect(attention.markSpaceUnread).toHaveBeenCalledTimes(1))
    expect(attention.markSpaceUnread).toHaveBeenCalledWith('room-1', Date.parse('2026-10-31T09:00:00Z') - 1)
    await waitFor(() => expect(attention.refresh).toHaveBeenCalledTimes(1))
  })

  it('draws no control for a post with no readable time, rather than guessing one', async () => {
    attention.room = { spaceRef: 'room-1' }
    renderRoom([{ thread_id: 'untimed', space_id: 'room-1', title: 'Uten tid', latest_run_status: 'completed' }])
    await screen.findByText('Uten tid')
    expect(screen.queryByRole('button', { name: MARK_UNREAD })).toBeNull()
  })
})
