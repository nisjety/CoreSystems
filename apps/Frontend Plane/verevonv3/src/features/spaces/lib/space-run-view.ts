import type { RoomRunFailureReason, RoomRunFrame, RoomRunOutcome, RoomRunStepFrame } from '@/shared/api/space-run-live-client'
import { ROOM_RUN_MAX_STEPS } from '@/shared/api/spaces-client/run-work'

/**
 * The reducer behind the room's run card (WS09-S5): a pure fold of live frames into what a member sees
 * of someone else's agent turn. No clock, no network, no DOM.
 *
 * Rules it enforces, each from the frame contract:
 *
 * - a `snapshot` REPLACES everything it carries (the server re-snapshots on every attach, so a
 *   reconnect heals whatever was missed);
 * - a `text` frame is positioned by a UTF-16 offset: one that ends at or before what is already held is
 *   a repeat and is ignored, one that overlaps appends only its new tail, and one that starts past the
 *   end of the held text is a GAP; the view then asks for a fresh attach (`needsReattach`) instead of
 *   splicing text together out of order;
 * - a step is upserted by id, so a running step settles in place rather than appearing twice;
 * - `finished` is final: nothing after it changes the outcome.
 */

export type SpaceRunViewStatus =
  /** Asked to attach, no snapshot yet. */
  | 'connecting'
  /** Attached and receiving. */
  | 'live'
  /** Waiting for one of the (at most three) attach slots. */
  | 'waiting'
  /** The run finished; the view keeps its last state for the disclosure. */
  | 'ended'
  /** The live view is not available (expired, refused, off). The legacy row takes over. */
  | 'unavailable'

export type SpaceRunViewStep = RoomRunStepFrame

export type SpaceRunView = {
  readonly runId: string
  readonly threadId: string
  readonly startedBy: string
  readonly agentName: string
  readonly phase: string
  /** Epoch seconds. */
  readonly startedAt: number
  readonly status: SpaceRunViewStatus
  /** Why the live view is unavailable, as the typed code (`run_stream_expired` ...). */
  readonly unavailableCode: string | null
  readonly text: {
    /** UTF-16 length of `value`: the offset the next `text` frame must start at. */
    readonly length: number
    readonly value: string
    readonly truncated: boolean
    readonly visible: boolean
    /** Characters written so far when the text itself is not shown to members. */
    readonly progressChars: number
  }
  readonly steps: readonly SpaceRunViewStep[]
  /** Steps dropped from the front once the cap was reached (the count shown stays honest). */
  readonly stepsDropped: number
  readonly steer: number
  readonly plan: { readonly planId: string; readonly steps: number } | null
  readonly artifacts: readonly { readonly id: string; readonly kind: string }[]
  readonly finished: { readonly outcome: RoomRunOutcome; readonly reason: RoomRunFailureReason | null } | null
  /** A `text` gap was seen; the store must attach again. Cleared by the next snapshot. */
  readonly needsReattach: boolean
  /** The server asked for the authorization to be renewed; the store re-attaches. */
  readonly reauthRequired: boolean
}

export function emptySpaceRunView(runId: string): SpaceRunView {
  return {
    runId,
    threadId: '',
    startedBy: '',
    agentName: '',
    phase: 'running',
    startedAt: 0,
    status: 'connecting',
    unavailableCode: null,
    text: { length: 0, value: '', truncated: false, visible: false, progressChars: 0 },
    steps: [],
    stepsDropped: 0,
    steer: 0,
    plan: null,
    artifacts: [],
    finished: null,
    needsReattach: false,
    reauthRequired: false,
  }
}

