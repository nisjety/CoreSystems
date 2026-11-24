import { describe, expect, it } from 'vitest'

import type { RoomRunFrame, RoomRunSnapshot, RoomRunStepFrame } from '@/shared/api/space-run-live-client'
import { ROOM_RUN_MAX_STEPS } from '@/shared/api/spaces-client/run-work'

import { applyRoomRunFrame, emptySpaceRunView, totalSteps, type SpaceRunView } from './space-run-view'

// WS09-S5: the reducer behind a member's run card. Pure: frames in, the next view out. Text offsets are
// UTF-16 code units, so the emoji below is two positions, not one.
const SMILE = '\u{1F600}'

const snapshotBody = (overrides: Partial<RoomRunSnapshot> = {}): RoomRunSnapshot => ({
  runId: 'run-1',
  threadId: 'thread-1',
  startedBy: 'Kari',
  agentName: 'Assistant',
  phase: 'running',
  startedAt: 1_700_000_000,
  text: { offset: 0, value: '', truncated: false, visible: true },
  steps: [],
  steer: 0,
  plan: null,
  artifacts: [],
  expiresAt: null,
  ...overrides,
})

const snapshot = (overrides: Partial<RoomRunSnapshot> = {}): RoomRunFrame => ({
  type: 'snapshot',
  snapshot: snapshotBody(overrides),
})

const text = (offset: number, delta: string): RoomRunFrame => ({ type: 'text', offset, delta })

const step = (id: string, state: RoomRunStepFrame['state'], stepClass: RoomRunStepFrame['class'] = 'web'): RoomRunFrame => ({
  type: 'step',
  step: { id, class: stepClass, state },
})

const fold = (view: SpaceRunView, frames: readonly RoomRunFrame[]): SpaceRunView =>
  frames.reduce((current, frame) => applyRoomRunFrame(current, frame), view)

const liveView = (overrides: Partial<RoomRunSnapshot> = {}): SpaceRunView =>
  fold(emptySpaceRunView('run-1'), [snapshot(overrides)])

describe('applyRoomRunFrame: a snapshot (WS09-S5)', () => {
  it('replaces what the view held, clears the flags a gap or reauth left, and makes the view live', () => {
    const before = fold(emptySpaceRunView('run-1'), [
      snapshot({
        steps: [{ id: 'a', class: 'web', state: 'running' }],
        text: { offset: 0, value: 'old text', truncated: false, visible: true },
      }),
      text(8, 'X'),
      text(50, 'late'),
      { type: 'reauth_required' },
      { type: 'finished', outcome: 'completed', reason: null },
    ])
    expect(before.needsReattach).toBe(true)
    expect(before.reauthRequired).toBe(true)
    expect(before.finished).not.toBeNull()

    const after = applyRoomRunFrame(
      before,
      snapshot({
        agentName: 'Ola',
        steps: [{ id: 'b', class: 'code', state: 'ok' }],
        steer: 1,
        text: { offset: 4, value: 'new', truncated: false, visible: true },
      }),
    )

    expect(after.status).toBe('live')
    expect(after.agentName).toBe('Ola')
    expect(after.steps).toEqual([{ id: 'b', class: 'code', state: 'ok' }])
    expect(after.stepsDropped).toBe(0)
    expect(after.steer).toBe(1)
    expect(after.text.value).toBe('new')
    expect(after.text.length).toBe(7)
    expect(after.needsReattach).toBe(false)
    expect(after.reauthRequired).toBe(false)
    expect(after.finished).toBeNull()
  })
})

