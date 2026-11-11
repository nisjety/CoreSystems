import { describe, expect, it } from 'vitest'
import {
  CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE,
  CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE,
  CHAT_RUN_CONTINUE_FRAME_REFUSALS,
  CHAT_RUN_CONTINUE_HTTP_EFFECTS,
  CHAT_RUN_CONTINUE_HTTP_REFUSALS,
  CHAT_RUN_CONTINUE_PARK_EFFECT,
  CHAT_RUN_CONTINUE_PARK_NOT_PARKED_CODES,
  CHAT_RUN_CONTINUE_PARK_UNKNOWN_CODES,
  CHAT_RUN_CONTINUE_PRE_CONSUME_ONLY_HTTP_REFUSALS,
  CHAT_RUN_CONTINUE_RECOVERY_WINDOW_MINUTES,
  CHAT_RUN_CONTINUE_SPENT_EFFECT,
  CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS,
  CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES,
  CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES,
  CHAT_RUN_CONTINUE_TRANSPORT_EFFECT,
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueParkDispatchUnavailableOf,
  chatRunContinueParkFailureOf,
  chatRunContinueParkRefusalOf,
  chatRunContinueSpentConnected,
  chatRunContinueSpentRunEndOf,
  chatRunContinueSpentSubscriptionFailureOf,
  chatRunContinueTransportFailureOf,
  chatRunContinueUnflaggedRefusalOf,
} from './chat-run-continue-contract'
import contractSource from './chat-run-continue-contract.ts?raw'

// The closed vocabulary below is the one model-gateway's /continue route and its
// stream speak (build plan WS03 S7c). The BFF relays the HTTP half
// (apps/gateway CONTINUE_RELAYED_ERRORS); the stream half is relayed byte for
// byte. The HTTP list is pinned to the BFF's list on the Rust side
// (apps/gateway/tests/continue_error_vocabulary.rs), which also pins the HTTP list
// and the 26 stream tokens to model-gateway's own constants
// (CONTINUE_HTTP_REFUSALS, CONTINUE_FRAME_REFUSALS), the three run-end frames
// to CONTINUE_SPENT_RUN_END_FRAMES and the seven typed subscription failures to
// CONTINUE_SPENT_SUBSCRIPTION_FRAMES.

const HTTP_PAIRS = [
  ['continue_disabled', 409],
  ['not_resumable', 409],
  ['approval_not_found', 404],
  ['approval_not_granted', 409],
  ['approval_expired', 409],
  ['approval_not_resumable', 409],
  ['continuation_already_consumed', 409],
  ['continue_unavailable', 503],
] as const

const EXECUTOR_TOKENS = [
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
  'skip_authority_unavailable',
  'park_not_restorable',
  'park_route_unavailable',
  'park_not_due',
] as const

const CODE_DERIVED = ['approval_not_found', 'durable_mutation_forbidden'] as const
const GATEWAY_MADE = ['continue_refused', 'continue_unreachable', 'continue_outcome_unknown'] as const

describe('the closed vocabularies', () => {
  it('lists exactly the 8 relayed HTTP refusals with their statuses', () => {
    expect(CHAT_RUN_CONTINUE_HTTP_REFUSALS.map((entry) => [entry.code, entry.status])).toEqual(
      HTTP_PAIRS.map((pair) => [...pair]),
    )
    expect(CHAT_RUN_CONTINUE_HTTP_REFUSALS).toHaveLength(8)
  })

  it('lists exactly the 26 stream refusals: 21 executor tokens, 2 code-derived, 3 gateway-made', () => {
    expect([...CHAT_RUN_CONTINUE_FRAME_REFUSALS].sort()).toEqual(
      [...EXECUTOR_TOKENS, ...CODE_DERIVED, ...GATEWAY_MADE].sort(),
    )
    expect(CHAT_RUN_CONTINUE_FRAME_REFUSALS).toHaveLength(26)
    expect(EXECUTOR_TOKENS).toHaveLength(21)
  })

  it('lists the three park refusals, the executor refusals of the resume of a parked run', () => {
    for (const code of ['park_not_restorable', 'park_route_unavailable', 'park_not_due']) {
      expect(CHAT_RUN_CONTINUE_FRAME_REFUSALS).toContain(code)
    }
  })

  it('has no duplicate code in either list', () => {
    const http = CHAT_RUN_CONTINUE_HTTP_REFUSALS.map((entry) => entry.code)
    expect(new Set(http).size).toBe(http.length)
    expect(new Set(CHAT_RUN_CONTINUE_FRAME_REFUSALS).size).toBe(CHAT_RUN_CONTINUE_FRAME_REFUSALS.length)
  })

  it('has an effect row for every HTTP code and no other row', () => {
    const rows = Object.keys(CHAT_RUN_CONTINUE_HTTP_EFFECTS).sort()
    expect(rows).toEqual(CHAT_RUN_CONTINUE_HTTP_REFUSALS.map((entry) => entry.code).sort())
  })

  it('keeps the HTTP codes that the stream can also carry inside the stream list', () => {
    // The gateway's HTTP codes and its stream tokens overlap on the refusal
    // class. The HTTP-only ones (a refusal before any stream exists) are the
    // two the stream has no frame for.
    const frame = new Set<string>(CHAT_RUN_CONTINUE_FRAME_REFUSALS)
    const httpOnly = CHAT_RUN_CONTINUE_HTTP_REFUSALS.map((entry) => entry.code).filter((code) => !frame.has(code))
    expect(httpOnly).toEqual(['continue_unavailable'])
  })

  it('marks only the two gateway-made frame codes that need a spent consume as spent-only', () => {
    expect([...CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS].sort()).toEqual([
      'continue_outcome_unknown',
      'continue_unreachable',
    ])
    for (const code of CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS) {
      expect(CHAT_RUN_CONTINUE_FRAME_REFUSALS).toContain(code)
    }
  })
})

