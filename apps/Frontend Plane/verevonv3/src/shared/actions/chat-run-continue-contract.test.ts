import { describe, expect, it } from 'vitest'
import {
  CHAT_RUN_CONTINUE_FRAME_REFUSALS,
  CHAT_RUN_CONTINUE_HTTP_EFFECTS,
  CHAT_RUN_CONTINUE_HTTP_REFUSALS,
  CHAT_RUN_CONTINUE_SPENT_ONLY_FRAME_REFUSALS,
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueSpentConnected,
  chatRunContinueUnflaggedRefusalOf,
} from './chat-run-continue-contract'

// The closed vocabulary below is the one model-gateway's /continue route and its
// stream speak (build plan WS03 S7c). The BFF relays the HTTP half
// (apps/gateway CONTINUE_RELAYED_ERRORS); the stream half is relayed byte for
// byte. Both lists are test-pinned on the Rust side as well
// (apps/gateway/tests/continue_error_vocabulary.rs).

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

  it('lists exactly the 22 stream refusals: 17 executor tokens, 2 code-derived, 3 gateway-made', () => {
    expect([...CHAT_RUN_CONTINUE_FRAME_REFUSALS].sort()).toEqual(
      [...EXECUTOR_TOKENS, ...CODE_DERIVED, ...GATEWAY_MADE].sort(),
    )
    expect(CHAT_RUN_CONTINUE_FRAME_REFUSALS).toHaveLength(22)
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

describe('chatRunContinueHttpRefusalOf', () => {
  it('says not spent, no retry and no follow for the pre-consume refusals', () => {
    for (const [code, status] of HTTP_PAIRS) {
      if (code === 'continuation_already_consumed' || code === 'continue_unavailable') continue
      expect(chatRunContinueHttpRefusalOf({ code, status })).toEqual({
        code,
        status,
        approvalSpent: 'no',
        retryable: false,
        followRun: false,
        reread: false,
      })
    }
  })

  it('says continuation_already_consumed is spent: by this client earlier or by another click', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'continuation_already_consumed', status: 409 })).toEqual({
      code: 'continuation_already_consumed',
      status: 409,
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
      reread: false,
    })
  })

  it('says continue_unavailable is unknown and resolved by the same token, not by a new one', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'continue_unavailable', status: 503 })).toEqual({
      code: 'continue_unavailable',
      status: 503,
      approvalSpent: 'unknown',
      retryable: true,
      followRun: true,
      reread: false,
    })
  })

  it('requires the code and the status to agree, as the BFF relay does', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_found', status: 409 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf({ code: 'continue_unavailable', status: 502 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf({ code: 'continuation_already_consumed', status: 200 })).toBeNull()
  })

  it('says nothing for any other failure of the request, which is never read as not spent', () => {
    // A request the BFF could not complete after delivering it upstream (a send
    // failure, a dropped connection) reaches the client as 502 upstream_unavailable
    // although the gateway may already have consumed the approval. A 401, a
    // model_error and a transport failure are the same: the guard returns null so
    // the caller cannot read the absence of a refusal as "not spent".
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
    }
  })

  it('refuses anything that is not an object with a string code and a numeric status', () => {
    for (const input of [null, undefined, 'continue_disabled', 409, {}, { code: 'continue_disabled' }, { status: 409 }, { code: 7, status: 409 }, { code: 'continue_disabled', status: '409' }]) {
      expect(chatRunContinueHttpRefusalOf(input)).toBeNull()
    }
  })

  it('reads a real ApiError by shape', () => {
    // ApiError carries status and code as own properties; the guard needs no import of it.
    const error = Object.assign(new Error('approval not found'), { status: 404, code: 'approval_not_found' })
    expect(chatRunContinueHttpRefusalOf(error)?.approvalSpent).toBe('no')
  })
})

describe('chatRunContinueFrameRefusalOf (regime 1: the gateway spent the approval)', () => {
  it('reads every one of the 22 codes as spent, final, and to be followed when approval_spent is true', () => {
    for (const code of CHAT_RUN_CONTINUE_FRAME_REFUSALS) {
      expect(
        chatRunContinueFrameRefusalOf({ code, message: code.replaceAll('_', ' '), retryable: false, request_id: 'r', run_id: 'run_1', approval_spent: true }),
      ).toEqual({
        regime: 1,
        code,
        approvalSpent: 'yes',
        retryable: false,
        followRun: true,
        reread: false,
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
  it('reads a tuple code without the flag as unknown, not retryable, not followed and re-read', () => {
    const expected = (code: string) => ({
      regime: 2,
      code,
      approvalSpent: 'unknown',
      retryable: false,
      followRun: false,
      reread: true,
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

describe('chatRunContinueSpentConnected', () => {
  it('reads the connected frame of a pre-consumed continuation as spent: follow the run, never re-POST', () => {
    expect(
      chatRunContinueSpentConnected({ ok: true, run_id: 'run_1', thread_id: 't', request_id: 'r', approval_spent: true }),
    ).toEqual({
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
      reread: false,
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
