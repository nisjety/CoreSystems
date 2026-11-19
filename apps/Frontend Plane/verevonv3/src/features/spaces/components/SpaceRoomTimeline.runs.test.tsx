import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { emptySpaceRunView, type SpaceRunView } from '../lib/space-run-view'

// WS09-S5 and WS09-S8 on the room timeline. A live view of somebody else's run replaces the polled "is working"
// row, once the first snapshot is in; the run's own starter keeps the plain row. The manager's Stop is offered only
// to a manager or owner while the listing says manager Stop is on. The run store is stubbed at the module boundary
// (its stream is covered by lib/space-run-store.test.ts), the way SpaceRunStrip.test.tsx does it.

const runs = vi.hoisted(() => new Map<string, unknown>())
vi.mock('../lib/space-run-store', () => ({
  spaceRunStore: {
    view: (runId: string) => runs.get(runId),
    views: () => [...runs.values()],
    attach: () => () => {},
    dispose: () => {},
  },
  trackLiveRuns: () => {},
}))

const spacesClient = vi.hoisted(() => ({
  loadSpaceTranscript: vi.fn(),
  getSpaceThreadTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
const stop = vi.hoisted(() => ({ stopSpaceRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/run-stop', () => stop)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')

const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }
const roster = [
  { subject_type: 'user', subject_id: 'owner-1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'editor-1', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
]
const running = {
  thread_id: 't1',
  space_id: 'room-1',
  title: 'Innkjøp',
  owner_subject_id: 'editor-1',
  latest_run_id: 'run-1',
  latest_run_status: 'running',
  updated_at: '2026-11-18T10:00:00Z',
}
const liveAgentWork = { live: true, text: false, steer_owner: false, stop_manager: true }

/** A run the live view has attached to: one running step, started by somebody other than the viewer. */
function liveView(overrides: Partial<SpaceRunView> = {}): SpaceRunView {
  return {
    ...emptySpaceRunView('run-1'),
    status: 'live',
    threadId: 't1',
    startedBy: 'editor-1',
    agentName: 'Lagerhjelpen',
    steps: [{ id: 's1', class: 'web', state: 'running' }],
    ...overrides,
  }
}

function renderRoom(extra: Record<string, unknown> = {}) {
  return render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => [running] as never}
      roster={() => roster as never}
      agents={() => [agent] as never}
      viewerSubjectId={() => 'viewer-1'}
      viewerRole={() => 'manager'}
      agentWork={() => liveAgentWork}
      {...(extra as object)}
    />
  ))
}

const card = () => document.querySelector('[data-run-card="true"]')
const stopButton = () => screen.queryByRole('button', { name: 'Stopp kjøringen' })

beforeEach(() => {
  runs.clear()
  stop.stopSpaceRun.mockReset().mockResolvedValue({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-1' })
  spacesClient.loadSpaceTranscript.mockReset().mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.updateSpaceThreadPresentation.mockReset()
})

afterEach(() => cleanup())

describe('SpaceRoomTimeline: a live run of someone else (WS09-S5)', () => {
  it('draws the run card in place of the polled working row once the first snapshot is in', async () => {
    runs.set('run-1', liveView())
    renderRoom()
    const group = await screen.findByRole('group', { name: 'Agentens arbeid' })
    expect(group.textContent).toContain('Søker på nettet')
    expect(card()).not.toBeNull()
    // The polled row is gone: no "is working" line and no status region of its own.
    expect(screen.queryByText('jobber …')).toBeNull()
    expect(document.querySelector('.verevon-room-turn--working[role="status"]')).toBeNull()
  })

  it('keeps the polled row until the first snapshot has arrived', async () => {
    runs.set('run-1', liveView({ status: 'connecting', steps: [] }))
    renderRoom()
    expect(await screen.findByText('jobber …')).toBeTruthy()
    expect(card()).toBeNull()
  })

  it('keeps the plain row for a run this viewer started', async () => {
    runs.set('run-1', liveView({ startedBy: 'viewer-1' }))
    renderRoom()
    expect(await screen.findByText('jobber …')).toBeTruthy()
    expect(card()).toBeNull()
    expect(stopButton()).toBeNull()
  })

  it('keeps the plain row when the listing does not offer the live view', async () => {
    runs.set('run-1', liveView())
    renderRoom({ agentWork: () => ({ live: false, text: false, steer_owner: false, stop_manager: false }) })
    expect(await screen.findByText('jobber …')).toBeTruthy()
    expect(card()).toBeNull()
  })

  it('draws no card for a view that has already finished', async () => {
    runs.set('run-1', liveView({ finished: { outcome: 'completed', reason: null } }))
    renderRoom()
    await screen.findByText('Innkjøp')
    expect(card()).toBeNull()
  })
})

describe('SpaceRoomTimeline: manager Stop on a live run (WS09-S8)', () => {
  it.each([
    ['manager', true, true],
    ['owner', true, true],
    ['editor', true, false],
    ['viewer', true, false],
    ['manager', false, false],
  ] as const)('a %s with stop_manager %s is offered Stop: %s', async (role, allowed, shown) => {
    runs.set('run-1', liveView())
    renderRoom({ viewerRole: () => role, agentWork: () => ({ ...liveAgentWork, stop_manager: allowed }) })
    await screen.findByRole('group', { name: 'Agentens arbeid' })
    expect(stopButton() !== null).toBe(shown)
  })

  it('sends the stop for this room and this run when the manager presses it, then says it was sent', async () => {
    runs.set('run-1', liveView())
    renderRoom()
    ;(await screen.findByRole('button', { name: 'Stopp kjøringen' })).click()
    await waitFor(() => expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1))
    expect(stop.stopSpaceRun.mock.calls[0]?.slice(0, 2)).toEqual(['room-1', 'run-1'])
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
  })
})