// The effect table, pinned row by row: [approvalSpent, retryable, followRun].
// `no` is claimed only for a code that cannot be produced after a consume. The
// gateway checks the switch, then the run's state, then the approval's pairing with
// the run, and only then whether it was already consumed; the consume itself reports
// a replay on a run that has ended as not found. So continue_disabled,
// not_resumable and approval_not_found can all answer a client that retries after a
// dropped stream or a double click for an approval that WAS spent. The other three
// refusals are only ever produced for an approval that is not consumed: an approval
// is granted for good once granted, and the consume answers an already-consumed one
// before it looks at the grant, the expiry or the retained descriptor.
const HTTP_EFFECT_TABLE = {
  continue_disabled: ['unknown', false, true],
  not_resumable: ['unknown', false, true],
  approval_not_found: ['unknown', false, true],
  approval_not_granted: ['no', false, false],
  approval_expired: ['no', false, false],
  approval_not_resumable: ['no', false, false],
  continuation_already_consumed: ['yes', false, true],
  continue_unavailable: ['unknown', true, true],
} as const

describe('chatRunContinueHttpRefusalOf', () => {
  it('pins the whole effect table, row by row, with no re-read field', () => {
    for (const [code, status] of HTTP_PAIRS) {
      const [approvalSpent, retryable, followRun] = HTTP_EFFECT_TABLE[code]
      const read = chatRunContinueHttpRefusalOf({ code, status })
      expect(read).toEqual({ code, status, approvalSpent, retryable, followRun })
      expect(Object.keys(read ?? {}).sort()).toEqual(['approvalSpent', 'code', 'followRun', 'retryable', 'status'])
    }
    expect(Object.keys(HTTP_EFFECT_TABLE).sort()).toEqual(HTTP_PAIRS.map(([code]) => code).sort())
  })

  it('claims not spent only for the three refusals that cannot follow a consume', () => {
    expect([...CHAT_RUN_CONTINUE_PRE_CONSUME_ONLY_HTTP_REFUSALS].sort()).toEqual([
      'approval_expired',
      'approval_not_granted',
      'approval_not_resumable',
    ])
    for (const [code, status] of HTTP_PAIRS) {
      const pre = (CHAT_RUN_CONTINUE_PRE_CONSUME_ONLY_HTTP_REFUSALS as readonly string[]).includes(code)
      expect(chatRunContinueHttpRefusalOf({ code, status })?.approvalSpent === 'no').toBe(pre)
    }
  })

  it('does not call not_resumable or continue_disabled unspent: a retry after a dropped stream, a double click or a switch flipped after a spend gets them for a spent approval', () => {
    for (const [code, status] of [
      ['not_resumable', 409],
      ['continue_disabled', 409],
      ['approval_not_found', 404],
    ] as const) {
      const read = chatRunContinueHttpRefusalOf({ code, status })
      expect(read?.approvalSpent).toBe('unknown')
      expect(read?.retryable).toBe(false)
      // What resolves it is the run, read by its id.
      expect(read?.followRun).toBe(true)
    }
  })

  it('says continuation_already_consumed is spent: by this client earlier or by another click', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'continuation_already_consumed', status: 409 })).toEqual({
      code: 'continuation_already_consumed',
      status: 409,
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
    })
  })

  it('says continue_unavailable is unknown and resolved by the same token, not by a new one', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'continue_unavailable', status: 503 })).toEqual({
      code: 'continue_unavailable',
      status: 503,
      approvalSpent: 'unknown',
      retryable: true,
      followRun: true,
    })
  })

  it('requires the code and the status to agree, as the BFF relay does', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_found', status: 409 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf({ code: 'continue_unavailable', status: 502 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf({ code: 'continuation_already_consumed', status: 200 })).toBeNull()
  })

  it('is null for any failure that is not one of the eight, which the transport guard then reads as unknown', () => {
    // A request the BFF could not complete after delivering it upstream (a send
    // failure, a dropped connection) reaches the client as 502 upstream_unavailable
    // although the gateway may already have consumed the approval. A 401, a
    // model_error and a transport failure are the same. This guard returns null so
    // it never reads them as "not spent"; chatRunContinueTransportFailureOf is the
    // typed answer, so a caller never has to read null as "not a continuation error".
    for (const input of [
      { code: 'upstream_unavailable', status: 502 },
      { code: 'model_error', status: 500 },
      { code: 'model_error', status: 409 },
      { code: 'unauthorized', status: 401 },
      { code: 'continue_incomplete', status: 400 },
      { code: 'missing_bearer', status: 401 },
      { code: null, status: 502 },
      { code: 'agent_dispatch_unavailable', status: 200 },
    ]) {
      expect(chatRunContinueHttpRefusalOf(input)).toBeNull()
      if (input.status >= 400) {
        expect(chatRunContinueTransportFailureOf(input)).toMatchObject({ kind: 'transport', approvalSpent: 'unknown' })
      }
    }
  })

  it('refuses anything that is not an object with a string code and a numeric status', () => {
    for (const input of [null, undefined, 'continue_disabled', 409, {}, { code: 'continue_disabled' }, { status: 409 }, { code: 7, status: 409 }, { code: 'continue_disabled', status: '409' }]) {
      expect(chatRunContinueHttpRefusalOf(input)).toBeNull()
    }
  })

  it('reads a real ApiError by shape', () => {
    // ApiError carries status and code as own properties; the guard needs no import of it.
    const error = Object.assign(new Error('approval not granted'), { status: 409, code: 'approval_not_granted' })
    expect(chatRunContinueHttpRefusalOf(error)?.approvalSpent).toBe('no')
  })
})

