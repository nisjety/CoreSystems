// The closed vocabulary of the `chat.run.continue` action (build plan WS03 S7c).
//
// A continuation spends a granted approval's claim token exactly once. What a
// client may do after a failure depends on whether that spend happened, so the
// vocabulary is closed, test-pinned on both sides (this module, and the BFF's
// CONTINUE_RELAYED_ERRORS in apps/gateway), and read only through the guards
// below. No UI calls these yet: the contract exists before any wiring.
//
// There are two surfaces.
//
// HTTP. A refusal that arrives before the stream opens. The model-gateway route
// answers `{ error: { code, message } }`; the BFF relays the eight codes below,
// with the upstream status and its own fixed message, and turns every other
// upstream error into `model_error` (a 401 into `unauthorized`).
//
// Stream. The 200 response is an SSE stream. Its `connected` frame and its
// `error` frames can carry `approval_spent: true`, and `error` frames carry a code
// from CHAT_RUN_CONTINUE_FRAME_REFUSALS. Three regimes say what a frame means:
//
//   1. `approval_spent: true` (any frame of a continuation the gateway
//      pre-consumed, the `connected` frame included): the approval is spent, the
//      frame is final and never retried, and the client follows the run.
//   2. A code from the tuple WITHOUT `approval_spent`, from a continuation the
//      gateway did not pre-consume: the executor refused, the gateway does not
//      know whether the executor spent the approval first, so the client
//      re-reads the approval and the run before any retry and never retries
//      automatically.
//   3. The committed dispatch frames (`agent_dispatch_unavailable`,
//      `agent_dispatch_rejected`, `agent_dispatch_unreachable`,
//      `session_terminalization_failed`): they say nothing about the approval,
//      so they are not in this vocabulary and every guard here returns null for
//      them. A retry with a new token is what the committed rules permit.
//
// Reading rule for callers: a guard returning null says NOTHING. In particular a
// failed request that is not one of the eight codes is not "not spent". The BFF
// reports a send failure that happens after the request reached model-gateway as
// 502 `upstream_unavailable`, and the gateway may have consumed the approval by
// then, so the approval's state after such a failure is unknown and the caller
// re-reads it before any retry.

/** The HTTP refusals the BFF relays, with the status each travels under. */
export const CHAT_RUN_CONTINUE_HTTP_REFUSALS = [
  { code: 'continue_disabled', status: 409 },
  { code: 'not_resumable', status: 409 },
  { code: 'approval_not_found', status: 404 },
  { code: 'approval_not_granted', status: 409 },
  { code: 'approval_expired', status: 409 },
  { code: 'approval_not_resumable', status: 409 },
  { code: 'continuation_already_consumed', status: 409 },
  { code: 'continue_unavailable', status: 503 },
] as const

export type ChatRunContinueHttpRefusalCode = (typeof CHAT_RUN_CONTINUE_HTTP_REFUSALS)[number]['code']

/**
 * The refusal codes an `error` frame of a continuation can carry: 17 message
 * tokens the executor returns, 2 the gateway derives from the status (a vanished
 * approval or run, a zero-retention refusal) and 3 the gateway makes itself.
 */
export const CHAT_RUN_CONTINUE_FRAME_REFUSALS = [
  // Message tokens carried by the executor.
  'continue_disabled',
  'resume_incomplete',
  'continuation_already_consumed',
  'approval_not_granted',
  'approval_expired',
  'approval_not_resumable',
  'approval_action_mismatch',
  'approval_snapshot_mismatch',
  'not_releasable',
  'authority_widened',
  'authority_space_mismatch',
  'authority_route_mismatch',
  'continue_space_scope_unsupported',
  'continue_in_progress',
  'resume_checkpoint_unavailable',
  'not_resumable',
  'resume_forbidden',
  // Derived from the status by the gateway.
  'approval_not_found',
  'durable_mutation_forbidden',
  // Made by the gateway.
  'continue_refused',
  'continue_unreachable',
  'continue_outcome_unknown',
] as const

export type ChatRunContinueFrameRefusalCode = (typeof CHAT_RUN_CONTINUE_FRAME_REFUSALS)[number]

/**
 * The frame codes that exist only after a gateway pre-consume (regime 1): there
 * is no unflagged form of them.
 */
export const CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS = [
  'continue_unreachable',
  'continue_outcome_unknown',
] as const

/** Whether the approval's claim token was spent. `unknown` means re-read it. */
export type ChatRunContinueApprovalSpent = 'no' | 'yes' | 'unknown'

export type ChatRunContinueEffect = {
  readonly approvalSpent: ChatRunContinueApprovalSpent
  /** Whether sending the SAME request again is the way to resolve it. */
  readonly retryable: boolean
  /** Whether the run continues (or was continued) and the client should follow it. */
  readonly followRun: boolean
  /** Whether the client reads the approval and the run again before any retry. */
  readonly reread: boolean
}

