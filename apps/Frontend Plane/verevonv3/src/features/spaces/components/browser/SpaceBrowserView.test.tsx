import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceBrowserRunHeader, SpaceBrowserTimeline } from '@/shared/api/spaces-client/browser-runs'

// The pane's reads and the room's Stop route are answered here, so no request leaves the test.
const browserRuns = vi.hoisted(() => ({ readSpaceBrowserTimeline: vi.fn() }))
vi.mock('@/shared/api/spaces-client/browser-runs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client/browser-runs')>()),
  ...browserRuns,
}))
const runStop = vi.hoisted(() => ({ stopSpaceRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/run-stop', () => ({ stopSpaceRun: runStop.stopSpaceRun }))

const { SpaceBrowserView } = await import('./SpaceBrowserView')

const SPACE = 'room-a'
const RUN_ID = 'run_1'

function header(phase: string, finishedAt: string | null = null): SpaceBrowserRunHeader {
  return {
    runId: RUN_ID,
    threadId: 'thr_1',
    startedBySubjectId: 'sub_kari',
    phase,
    startedAt: '2026-11-19T09:00:00Z',
    finishedAt,
    controlMode: 'agent',
    handoffOpen: false,
    viewScope: 'owner',
  }
}

const answer = (run: SpaceBrowserRunHeader): SpaceBrowserTimeline => ({
  run,
  events: [],
  nextEventId: null,
  validUntil: '2026-11-19T09:05:00Z',
})

function mountView(canStop: boolean | undefined) {
  render(() => (
    <SpaceBrowserView
      spaceRef={SPACE}
      runId={() => RUN_ID}
      viewerSubjectId="sub_ola"
      nameOf={() => 'Kari'}
      canStop={canStop}
    />
  ))
  flush()
}

const stopButton = () => screen.queryByRole('button', { name: 'Stopp kjøringen' })
const settle = async () => {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  browserRuns.readSpaceBrowserTimeline.mockReset()
  runStop.stopSpaceRun.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceBrowserView: Stop in the pane (WS20, WS09-S8)', () => {
  it('is offered to a viewer who may stop, on a live run, and stops this run', async () => {
    browserRuns.readSpaceBrowserTimeline.mockResolvedValue(answer(header('running')))
    runStop.stopSpaceRun.mockResolvedValue({ runId: RUN_ID, status: 'cancelled', cancelled: true, receiptId: 'rcpt_1' })
    mountView(true)
    await settle()

    fireEvent.click(stopButton() as HTMLElement)
    await settle()
    expect(runStop.stopSpaceRun).toHaveBeenCalledWith(SPACE, RUN_ID, expect.stringMatching(/^stop-/))
    expect(screen.getByText('Stopp sendt …')).toBeTruthy()
  })

  it('is not offered to a viewer who may not, nor once the run has ended', async () => {
    browserRuns.readSpaceBrowserTimeline.mockResolvedValue(answer(header('running')))
    mountView(false)
    await settle()
    expect(screen.getByText('Kjører')).toBeTruthy()
    expect(stopButton()).toBeNull()
    cleanup()

    browserRuns.readSpaceBrowserTimeline.mockResolvedValue(answer(header('completed', '2026-11-19T09:02:00Z')))
    mountView(true)
    await settle()
    expect(screen.getByText('Fullført')).toBeTruthy()
    expect(stopButton()).toBeNull()
    expect(runStop.stopSpaceRun).not.toHaveBeenCalled()
  })
})