describe('chatRunContinueFrameRefusalOf (regime 1: the gateway spent the approval)', () => {
  it('reads every one of the 26 codes as spent, final, and to be followed when approval_spent is true', () => {
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      expect(
        chatRunContinueFrameRefusalOf({ code, message: code.replaceAll('_', ' '), retryable: false, request_id: 'r', run_id: 'run_1', approval_spent: true }),
      ).toEqual({
        regime: 1,
        code,
        approvalSpent: 'yes',
        retryable: false,
        followRun: true,
      })
    }
  })

  it('does not read a frame code as spent without approval_spent === true', () => {
    for (const flag of [undefined, false, 'true', 1, null]) {
      const frame: Record<string, unknown> = { code: 'continuation_already_consumed', retryable: false, run_id: 'run_1' }
      if (flag !== undefined) frame.approval_spent = flag
      expect(chatRunContinueFrameRefusalOf(frame)).toBeNull()
    }
  })

  it('gives null for a code outside the tuple, flagged or not', () => {
    for (const code of ['agent_dispatch_unavailable', 'agent_dispatch_rejected', 'agent_dispatch_unreachable', 'session_terminalization_failed', 'agent_run_failed', 'nope', '']) {
      expect(chatRunContinueFrameRefusalOf({ code, approval_spent: true })).toBeNull()
    }
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'x', 7, [], { approval_spent: true }, { code: 7, approval_spent: true }]) {
      expect(chatRunContinueFrameRefusalOf(input)).toBeNull()
    }
  })
})

describe('chatRunContinueUnflaggedRefusalOf (regime 2: refused by the executor, state unknown)', () => {
  it('reads a tuple code without the flag as unknown, never retried automatically, and resolved by following the run', () => {
    const expected = (code: string) => ({
      regime: 2,
      code,
      approvalSpent: 'unknown',
      retryable: false,
      followRun: true,
    })
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      if ((CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS as readonly string[]).includes(code)) continue
      expect(chatRunContinueUnflaggedRefusalOf({ code, retryable: false })).toEqual(expected(code))
      expect(chatRunContinueUnflaggedRefusalOf({ code, retryable: false, approval_spent: false })).toEqual(expected(code))
    }
  })

  it('never reads the two spent-only codes, which cannot occur without a gateway pre-consume', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS) {
      expect(chatRunContinueUnflaggedRefusalOf({ code })).toBeNull()
    }
  })

  it('gives null for a flagged frame: that is regime 1', () => {
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'not_releasable', approval_spent: true })).toBeNull()
  })

  it('gives null for a code outside the tuple, including the committed dispatch frames of regime 3', () => {
    for (const code of ['agent_dispatch_unavailable', 'agent_dispatch_rejected', 'agent_dispatch_unreachable', 'session_terminalization_failed', 'upstream_unavailable', 'model_error']) {
      expect(chatRunContinueUnflaggedRefusalOf({ code })).toBeNull()
    }
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'not_releasable', 3, [], {}]) {
      expect(chatRunContinueUnflaggedRefusalOf(input)).toBeNull()
    }
  })

  it('keeps the two regimes disjoint for every frame', () => {
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      for (const flag of [true, false, undefined]) {
        const frame: Record<string, unknown> = { code }
        if (flag !== undefined) frame.approval_spent = flag
        const spent = chatRunContinueFrameRefusalOf(frame)
        const unflagged = chatRunContinueUnflaggedRefusalOf(frame)
        expect(spent !== null && unflagged !== null).toBe(false)
      }
    }
  })
})

describe('chatRunContinueSpentRunEndOf (regime 1 for the run-end frames after the executor answered)', () => {
  const spentFrame = (code: string) => ({
    code,
    message: 'x',
    retryable: false,
    request_id: 'r',
    run_id: 'run_1',
    approval_spent: true,
  })

  it('lists exactly the three codes, outside the refusal tuple', () => {
    expect([...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES]).toEqual([
      'agent_run_failed',
      'agent_run_state_unknown',
      'agent_run_output_unavailable',
    ])
    for (const code of CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES) {
      expect((CHAT_RUN_CONTINUE_FRAME_REFUSALS as readonly string[]).includes(code)).toBe(false)
    }
  })

  it('reads each as spent, final and to be followed when approval_spent is true', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES) {
      expect(chatRunContinueSpentRunEndOf(spentFrame(code))).toEqual({
        regime: 1,
        code,
        approvalSpent: 'yes',
        retryable: false,
        followRun: true,
      })
      // The same effect the refusal frames of regime 1 have: one table, no second opinion.
      expect(chatRunContinueSpentRunEndOf(spentFrame(code))).toMatchObject(CHAT_RUN_CONTINUE_SPENT_EFFECT)
    }
  })

  it('never reads them without approval_spent === true: those are the committed, retryable frames of every ordinary run', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES) {
      for (const flag of [undefined, false, 'true', 1, null]) {
        const frame: Record<string, unknown> = { code, retryable: true }
        if (flag !== undefined) frame.approval_spent = flag
        expect(chatRunContinueSpentRunEndOf(frame)).toBeNull()
        // And they are not regime 2 either: the code is not in the refusal tuple.
        expect(chatRunContinueUnflaggedRefusalOf(frame)).toBeNull()
      }
    }
  })

  it('does not read the refusal codes or the committed dispatch frames, flagged or not', () => {
    for (const code of [...CHAT_RUN_CONTINUE_FRAME_REFUSALS, 'agent_dispatch_unavailable', 'agent_dispatch_rejected', ...CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES, 'nope', '']) {
      expect(chatRunContinueSpentRunEndOf({ code, approval_spent: true })).toBeNull()
    }
  })

  it('is not read by the refusal guard: one guard per vocabulary, never both', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES) {
      expect(chatRunContinueFrameRefusalOf(spentFrame(code))).toBeNull()
    }
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      expect(chatRunContinueSpentRunEndOf(spentFrame(code))).toBeNull()
    }
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'agent_run_failed', 7, [], { approval_spent: true }, { code: 7, approval_spent: true }]) {
      expect(chatRunContinueSpentRunEndOf(input)).toBeNull()
    }
  })
})

