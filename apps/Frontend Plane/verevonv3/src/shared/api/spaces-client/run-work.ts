// The durable, content-free summary of an agent turn that a room reader sees (WS09-S5).
//
// Model Plane writes `metadata.work` on the assistant message (WS09-S3, `room_run.rs`) and rebuilds it to
// this closed shape on the way out of a room read. This file is the browser's second check of the same
// shape: anything outside it is dropped, never rendered. Free text (tool arguments, output, titles) is
// never part of it, so nothing here can leak what the owner's turn actually did.

/**
 * What kind of step an agent took, as a closed set the room has words for. `browser` is reserved for
 * the browser work stream (WS20) and is not emitted today. A class this build does not know reads as
 * the generic `tool` rather than being dropped, so the step count stays honest.
 */
export const ROOM_RUN_STEP_CLASSES = [
  'search_knowledge',
  'web',
  'code',
  'artifact',
  'lookup',
  'effect',
  'memory',
  'delegate',
  'plan',
  'check',
  'tool',
] as const

export type RoomRunStepClass = (typeof ROOM_RUN_STEP_CLASSES)[number]

/** Live step states; a stored summary carries only the settled ones. */
export type RoomRunStepState = 'running' | 'ok' | 'error' | 'denied'
export type SpaceRunStoredStepState = Exclude<RoomRunStepState, 'running'>

/** At most this many steps are kept, on both the live view and the stored summary. */
export const ROOM_RUN_MAX_STEPS = 64

export type SpaceRunWorkStep = {
  readonly id: string
  readonly class: RoomRunStepClass
  readonly state: SpaceRunStoredStepState
  /** Wall time of the step in milliseconds, when the server recorded one. */
  readonly ms?: number
}

export type SpaceRunWork = {
  readonly v: 1
  readonly steps: readonly SpaceRunWorkStep[]
  /** How many messages the run's owner steered into it while it worked. */
  readonly steer: number
}

export function roomRunStepClass(value: unknown): RoomRunStepClass {
  return typeof value === 'string' && (ROOM_RUN_STEP_CLASSES as readonly string[]).includes(value)
    ? (value as RoomRunStepClass)
    : 'tool'
}

/**
 * Read `work` off a transcript turn. Returns undefined for anything that is not the closed shape; a
 * malformed step is skipped rather than failing the whole summary. A summary with no steps and no
 * steer carries nothing to show and also reads as undefined.
 */
export function spaceRunWorkFrom(raw: unknown): SpaceRunWork | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const value = raw as Record<string, unknown>
  if (value.v !== 1 || !Array.isArray(value.steps)) return undefined
  const steps: SpaceRunWorkStep[] = []
  for (const entry of value.steps) {
    if (steps.length >= ROOM_RUN_MAX_STEPS) break
    if (!entry || typeof entry !== 'object') continue
    const step = entry as Record<string, unknown>
    const id = typeof step.id === 'string' ? step.id.trim() : ''
    if (!id || id.length > 200) continue
    const state = step.state
    if (state !== 'ok' && state !== 'error' && state !== 'denied') continue
    const ms = typeof step.ms === 'number' && Number.isFinite(step.ms) && step.ms >= 0 ? Math.round(step.ms) : undefined
    steps.push({
      id,
      class: roomRunStepClass(step.class),
      state,
      ...(ms !== undefined ? { ms } : {}),
    })
  }
  const steer =
    typeof value.steer === 'number' && Number.isInteger(value.steer) && value.steer > 0
      ? Math.min(value.steer, 1000)
      : 0
  if (steps.length === 0 && steer === 0) return undefined
  return { v: 1, steps, steer }
}
