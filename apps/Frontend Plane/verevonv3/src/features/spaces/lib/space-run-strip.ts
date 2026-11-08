/**
 * The rules behind the composer's activity strip (WS09-S6): which running agents the strip names, when
 * a name appears and how fast its words may change. Pure: no clock, no timers, no DOM. The caller
 * feeds observations plus `now` and gets the next state and the moment it should look again.
 *
 * - ENTER (commit): a run joins the strip when its first `step` or `text_progress` frame has arrived
 *   (`committed`). A run that is merely attached and reading does not, so a turn that is refused or
 *   ends at once never flashes a line.
 * - LATCH: once a run is in, it stays until it is finished or gone from the room's live listing. A
 *   re-attach that momentarily empties its view (`committed` false again) does not drop it.
 * - SHOW after 800 ms: a committed run is held back for `STRIP_MIN_DISPLAY_MS`, so a one-step turn
 *   that finishes inside it never draws at all. This is the minimum before the first appearance.
 * - ROTATE no faster than every 2.2 s: the headline shown changes to the wanted one only when the
 *   current one has been up for `STRIP_ROTATION_MS`.
 * - FALL BACK: no step class yet reads as the plain "Working" headline (decided by the caller).
 */

export const STRIP_MIN_DISPLAY_MS = 800
export const STRIP_ROTATION_MS = 2200

export type StripObservation = {
  readonly runId: string
  readonly threadId: string
  /** A step or text progress frame has been received for this run. */
  readonly committed: boolean
  /** The run finished (the view says so). */
  readonly finished: boolean
  /** What the run is doing right now, in words. */
  readonly headline: string
}

type StripEntry = {
  readonly runId: string
  readonly threadId: string
  /** When the run committed, in the caller's clock (ms). */
  readonly committedAt: number
  /** Headline on screen; null until the first appearance. */
  readonly shown: string | null
  readonly shownAt: number
  readonly wanted: string
}

export type StripState = {
  readonly entries: readonly StripEntry[]
}

export type StripLine = {
  readonly runId: string
  readonly threadId: string
  readonly headline: string
}

export const EMPTY_STRIP: StripState = { entries: [] }

export type StripStep = {
  readonly state: StripState
  /** What to draw now, oldest run first. */
  readonly lines: readonly StripLine[]
  /** When to call `advanceStrip` again (ms, caller's clock), or null when nothing is pending. */
  readonly nextWakeAt: number | null
}

export function advanceStrip(
  previous: StripState,
  observations: readonly StripObservation[],
  now: number,
): StripStep {
  const byRun = new Map(observations.map((observation) => [observation.runId, observation]))
  const held = new Map(previous.entries.map((entry) => [entry.runId, entry]))
  const next: StripEntry[] = []
  let wake: number | null = null
  const wakeAt = (at: number) => {
    if (wake === null || at < wake) wake = at
  }

  // Latched entries first (keeps the order they joined), then runs that just committed.
  const order = [
    ...previous.entries.map((entry) => entry.runId),
    ...observations.filter((observation) => !held.has(observation.runId)).map((observation) => observation.runId),
  ]
  for (const runId of order) {
    const observation = byRun.get(runId)
    // Gone from the room's live listing, or finished: it leaves.
    if (!observation || observation.finished) continue
    const entry = held.get(runId)
    if (!entry) {
      if (!observation.committed) continue
      next.push({
        runId,
        threadId: observation.threadId,
        committedAt: now,
        shown: null,
        shownAt: 0,
        wanted: observation.headline,
      })
      continue
    }
    next.push({ ...entry, threadId: observation.threadId || entry.threadId, wanted: observation.headline })
  }

  const lines: StripLine[] = []
  const settled = next.map((entry): StripEntry => {
    if (entry.shown === null) {
      const appearsAt = entry.committedAt + STRIP_MIN_DISPLAY_MS
      if (now < appearsAt) {
        wakeAt(appearsAt)
        return entry
      }
      return { ...entry, shown: entry.wanted, shownAt: now }
    }
    if (entry.wanted !== entry.shown) {
      const mayChangeAt = entry.shownAt + STRIP_ROTATION_MS
      if (now < mayChangeAt) {
        wakeAt(mayChangeAt)
        return entry
      }
      return { ...entry, shown: entry.wanted, shownAt: now }
    }
    return entry
  })
  for (const entry of settled) {
    if (entry.shown !== null) lines.push({ runId: entry.runId, threadId: entry.threadId, headline: entry.shown })
  }
  return { state: { entries: settled }, lines, nextWakeAt: wake }
}
