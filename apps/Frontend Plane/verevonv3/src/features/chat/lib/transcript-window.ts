/**
 * Transcript window for the chat timeline (WS09 S10, the last resort).
 *
 * Built only if the S1 budget still fails after the render work of S2, S3 and
 * S7. Behind `VITE_CHAT_VIRTUALIZE`, which enables only on the literal `true`.
 * Off, the page mounts every turn exactly as it did before.
 *
 * On, the page mounts a window of turns: the ones on screen with an overscan
 * band, the last few, the streaming turn and the focused turn. Each run of turns
 * left out is replaced by one spacer, so the scroll height and the position of
 * every mounted turn stay where a fully mounted transcript would put them. The
 * streaming and focused turns are mounted for correctness, not speed: a focused
 * element that unmounts drops keyboard focus to the body, and a streaming answer
 * that unmounts stops being live.
 *
 * A turn's height is measured once it has been mounted and kept by turn id. A
 * turn never mounted is planned at the mean of the measured turns, so the
 * estimate follows the thread it belongs to. The planner is pure: the page
 * supplies the heights, the scroll position and the viewport.
 */

/** Only the literal `true` enables the window, as for the other chat switches. */
export function isChatVirtualizeEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const virtualizeTranscript = isChatVirtualizeEnabled(import.meta.env.VITE_CHAT_VIRTUALIZE)

/** Height planned for a turn before any turn in the thread has been measured. */
export const DEFAULT_TURN_HEIGHT_PX = 240

/** Turns kept mounted at the end of the transcript, wherever the reader is. */
export const TAIL_TURN_COUNT = 8

/** Mounted band above and below the viewport, so a short scroll does not wait for a mount. */
export const OVERSCAN_PX = 600

/** The reader scrolled this far from the bottom stops being followed... */
export const FOLLOW_DISENGAGE_PX = 80

/** ...and is followed again once back within this distance of the bottom. */
export const FOLLOW_REARM_PX = 40

/**
 * Whether new content is followed. Between the two thresholds the rule keeps its
 * state, so a measurement that moves the bottom by a few pixels cannot switch
 * the follow off and on under the reader.
 */
export function nextFollowState(following: boolean, distanceFromBottomPx: number): boolean {
  if (distanceFromBottomPx >= FOLLOW_DISENGAGE_PX) return false
  if (distanceFromBottomPx <= FOLLOW_REARM_PX) return true
  return following
}

/**
 * The height each turn is planned at: its measured height once mounted, otherwise
 * the mean of the measured turns of this thread, or the default before anything
 * has been measured.
 */
export function resolveTurnHeights(
  turnIds: readonly string[],
  measured: ReadonlyMap<string, number>,
): number[] {
  let total = 0
  let known = 0
  for (const id of turnIds) {
    const height = measured.get(id)
    if (height === undefined) continue
    total += height
    known += 1
  }
  const estimate = known > 0 ? total / known : DEFAULT_TURN_HEIGHT_PX
  return turnIds.map((id) => measured.get(id) ?? estimate)
}

export interface TranscriptWindowInput {
  /** Height of each turn in order, measured or estimated, in px. */
  heights: readonly number[]
  /** The gap the transcript's flex column puts between its children, in px. */
  rowGap: number
  /** Top of the viewport, measured from the top of the transcript, in px. */
  scrollTop: number
  /** Height of the viewport, in px. */
  viewportHeight: number
  /** Index of the streaming turn, while an answer streams. */
  streamingIndex?: number
  /** Index of the turn that holds focus, when focus is inside the transcript. */
  focusedIndex?: number
}

export interface TranscriptWindowItem {
  /** Index of the mounted turn. */
  index: number
  /** Height of the turns left out directly above it, in px; 0 when none. */
  gapBefore: number
}

export interface TranscriptWindow {
  /** The mounted turns, in order. */
  items: TranscriptWindowItem[]
  /** Height of the turns left out below the last mounted one, in px; 0 when none. */
  gapAfter: number
}

/**
 * The last turn starting at or before `y` (strictly before when `exclusive`), or
 * the first turn when none does. `tops` is ascending.
 */
function turnAt(tops: readonly number[], count: number, y: number, exclusive: boolean): number {
  let low = 0
  let high = count - 1
  let found = 0
  while (low <= high) {
    const middle = (low + high) >> 1
    const top = tops[middle] ?? 0
    if (exclusive ? top < y : top <= y) {
      found = middle
      low = middle + 1
    } else {
      high = middle - 1
    }
  }
  return found
}

/**
 * Chooses the turns to mount.
 *
 * Turn `i` starts at `tops[i]`, the sum of the heights and gaps above it. A spacer
 * that stands in for the turns `[from, to)` is `tops[to] - tops[from] - rowGap`
 * tall, because the flex gap sits on both sides of it. That identity keeps every
 * mounted turn at its fully mounted position; the tests check it.
 */
export function planTranscriptWindow(input: TranscriptWindowInput): TranscriptWindow {
  const { heights, rowGap } = input
  const count = heights.length
  if (count === 0) return { items: [], gapAfter: 0 }

  const tops = new Array<number>(count + 1)
  tops[0] = 0
  for (let index = 0; index < count; index += 1) {
    tops[index + 1] = (tops[index] ?? 0) + (heights[index] ?? 0) + rowGap
  }
  const topOf = (index: number): number => tops[index] ?? 0

  const mounted = new Uint8Array(count)
  const firstVisible = turnAt(tops, count, Math.max(0, input.scrollTop - OVERSCAN_PX), false)
  const lastVisible = Math.max(
    firstVisible,
    turnAt(tops, count, input.scrollTop + input.viewportHeight + OVERSCAN_PX, true),
  )
  for (let index = firstVisible; index <= lastVisible; index += 1) mounted[index] = 1
  for (let index = Math.max(0, count - TAIL_TURN_COUNT); index < count; index += 1) mounted[index] = 1
  for (const index of [input.streamingIndex, input.focusedIndex]) {
    if (index !== undefined && index >= 0 && index < count) mounted[index] = 1
  }

  const items: TranscriptWindowItem[] = []
  let next = 0
  for (let index = 0; index < count; index += 1) {
    if (mounted[index] !== 1) continue
    items.push({
      index,
      gapBefore: index > next ? topOf(index) - topOf(next) - rowGap : 0,
    })
    next = index + 1
  }
  const gapAfter = next < count ? topOf(count) - topOf(next) - rowGap : 0
  return { items, gapAfter }
}
