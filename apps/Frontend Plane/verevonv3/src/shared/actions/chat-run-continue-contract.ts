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
//      know whether the executor spent the approval first, so the spend is
//      unknown. The client never retries automatically; it follows the run (see
//      the guidance below).
//   3. The committed dispatch frames (`agent_dispatch_unavailable`,
//      `agent_dispatch_rejected`, `agent_dispatch_unreachable`,
//      `session_terminalization_failed`): they say nothing about the approval,
//      so they are not in this vocabulary and every guard here returns null for
//      them. A retry with a new token is what the committed rules permit.
//
// What the spend flag can honestly say. `no` is claimed only for a refusal that
// cannot be produced after a consume. Most refusals can: the gateway checks the
// switch, the run's state and the approval's pairing with the run BEFORE it asks
// whether the approval was already consumed, and the consume reports a replay on a
// run that has ended as not found. So a client that retries after a dropped stream,
// or double-clicks, can be told `not_resumable`, `continue_disabled` or
// `approval_not_found` for an approval that WAS spent. Those three are `unknown`.
//
// Reading rule for callers. Read a thrown value with
// [chatRunContinueHttpRefusalOf] first (the eight listed pairs), then with
// [chatRunContinueTransportFailureOf] (everything else that failed: a 502
// `upstream_unavailable`, a 504, a network error, an unlisted body). Between them
// they read every failed request, and none of them says the approval was unspent
// unless the code cannot follow a spend. A null from both means the value is not a
// failed request at all.
//
// No read the SPA has shows whether an approval was spent (the approval carries no
// consumed state), so nothing here asks a client to look at the approval. The
// guidance a client CAN follow is [CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE]
// and what following means is [CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE].

/**
 * About how long, from the consume, a run whose continuation died is left alone
 * before the platform recovers it (the renewed recovery window of the spend-only
 * consume).
 */
export const CHAT_RUN_CONTINUE_RECOVERY_WINDOW_MINUTES = 15

/**
 * What a client does after an ambiguous failure (an effect whose `approvalSpent`
 * is `unknown`). It rests on the standing rules: every consumed approval is
 * refused with 409 `continuation_already_consumed`, the same token included, and
 * an ambiguous consume ends in that 409 on retry.
 */
export const CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE =
  'After an ambiguous failure do not assume either way: the approval may or may not have been spent. '
  + 'Follow the run by its id with the existing run read; the run read is the arbiter. '
  + 'Sending the request again with the same claim token is answered either with the continuation or with a refusal. '
  + 'Only 409 continuation_already_consumed means the click was spent. '
  + 'A refusal of any other code is not a continuation, and not_resumable, approval_not_found and continue_disabled '
  + 'leave the spend unknown: a spent approval whose run has since ended, or whose switch was turned off, gets them too.'

/** What `followRun` means, and what it does not promise. */
export const CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE =
  'followRun means read the run by its id; it does not promise that a run is live. '
  + 'After continue_unavailable a retry with the same token can be answered 409 continuation_already_consumed, '
  + 'which means spent, although no run was launched: the run stays parked and may still be recovered '
  + `inside the recovery window, about ${CHAT_RUN_CONTINUE_RECOVERY_WINDOW_MINUTES} minutes from the consume.`

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

/**
 * Whether the approval's claim token was spent: `yes`, `no` (only for a refusal
 * that cannot follow a consume) or `unknown`. `unknown` is resolved by following
 * the run, see [CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE].
 */
export type ChatRunContinueApprovalSpent = 'no' | 'yes' | 'unknown'

export type ChatRunContinueEffect = {
  readonly approvalSpent: ChatRunContinueApprovalSpent
  /**
   * Whether sending the SAME request again (the same claim token) is the way to
   * resolve it. Never automatic after an `unknown`: the answer is the continuation
   * or a refusal, and only 409 `continuation_already_consumed` means spent (see
   * [CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE]).
   */
  readonly retryable: boolean
  /**
   * Whether the client should read the run by its id (the existing run read) to
   * see what became of it, not that a run is live: see
   * [CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE]. After continue_unavailable a later 409
   * means spent while no run may have been launched, and the run may still be
   * recovered inside the recovery window, about 15 minutes from the consume.
   */
  readonly followRun: boolean
}

const UNSPENT: ChatRunContinueEffect = {
  approvalSpent: 'no',
  retryable: false,
  followRun: false,
}

/** The spend is unknown and no request of the same kind resolves it: follow the run. */
const UNKNOWN_FOLLOW: ChatRunContinueEffect = {
  approvalSpent: 'unknown',
  retryable: false,
  followRun: true,
}

/**
 * The refusals that are only ever produced for an approval that is not consumed
 * (build plan WS03 S7c; the gateway's consume refusals): an approval is granted
 * for good once granted, and the consume answers an already-consumed approval
 * before it looks at the grant, the expiry or the retained descriptor. Only these
 * say `no`.
 */
export const CHAT_RUN_CONTINUE_PRE_CONSUME_ONLY_HTTP_REFUSALS = [
  'approval_not_granted',
  'approval_expired',
  'approval_not_resumable',
] as const

/** What each relayed HTTP code means for the approval. */
export const CHAT_RUN_CONTINUE_HTTP_EFFECTS: Readonly<
  Record<ChatRunContinueHttpRefusalCode, ChatRunContinueEffect>
