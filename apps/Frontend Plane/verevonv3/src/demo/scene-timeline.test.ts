import { describe, expect, it } from 'vitest'
import { createSceneScheduler, easeOut, pointerAt } from './scene-timeline'
import type { SceneEvent, ScenePointerKeyframe, SceneTarget } from './scene-types'

function rect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    left, top, width, height,
    right: left + width, bottom: top + height, x: left, y: top,
    toJSON: () => ({}),
  } as DOMRect
}

describe('createSceneScheduler', () => {
  it('schedules every event with setTimeout relative to start(), in time order', () => {
    const scheduled: Array<{ ms: number; run: () => void }> = []
    const fired: string[] = []
    const events: SceneEvent[] = [
      { at: 900, kind: 'note', note: 'late' },
      { at: 0, kind: 'note', note: 'first' },
      { at: 300, kind: 'note', note: 'middle' },
    ]
    const scheduler = createSceneScheduler(
      events,
      (event) => fired.push(event.note),
      (run, ms) => { scheduled.push({ ms, run }); return scheduled.length },
      () => undefined,
    )
    expect(scheduled).toHaveLength(0)
    scheduler.start()
    expect(scheduled.map((entry) => entry.ms)).toEqual([0, 300, 900])
    for (const entry of scheduled) entry.run()
    expect(fired).toEqual(['first', 'middle', 'late'])
  })

  it('throws when started twice and clears timers on dispose', () => {
    const cleared: number[] = []
    const scheduler = createSceneScheduler(
      [{ at: 10, kind: 'note', note: 'x' }],
      () => undefined,
      () => 42,
      (id) => cleared.push(id),
    )
    scheduler.start()
    expect(() => scheduler.start()).toThrow(/already started/)
    scheduler.dispose()
    expect(cleared).toEqual([42])
  })
})

describe('easeOut', () => {
  it('is 1 - (1 - p)^3, clamped', () => {
    expect(easeOut(0)).toBe(0)
    expect(easeOut(1)).toBe(1)
    expect(easeOut(0.5)).toBeCloseTo(0.875)
    expect(easeOut(-1)).toBe(0)
    expect(easeOut(2)).toBe(1)
  })
})

describe('pointerAt', () => {
  const send: SceneTarget = { name: 'Send melding', role: 'button' }
  const approve: SceneTarget = { selector: '.approve' }
  const layout = new Map<SceneTarget, DOMRect | null>([
    [send, rect(900, 300, 40, 40)],
    [approve, rect(100, 500, 100, 36)],
  ])
  const resolve = (target: SceneTarget) => layout.get(target) ?? null
  const keyframes: ScenePointerKeyframe[] = [
    { at: 850, target: send, nudge: { x: -100, y: 0 }, visible: true, pressed: false },
    { at: 1000, target: send, nudge: { x: -100, y: 0 }, visible: true, pressed: false, ease: 'hold' },
    { at: 1350, target: send, visible: true, pressed: false },
    { at: 1380, target: send, visible: true, pressed: true },
    { at: 1440, target: send, visible: true, pressed: false },
    { at: 1650, target: send, visible: false, pressed: false },
    { at: 6350, target: approve, visible: true, pressed: false },
    { at: 7000, target: approve, visible: true, pressed: false, ease: 'hold' },
  ]

  it('is hidden before the first keyframe', () => {
    expect(pointerAt(keyframes, 0, resolve)).toEqual({ x: 820, y: 320, visible: false, pressed: false })
    expect(pointerAt(keyframes, 499, resolve).visible).toBe(false)
  })

  it('holds still between a keyframe and a hold keyframe', () => {
    expect(pointerAt(keyframes, 900, resolve)).toEqual({ x: 820, y: 320, visible: true, pressed: false })
  })

  it('eases out towards the next target, resolved from the layout', () => {
    const halfway = pointerAt(keyframes, 1175, resolve)
    expect(halfway.visible).toBe(true)
    expect(halfway.x).toBeCloseTo(820 + 100 * easeOut(0.5))
    expect(halfway.y).toBe(320)
    expect(pointerAt(keyframes, 1350, resolve)).toEqual({ x: 920, y: 320, visible: true, pressed: false })
  })

  it('steps pressed on and off at the keyframes', () => {
    expect(pointerAt(keyframes, 1379, resolve).pressed).toBe(false)
    expect(pointerAt(keyframes, 1380, resolve).pressed).toBe(true)
    expect(pointerAt(keyframes, 1439, resolve).pressed).toBe(true)
    expect(pointerAt(keyframes, 1440, resolve).pressed).toBe(false)
  })

  it('does not travel or resolve the next target while hidden', () => {
    const calls: SceneTarget[] = []
    const spy = (target: SceneTarget) => { calls.push(target); return resolve(target) }
    const hidden = pointerAt(keyframes, 4000, spy)
    expect(hidden).toEqual({ x: 920, y: 320, visible: false, pressed: false })
    expect(calls).not.toContain(approve)
  })

  it('reappears on the approval button and holds there', () => {
    expect(pointerAt(keyframes, 6350, resolve)).toEqual({ x: 150, y: 518, visible: true, pressed: false })
    expect(pointerAt(keyframes, 7900, resolve)).toEqual({ x: 150, y: 518, visible: true, pressed: false })
  })

  it('falls back to the previous point when a target is not in the layout yet', () => {
    const missing = (target: SceneTarget) => (target === approve ? null : resolve(target))
    const at = pointerAt(keyframes, 6500, missing)
    expect(at.visible).toBe(true)
    expect(at).toMatchObject({ x: 0, y: 0 })
  })

  it('is a pure function of time and layout', () => {
    const a = pointerAt(keyframes, 1200, resolve)
    const b = pointerAt(keyframes, 1200, resolve)
    expect(a).toEqual(b)
  })
})