const NOT_SPENT: ChatRunContinueEffect = {
  approvalSpent: 'no',
  retryable: false,
  followRun: false,
  reread: false,
}

/** What each relayed HTTP code means for the approval. */
export const CHAT_RUN_CONTINUE_HTTP_EFFECTS: Readonly<
  Record<ChatRunContinueHttpRefusalCode, ChatRunContinueEffect>
> = {
  continue_disabled: NOT_SPENT,
  not_resumable: NOT_SPENT,
  approval_not_found: NOT_SPENT,
  approval_not_granted: NOT_SPENT,
  approval_expired: NOT_SPENT,
  approval_not_resumable: NOT_SPENT,
  // Spent by this client earlier, or by another click on the same approval.
  continuation_already_consumed: {
    approvalSpent: 'yes',
    retryable: false,
    followRun: true,
    reread: false,
  },
  // The consume's outcome is ambiguous. The same token resolves it: a retry
  // either wins, or gets a 409 continuation_already_consumed, which means spent.
  continue_unavailable: {
    approvalSpent: 'unknown',
    retryable: true,
    followRun: true,
    reread: false,
  },
}

/** Regime 1: a frame of a continuation the gateway pre-consumed. */
export const CHAT_RUN_CONTINUE_SPENT_EFFECT: ChatRunContinueEffect = {
  approvalSpent: 'yes',
  retryable: false,
  followRun: true,
  reread: false,
}

/** Regime 2: an executor refusal the gateway did not pre-consume. */
export const CHAT_RUN_CONTINUE_UNFLAGGED_EFFECT: ChatRunContinueEffect = {
  approvalSpent: 'unknown',
  retryable: false,
  followRun: false,
  reread: true,
}

export type ChatRunContinueHttpRefusal = ChatRunContinueEffect & {
  readonly code: ChatRunContinueHttpRefusalCode
  readonly status: number
}

export type ChatRunContinueFrameRefusal = ChatRunContinueEffect & {
  readonly regime: 1 | 2
  readonly code: ChatRunContinueFrameRefusalCode
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function isFrameRefusalCode(value: unknown): value is ChatRunContinueFrameRefusalCode {
  return typeof value === 'string' && (CHAT_RUN_CONTINUE_FRAME_REFUSALS as readonly string[]).includes(value)
}

/**
 * Reads a failed continuation request (an `ApiError`, or anything with a string
 * `code` and a numeric `status`). The code AND the status must be a listed pair,
 * as the BFF relays them; anything else is null, which says nothing about the
 * approval (see the reading rule above).
 */
export function chatRunContinueHttpRefusalOf(input: unknown): ChatRunContinueHttpRefusal | null {
  const record = asRecord(input)
  if (!record) return null
  const { code, status } = record
  if (typeof code !== 'string' || typeof status !== 'number') return null
  const entry = CHAT_RUN_CONTINUE_HTTP_REFUSALS.find((candidate) => candidate.code === code && candidate.status === status)
  if (!entry) return null
  return { code: entry.code, status: entry.status, ...CHAT_RUN_CONTINUE_HTTP_EFFECTS[entry.code] }
}

/**
 * Regime 1: an `error` frame whose code is in the tuple AND that carries
 * `approval_spent === true`. A frame code without the flag is never read as
 * spent: that is regime 2, [chatRunContinueUnflaggedRefusalOf].
 */
export function chatRunContinueFrameRefusalOf(frame: unknown): ChatRunContinueFrameRefusal | null {
  const record = asRecord(frame)
  if (!record || record.approval_spent !== true || !isFrameRefusalCode(record.code)) return null
  return { regime: 1, code: record.code, ...CHAT_RUN_CONTINUE_SPENT_EFFECT }
}

/**
 * Regime 2: an `error` frame whose code is in the tuple, without
 * `approval_spent === true`, and not one of the spent-only codes. The executor
 * refused and the gateway does not know whether the approval was spent: the
 * client re-reads the approval and the run before any retry.
 */
export function chatRunContinueUnflaggedRefusalOf(frame: unknown): ChatRunContinueFrameRefusal | null {
  const record = asRecord(frame)
  if (!record || record.approval_spent === true || !isFrameRefusalCode(record.code)) return null
  if ((CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS as readonly string[]).includes(record.code)) return null
  return { regime: 2, code: record.code, ...CHAT_RUN_CONTINUE_UNFLAGGED_EFFECT }
}

/**
 * Regime 1 from the first frame: the `connected` frame of a pre-consumed
 * continuation carries `approval_spent: true`, so the client knows before any
 * step frame that it must never POST this approval again. The committed
 * `connected` payload (`ok: true`, no flag) is null.
 */
export function chatRunContinueSpentConnected(frame: unknown): ChatRunContinueEffect | null {
  const record = asRecord(frame)
  if (!record || record.ok !== true || record.approval_spent !== true) return null
  return CHAT_RUN_CONTINUE_SPENT_EFFECT
}