describe('chatRunContinueSpentSubscriptionFailureOf (regime 1 for the typed subscription failure of a spent run)', () => {
  // The frame keeps its own code and text, and its timing fields; only retryable, run_id and
  // approval_spent are the gateway's after a spend.
  const spentFrame = (code: string) => ({
    code,
    message: 'The usage limit is reached.',
    retryable: false,
    retry_after_ms: 90_000,
    resets_at: '2026-10-19T12:00:00Z',
    retry_at: '2026-10-19T12:00:00Z',
    request_id: 'r',
    run_id: 'run_1',
    approval_spent: true,
  })

  it('lists exactly the seven typed failures, outside every other vocabulary here', () => {
    expect([...CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES]).toEqual([
      'subscription_usage_limit',
      'subscription_reauth_required',
      'subscription_upstream_unavailable',
      'subscription_bad_request',
      'rate_limited',
      'subscription_route_unavailable',
      'prompt_too_long',
    ])
    for (const code of CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES) {
      expect((CHAT_RUN_CONTINUE_FRAME_REFUSALS as readonly string[]).includes(code)).toBe(false)
      expect((CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES as readonly string[]).includes(code)).toBe(false)
    }
  })

  it('reads each as spent, final and to be followed when approval_spent is true', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES) {
      expect(chatRunContinueSpentSubscriptionFailureOf(spentFrame(code))).toEqual({
        regime: 1,
        code,
        approvalSpent: 'yes',
        retryable: false,
        followRun: true,
      })
      expect(chatRunContinueSpentSubscriptionFailureOf(spentFrame(code))).toMatchObject(CHAT_RUN_CONTINUE_SPENT_EFFECT)
    }
  })

  it('says final even for a code whose ordinary frame is retryable: the click, not the code, decides', () => {
    // subscription_upstream_unavailable, rate_limited and subscription_route_unavailable are
    // retryable on an ordinary run; the spent frame says retryable false and so does the effect.
    for (const code of ['subscription_upstream_unavailable', 'rate_limited', 'subscription_route_unavailable']) {
      expect(chatRunContinueSpentSubscriptionFailureOf(spentFrame(code))?.retryable).toBe(false)
    }
  })

  it('never reads them without approval_spent === true: those are the committed frames of every ordinary run', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES) {
      for (const flag of [undefined, false, 'true', 1, null]) {
        const frame: Record<string, unknown> = { code, retryable: true }
        if (flag !== undefined) frame.approval_spent = flag
        expect(chatRunContinueSpentSubscriptionFailureOf(frame)).toBeNull()
        expect(chatRunContinueUnflaggedRefusalOf(frame)).toBeNull()
        expect(chatRunContinueSpentRunEndOf(frame)).toBeNull()
      }
    }
  })

  it('fails closed: a code outside the seven is never trusted as spent, flagged or not', () => {
    for (const code of [
      ...CHAT_RUN_CONTINUE_FRAME_REFUSALS,
      ...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES,
      'agent_dispatch_unavailable',
      'agent_dispatch_rejected',
      'subscription_something_new',
      'subscription_',
      'SUBSCRIPTION_USAGE_LIMIT',
      'nope',
      '',
    ]) {
      expect(chatRunContinueSpentSubscriptionFailureOf({ code, approval_spent: true })).toBeNull()
    }
  })

  it('is read by this guard alone: one guard per vocabulary, never two', () => {
    for (const code of CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES) {
      expect(chatRunContinueFrameRefusalOf(spentFrame(code))).toBeNull()
      expect(chatRunContinueSpentRunEndOf(spentFrame(code))).toBeNull()
    }
    for (const code of [...CHAT_RUN_CONTINUE_FRAME_REFUSALS, ...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES]) {
      expect(chatRunContinueSpentSubscriptionFailureOf(spentFrame(code))).toBeNull()
    }
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'rate_limited', 7, [], { approval_spent: true }, { code: 7, approval_spent: true }]) {
      expect(chatRunContinueSpentSubscriptionFailureOf(input)).toBeNull()
    }
  })
})

describe('chatRunContinueSpentConnected', () => {
  it('reads the connected frame of a pre-consumed continuation as spent: follow the run, never re-POST', () => {
    expect(
      chatRunContinueSpentConnected({ ok: true, run_id: 'run_1', thread_id: 't', request_id: 'r', approval_spent: true }),
    ).toEqual({
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
    })
  })

  it('says nothing for a connected frame without the flag, the committed payload', () => {
    expect(chatRunContinueSpentConnected({ ok: true, run_id: 'run_1', thread_id: 't', request_id: 'r' })).toBeNull()
    expect(chatRunContinueSpentConnected({ ok: true, approval_spent: false })).toBeNull()
    expect(chatRunContinueSpentConnected({ ok: true, approval_spent: 'true' })).toBeNull()
  })

  it('does not read an error frame as a connected frame', () => {
    expect(chatRunContinueSpentConnected({ code: 'not_releasable', approval_spent: true })).toBeNull()
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'x', 1, []]) {
      expect(chatRunContinueSpentConnected(input)).toBeNull()
    }
  })
})

// WS03 S8-3: the resume of a PARKED run. It carries an empty approval id, so there is no
// approval and nothing to spend: its failure frames say `park_continue: true` and never
// `approval_spent`, and only the park guards read them. The park stream's bare
// `agent_dispatch_unavailable` (a follow deadline, a join error) has no marker; it is read
// only when the caller says the stream is a park stream.
const PARK_NOT_PARKED = ['not_resumable', 'park_not_restorable'] as const
const PARK_UNKNOWN = ['continue_outcome_unknown', 'continue_in_progress', 'agent_dispatch_unavailable'] as const
const DISPATCH_FRAMES = [
  'agent_dispatch_unavailable',
  'agent_dispatch_rejected',
  'agent_dispatch_unreachable',
  'session_terminalization_failed',
] as const

