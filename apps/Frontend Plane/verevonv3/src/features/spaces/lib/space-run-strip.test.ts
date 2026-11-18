import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  EMPTY_STRIP,
  STRIP_MIN_DISPLAY_MS,
  STRIP_ROTATION_MS,
  advanceStrip,
  type StripObservation,
} from './space-run-strip'

// WS09-S6: when a running agent's name may appear in the composer strip, and how fast its words may
// change. The rules are pure; time comes from the fake clock, and each step passes `Date.now()`.

const observation = (runId: string, overrides: Partial<StripObservation> = {}): StripObservation => ({
  runId,
  threadId: `thread-${runId}`,
  committed: true,
  finished: false,
  headline: 'Searching the web',
  ...overrides,
})

const later = (ms: number) => vi.advanceTimersByTime(ms)

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(1_700_000_000_000)
})

afterEach(() => {
  vi.useRealTimers()
})

describe('the composer strip: entering (WS09-S6)', () => {
  it('lets a run in only once it has committed a step or progress frame', () => {
    const attached = advanceStrip(EMPTY_STRIP, [observation('run-1', { committed: false })], Date.now())
    expect(attached.state.entries).toEqual([])
    expect(attached.lines).toEqual([])
    expect(attached.nextWakeAt).toBeNull()

    later(3_000)
    const committed = advanceStrip(attached.state, [observation('run-1')], Date.now())
    expect(committed.state.entries.map((entry) => entry.runId)).toEqual(['run-1'])
    expect(committed.lines).toEqual([])
  })

  it('does not draw a committed run before 800 ms, and draws it at 800 ms', () => {
    expect(STRIP_MIN_DISPLAY_MS).toBe(800)
    const start = advanceStrip(EMPTY_STRIP, [observation('run-1')], Date.now())
    expect(start.lines).toEqual([])
    expect(start.nextWakeAt).toBe(Date.now() + 800)

    later(799)
    const almost = advanceStrip(start.state, [observation('run-1')], Date.now())
    expect(almost.lines).toEqual([])
    expect(almost.nextWakeAt).toBe(Date.now() + 1)

    later(1)
    const shown = advanceStrip(almost.state, [observation('run-1')], Date.now())
    expect(shown.lines).toEqual([{ runId: 'run-1', threadId: 'thread-run-1', headline: 'Searching the web' }])
    expect(shown.nextWakeAt).toBeNull()
  })
})

describe('the composer strip: rotating the words (WS09-S6)', () => {
  it('changes the headline no faster than every 2.2 s, counted from the last change', () => {
    expect(STRIP_ROTATION_MS).toBe(2_200)
    let step = advanceStrip(EMPTY_STRIP, [observation('run-1')], Date.now())
    later(800)
    step = advanceStrip(step.state, [observation('run-1')], Date.now())
    const shownAt = Date.now()
    expect(step.lines[0]?.headline).toBe('Searching the web')

    later(100)
    step = advanceStrip(step.state, [observation('run-1', { headline: 'Running code' })], Date.now())
    expect(step.lines[0]?.headline).toBe('Searching the web')
    expect(step.nextWakeAt).toBe(shownAt + 2_200)

    later(2_099)
    step = advanceStrip(step.state, [observation('run-1', { headline: 'Running code' })], Date.now())
    expect(step.lines[0]?.headline).toBe('Searching the web')

    later(1)
    step = advanceStrip(step.state, [observation('run-1', { headline: 'Running code' })], Date.now())
    expect(step.lines[0]?.headline).toBe('Running code')
    expect(step.nextWakeAt).toBeNull()

    // The next change is counted from the change just made, not from the first appearance.
    later(100)
    step = advanceStrip(step.state, [observation('run-1', { headline: 'Checking' })], Date.now())
    expect(step.lines[0]?.headline).toBe('Running code')
    expect(step.nextWakeAt).toBe(shownAt + 2_200 + 2_200)
  })
})

