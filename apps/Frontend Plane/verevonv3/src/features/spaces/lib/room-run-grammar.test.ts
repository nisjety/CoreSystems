import { describe, expect, it } from 'vitest'

import type { RoomRunStepFrame } from '@/shared/api/space-run-live-client'
import {
  ROOM_RUN_STEP_CLASSES,
  type RoomRunStepState,
  type SpaceRunStoredStepState,
  type SpaceRunWorkStep,
} from '@/shared/api/spaces-client/run-work'

import { runHeadline, stepDoing, withLiveRunItems, workSummaryLine } from './room-run-grammar'
import type { SpaceActivityItem } from './activity-grammar'
import { emptySpaceRunView, type SpaceRunView } from './space-run-view'

// WS09-S5: the words for a live room run. The translator is a plain pair so each assertion names the
// language it reads in. Nothing here takes free text from a frame; the classes below are the closed set.
const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

const NOW = 1_700_000_000_000

/** A step as a live view holds it: it may still be running. */
const liveStep = (id: string, stepClass: RoomRunStepFrame['class'], state: RoomRunStepState): RoomRunStepFrame => ({
  id,
  class: stepClass,
  state,
})

/** A step as the stored summary holds it: settled only. */
const storedStep = (id: string, stepClass: SpaceRunWorkStep['class'], state: SpaceRunStoredStepState): SpaceRunWorkStep => ({
  id,
  class: stepClass,
  state,
})

const polledRow = (id: string, object: string): SpaceActivityItem => ({
  id,
  renderClass: 'run',
  verb: 'Run',
  object,
  outcome: { label: 'Polled label', tone: 'pending', live: false },
  salience: 'normal',
})

const liveRun = (runId: string, steps: readonly RoomRunStepFrame[]): SpaceRunView => ({
  ...emptySpaceRunView(runId),
  status: 'live',
  threadId: 'thread-1',
  agentName: 'Assistant',
  startedAt: NOW / 1000 - 12,
  steps,
})

describe('the words for a step (WS09-S5)', () => {
  it('has a Norwegian and an English phrase for every step class, and no two classes share an English phrase', () => {
    const english = ROOM_RUN_STEP_CLASSES.map((stepClass) => stepDoing(stepClass, en))
    for (const stepClass of ROOM_RUN_STEP_CLASSES) {
      const norwegian = stepDoing(stepClass, no)
      expect(norwegian.length).toBeGreaterThan(0)
      expect(stepDoing(stepClass, en).length).toBeGreaterThan(0)
      expect(norwegian).not.toBe(stepDoing(stepClass, en))
    }
    expect(new Set(english).size).toBe(ROOM_RUN_STEP_CLASSES.length)
  })

  it('reads a sample of classes in English and in Norwegian', () => {
    expect(stepDoing('web', en)).toBe('Searching the web')
    expect(stepDoing('web', no)).toBe('Søker på nettet')
    expect(stepDoing('lookup', en)).toBe('Looking something up')
    expect(stepDoing('tool', en)).toBe('Using a tool')
  })
})

describe('runHeadline (WS09-S5)', () => {
  it('names what the newest running step is', () => {
    const headline = runHeadline(
      {
        steps: [liveStep('a', 'web', 'ok'), liveStep('b', 'code', 'running'), liveStep('c', 'lookup', 'running')],
      },
      en,
    )
    expect(headline).toBe('Looking something up')
  })

  it('falls back to the last step when none is running', () => {
    expect(runHeadline({ steps: [liveStep('a', 'web', 'ok'), liveStep('b', 'code', 'ok')] }, en)).toBe('Running code')
  })

  it('says a run waiting for approval so, before its steps', () => {
    expect(runHeadline({ phase: 'awaiting_approval', steps: [liveStep('a', 'effect', 'running')] }, en)).toBe(
      'Waiting for approval',
    )
    expect(runHeadline({ phase: 'awaiting_approval', steps: [] }, no)).toBe('Venter på godkjenning')
  })

  it('says Working when there is no step yet', () => {
    expect(runHeadline({ steps: [] }, en)).toBe('Working')
    expect(runHeadline({ steps: [] }, no)).toBe('Jobber')
  })
})