> = {
  // The three below are checked before the consumed flag, so a retry after a
  // spend can hit them: a switch flipped off after the click, a run that has
  // finished since (a dropped stream, a double click), a consume that reports a
  // replay on an ended run as not found.
  continue_disabled: UNKNOWN_FOLLOW,
  not_resumable: UNKNOWN_FOLLOW,
  approval_not_found: UNKNOWN_FOLLOW,
  approval_not_granted: UNSPENT,
  approval_expired: UNSPENT,
  approval_not_resumable: UNSPENT,
  // Spent by this client earlier, or by another click on the same approval. The
  // run may be live, finished, or parked (see the followRun note).
  continuation_already_consumed: {
    approvalSpent: 'yes',
    retryable: false,
    followRun: true,
  },
  // The consume's outcome is ambiguous. A retry with the same claim token is
  // answered with the continuation or with a refusal; a 409
  // continuation_already_consumed means spent, any other refusal leaves it open.
  continue_unavailable: {
    approvalSpent: 'unknown',
    retryable: true,
    followRun: true,
  },
}

/** Regime 1: a frame of a continuation the gateway pre-consumed. */
export const CHAT_RUN_CONTINUE_SPENT_EFFECT: ChatRunContinueEffect = {
  approvalSpent: 'yes',
  retryable: false,
  followRun: true,
}

/** Regime 2: an executor refusal the gateway did not pre-consume. */
export const CHAT_RUN_CONTINUE_UNFLAGGED_EFFECT: ChatRunContinueEffect = UNKNOWN_FOLLOW

/**
 * A transport failure: a 502 `upstream_unavailable`, a 504, a network error, an
 * unlisted body. The request may or may not have reached the gateway, and the
 * gateway may have consumed the approval before it failed. A retry with the same
 * claim token is answered with the continuation or with a refusal; only 409
 * `continuation_already_consumed` means spent.
 */
export const CHAT_RUN_CONTINUE_TRANSPORT_EFFECT: ChatRunContinueEffect = {
  approvalSpent: 'unknown',
  retryable: true,
  followRun: true,
}

export type ChatRunContinueHttpRefusal = ChatRunContinueEffect & {
  readonly code: ChatRunContinueHttpRefusalCode
  readonly status: number
}

export type ChatRunContinueFrameRefusal = ChatRunContinueEffect & {
  readonly regime: 1 | 2
  readonly code: ChatRunContinueFrameRefusalCode
}

/** Why a failure is a transport failure (see [chatRunContinueTransportFailureOf]). */
export type ChatRunContinueTransportReason =
  | 'upstream_unavailable'
  | 'gateway_timeout'
  | 'network'
  | 'unlisted'

export type ChatRunContinueTransportFailure = ChatRunContinueEffect & {
  readonly kind: 'transport'
  readonly reason: ChatRunContinueTransportReason
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
 * approval: read it with [chatRunContinueTransportFailureOf] (see the reading rule
 * above).
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
 * Reads every other failure of a continuation request as a transport failure:
 * spent unknown, the same request resolves it, follow the run
 * ([CHAT_RUN_CONTINUE_TRANSPORT_EFFECT]). It exists so that a caller never has to
 * read the null of [chatRunContinueHttpRefusalOf] as "this is not a continuation
 * error".
 *
 * Reads, by reason:
 *  - `upstream_unavailable`: 502 `upstream_unavailable` (the BFF could not finish
 *    the call to model-gateway, possibly after delivering it), or the BFF's
 *    midstream error envelope `{ error: { code: 'upstream_unavailable' } }`;
 *  - `gateway_timeout`: any 504;
 *  - `network`: an error with no HTTP status (a failed fetch, a stream that broke);
 *  - `unlisted`: any other failed status, such as a `model_error` or an
 *    `unauthorized`, or one of the eight codes under a status the BFF never gives it.
 *
 * Null for the eight listed pairs (the HTTP guard reads those), for a status below
 * 400, and for anything that is not an object, so exactly one of the two guards
 * reads a failed request.
 */
export function chatRunContinueTransportFailureOf(input: unknown): ChatRunContinueTransportFailure | null {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) return null
  if (chatRunContinueHttpRefusalOf(input) !== null) return null
  const record = input as Record<string, unknown>
  const { status, code } = record
  const transport = (reason: ChatRunContinueTransportReason): ChatRunContinueTransportFailure => ({
    kind: 'transport',
    reason,
    ...CHAT_RUN_CONTINUE_TRANSPORT_EFFECT,
  })
  if (typeof status === 'number') {
    if (status < 400) return null
    if (status === 504) return transport('gateway_timeout')
    return transport(status === 502 && code === 'upstream_unavailable' ? 'upstream_unavailable' : 'unlisted')
  }
  if (asRecord(record.error)?.code === 'upstream_unavailable') return transport('upstream_unavailable')
  // An error object, by shape as well as by class: a DOMException from another
  // realm is not `instanceof Error` there.
  const errorLike = input instanceof Error || (typeof record.name === 'string' && typeof record.message === 'string')
  return transport(errorLike ? 'network' : 'unlisted')
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
 * refused and the gateway does not know whether the approval was spent: the spend
 * is unknown, nothing is retried automatically, and the client follows the run
 * ([CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE]).
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