const parkFrame = (code: string, extra: Record<string, unknown> = {}): Record<string, unknown> => ({
  code,
  message: code.replaceAll('_', ' '),
  retryable: false,
  request_id: 'r',
  run_id: 'run_1',
  park_continue: true,
  ...extra,
})

const staysParkedOf = (code: string) =>
  (PARK_NOT_PARKED as readonly string[]).includes(code)
    ? 'no'
    : (PARK_UNKNOWN as readonly string[]).includes(code)
      ? 'unknown'
      : 'yes'

describe('the park effect (regime P: the resume of a parked run)', () => {
  it('says nothing is spent and the run is followed, and is not the spent effect or the unknown one', () => {
    expect(CHAT_RUN_CONTINUE_PARK_EFFECT).toEqual({ approvalSpent: 'no', follow: true })
    expect(CHAT_RUN_CONTINUE_PARK_EFFECT.approvalSpent).not.toBe(CHAT_RUN_CONTINUE_SPENT_EFFECT.approvalSpent)
  })

  it('pins which codes end the park, and which leave it unknown', () => {
    expect([...CHAT_RUN_CONTINUE_PARK_NOT_PARKED_CODES].sort()).toEqual([...PARK_NOT_PARKED].sort())
    expect([...CHAT_RUN_CONTINUE_PARK_UNKNOWN_CODES].sort()).toEqual([...PARK_UNKNOWN].sort())
    // Every code the park reader can read is in the tuple, except the one bare dispatch frame.
    for (const code of [...CHAT_RUN_CONTINUE_PARK_NOT_PARKED_CODES, ...CHAT_RUN_CONTINUE_PARK_UNKNOWN_CODES]) {
      expect(code === 'agent_dispatch_unavailable' || (CHAT_RUN_CONTINUE_FRAME_REFUSALS as readonly string[]).includes(code)).toBe(true)
    }
  })
})

describe('chatRunContinueParkRefusalOf (the frames of a park continue)', () => {
  it('reads every one of the 26 codes with the marker: nothing spent, follow the run, and whether the park stays', () => {
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      const read = chatRunContinueParkRefusalOf(parkFrame(code))
      expect(read).toEqual({
        regime: 'park',
        code,
        approvalSpent: 'no',
        follow: true,
        runStaysParked: staysParkedOf(code),
      })
      expect(read).toMatchObject(CHAT_RUN_CONTINUE_PARK_EFFECT)
      // Its own shape: no retryable and no followRun, which belong to the approval regimes.
      expect(Object.keys(read ?? {}).sort()).toEqual(['approvalSpent', 'code', 'follow', 'regime', 'runStaysParked'])
    }
  })

  it('pins the runStaysParked table: no for not_resumable and park_not_restorable, unknown for the in-flight and unknown outcomes, yes for the rest', () => {
    const rows = CHAT_RUN_CONTINUE_FRAME_REFUSALS.map((code) => [code, chatRunContinueParkRefusalOf(parkFrame(code))?.runStaysParked])
    expect(rows.filter(([, stays]) => stays === 'no').map(([code]) => code).sort()).toEqual(['not_resumable', 'park_not_restorable'])
    expect(rows.filter(([, stays]) => stays === 'unknown').map(([code]) => code).sort()).toEqual([
      'continue_in_progress',
      'continue_outcome_unknown',
    ])
    expect(rows.filter(([, stays]) => stays === 'yes')).toHaveLength(CHAT_RUN_CONTINUE_FRAME_REFUSALS.length - 4)
    for (const code of ['park_not_due', 'park_route_unavailable', 'continue_disabled', 'continue_unreachable', 'continue_refused', 'durable_mutation_forbidden']) {
      expect(chatRunContinueParkRefusalOf(parkFrame(code))?.runStaysParked).toBe('yes')
    }
  })

  it('needs the marker to be exactly true: any other value is not a park frame', () => {
    for (const marker of [undefined, false, 'true', 1, null, [], {}]) {
      const frame = parkFrame('park_not_due')
      if (marker === undefined) delete frame.park_continue
      else frame.park_continue = marker
      expect(chatRunContinueParkRefusalOf(frame)).toBeNull()
      expect(chatRunContinueParkFailureOf(frame)).toBeNull()
    }
  })

  it('never reads a frame that carries approval_spent, whatever its value: a park spends nothing', () => {
    for (const flag of [true, false, 'true', 1, null]) {
      for (const code of ['park_not_due', 'continue_unreachable', 'continue_outcome_unknown']) {
        const frame = parkFrame(code, { approval_spent: flag })
        expect(chatRunContinueParkRefusalOf(frame)).toBeNull()
        expect(chatRunContinueParkFailureOf(frame)).toBeNull()
        expect(chatRunContinueParkFailureOf(frame, { parkStream: true })).toBeNull()
        // The regime 2 reader never reads the contradiction either.
        expect(chatRunContinueUnflaggedRefusalOf(frame)).toBeNull()
        // Only the exact claim `approval_spent === true` is read, and then by the SPENT reader
        // (a click spent is never forgotten); any other value is read by nobody.
        if (flag === true) expect(chatRunContinueFrameRefusalOf(frame)).toMatchObject({ regime: 1, code })
        else expect(chatRunContinueFrameRefusalOf(frame)).toBeNull()
      }
    }
  })

  it('trusts nothing outside the closed vocabulary: a code outside the tuple is not read, marked or not', () => {
    const outside = [
      ...DISPATCH_FRAMES,
      ...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES,
      ...CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES,
      'park_not_due ',
      'PARK_NOT_DUE',
      'park_',
      'park_something_new',
      'nope',
      '',
    ]
    for (const code of outside) {
      expect(chatRunContinueParkRefusalOf(parkFrame(code))).toBeNull()
      expect(chatRunContinueParkFailureOf(parkFrame(code))).toBeNull()
      expect(chatRunContinueParkFailureOf(parkFrame(code), { parkStream: true })).toBeNull()
    }
    for (const code of [7, null, undefined, {}, ['park_not_due']]) {
      expect(chatRunContinueParkRefusalOf(parkFrame('x', { code }))).toBeNull()
    }
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'park_not_due', 7, [], { park_continue: true }, { code: 'park_not_due' }]) {
      expect(chatRunContinueParkRefusalOf(input)).toBeNull()
      expect(chatRunContinueParkFailureOf(input)).toBeNull()
      expect(chatRunContinueParkFailureOf(input, { parkStream: true })).toBeNull()
    }
  })
})

