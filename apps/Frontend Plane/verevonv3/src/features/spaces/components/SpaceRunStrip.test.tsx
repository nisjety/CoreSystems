import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { emptySpaceRunView, type SpaceRunView } from '../lib/space-run-view'

// The strip reads each run's view from the shared run store and keeps a live attach for its own life.
// The store's stream is covered by lib/space-run-store.test.ts, so here the store is stubbed at the module
// boundary and the views are set by the test.
const runViews = vi.hoisted(() => new Map<string, SpaceRunView>())
vi.mock('../lib/space-run-store', () => ({
  spaceRunStore: {
    view: (runId: string) => runViews.get(runId),
    views: () => [...runViews.values()],
    attach: () => () => {},
    dispose: () => {},
  },
  trackLiveRuns: () => {},
}))

const { SpaceRunStrip } = await import('./SpaceRunStrip')

// No I18nProvider wraps these renders, so the copy is the default Norwegian.
const SPACE = 'space-1'
const START = Date.parse('2026-11-18T10:00:00Z')

/** A run that has committed: one running step has been received. */
function committedView(runId: string, stepClass: 'web' | 'code' = 'web'): SpaceRunView {
  return {
    ...emptySpaceRunView(runId),
    status: 'live',
    steps: [{ id: `${runId}-step`, class: stepClass, state: 'running' }],
  }
}

function thread(threadId: string, runId: string, title: string): SpaceThread {
  return { thread_id: threadId, space_id: SPACE, title, latest_run_id: runId, latest_run_status: 'running' }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(START)
})

afterEach(() => {
  cleanup()
  runViews.clear()
  vi.useRealTimers()
})

describe('SpaceRunStrip (WS09-S6)', () => {
  it('draws nothing before 800 ms, then the run line appears at 800 ms', async () => {
    runViews.set('run-1', committedView('run-1'))
    render(() => (
      <SpaceRunStrip spaceRef={SPACE} threads={() => [thread('t-1', 'run-1', 'Innkjøp')]} onOpenRun={() => {}} />
    ))
    flush()
    expect(screen.queryByRole('status')).toBeNull()

    await vi.advanceTimersByTimeAsync(799)
    flush()
    expect(screen.queryByRole('status')).toBeNull()

    await vi.advanceTimersByTimeAsync(1)
    flush()
    const strip = screen.getByRole('status')
    expect(strip.getAttribute('aria-live')).toBe('polite')
    expect(strip.textContent).toContain('Søker på nettet')
    expect(strip.textContent).toContain('Innkjøp')
  })

  it('keeps the line on screen once it has appeared, without remounting it', async () => {
    runViews.set('run-1', committedView('run-1'))
    render(() => (
      <SpaceRunStrip spaceRef={SPACE} threads={() => [thread('t-1', 'run-1', 'Innkjøp')]} onOpenRun={() => {}} />
    ))
    flush()
    await vi.advanceTimersByTimeAsync(800)
    flush()
    const strip = screen.getByRole('status')

    await vi.advanceTimersByTimeAsync(1_000)
    flush()
    expect(screen.getByRole('status')).toBe(strip)
    expect(within(strip).getAllByRole('button')).toHaveLength(1)
  })

  it('pressing the line calls onOpenRun with its thread, and without a handler the line cannot be pressed', async () => {
    const onOpenRun = vi.fn()
    runViews.set('run-1', committedView('run-1'))
    render(() => (
      <SpaceRunStrip spaceRef={SPACE} threads={() => [thread('t-1', 'run-1', 'Innkjøp')]} onOpenRun={onOpenRun} />
    ))
    flush()
    await vi.advanceTimersByTimeAsync(800)
    flush()

    fireEvent.click(screen.getByRole('button', { name: /Søker på nettet/ }))
    expect(onOpenRun).toHaveBeenCalledTimes(1)
    expect(onOpenRun).toHaveBeenCalledWith('t-1')
  })

  it('without an open handler the line is shown but disabled', async () => {
    runViews.set('run-1', committedView('run-1'))
    render(() => <SpaceRunStrip spaceRef={SPACE} threads={() => [thread('t-1', 'run-1', 'Innkjøp')]} />)
    flush()
    await vi.advanceTimersByTimeAsync(800)
    flush()
    expect((screen.getByRole('button', { name: /Søker på nettet/ }) as HTMLButtonElement).disabled).toBe(true)
  })

  it.each(['unavailable', 'waiting'] as const)(
    'a %s live view degrades to the plain Working headline, whatever steps it had',
    async (status) => {
      runViews.set('run-1', { ...committedView('run-1'), status })
      render(() => (
        <SpaceRunStrip spaceRef={SPACE} threads={() => [thread('t-1', 'run-1', 'Innkjøp')]} onOpenRun={() => {}} />
      ))
      flush()
      await vi.advanceTimersByTimeAsync(800)
      flush()
      const strip = screen.getByRole('status')
      expect(strip.textContent).toContain('Jobber')
      expect(strip.textContent).not.toContain('Søker på nettet')
    },
  )

  it('names the first two runs and folds the rest into "og N til"', async () => {
    runViews.set('run-1', committedView('run-1'))
    runViews.set('run-2', committedView('run-2', 'code'))
    runViews.set('run-3', committedView('run-3'))
    render(() => (
      <SpaceRunStrip
        spaceRef={SPACE}
        threads={() => [thread('t-1', 'run-1', 'Innkjøp'), thread('t-2', 'run-2', 'Drift'), thread('t-3', 'run-3', 'Kundesak')]}
        onOpenRun={() => {}}
      />
    ))
    flush()
    await vi.advanceTimersByTimeAsync(800)
    flush()
    const strip = screen.getByRole('status')
    expect(within(strip).getAllByRole('button')).toHaveLength(2)
    expect(strip.textContent).toContain('Innkjøp')
    expect(strip.textContent).toContain('Drift')
    expect(strip.textContent).not.toContain('Kundesak')
    expect(strip.textContent).toContain('og 1 til')
  })
})
