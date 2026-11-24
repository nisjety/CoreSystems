import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../lib/space-room-features'

// WS11-S11: "Remind me" on each post of the room timeline. The control is drawn only while the room's
// `features.reminders` is on, and it reads nothing until someone opens it. The network reads are mocked.

const spacesClient = vi.hoisted(() => ({
  loadSpaceTranscript: vi.fn(),
  getSpaceThreadTranscript: vi.fn(),
  listPendingSpaceReminders: vi.fn(),
  createSpaceReminder: vi.fn(),
  cancelSpaceReminder: vi.fn(),
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')

const roster = [
  { subject_type: 'user', subject_id: 'u-kari', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'u-ola', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
]
const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }

/** A post, ordered by its minute of the day. */
const post = (id: string, minute: number) => ({
  thread_id: id,
  space_id: 'room-1',
  title: `Sak ${id}`,
  owner_subject_id: 'u-ola',
  latest_run_status: 'completed',
  updated_at: new Date(Date.UTC(2026, 10, 18, 9, minute)).toISOString(),
  last_message_seq: minute,
})

const REMIND = 'Påminn meg'

beforeEach(() => {
  // The post's visibility watcher needs an observer; these tests do not depend on what comes near.
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  spacesClient.loadSpaceTranscript.mockReset().mockResolvedValue({ state: 'ok', threadId: 'x', turns: [] })
  spacesClient.listPendingSpaceReminders.mockReset().mockResolvedValue([])
})

afterEach(() => {
  clearSpaceRoomFeatures()
  vi.unstubAllGlobals()
  cleanup()
})

describe('SpaceRoomTimeline: the post reminder control (WS11-S11)', () => {
  it('draws Remind me on every post only while the room reports features.reminders on', async () => {
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [post('t-1', 5), post('t-2', 6)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    flush()
    expect(screen.queryAllByRole('button', { name: REMIND })).toHaveLength(0)
    expect(spacesClient.listPendingSpaceReminders).not.toHaveBeenCalled()

    publishSpaceRoomFeatures({ reminders: true })
    flush()
    await waitFor(() => expect(screen.queryAllByRole('button', { name: REMIND })).toHaveLength(2))
    // Drawing the control reads nothing: the list is read only when someone opens it.
    expect(spacesClient.listPendingSpaceReminders).not.toHaveBeenCalled()
  })

  it('takes the control away when the room turns the flag off after the timeline is drawn', async () => {
    publishSpaceRoomFeatures({ reminders: true })
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [post('t-1', 5)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    await waitFor(() => expect(screen.queryAllByRole('button', { name: REMIND })).toHaveLength(1))
    publishSpaceRoomFeatures({ reminders: false })
    flush()
    await waitFor(() => expect(screen.queryAllByRole('button', { name: REMIND })).toHaveLength(0))
  })
})