describe('chatRunContinueParkDispatchUnavailableOf (the bare frame of a park stream)', () => {
  // The committed frame, as the gateway sends it for a follow deadline or a join error.
  const bare = (): Record<string, unknown> => ({
    code: 'agent_dispatch_unavailable',
    message: 'The agent dispatch outcome is unknown; the run remains retriable.',
    retryable: true,
    request_id: 'r',
    run_id: 'run_1',
  })

  it('reads it as unknown, followed and nothing spent, but only when the caller says the stream is a park stream', () => {
    const expected = {
      regime: 'park',
      code: 'agent_dispatch_unavailable',
      approvalSpent: 'no',
      follow: true,
      runStaysParked: 'unknown',
    }
    expect(chatRunContinueParkDispatchUnavailableOf(bare(), true)).toEqual(expected)
    expect(chatRunContinueParkFailureOf(bare(), { parkStream: true })).toEqual(expected)
    for (const parkStream of [false, undefined, 'true', 1, null]) {
      expect(chatRunContinueParkDispatchUnavailableOf(bare(), parkStream as unknown as boolean)).toBeNull()
      expect(chatRunContinueParkFailureOf(bare(), { parkStream: parkStream as unknown as boolean })).toBeNull()
    }
    // Without a stream context it is the committed frame of an ordinary run: read by no guard here.
    expect(chatRunContinueParkFailureOf(bare())).toBeNull()
    expect(chatRunContinueParkFailureOf(bare(), {})).toBeNull()
  })

  it('reads only that one code, and only without either marker', () => {
    for (const code of [...DISPATCH_FRAMES.filter((c) => c !== 'agent_dispatch_unavailable'), ...CHAT_RUN_CONTINUE_FRAME_REFUSALS, 'nope', '']) {
      expect(chatRunContinueParkDispatchUnavailableOf({ ...bare(), code }, true)).toBeNull()
    }
    expect(chatRunContinueParkDispatchUnavailableOf({ ...bare(), park_continue: true }, true)).toBeNull()
    expect(chatRunContinueParkDispatchUnavailableOf({ ...bare(), park_continue: false }, true)).toBeNull()
    for (const flag of [true, false, 'true', null]) {
      expect(chatRunContinueParkDispatchUnavailableOf({ ...bare(), approval_spent: flag }, true)).toBeNull()
    }
  })

  it('is not read by any approval-regime guard, with or without the stream context', () => {
    expect(chatRunContinueFrameRefusalOf(bare())).toBeNull()
    expect(chatRunContinueUnflaggedRefusalOf(bare())).toBeNull()
    expect(chatRunContinueSpentRunEndOf(bare())).toBeNull()
    expect(chatRunContinueSpentSubscriptionFailureOf(bare())).toBeNull()
  })

  it('gives null for anything that is not a frame object', () => {
    for (const input of [null, undefined, 'agent_dispatch_unavailable', 7, [], {}]) {
      expect(chatRunContinueParkDispatchUnavailableOf(input, true)).toBeNull()
    }
  })
})

