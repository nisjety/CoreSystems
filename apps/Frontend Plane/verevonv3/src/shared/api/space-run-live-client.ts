import { ApiError } from './api-error'
import { readSseStream, type SseEvent } from './sse'
import {
  roomRunStepClass,
  type RoomRunStepClass,
  type RoomRunStepState,
  ROOM_RUN_MAX_STEPS,
} from './spaces-client/run-work'

/**
 * Member live view of an agent run (WS09-S5): `GET /api/v1/spaces/{space_ref}/runs/{run_id}/live`.
 *
 * The frames are projected by Model Plane from an allowlist (`room_run.rs`, golden
 * `contracts/space-room-run-frames.v1.json`) after Session Core authorized this attach, so this client
 * decides nothing about what a reader may see. It parses each frame to the closed shape and drops what it
 * does not know: an unknown event name, a malformed payload or an unknown step state is skipped, never
 * guessed at. No decision, token or run owner credential ever travels through here; the browser holds
 * only its cookie session.
 *
 * Offsets in `text` frames are UTF-16 code units, the unit of a JavaScript string, so a delta can be
 * applied with plain `slice` arithmetic.
 */

export type RoomRunPhase = 'running' | 'awaiting_approval' | 'queued' | 'completed' | 'failed' | 'cancelled' | string

export type RoomRunOutcome = 'completed' | 'failed' | 'stopped'

export type RoomRunFailureReason = 'model_unavailable' | 'budget' | 'policy' | 'internal' | 'unknown'

export type RoomRunStepFrame = {
  readonly id: string
  readonly class: RoomRunStepClass
  readonly state: RoomRunStepState
  readonly ms?: number
}

export type RoomRunSnapshot = {
  readonly runId: string
  readonly threadId: string
  /** The Control subject that started the run. Empty when the server named none. */
  readonly startedBy: string
  readonly agentName: string
  readonly phase: RoomRunPhase
  /** Epoch seconds. */
  readonly startedAt: number
  readonly text: {
    readonly offset: number
    readonly value: string
    readonly truncated: boolean
    /** False when the deployment does not stream text to members (`space_run_live_text` off). */
    readonly visible: boolean
  }
  readonly steps: readonly RoomRunStepFrame[]
  readonly steer: number
  readonly plan: { readonly planId: string; readonly steps: number } | null
  readonly artifacts: readonly { readonly id: string; readonly kind: string }[]
  /** Epoch seconds when this attach's authorization ends (a `reauth_required` frame follows). */
  readonly expiresAt: number | null
}

export type RoomRunFrame =
  | { readonly type: 'snapshot'; readonly snapshot: RoomRunSnapshot }
  | { readonly type: 'text'; readonly offset: number; readonly delta: string }
  | { readonly type: 'text_progress'; readonly chars: number }
  | { readonly type: 'step'; readonly step: RoomRunStepFrame }
  | { readonly type: 'steer'; readonly count: number }
  | { readonly type: 'plan'; readonly planId: string; readonly steps: number }
  | { readonly type: 'artifact'; readonly id: string; readonly artifactKind: string }
  | { readonly type: 'finished'; readonly outcome: RoomRunOutcome; readonly reason: RoomRunFailureReason | null }
  | { readonly type: 'reauth_required' }
  | {
      readonly type: 'error'
      readonly code: string
      readonly retryable: boolean
      /** The feature a zero-retention refusal names, when the frame carries one. */
      readonly feature: string | null
    }

const STEP_STATES: readonly string[] = ['running', 'ok', 'error', 'denied']
const OUTCOMES: readonly string[] = ['completed', 'failed', 'stopped']
const REASONS: readonly string[] = ['model_unavailable', 'budget', 'policy', 'internal', 'unknown']

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function str(value: unknown, max = 200): string {
  return typeof value === 'string' ? value.trim().slice(0, max) : ''
}

function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0
}

function stepFrom(value: unknown): RoomRunStepFrame | undefined {
  const raw = record(value)
  if (!raw) return undefined
  const id = str(raw.id)
  const state = raw.state
  if (!id || typeof state !== 'string' || !STEP_STATES.includes(state)) return undefined
  const ms = typeof raw.ms === 'number' && Number.isFinite(raw.ms) && raw.ms >= 0 ? Math.round(raw.ms) : undefined
  return { id, class: roomRunStepClass(raw.class), state: state as RoomRunStepState, ...(ms !== undefined ? { ms } : {}) }
}