describe('workSummaryLine (WS09-S5)', () => {
  const steps = (count: number): SpaceRunWorkStep[] =>
    Array.from({ length: count }, (_, index) => storedStep(`s${index}`, 'web', 'ok'))

  it('words zero, one and several tools, in both languages', () => {
    expect(workSummaryLine({ steps: [], steer: 0 }, en)).toBe('Used no tools')
    expect(workSummaryLine({ steps: [], steer: 0 }, no)).toBe('Brukte ingen verktøy')
    expect(workSummaryLine({ steps: steps(1), steer: 0 }, en)).toBe('Used 1 tool')
    expect(workSummaryLine({ steps: steps(1), steer: 0 }, no)).toBe('Brukte 1 verktøy')
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, en)).toBe('Used 4 tools')
  })

  it('counts the steps it is told to, when the live view holds more than the summary keeps', () => {
    expect(workSummaryLine({ steps: steps(1), steer: 0 }, en, undefined, 4)).toBe('Used 4 tools')
  })

  it('adds the time in seconds, or in minutes and seconds, when it is known', () => {
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, en, 12)).toBe('Used 4 tools in 12 s')
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, no, 12)).toBe('Brukte 4 verktøy på 12 s')
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, en, 65)).toBe('Used 4 tools in 1 min 5 s')
  })

  it('leaves the time out when it is not known or is zero', () => {
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, en)).toBe('Used 4 tools')
    expect(workSummaryLine({ steps: steps(4), steer: 0 }, en, 0)).toBe('Used 4 tools')
  })

  it('counts the owner steers, singular and plural, in both languages', () => {
    expect(workSummaryLine({ steps: steps(4), steer: 1 }, en, 12)).toBe('Used 4 tools in 12 s, 1 steer from the owner')
    expect(workSummaryLine({ steps: steps(4), steer: 3 }, en, 12)).toBe('Used 4 tools in 12 s, 3 steers from the owner')
    expect(workSummaryLine({ steps: steps(4), steer: 1 }, no, 12)).toBe('Brukte 4 verktøy på 12 s, 1 styring fra eieren')
    expect(workSummaryLine({ steps: steps(4), steer: 3 }, no, 12)).toBe('Brukte 4 verktøy på 12 s, 3 styringer fra eieren')
    expect(workSummaryLine({ steps: [], steer: 2 }, en)).toBe('Used no tools, 2 steers from the owner')
  })
})

describe('withLiveRunItems (WS09-S5)', () => {
  const polled = [
    polledRow('approval-1', 'Kari'),
    polledRow('work-run:run-1', 'Assistant'),
    polledRow('work-run:run-2', 'Ola'),
  ]

  it('replaces the polled row with the same id by the live row, and keeps the other rows in their order', () => {
    const merged = withLiveRunItems(
      polled,
      [liveRun('run-1', [liveStep('s1', 'web', 'running')])],
      new Set(['run-1', 'run-2']),
      'room-a',
      en,
      NOW,
    )

    const runRows = merged.filter((item) => item.id === 'work-run:run-1')
    expect(runRows).toHaveLength(1)
    expect(runRows[0]).toEqual(
      expect.objectContaining({
        object: 'Assistant',
        outcome: { label: 'Searching the web', tone: 'pending', live: true },
      }),
    )
    expect(merged.filter((item) => item.id !== 'work-run:run-1').map((item) => item.id)).toEqual([
      'approval-1',
      'work-run:run-2',
    ])
  })

  it('ignores a live run the listing did not name, and returns the rows unchanged when none is named', () => {
    const merged = withLiveRunItems(
      polled,
      [liveRun('run-9', [liveStep('s1', 'web', 'running')])],
      new Set(['run-1', 'run-2']),
      'room-a',
      en,
      NOW,
    )
    expect(merged.map((item) => item.id)).not.toContain('work-run:run-9')
    expect(merged).toEqual(polled)

    const withOwnRun = withLiveRunItems(
      polled,
      [liveRun('run-9', [liveStep('s1', 'web', 'running')]), liveRun('run-1', [liveStep('s1', 'web', 'running')])],
      new Set(['run-1']),
      'room-a',
      en,
      NOW,
    )
    expect(withOwnRun.map((item) => item.id)).not.toContain('work-run:run-9')
    expect(withOwnRun.filter((item) => item.id === 'work-run:run-1')).toHaveLength(1)
  })
})