describe('the park regime against the other regimes', () => {
  const approvalGuards = (frame: unknown) => [
    chatRunContinueFrameRefusalOf(frame),
    chatRunContinueUnflaggedRefusalOf(frame),
    chatRunContinueSpentRunEndOf(frame),
    chatRunContinueSpentSubscriptionFailureOf(frame),
    chatRunContinueSpentConnected(frame),
  ]

  it('is null in regime 1 and regime 2 and in the spent readers for every park_continue frame without approval_spent === true', () => {
    const codes = [
      ...CHAT_RUN_CONTINUE_FRAME_REFUSALS,
      ...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES,
      ...CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES,
      ...DISPATCH_FRAMES,
    ]
    for (const code of codes) {
      for (const flag of [undefined, false, 'true', 1, null]) {
        const extra: Record<string, unknown> = flag === undefined ? {} : { approval_spent: flag }
        expect(approvalGuards(parkFrame(code, extra)).filter((read) => read !== null)).toEqual([])
      }
    }
    // The connected frame, too: a marked one that does not say it spent is not a spent connect.
    expect(chatRunContinueSpentConnected({ ok: true, park_continue: true })).toBeNull()
    expect(chatRunContinueSpentConnected({ ok: true, approval_spent: false, park_continue: true })).toBeNull()
    expect(chatRunContinueSpentConnected({ ok: true, approval_spent: true })).toEqual(CHAT_RUN_CONTINUE_SPENT_EFFECT)
  })

  it('reads a contradictory frame (park_continue and approval_spent both true) as SPENT, final and never retried, never as a park frame', () => {
    // The stronger claim wins: a click spent is never forgotten, and a retry with a new token
    // (regime 3) is never offered for it, as it was not before the park marker existed.
    const spentRead = (code: string) => {
      const frame = parkFrame(code, { approval_spent: true })
      const reads = approvalGuards(frame).filter((read) => read !== null)
      expect(reads).toHaveLength(1)
      // Never a park reading, in either stream context.
      expect(chatRunContinueParkRefusalOf(frame)).toBeNull()
      expect(chatRunContinueParkFailureOf(frame)).toBeNull()
      expect(chatRunContinueParkFailureOf(frame, { parkStream: true })).toBeNull()
      return reads[0]
    }
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      expect(spentRead(code)).toEqual({ regime: 1, code, ...CHAT_RUN_CONTINUE_SPENT_EFFECT })
    }
    for (const code of CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES) {
      expect(spentRead(code)).toEqual({ regime: 1, code, ...CHAT_RUN_CONTINUE_SPENT_EFFECT })
    }
    for (const code of CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES) {
      expect(spentRead(code)).toEqual({ regime: 1, code, ...CHAT_RUN_CONTINUE_SPENT_EFFECT })
    }
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      const read = chatRunContinueFrameRefusalOf(parkFrame(code, { approval_spent: true }))
      expect(read?.retryable).toBe(false)
      expect(read?.approvalSpent).toBe('yes')
    }
    // The closed vocabulary still rules: a code outside it is read by nobody, spent flag or not.
    for (const code of [...DISPATCH_FRAMES, 'nope', '']) {
      expect(approvalGuards(parkFrame(code, { approval_spent: true })).filter((read) => read !== null)).toEqual([])
    }
    // The contradictory connected frame is a spent connect.
    expect(chatRunContinueSpentConnected({ ok: true, approval_spent: true, park_continue: true })).toEqual(
      CHAT_RUN_CONTINUE_SPENT_EFFECT,
    )
    // Only an exact approval_spent: true is the stronger claim.
    for (const flag of [false, 'true', 1, null]) {
      expect(chatRunContinueSpentConnected({ ok: true, approval_spent: flag, park_continue: true })).toBeNull()
    }
  })

  it('leaves a pure park frame (the marker, no approval_spent) to the park reader alone', () => {
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      const frame = parkFrame(code)
      expect(approvalGuards(frame).filter((read) => read !== null)).toEqual([])
      expect(chatRunContinueParkRefusalOf(frame)).toMatchObject({ regime: 'park', code })
    }
  })

  it('leaves the committed readers as they were for an unmarked frame', () => {
    expect(chatRunContinueFrameRefusalOf({ code: 'park_not_due', approval_spent: true })).toMatchObject({ regime: 1 })
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'park_not_due' })).toMatchObject({ regime: 2 })
    expect(chatRunContinueSpentRunEndOf({ code: 'agent_run_failed', approval_spent: true })).toMatchObject({ regime: 1 })
    expect(chatRunContinueSpentSubscriptionFailureOf({ code: 'rate_limited', approval_spent: true })).toMatchObject({ regime: 1 })
    // Only an exact true is the marker; anything else keeps the committed reading.
    for (const marker of [false, 'true', 1, null]) {
      expect(chatRunContinueUnflaggedRefusalOf({ code: 'park_not_due', park_continue: marker })).toMatchObject({ regime: 2 })
    }
  })

  it('does not read a bare continue_unreachable or continue_outcome_unknown, with no park_continue, as a park frame or as any frame', () => {
    // Without approval_spent these two codes cannot occur on an approval continuation (regime 2
    // never reads them), and without the marker they are not a park frame either.
    for (const code of CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS) {
      for (const flag of [undefined, false]) {
        const frame: Record<string, unknown> = { code, message: code, retryable: false, request_id: 'r', run_id: 'run_1' }
        if (flag !== undefined) frame.approval_spent = flag
        expect(chatRunContinueParkRefusalOf(frame)).toBeNull()
        expect(chatRunContinueParkFailureOf(frame)).toBeNull()
        expect(chatRunContinueParkFailureOf(frame, { parkStream: true })).toBeNull()
        expect(chatRunContinueParkDispatchUnavailableOf(frame, true)).toBeNull()
        expect(approvalGuards(frame).filter((read) => read !== null)).toEqual([])
      }
    }
  })

  it('reads no frame with more than one guard, in either stream context', () => {
    const codes = [
      ...CHAT_RUN_CONTINUE_FRAME_REFUSALS,
      ...CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES,
      ...CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES,
      ...DISPATCH_FRAMES,
      'nope',
    ]
    for (const code of codes) {
      for (const flag of [undefined, false, true]) {
        for (const marker of [undefined, false, true]) {
          for (const parkStream of [false, true]) {
            const frame: Record<string, unknown> = { code }
            if (flag !== undefined) frame.approval_spent = flag
            if (marker !== undefined) frame.park_continue = marker
            const reads = [...approvalGuards(frame), chatRunContinueParkFailureOf(frame, { parkStream })].filter(
              (read) => read !== null,
            )
            expect(reads.length).toBeLessThanOrEqual(1)
          }
        }
      }
    }
  })

  it('says in its own header that regime P exists, which guard reads it, and that it never spends', () => {
    expect(contractSource).toContain('P. A PARK continue')
    expect(contractSource).toContain('park_continue: true')
    expect(contractSource).toContain('chatRunContinueParkFailureOf')
    expect(contractSource).toContain('never `approval_spent`')
    // And that the stronger claim wins when a frame carries both.
    expect(contractSource).toContain('a click spent is never forgotten')
  })
})