function snapshotFrom(raw: Record<string, unknown>): RoomRunSnapshot | undefined {
  const runId = str(raw.run_id)
  if (!runId) return undefined
  const text = record(raw.text)
  const steps: RoomRunStepFrame[] = []
  if (Array.isArray(raw.steps)) {
    for (const entry of raw.steps) {
      const step = stepFrom(entry)
      if (step && steps.length < ROOM_RUN_MAX_STEPS) steps.push(step)
    }
  }
  const plan = record(raw.plan)
  const planId = plan ? str(plan.plan_id) : ''
  const artifacts: { id: string; kind: string }[] = []
  if (Array.isArray(raw.artifacts)) {
    for (const entry of raw.artifacts) {
      const artifact = record(entry)
      const id = artifact ? str(artifact.id) : ''
      if (artifact && id && artifacts.length < 16) artifacts.push({ id, kind: str(artifact.kind, 40) })
    }
  }
  return {
    runId,
    threadId: str(raw.thread_id),
    startedBy: str(raw.started_by),
    agentName: str(raw.agent_name, 120),
    phase: str(raw.phase, 40) || 'running',
    startedAt: count(raw.started_at),
    text: {
      offset: text ? count(text.offset) : 0,
      value: text && typeof text.value === 'string' ? text.value : '',
      truncated: text?.truncated === true,
      visible: text?.visible === true,
    },
    steps,
    steer: count(raw.steer),
    plan: plan && planId ? { planId, steps: count(plan.steps) } : null,
    artifacts,
    expiresAt: typeof raw.expires_at === 'number' && Number.isFinite(raw.expires_at) ? raw.expires_at : null,
  }
}

/** One SSE event to one typed frame, or `undefined` for anything this client does not recognize. */
export function parseRoomRunFrame(event: Pick<SseEvent, 'event' | 'data'>): RoomRunFrame | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(event.data)
  } catch {
    return undefined
  }
  const raw = record(parsed)
  if (!raw) return undefined
  switch (event.event) {
    case 'snapshot': {
      const snapshot = snapshotFrom(raw)
      return snapshot ? { type: 'snapshot', snapshot } : undefined
    }
    case 'text': {
      if (typeof raw.delta !== 'string' || typeof raw.offset !== 'number' || !Number.isFinite(raw.offset)) return undefined
      return { type: 'text', offset: Math.max(0, Math.floor(raw.offset)), delta: raw.delta }
    }
    case 'text_progress':
      return { type: 'text_progress', chars: count(raw.chars) }
    case 'step': {
      const step = stepFrom(raw)
      return step ? { type: 'step', step } : undefined
    }
    case 'steer':
      return { type: 'steer', count: count(raw.count) }
    case 'plan': {
      const planId = str(raw.plan_id)
      return planId ? { type: 'plan', planId, steps: count(raw.steps) } : undefined
    }
    case 'artifact': {
      const id = str(raw.id)
      return id ? { type: 'artifact', id, artifactKind: str(raw.kind, 40) } : undefined
    }
    case 'finished': {
      const outcome = raw.outcome
      if (typeof outcome !== 'string' || !OUTCOMES.includes(outcome)) return undefined
      const reason = typeof raw.reason === 'string' && REASONS.includes(raw.reason) ? (raw.reason as RoomRunFailureReason) : null
      return { type: 'finished', outcome: outcome as RoomRunOutcome, reason }
    }
    case 'reauth_required':
      return { type: 'reauth_required' }
    case 'error': {
      const code = str(raw.code, 80)
      if (!code) return undefined
      return { type: 'error', code, retryable: raw.retryable === true, feature: str(raw.feature, 80) || null }
    }
    default:
      return undefined
  }
}

/** How an attach ended, once it has. A clean close and a failed connect are told apart. */
export type SpaceRunLiveEnd =
  | { readonly kind: 'closed' }
  | { readonly kind: 'failed'; readonly status: number; readonly code: string | null; readonly feature: string | null }
  | { readonly kind: 'aborted' }

export type SpaceRunLiveHandlers = {
  readonly onFrame: (frame: RoomRunFrame, sseId: string | undefined) => void
  /** Called exactly once, when the stream is over. */
  readonly onEnd: (end: SpaceRunLiveEnd) => void
}

export type SpaceRunLiveOptions = {
  readonly signal?: AbortSignal
  readonly lastEventId?: string
}

/** The route is a read: `GET`, cookie session, no body. */
export function spaceRunLivePath(spaceRef: string, runId: string): string {
  return `/api/v1/spaces/${encodeURIComponent(spaceRef)}/runs/${encodeURIComponent(runId)}/live`
}

/**
 * Attach to a run's live view and resolve when the stream is over. Frames go to `onFrame` in order;
 * `onEnd` fires once. A 4xx/5xx answer arrives as `failed` with the typed code (`run_stream_expired`,
 * `feature_unavailable`, `zdr_feature_not_supported`, `too_many_live_views` ...), never as a frame.
 */
export async function streamSpaceRunLive(
  spaceRef: string,
  runId: string,
  handlers: SpaceRunLiveHandlers,
  options: SpaceRunLiveOptions = {},
): Promise<void> {
  let failure: SpaceRunLiveEnd | undefined
  await readSseStream(
    spaceRunLivePath(spaceRef, runId),
    { method: 'GET', signal: options.signal, lastEventId: options.lastEventId },
    (event) => {
      const frame = parseRoomRunFrame(event)
      if (frame) handlers.onFrame(frame, event.id)
    },
    (err) => {
      failure =
        err instanceof ApiError
          ? { kind: 'failed', status: err.status, code: err.code, feature: err.feature }
          : { kind: 'failed', status: 0, code: null, feature: null }
    },
    () => {
      handlers.onEnd(failure ?? (options.signal?.aborted ? { kind: 'aborted' } : { kind: 'closed' }))
    },
  )
}