describe('applyRoomRunFrame: the text stream (WS09-S5)', () => {
  it('ignores text the view already holds, and hands back the same view', () => {
    const view = liveView({ text: { offset: 0, value: 'hello', truncated: false, visible: true } })
    expect(applyRoomRunFrame(view, text(0, 'hel'))).toBe(view)
    expect(applyRoomRunFrame(view, text(2, 'llo'))).toBe(view)
  })

  it('appends only the new tail of a frame that overlaps the held text', () => {
    const view = liveView({ text: { offset: 0, value: 'hello', truncated: false, visible: true } })
    const next = applyRoomRunFrame(view, text(3, 'lo world'))
    expect(next.text.value).toBe('hello world')
    expect(next.text.length).toBe(11)
    expect(next.needsReattach).toBe(false)
  })

  it('reads a frame that starts past the held text as a gap: the view asks for a fresh attach and keeps its text', () => {
    const view = liveView({ text: { offset: 0, value: 'hello', truncated: false, visible: true } })
    const gapped = applyRoomRunFrame(view, text(9, 'x'))
    expect(gapped.needsReattach).toBe(true)
    expect(gapped.text.value).toBe('hello')
    expect(gapped.text.length).toBe(5)
  })

  it('counts an emoji as two UTF-16 positions when it judges a repeat, a continuation and a gap', () => {
    const view = liveView({ text: { offset: 0, value: SMILE, truncated: false, visible: true } })
    expect(view.text.length).toBe(2)

    // Offset 1 is inside the pair, and everything up to 2 is already held: a repeat, not new text.
    expect(applyRoomRunFrame(view, text(1, 'a'))).toBe(view)

    // Offset 2 is exactly the end of the held text: contiguous, so it appends.
    const continued = applyRoomRunFrame(view, text(2, 'a'))
    expect(continued.text.value).toBe(`${SMILE}a`)
    expect(continued.text.length).toBe(3)
    expect(continued.needsReattach).toBe(false)

    // Offset 4 starts past the three held positions: a gap.
    const gapped = applyRoomRunFrame(continued, text(4, 'b'))
    expect(gapped.needsReattach).toBe(true)
    expect(gapped.text.value).toBe(`${SMILE}a`)
  })
})

describe('applyRoomRunFrame: steps (WS09-S5)', () => {
  it('upserts a step by id, so a running step settles in place rather than appearing twice', () => {
    let view = liveView()
    view = applyRoomRunFrame(view, step('s1', 'running'))
    view = applyRoomRunFrame(view, step('s2', 'running', 'code'))
    view = applyRoomRunFrame(view, step('s1', 'ok'))

    expect(view.steps.map((entry) => entry.id)).toEqual(['s1', 's2'])
    expect(view.steps[0]).toEqual({ id: 's1', class: 'web', state: 'ok' })
    expect(view.steps[1]).toEqual({ id: 's2', class: 'code', state: 'running' })
  })

  it('does not reopen a settled step when a late running repeat arrives', () => {
    const view = fold(liveView(), [step('s1', 'ok'), step('s1', 'running')])
    expect(view.steps).toHaveLength(1)
    expect(view.steps[0]?.state).toBe('ok')
  })

  it('keeps the newest steps at the cap and counts the ones dropped from the front', () => {
    const total = ROOM_RUN_MAX_STEPS + 2
    let view = liveView()
    for (let index = 0; index < total; index += 1) {
      view = applyRoomRunFrame(view, step(`step-${index}`, 'ok'))
    }
    expect(view.steps).toHaveLength(ROOM_RUN_MAX_STEPS)
    expect(view.steps[0]?.id).toBe('step-2')
    expect(view.stepsDropped).toBe(2)
    expect(totalSteps(view)).toBe(total)
  })
})