describe('the composer strip: latching and leaving (WS09-S6)', () => {
  it('keeps a shown run when its commit flips back to false, as a re-attach empties its view', () => {
    let step = advanceStrip(EMPTY_STRIP, [observation('run-1')], Date.now())
    later(800)
    step = advanceStrip(step.state, [observation('run-1')], Date.now())
    expect(step.lines).toHaveLength(1)

    later(500)
    step = advanceStrip(step.state, [observation('run-1', { committed: false })], Date.now())
    expect(step.state.entries.map((entry) => entry.runId)).toEqual(['run-1'])
    expect(step.lines).toEqual([{ runId: 'run-1', threadId: 'thread-run-1', headline: 'Searching the web' }])
  })

  it('holds a run still waiting to appear through the same flip, and draws it when its time comes', () => {
    let step = advanceStrip(EMPTY_STRIP, [observation('run-1')], Date.now())
    later(300)
    step = advanceStrip(step.state, [observation('run-1', { committed: false })], Date.now())
    expect(step.state.entries.map((entry) => entry.runId)).toEqual(['run-1'])
    expect(step.lines).toEqual([])
    expect(step.nextWakeAt).toBe(Date.now() + 500)

    later(500)
    step = advanceStrip(step.state, [observation('run-1', { committed: false })], Date.now())
    expect(step.lines.map((line) => line.runId)).toEqual(['run-1'])
  })

  it('removes a run at once when it has finished, and when it is missing from the observations', () => {
    let step = advanceStrip(EMPTY_STRIP, [observation('run-1'), observation('run-2')], Date.now())
    later(800)
    step = advanceStrip(step.state, [observation('run-1'), observation('run-2')], Date.now())
    expect(step.lines.map((line) => line.runId)).toEqual(['run-1', 'run-2'])

    step = advanceStrip(step.state, [observation('run-1', { finished: true }), observation('run-2')], Date.now())
    expect(step.state.entries.map((entry) => entry.runId)).toEqual(['run-2'])
    expect(step.lines.map((line) => line.runId)).toEqual(['run-2'])

    step = advanceStrip(step.state, [], Date.now())
    expect(step.state.entries).toEqual([])
    expect(step.lines).toEqual([])
    expect(step.nextWakeAt).toBeNull()
  })
})

describe('the composer strip: when to look again (WS09-S6)', () => {
  it('reports the earliest pending boundary, whichever run it belongs to', () => {
    const start = Date.now()
    let step = advanceStrip(EMPTY_STRIP, [observation('run-1')], start)
    expect(step.nextWakeAt).toBe(start + 800)

    later(300)
    step = advanceStrip(step.state, [observation('run-1'), observation('run-2')], Date.now())
    // run-2 would appear at start + 1100; run-1 at start + 800 is still the earliest.
    expect(step.nextWakeAt).toBe(start + 800)

    later(500)
    step = advanceStrip(step.state, [observation('run-1'), observation('run-2')], Date.now())
    expect(step.lines.map((line) => line.runId)).toEqual(['run-1'])
    expect(step.nextWakeAt).toBe(start + 1_100)

    // run-1 wants new words, which may not show before start + 3000; run-2's appearance at 1100 is still earlier.
    later(100)
    step = advanceStrip(
      step.state,
      [observation('run-1', { headline: 'Running code' }), observation('run-2')],
      Date.now(),
    )
    expect(step.nextWakeAt).toBe(start + 1_100)

    later(200)
    step = advanceStrip(
      step.state,
      [observation('run-1', { headline: 'Running code' }), observation('run-2')],
      Date.now(),
    )
    expect(step.lines.map((line) => line.runId)).toEqual(['run-1', 'run-2'])
    expect(step.lines[0]?.headline).toBe('Searching the web')
    expect(step.nextWakeAt).toBe(start + 3_000)
  })
})
