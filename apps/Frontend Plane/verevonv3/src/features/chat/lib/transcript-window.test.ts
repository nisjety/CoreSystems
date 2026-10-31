import { describe, expect, it } from 'vitest'
import {
  DEFAULT_TURN_HEIGHT_PX,
  OVERSCAN_PX,
  TAIL_TURN_COUNT,
  type TranscriptWindow,
  isChatVirtualizeEnabled,
  nextFollowState,
  planTranscriptWindow,
  resolveTurnHeights,
} from './transcript-window'

/**
 * Lays the plan out the way the page's flex column does: every mounted turn and
 * every spacer is a child, with the row gap between consecutive children. The
 * result is where each mounted turn starts and where the column ends, to compare
 * with a fully mounted transcript.
 */
function layOut(plan: TranscriptWindow, heights: readonly number[], rowGap: number) {
  const children: Array<{ index?: number; height: number }> = []
  for (const item of plan.items) {
    if (item.gapBefore > 0) children.push({ height: item.gapBefore })
    children.push({ index: item.index, height: heights[item.index] ?? 0 })
  }
  if (plan.gapAfter > 0) children.push({ height: plan.gapAfter })
  const tops = new Map<number, number>()
  let top = 0
  for (const child of children) {
    if (child.index !== undefined) tops.set(child.index, top)
    top += child.height + rowGap
  }
  return { tops, end: top - rowGap }
}

/** Where a fully mounted transcript starts turn `index`, and where it ends. */
function fullLayout(heights: readonly number[], rowGap: number) {
  const tops = new Map<number, number>()
  let top = 0
  heights.forEach((height, index) => {
    tops.set(index, top)
    top += height + rowGap
  })
  return { tops, end: top - rowGap }
}

function mountedIndices(plan: TranscriptWindow): number[] {
  return plan.items.map((item) => item.index)
}

/** Deterministic, varied heights, so the layout checks are not a happy accident of equal turns. */
function variedHeights(count: number): number[] {
  return Array.from({ length: count }, (_, index) => 90 + ((index * 37) % 160))
}

describe('isChatVirtualizeEnabled', () => {
  it('enables only on the literal true', () => {
    expect(isChatVirtualizeEnabled('true')).toBe(true)
    for (const raw of [undefined, '', 'TRUE', 'True', '1', 'yes', ' true', 'true ', 'false']) {
      expect(isChatVirtualizeEnabled(raw)).toBe(false)
    }
  })
})

describe('nextFollowState', () => {
  it('follows once the reader is within the re-arm distance', () => {
    expect(nextFollowState(false, 0)).toBe(true)
    expect(nextFollowState(false, 40)).toBe(true)
  })

  it('stops following at the disengage distance', () => {
    expect(nextFollowState(true, 80)).toBe(false)
    expect(nextFollowState(true, 500)).toBe(false)
  })

  it('keeps its state between the two thresholds', () => {
    expect(nextFollowState(true, 41)).toBe(true)
    expect(nextFollowState(true, 79)).toBe(true)
    expect(nextFollowState(false, 41)).toBe(false)
    expect(nextFollowState(false, 79)).toBe(false)
  })
})

describe('resolveTurnHeights', () => {
  it('keeps the measured height of a mounted turn and plans the rest at the mean of this thread', () => {
    const measured = new Map([
      ['a', 100],
      ['c', 300],
      ['elsewhere', 9_000],
    ])
    expect(resolveTurnHeights(['a', 'b', 'c'], measured)).toEqual([100, 200, 300])
  })

  it('plans every turn at the default before anything is measured', () => {
    expect(resolveTurnHeights(['a', 'b'], new Map())).toEqual([
      DEFAULT_TURN_HEIGHT_PX,
      DEFAULT_TURN_HEIGHT_PX,
    ])
  })
})