function upsertStep(
  view: SpaceRunView,
  step: SpaceRunViewStep,
): Pick<SpaceRunView, 'steps' | 'stepsDropped'> {
  const index = view.steps.findIndex((entry) => entry.id === step.id)
  if (index >= 0) {
    // A step that has settled stays settled: a late `running` repeat must not reopen it.
    const current = view.steps[index] as SpaceRunViewStep
    if (current.state !== 'running' && step.state === 'running') return { steps: view.steps, stepsDropped: view.stepsDropped }
    const next = view.steps.slice()
    next[index] = { ...current, ...step }
    return { steps: next, stepsDropped: view.stepsDropped }
  }
  const next = [...view.steps, step]
  if (next.length <= ROOM_RUN_MAX_STEPS) return { steps: next, stepsDropped: view.stepsDropped }
  const overflow = next.length - ROOM_RUN_MAX_STEPS
  return { steps: next.slice(overflow), stepsDropped: view.stepsDropped + overflow }
}

export function applyRoomRunFrame(view: SpaceRunView, frame: RoomRunFrame): SpaceRunView {
  // Once finished, only a fresh snapshot (a new attach to a finished run) may change anything.
  if (view.finished && frame.type !== 'snapshot') return view
  switch (frame.type) {
    case 'snapshot': {
      const snap = frame.snapshot
      return {
        ...view,
        runId: snap.runId || view.runId,
        threadId: snap.threadId,
        startedBy: snap.startedBy,
        agentName: snap.agentName,
        phase: snap.phase,
        startedAt: snap.startedAt,
        status: 'live',
        unavailableCode: null,
        text: {
          length: snap.text.offset + snap.text.value.length,
          value: snap.text.value,
          truncated: snap.text.truncated,
          visible: snap.text.visible,
          progressChars: view.text.progressChars,
        },
        steps: snap.steps,
        stepsDropped: 0,
        steer: snap.steer,
        plan: snap.plan,
        artifacts: snap.artifacts,
        finished: null,
        needsReattach: false,
        reauthRequired: false,
      }
    }
    case 'text': {
      if (!view.text.visible) return view
      const end = frame.offset + frame.delta.length
      if (end <= view.text.length) return view
      if (frame.offset > view.text.length) return { ...view, needsReattach: true }
      const tail = frame.delta.slice(view.text.length - frame.offset)
      return {
        ...view,
        text: { ...view.text, value: view.text.value + tail, length: end, progressChars: Math.max(view.text.progressChars, end) },
      }
    }
    case 'text_progress':
      return { ...view, text: { ...view.text, progressChars: Math.max(view.text.progressChars, frame.chars) } }
    case 'step':
      return { ...view, ...upsertStep(view, frame.step) }
    case 'steer':
      return { ...view, steer: Math.max(view.steer, frame.count) }
    case 'plan':
      return { ...view, plan: { planId: frame.planId, steps: frame.steps } }
    case 'artifact':
      return view.artifacts.some((entry) => entry.id === frame.id) || view.artifacts.length >= 16
        ? view
        : { ...view, artifacts: [...view.artifacts, { id: frame.id, kind: frame.artifactKind }] }
    case 'finished':
      return {
        ...view,
        status: 'ended',
        finished: { outcome: frame.outcome, reason: frame.reason },
        phase: frame.outcome === 'completed' ? 'completed' : frame.outcome === 'stopped' ? 'cancelled' : 'failed',
        // Anything still running when the run ended is not running any more; leave it as the last
        // known state rather than claim an outcome for it.
      }
    case 'reauth_required':
      return { ...view, reauthRequired: true }
    case 'error':
      return { ...view, status: 'unavailable', unavailableCode: frame.code }
    default:
      return view
  }
}

/** Steps that are still in flight. */
export function runningSteps(view: SpaceRunView): readonly SpaceRunViewStep[] {
  return view.steps.filter((step) => step.state === 'running')
}

/** Total steps taken, counting those dropped from the front at the cap. */
export function totalSteps(view: SpaceRunView): number {
  return view.steps.length + view.stepsDropped
}

/** Whole seconds the run has been going, from its start (epoch seconds) to `nowMs`. Zero when unknown. */
export function elapsedSeconds(view: Pick<SpaceRunView, 'startedAt'>, nowMs: number): number {
  if (!view.startedAt) return 0
  return Math.max(0, Math.floor(nowMs / 1000 - view.startedAt))
}