describe('applyRoomRunFrame: a run that waited for approval moves again (WS10-S1)', () => {
  const waiting = () =>
    liveView({ phase: 'awaiting_approval', steps: [{ id: 's1', class: 'effect', state: 'running' }] })

  it('keeps awaiting_approval from the snapshot until the run shows progress', () => {
    expect(waiting().phase).toBe('awaiting_approval')
  })

  it('reads a repeat of the open step in the same state as no progress', () => {
    expect(applyRoomRunFrame(waiting(), step('s1', 'running', 'effect')).phase).toBe('awaiting_approval')
  })

  it('gives way to running when the waiting step settles', () => {
    expect(applyRoomRunFrame(waiting(), step('s1', 'ok', 'effect')).phase).toBe('running')
  })

  it('gives way to running when a new step starts', () => {
    expect(applyRoomRunFrame(waiting(), step('s2', 'running', 'web')).phase).toBe('running')
  })

  it('gives way to running when new text arrives, but not on a repeat of held text', () => {
    const view = liveView({ phase: 'awaiting_approval', text: { offset: 0, value: 'hi', truncated: false, visible: true } })
    expect(applyRoomRunFrame(view, text(0, 'hi'))).toBe(view)
    expect(applyRoomRunFrame(view, text(2, ' there')).phase).toBe('running')
  })

  it('gives way to running when the hidden text grows', () => {
    const view = liveView({ phase: 'awaiting_approval', text: { offset: 0, value: '', truncated: false, visible: false } })
    expect(applyRoomRunFrame(view, { type: 'text_progress', chars: 0 })).toBe(view)
    expect(applyRoomRunFrame(view, { type: 'text_progress', chars: 12 }).phase).toBe('running')
  })

  it('leaves any other phase alone', () => {
    expect(applyRoomRunFrame(liveView(), step('s2', 'running')).phase).toBe('running')
  })
})

describe('applyRoomRunFrame: the end of a run (WS09-S5)', () => {
  it('is final: every later frame hands back the same view', () => {
    const done = fold(liveView(), [{ type: 'finished', outcome: 'completed', reason: null }])
    expect(done.status).toBe('ended')
    expect(done.phase).toBe('completed')
    expect(done.finished).toEqual({ outcome: 'completed', reason: null })

    const later: RoomRunFrame[] = [
      text(0, 'more'),
      step('s9', 'running'),
      { type: 'steer', count: 3 },
      { type: 'artifact', id: 'art-1', artifactKind: 'doc' },
      { type: 'finished', outcome: 'failed', reason: 'budget' },
      { type: 'reauth_required' },
    ]
    for (const frame of later) {
      expect(applyRoomRunFrame(done, frame)).toBe(done)
    }
  })

  it('names the phase from the outcome: a stop reads as cancelled, a failure as failed with its reason', () => {
    const stopped = applyRoomRunFrame(liveView(), { type: 'finished', outcome: 'stopped', reason: null })
    expect(stopped.phase).toBe('cancelled')

    const failed = applyRoomRunFrame(liveView(), { type: 'finished', outcome: 'failed', reason: 'budget' })
    expect(failed.phase).toBe('failed')
    expect(failed.finished).toEqual({ outcome: 'failed', reason: 'budget' })
  })
})

describe('applyRoomRunFrame: reauth and errors (WS09-S5)', () => {
  it('marks the view for a renewed attach without changing its status', () => {
    const reauth = applyRoomRunFrame(liveView(), { type: 'reauth_required' })
    expect(reauth.reauthRequired).toBe(true)
    expect(reauth.status).toBe('live')
  })

  it.each([true, false])('makes the view unavailable with the typed code when an error arrives (retryable %s), keeping its steps', (retryable) => {
    const live = fold(liveView(), [step('s1', 'ok')])
    const down = applyRoomRunFrame(live, { type: 'error', code: 'run_stream_expired', retryable, feature: null })
    expect(down.status).toBe('unavailable')
    expect(down.unavailableCode).toBe('run_stream_expired')
    expect(down.steps).toEqual(live.steps)
  })

  it('goes back to live when a fresh snapshot arrives after an error', () => {
    const down = applyRoomRunFrame(liveView(), { type: 'error', code: 'run_stream_expired', retryable: true, feature: null })
    const back = applyRoomRunFrame(down, snapshot())
    expect(back.status).toBe('live')
    expect(back.unavailableCode).toBeNull()
  })
})
