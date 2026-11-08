/**
 * Replay scrubber for the browser chrome timeline (F-t3-050, R1). Pure logic: it maps a
 * slider position to a recorded step and steps through the recorded steps for auto-play.
 * It reads only the timeline the chrome already holds. It never fetches, stores or
 * persists anything, so the session's ZDR posture is unchanged. The switch is
 * `VITE_BROWSER_REPLAY_SCRUBBER`, which is off unless its value is the literal `true`.
 */

/** How long one recorded step stays on screen during auto-play. */
export const REPLAY_AUTO_PLAY_INTERVAL_MS = 1500

export type ReplayScrubStep = {
  step: number
}

/** Build-time switch. Only the literal `true` turns the scrubber on. */
export function replayScrubberEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_REPLAY_SCRUBBER === 'true'
}

/**
 * The recorded steps in replay order: ascending step number, each step once. The opening
 * observation (step 0) is kept, so a replay starts at the first recorded page. Entries with
 * a step that is not a non-negative integer are dropped, and the first entry wins a tie.
 */
export function replayScrubSteps<T extends ReplayScrubStep>(entries: readonly T[]): T[] {
  const byStep = new Map<number, T>()
  for (const entry of entries) {
    if (!Number.isInteger(entry.step) || entry.step < 0 || byStep.has(entry.step)) continue
    byStep.set(entry.step, entry)
  }
  return [...byStep.values()].sort((left, right) => left.step - right.step)
}

/** The scrubber needs at least two recorded steps to move between. */
export function replayScrubberAvailable(steps: readonly ReplayScrubStep[]): boolean {
  return steps.length >= 2
}

/**
 * The slider position of the step on screen. The live page (null) and a step that is not
 * recorded both sit on the last position, which is where a live session already is.
 */
export function scrubPositionFor(steps: readonly ReplayScrubStep[], selectedStep: number | null): number {
  if (steps.length === 0) return 0
  const index = selectedStep === null ? -1 : steps.findIndex((entry) => entry.step === selectedStep)
  return index < 0 ? steps.length - 1 : index
}

/** The recorded step at a slider position, clamped into range. Null when nothing is recorded. */
export function scrubStepAt(steps: readonly ReplayScrubStep[], position: number): number | null {
  if (steps.length === 0) return null
  const index = Number.isFinite(position) ? Math.trunc(position) : 0
  const clamped = Math.min(Math.max(index, 0), steps.length - 1)
  return steps[clamped]?.step ?? null
}

/**
 * The position auto-play starts from: the current position, or the first step when the
 * current position is already the last, so Play after the end replays from the start.
 * Null when there is fewer than two steps to play.
 */
export function autoPlayStartPosition(length: number, position: number): number | null {
  if (length < 2) return null
  return position >= length - 1 ? 0 : position
}

/** The position auto-play moves to next, or null once the last step has been shown. */
export function nextAutoPlayPosition(length: number, position: number): number | null {
  const next = position + 1
  return next < length ? next : null
}

/**
 * The session frame a recorded step may fall back to. While the scrubber shows a recorded
 * step, the session's current frame is never a fallback: the step shows its own screenshot
 * or nothing, so no later page appears under an earlier step.
 */
export function replayFallbackFrame(scrubbing: boolean, sessionFrameUrl: string | null | undefined): string | null {
  if (scrubbing) return null
  return sessionFrameUrl ?? null
}