describe('chatRunContinueTransportFailureOf (a failure that says nothing about the spend)', () => {
  const effect = { approvalSpent: 'unknown', retryable: true, followRun: true }

  it('exports the effect: spent unknown, the same request resolves it, follow the run', () => {
    expect(CHAT_RUN_CONTINUE_TRANSPORT_EFFECT).toEqual(effect)
  })

  it('reads a 502 upstream_unavailable and a 504 as transport failures', () => {
    expect(chatRunContinueTransportFailureOf({ code: 'upstream_unavailable', status: 502 })).toEqual({
      kind: 'transport',
      reason: 'upstream_unavailable',
      ...effect,
    })
    for (const code of [null, 'gateway_timeout', 'model_error']) {
      expect(chatRunContinueTransportFailureOf({ code, status: 504 })).toEqual({
        kind: 'transport',
        reason: 'gateway_timeout',
        ...effect,
      })
    }
  })

  it('reads a network error, with no status at all, as a transport failure', () => {
    for (const error of [new TypeError('Failed to fetch'), new Error('SSE connection failed (200)'), new DOMException('gone', 'NetworkError')]) {
      expect(chatRunContinueTransportFailureOf(error)).toEqual({ kind: 'transport', reason: 'network', ...effect })
    }
  })

  it('reads the BFF midstream transport frame, an error envelope naming upstream_unavailable, the same way', () => {
    expect(
      chatRunContinueTransportFailureOf({ error: { code: 'upstream_unavailable', message: 'The upstream service is unavailable.' } }),
    ).toEqual({ kind: 'transport', reason: 'upstream_unavailable', ...effect })
  })

  it('reads an unlisted body, whatever its status, as unknown too', () => {
    for (const input of [
      { code: 'model_error', status: 500 },
      { code: 'model_error', status: 409 },
      { code: 'unauthorized', status: 401 },
      { code: 'continue_incomplete', status: 400 },
      { code: 'missing_bearer', status: 401 },
      { code: 'approval_not_found', status: 409 },
      { code: 'continue_unavailable', status: 502 },
      { code: 'model_error', status: 502 },
      { code: null, status: 502 },
      { code: 7, status: 500 },
      { status: 500 },
    ]) {
      expect(chatRunContinueTransportFailureOf(input)).toEqual({ kind: 'transport', reason: 'unlisted', ...effect })
    }
  })

  it('is null for the eight listed refusals: the HTTP guard reads those, and never both', () => {
    for (const [code, status] of HTTP_PAIRS) {
      expect(chatRunContinueTransportFailureOf({ code, status })).toBeNull()
      expect(chatRunContinueHttpRefusalOf({ code, status })).not.toBeNull()
    }
  })

  it('is null for a value that is not a failed request: nothing thrown, a success status, no object', () => {
    for (const input of [null, undefined, 'upstream_unavailable', 502, true, [], { code: 'x', status: 200 }, { code: 'x', status: 204 }]) {
      expect(chatRunContinueTransportFailureOf(input)).toBeNull()
    }
  })

  it('leaves no failed request unread: every object with an error status is read by exactly one of the two guards', () => {
    const inputs: unknown[] = [
      ...HTTP_PAIRS.map(([code, status]) => ({ code, status })),
      { code: 'upstream_unavailable', status: 502 },
      { code: 'model_error', status: 500 },
      { status: 504 },
      new TypeError('Failed to fetch'),
    ]
    for (const input of inputs) {
      const refusal = chatRunContinueHttpRefusalOf(input)
      const transport = chatRunContinueTransportFailureOf(input)
      expect([refusal, transport].filter((read) => read !== null)).toHaveLength(1)
    }
  })
})

describe('the wording of the contract', () => {
  const GUIDANCE = CHAT_RUN_CONTINUE_AMBIGUOUS_FAILURE_GUIDANCE

  it('tells a client what it CAN do after an ambiguous failure: no assumption, follow the run by its id, resend the same token', () => {
    expect(GUIDANCE).toContain('do not assume')
    expect(GUIDANCE).toContain('Follow the run by its id with the existing run read')
    expect(GUIDANCE).toContain('the same claim token')
    expect(GUIDANCE).toContain('409 continuation_already_consumed')
    expect(GUIDANCE).toContain('the click was spent')
  })

  it('says a retry is answered with the continuation or with a refusal, and only 409 continuation_already_consumed means spent', () => {
    expect(GUIDANCE).toContain('answered either with the continuation or with a refusal')
    expect(GUIDANCE).toContain('Only 409 continuation_already_consumed means the click was spent')
    // The three refusals a spent approval can also get leave the spend unknown, and
    // the run read decides.
    for (const code of ['not_resumable', 'approval_not_found', 'continue_disabled']) {
      expect(GUIDANCE).toContain(code)
    }
    expect(GUIDANCE).toContain('leave the spend unknown')
    expect(GUIDANCE).toContain('the run read is the arbiter')
  })

  it('never claims that a retry that is not a 409 continues the run, or that a refusal other than the 409 shows the approval unspent', () => {
    const wording = [GUIDANCE, CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE, contractSource]
    for (const text of wording) {
      expect(text).not.toMatch(/otherwise it continues/i)
      expect(text).not.toMatch(/either the continuation or 409/i)
      expect(text).not.toMatch(/the retry is the continuation, or 409/i)
      expect(text).not.toMatch(/either wins, or gets a 409/i)
    }
  })

  it('never tells a client to re-read an approval: no read it has shows whether one was spent', () => {
    const wording = [GUIDANCE, CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE, contractSource]
    for (const text of wording) {
      expect(text).not.toMatch(/re-?read\s+the\s+approval/i)
      expect(text).not.toMatch(/reread/i)
    }
  })

  it('says honestly that following a run does not promise a live run, and names the recovery window', () => {
    const note = CHAT_RUN_CONTINUE_FOLLOW_RUN_NOTE
    expect(CHAT_RUN_CONTINUE_RECOVERY_WINDOW_MINUTES).toBe(15)
    expect(note).toContain('read the run by its id')
    expect(note).toContain('does not promise that a run is live')
    expect(note).toContain('continue_unavailable')
    expect(note).toContain('409 continuation_already_consumed')
    expect(note).toContain('no run was launched')
    expect(note).toContain('recovery window')
    expect(note).toContain(`about ${CHAT_RUN_CONTINUE_RECOVERY_WINDOW_MINUTES} minutes from the consume`)
  })

  it('puts the followRun caveat on the effect field itself, where a reader of the table finds it', () => {
    // One doc comment, immediately above the field: no comment may contain a `*/`.
    const field = contractSource.match(/\/\*\*(?:[^*]|\*(?!\/))*\*\/\s*readonly followRun: boolean/)?.[0] ?? ''
    expect(field).toContain('read the run by its id')
    expect(field).toContain('not that a run is live')
  })
})