describe('planTranscriptWindow', () => {
  it('plans an empty transcript as nothing to mount', () => {
    expect(planTranscriptWindow({ heights: [], rowGap: 28, scrollTop: 0, viewportHeight: 800 })).toEqual({
      items: [],
      gapAfter: 0,
    })
  })

  it('mounts every turn, with no spacers, when the transcript fits', () => {
    const plan = planTranscriptWindow({
      heights: [100, 100, 100],
      rowGap: 10,
      scrollTop: 0,
      viewportHeight: 1_000,
    })
    expect(plan.items).toEqual([
      { index: 0, gapBefore: 0 },
      { index: 1, gapBefore: 0 },
      { index: 2, gapBefore: 0 },
    ])
    expect(plan.gapAfter).toBe(0)
  })

  it('mounts the turns on screen, the tail and nothing else in a long transcript', () => {
    // 500 turns of 200 px with no gap: turn i starts at 200 * i.
    const heights = Array.from({ length: 500 }, () => 200)
    const plan = planTranscriptWindow({
      heights,
      rowGap: 0,
      scrollTop: 40_000,
      viewportHeight: 800,
      streamingIndex: 499,
    })
    // The band is 39 400 to 41 400, so turns 197 to 206 are on screen.
    const onScreen = [197, 198, 199, 200, 201, 202, 203, 204, 205, 206]
    const tail = Array.from({ length: TAIL_TURN_COUNT }, (_, offset) => 500 - TAIL_TURN_COUNT + offset)
    expect(mountedIndices(plan)).toEqual([...onScreen, ...tail])
    expect(plan.items[0]).toEqual({ index: 197, gapBefore: 39_400 })
    expect(plan.items[onScreen.length]).toEqual({ index: 492, gapBefore: 57_000 })
    expect(plan.gapAfter).toBe(0)
  })

  it('keeps the streaming and the focused turn mounted wherever they are', () => {
    const heights = Array.from({ length: 500 }, () => 200)
    const plan = planTranscriptWindow({
      heights,
      rowGap: 0,
      scrollTop: 40_000,
      viewportHeight: 800,
      streamingIndex: 10,
      focusedIndex: 300,
    })
    const indices = mountedIndices(plan)
    expect(indices).toContain(10)
    expect(indices).toContain(300)
    expect(indices.length).toBe(10 + 2 + TAIL_TURN_COUNT)
  })

  it('ignores a streaming or focused index that is not in the transcript', () => {
    const heights = Array.from({ length: 20 }, () => 100)
    const bare = planTranscriptWindow({ heights, rowGap: 0, scrollTop: 0, viewportHeight: 300 })
    const stray = planTranscriptWindow({
      heights,
      rowGap: 0,
      scrollTop: 0,
      viewportHeight: 300,
      streamingIndex: -1,
      focusedIndex: 20,
    })
    expect(stray).toEqual(bare)
  })

  it('mounts the last turns when the reader is scrolled past the end', () => {
    const heights = variedHeights(50)
    const plan = planTranscriptWindow({ heights, rowGap: 28, scrollTop: 1_000_000, viewportHeight: 800 })
    expect(mountedIndices(plan)).toEqual(
      Array.from({ length: TAIL_TURN_COUNT }, (_, offset) => 50 - TAIL_TURN_COUNT + offset),
    )
    expect(plan.gapAfter).toBe(0)
  })

  it('places every mounted turn where a fully mounted transcript puts it, at any scroll position', () => {
    const count = 240
    const heights = variedHeights(count)
    const rowGap = 28
    const expected = fullLayout(heights, rowGap)
    for (const scrollTop of [0, 1, 2_500, 9_000, 15_000, 26_000, 40_000, 60_000]) {
      const plan = planTranscriptWindow({
        heights,
        rowGap,
        scrollTop,
        viewportHeight: 700,
        streamingIndex: count - 1,
        focusedIndex: 17,
      })
      const laid = layOut(plan, heights, rowGap)
      for (const item of plan.items) {
        expect(laid.tops.get(item.index)).toBeCloseTo(expected.tops.get(item.index) ?? Number.NaN, 9)
      }
      expect(laid.end).toBeCloseTo(expected.end, 9)
    }
  })

  it('mounts far fewer turns than the transcript holds', () => {
    const heights = variedHeights(1_000)
    const plan = planTranscriptWindow({ heights, rowGap: 28, scrollTop: 60_000, viewportHeight: 900 })
    const onScreenBound = Math.ceil((900 + 2 * OVERSCAN_PX) / 90) + 2
    expect(plan.items.length).toBeLessThanOrEqual(onScreenBound + TAIL_TURN_COUNT)
    expect(plan.items.length).toBeGreaterThan(0)
  })
})
