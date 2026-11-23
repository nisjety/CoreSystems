import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  activityFromRun,
  activityFromSchedule,
  buildSpaceActivity,
  type SpaceActivityItem,
} from '@/features/spaces/lib/activity-grammar'
import { activityFromRunView, liveRunIdOfRow, withLiveRunItems } from '@/features/spaces/lib/room-run-grammar'
import { RUN_STOP_TIMEOUT_MS } from '@/features/spaces/lib/space-run-stop'
import { emptySpaceRunView, type SpaceRunView } from '@/features/spaces/lib/space-run-view'
import { ApiError } from '@/shared/api/api-error'

// WS09-S8 on the Work and Activity rows: the manager's Stop on a live run row. The feed holds each run's attempt, so a
// row that the next live frame rebuilds keeps its "sent" and its one key. The stop call is stubbed at the module
// boundary, and the panels read a stubbed listing. No I18nProvider wraps these renders, so the copy is Norwegian.

const stop = vi.hoisted(() => ({ stopSpaceRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/run-stop', () => stop)

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceWork: vi.fn(),
  getSpaceActivity: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceActivityFeed } = await import('./SpaceActivityFeed')
const { SpaceRunCard } = await import('./SpaceRunCard')
const { SpaceWorkPanel } = await import('./SpaceWorkPanel')
const { SpaceActivityPanel } = await import('./SpaceActivityPanel')

const tr = (no: string) => no

function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error('expected a row')
  return value
}

/** A run as the Work and Activity listings carry it (the polled row). */
function runRow(id: string, status: string, goal = 'Rydde lager'): SpaceActivityItem {
  return must(activityFromRun({ id, goal, status, space_id: 'room-1', thread_id: 't1' }, tr))
}

const stopButtons = (container: HTMLElement) =>
  [...container.querySelectorAll<HTMLButtonElement>('.verevon-run-card__stop-button')]

const space = { space_ref: 'room-1', name: 'Leveranse', kind: 'room', lifecycle: 'active' }
const membership = {
  space_ref: 'room-1', org_id: 'org_1', subject_id: 'user_1', kind: 'room', role: 'manager',
  revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
}

beforeEach(() => {
  stop.stopSpaceRun.mockReset()
  spacesClient.getSpaceWork.mockReset()
  spacesClient.getSpaceActivity.mockReset()
})

afterEach(() => cleanup())

describe('SpaceActivityFeed: the manager Stop on a live run row (WS09-S8)', () => {
  it('offers no Stop unless the room has said the reader may stop runs', () => {
    const { container } = render(() => <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} />)
    expect(stopButtons(container)).toHaveLength(0)
  })

  it('offers Stop on a live run row once the gate is open, named for the run it stops', () => {
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    const buttons = stopButtons(container)
    expect(buttons).toHaveLength(1)
    expect(buttons[0]?.textContent).toBe('Stopp kjøringen')
    expect(buttons[0]?.getAttribute('aria-label')).toBe('Stopp kjøringen: Rydde lager')
  })

  // A finished run, a schedule and a conversation are not running, so none of them can be stopped. The thread's
  // latest-run row repeats a run the listing already has, and only the run's own row carries the Stop.
  it('offers Stop only on the run row of a run still in flight', () => {
    const items = [
      runRow('run-live', 'running', 'Pågående'),
      runRow('run-done', 'completed', 'Ferdig'),
      must(activityFromSchedule({ id: 'cron-1', name: 'Daglig rapport', enabled: true }, tr)),
      ...buildSpaceActivity([{ thread_id: 't9', title: 'Samtale', latest_run_status: 'running', space_id: 'room-1' }], tr),
    ]
    const { container } = render(() => <SpaceActivityFeed threads={[]} items={items} stop={{ spaceRef: 'room-1' }} />)
    const rowsWithStop = [...container.querySelectorAll('li')].filter((row) => row.querySelector('.verevon-run-card__stop-button'))
    expect(rowsWithStop).toHaveLength(1)
    expect(rowsWithStop[0]?.textContent).toContain('Pågående')
  })

  it('sends one stop for the run, with one key, and says it was sent until the run ends', async () => {
    stop.stopSpaceRun.mockResolvedValue({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-1' })
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    await waitFor(() => expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1))
    const [spaceRef, runId, key] = stop.stopSpaceRun.mock.calls[0] ?? []
    expect([spaceRef, runId]).toEqual(['room-1', 'run-1'])
    expect(String(key)).toMatch(/^stop-/)
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
    expect(stopButtons(container)[0]?.disabled).toBe(true)
    stopButtons(container)[0]?.click()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1)
  })

  // The live frames rebuild every row. The attempt must survive that: the same run, read again, still says "sent"
  // and a click on it still sends nothing.
  it('keeps its attempt when a newer read rebuilds the row for the same run', async () => {
    stop.stopSpaceRun.mockResolvedValue({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-1' })
    const [items, setItems] = createSignal<readonly SpaceActivityItem[]>([runRow('run-1', 'running')])
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={items()} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()

    setItems([runRow('run-1', 'running', 'Rydde lager, ny lesning')])
    await waitFor(() => expect(container.querySelector('.verevon-activity-object')?.textContent).toBe('Rydde lager, ny lesning'))
    expect(screen.getByText('Stopp sendt …')).toBeTruthy()
    expect(stopButtons(container)[0]?.disabled).toBe(true)
    stopButtons(container)[0]?.click()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1)
  })

  it('says the organisation has not enabled stopping when the server refuses for that reason', async () => {
    stop.stopSpaceRun.mockRejectedValue(new ApiError('forbidden', 403, 'space_run_control_not_authorized'))
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Organisasjonen din har ikke slått på stopping av andre medlemmers kjøringer.')
    expect(screen.queryByText('Stopp sendt …')).toBeNull()
  })

  it('closes Stop after the organisation refuses it, so no retry is offered that cannot work', async () => {
    stop.stopSpaceRun.mockRejectedValue(new ApiError('forbidden', 403, 'space_run_control_not_authorized'))
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    await screen.findByRole('alert')
    expect(stopButtons(container)[0]?.disabled).toBe(true)
    stopButtons(container)[0]?.click()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1)
  })

  it('says a stop that gets no answer in time is not confirmed yet, and lets the reader send it again under the same key', async () => {
    vi.useFakeTimers()
    try {
      stop.stopSpaceRun
        .mockReturnValueOnce(new Promise<never>(() => {}))
        .mockResolvedValueOnce({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-3' })
      const { container } = render(() => (
        <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
      ))
      stopButtons(container)[0]?.click()
      await vi.advanceTimersByTimeAsync(RUN_STOP_TIMEOUT_MS)
      expect(container.querySelector('[role="alert"]')?.textContent).toBe('Stoppen er ikke bekreftet ennå. Prøv igjen.')
      expect(stopButtons(container)[0]?.disabled).toBe(false)
      stopButtons(container)[0]?.click()
      await vi.advanceTimersByTimeAsync(0)
      expect(container.querySelector('[role="status"]')?.textContent).toBe('Stopp sendt …')
      expect(stop.stopSpaceRun).toHaveBeenCalledTimes(2)
      expect(stop.stopSpaceRun.mock.calls[1]?.[2]).toBe(stop.stopSpaceRun.mock.calls[0]?.[2])
    } finally {
      vi.useRealTimers()
    }
  })

  it('says a zero-retention refusal is a refusal, with nothing changed, and offers no retry', async () => {
    stop.stopSpaceRun.mockRejectedValue(new ApiError('precondition', 412, 'zdr_durable_mutation_forbidden'))
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Med nulllagring av data kan kjøringen ikke stoppes herfra ennå. Ingenting er endret.')
    expect(stopButtons(container)[0]?.disabled).toBe(true)
  })

  it('lets the reader retry a failed stop under the same key', async () => {
    stop.stopSpaceRun
      .mockRejectedValueOnce(new ApiError('unavailable', 503, 'run_control_unavailable'))
      .mockResolvedValueOnce({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-2' })
    const { container } = render(() => (
      <SpaceActivityFeed threads={[]} items={[runRow('run-1', 'running')]} stop={{ spaceRef: 'room-1' }} />
    ))
    stopButtons(container)[0]?.click()
    expect((await screen.findByRole('alert')).textContent).toBe('Kunne ikke stoppe kjøringen. Prøv igjen.')
    expect(stopButtons(container)[0]?.disabled).toBe(false)
    stopButtons(container)[0]?.click()
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(2)
    expect(stop.stopSpaceRun.mock.calls[1]?.[2]).toBe(stop.stopSpaceRun.mock.calls[0]?.[2])
  })

  it('offers Stop on the row of a run followed through its live view, named for the agent', () => {
    const view: SpaceRunView = {
      ...emptySpaceRunView('run-live'),
      status: 'live',
      threadId: 't1',
      agentName: 'Lagerhjelpen',
    }
    const merged = withLiveRunItems([runRow('run-live', 'running')], [view], new Set(['run-live']), 'room-1', tr)
    expect(merged).toHaveLength(1)
    const { container } = render(() => <SpaceActivityFeed threads={[]} items={merged} stop={{ spaceRef: 'room-1' }} />)
    expect(stopButtons(container)[0]?.getAttribute('aria-label')).toBe('Stopp kjøringen: Lagerhjelpen')
  })
})

describe('SpaceRunCard: a zero-retention refusal (WS09-S8)', () => {
  it('says the run cannot be stopped from here, nothing changed, and holds the control', async () => {
    stop.stopSpaceRun.mockRejectedValue(new ApiError('precondition', 412, 'zdr_durable_mutation_forbidden'))
    const view: SpaceRunView = {
      ...emptySpaceRunView('run-1'),
      status: 'live',
      threadId: 't1',
      startedBy: 'editor-1',
      agentName: 'Lagerhjelpen',
      steps: [{ id: 's1', class: 'web', state: 'running' }],
    }
    const { container } = render(() => (
      <SpaceRunCard view={view} startedByName="Ola Hansen" stop={{ spaceRef: 'room-1', runId: 'run-1' }} />
    ))
    const button = () => container.querySelector('.verevon-run-card__stop-button') as HTMLButtonElement
    button().click()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Med nulllagring av data kan kjøringen ikke stoppes herfra ennå. Ingenting er endret.')
    expect(button().disabled).toBe(true)
  })
})

describe('liveRunIdOfRow (WS09-S8)', () => {
  it('names the run of an in-flight run row, from either builder', () => {
    expect(liveRunIdOfRow(runRow('run-1', 'queued'))).toBe('run-1')
    expect(liveRunIdOfRow(activityFromRunView({ ...emptySpaceRunView('run-2'), status: 'live' }, 'room-1', tr, 0))).toBe('run-2')
  })

  it('names no run for a finished run, a schedule, or a conversation', () => {
    expect(liveRunIdOfRow(runRow('run-3', 'completed'))).toBeUndefined()
    expect(liveRunIdOfRow(activityFromRunView({ ...emptySpaceRunView('run-4'), status: 'ended', finished: { outcome: 'completed', reason: null } }, 'room-1', tr, 0))).toBeUndefined()
    expect(liveRunIdOfRow(must(activityFromSchedule({ id: 'cron-1', name: 'Daglig', enabled: true }, tr)))).toBeUndefined()
    expect(liveRunIdOfRow(buildSpaceActivity([{ thread_id: 't9', title: 'Samtale' }], tr)[0] as SpaceActivityItem)).toBeUndefined()
  })
})

describe('the Work and Activity panels pass the gate to their feed (WS09-S8)', () => {
  const runsAnswer = {
    space,
    membership,
    runs: [{ id: 'run-1', goal: 'Kjører nå', status: 'running', space_id: 'room-1', thread_id: 't1' }],
    schedules: [],
    unavailable: [],
  }

  it('the Work panel offers the Stop on a live run only when the gate is on', async () => {
    spacesClient.getSpaceWork.mockResolvedValue(runsAnswer)
    const { container } = render(() => <SpaceWorkPanel spaceRef="room-1" canStopRuns />)
    expect(await screen.findByText('Kjører nå')).toBeTruthy()
    expect(stopButtons(container)).toHaveLength(1)
  })

  it('the Work panel offers no Stop without the gate', async () => {
    spacesClient.getSpaceWork.mockResolvedValue(runsAnswer)
    const { container } = render(() => <SpaceWorkPanel spaceRef="room-1" />)
    expect(await screen.findByText('Kjører nå')).toBeTruthy()
    expect(stopButtons(container)).toHaveLength(0)
  })

  it('the Activity panel offers the Stop on a live run only when the gate is on', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue({ ...runsAnswer, approvals: [], operations: [], authority: [] })
    const { container } = render(() => (
      <SpaceActivityPanel spaceRef="room-1" threads={() => []} threadsLoading={() => false} canStopRuns />
    ))
    expect(await screen.findByText('Kjører nå')).toBeTruthy()
    expect(stopButtons(container)).toHaveLength(1)
  })

  it('the Activity panel offers no Stop without the gate', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue({ ...runsAnswer, approvals: [], operations: [], authority: [] })
    const { container } = render(() => <SpaceActivityPanel spaceRef="room-1" threads={() => []} threadsLoading={() => false} />)
    expect(await screen.findByText('Kjører nå')).toBeTruthy()
    expect(stopButtons(container)).toHaveLength(0)
  })
})
