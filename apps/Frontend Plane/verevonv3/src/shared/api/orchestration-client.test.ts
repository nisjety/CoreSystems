import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueSpentConnected,
  chatRunContinueUnflaggedRefusalOf,
  continueRun,
  decideApproval,
  listApprovals,
  normalizePlan,
  planStepEditToWire,
} from './orchestration-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

/**
 * Regression coverage for a real bug: model-gateway's orchestration HTTP
 * layer reports approval state as the FULL protobuf enum name
 * (`APPROVAL_STATE_REQUESTED`, `model_plane/v1/orchestration.proto`'s
 * `ApprovalState`) — never the bare `PENDING`/`GRANTED`/`DENIED`/`EXPIRED`
 * every caller of `listApprovals` filters on (`AgentRunConsole.tsx`,
 * `use-chat-controller.ts`, and `KnowledgeComposer.tsx`'s browser-workspace
 * HITL gate all do `(approval.status ?? 'PENDING').toUpperCase() ===
 * 'PENDING'`). Before `normalizeApproval` canonicalized this, a real,
 * correctly-fired backend approval gate looked to every one of those UIs
 * like there was nothing pending to decide — confirmed live via
 * `tests/e2e/browser-workspace-hitl.spec.ts`, which hung for the full
 * approval flow until this was fixed.
 */
describe('listApprovals', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('canonicalizes the live wire format (full APPROVAL_STATE_* enum name) to the short PENDING form', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        approvals: [
          {
            id: 'appr_1',
            run_id: 'run_1',
            kind: 'APPROVAL_KIND_DESTRUCTIVE',
            state: 'APPROVAL_STATE_REQUESTED',
          },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const approvals = await listApprovals('run_1')

    expect(approvals).toEqual([
      {
        id: 'appr_1',
        runId: 'run_1',
        planId: undefined,
        stepId: undefined,
        kind: 'APPROVAL_KIND_DESTRUCTIVE',
        status: 'PENDING',
        requestedBy: undefined,
        detail: undefined,
      },
    ])
    // The exact check every consumer performs — must actually match now.
    expect((approvals[0]?.status ?? '').toUpperCase()).toBe('PENDING')
  })

  it('still recognizes an already-short status string (defensive, in case a caller sends one)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ approvals: [{ id: 'appr_2', run_id: 'run_2', status: 'PENDING' }] }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const approvals = await listApprovals('run_2')
    expect(approvals[0]?.status).toBe('PENDING')
  })

  it('canonicalizes granted/denied/timed-out terminal states', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        approvals: [
          { id: 'a', run_id: 'r', state: 'APPROVAL_STATE_GRANTED' },
          { id: 'b', run_id: 'r', state: 'APPROVAL_STATE_DENIED' },
          { id: 'c', run_id: 'r', state: 'APPROVAL_STATE_TIMED_OUT' },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const approvals = await listApprovals('r')
    expect(approvals.map((a) => a.status)).toEqual(['GRANTED', 'DENIED', 'EXPIRED'])
  })
})

describe('decideApproval', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts the decision and normalizes the returned approval', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ approval: { id: 'appr_1', run_id: 'run_1', state: 'APPROVAL_STATE_GRANTED' } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await decideApproval('appr_1', 'approve')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/orchestration/approvals/appr_1/decide')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({ decision: 'approve', reason: '' })
    expect(result?.status).toBe('GRANTED')
  })
})

/** plan-gate §2.2/§2.3: the wire shape `approvePlan`/`rejectPlan` share. */
describe('planStepEditToWire', () => {
  it('defaults an empty stepId and operation to empty strings, never undefined', () => {
    expect(planStepEditToWire({ title: 'New step' })).toEqual({
      step_id: '',
      title: 'New step',
      operation: '',
      removed: false,
    })
  })

  it('carries every field through explicitly', () => {
    expect(
      planStepEditToWire({
        stepId: 'step_1',
        title: 'Draft the email',
        operation: 'email_send',
        removed: true,
      }),
    ).toEqual({
      step_id: 'step_1',
      title: 'Draft the email',
      operation: 'email_send',
      removed: true,
    })
  })
})

describe('normalizePlan', () => {
  it('reads the full plan_value wire shape model-gateway serializes', () => {
    const plan = normalizePlan({
      id: 'plan_1',
      run_id: 'run_1',
      thread_id: 'thread_1',
      author: 'user_1',
      state: 'PLAN_STATE_PROPOSED',
      summary: 'Send the overdue invoice',
      steps: [
        {
          id: 'step_1',
          title: 'Draft the email',
          operation: '',
          state: 'PLAN_STEP_STATE_PENDING',
          detail: null,
        },
      ],
      supersedes: null,
    })
    expect(plan).toEqual({
      id: 'plan_1',
      runId: 'run_1',
      threadId: 'thread_1',
      author: 'user_1',
      state: 'PLAN_STATE_PROPOSED',
      summary: 'Send the overdue invoice',
      steps: [
        {
          id: 'step_1',
          title: 'Draft the email',
          operation: undefined,
          state: 'PLAN_STEP_STATE_PENDING',
          detail: undefined,
        },
      ],
    })
  })

  it('returns null for a payload with no readable id', () => {
    expect(normalizePlan({ summary: 'no id here' })).toBeNull()
  })
})

describe('continueRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts only the approval id and claim token, and relays the continuation frames', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response('event: step_update\ndata: {"status":"running"}\n\n', {
        status: 200,
        headers: { 'Content-Type': 'text/event-stream' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const events: string[] = []

    await continueRun('run_1', 'appr_1', 'claim_1', (event) => {
      events.push(event.event ?? '')
    })

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toContain('/api/v1/orchestration/runs/run_1/continue')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({
      approval_id: 'appr_1',
      claim_token: 'claim_1',
    })
    expect(events).toEqual(['step_update'])
  })

  it('rejects with the gateway error code when the continuation is refused', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({ error: { code: 'approval_not_granted', message: 'approval not granted' } }),
          { status: 409, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )

    await expect(continueRun('run_1', 'appr_1', 'claim_1', () => {})).rejects.toMatchObject({
      status: 409,
      code: 'approval_not_granted',
    })
  })

  function refusedWith(status: number, code: string): void {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: { code, message: code.replaceAll('_', ' ') } }), {
          status,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
  }

  async function rejectionOf(): Promise<unknown> {
    try {
      await continueRun('run_1', 'appr_1', 'claim_1', () => {})
    } catch (error) {
      return error
    }
    throw new Error('continueRun resolved; a refusal was expected')
  }

  it('rejects with continuation_already_consumed / 409 and the contract reads it as spent', async () => {
    refusedWith(409, 'continuation_already_consumed')

    const error = await rejectionOf()

    expect(error).toMatchObject({ status: 409, code: 'continuation_already_consumed' })
    expect(chatRunContinueHttpRefusalOf(error)).toMatchObject({
      code: 'continuation_already_consumed',
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
    })
  })

  it('rejects with continue_unavailable / 503 and the contract reads it as unknown, resolved by the same token', async () => {
    refusedWith(503, 'continue_unavailable')

    const error = await rejectionOf()

    expect(error).toMatchObject({ status: 503, code: 'continue_unavailable' })
    expect(chatRunContinueHttpRefusalOf(error)).toMatchObject({
      code: 'continue_unavailable',
      approvalSpent: 'unknown',
      retryable: true,
      followRun: true,
    })
  })

  it('does not read a BFF send failure as a refusal: a 502 upstream_unavailable says nothing about the spend', async () => {
    refusedWith(502, 'upstream_unavailable')

    const error = await rejectionOf()

    expect(error).toMatchObject({ status: 502, code: 'upstream_unavailable' })
    expect(chatRunContinueHttpRefusalOf(error)).toBeNull()
  })
})

describe('the chat.run.continue contract guards, re-exported by the client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads an HTTP refusal only for a listed code and status pair', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_found', status: 404 })).toMatchObject({
      approvalSpent: 'no',
      retryable: false,
      followRun: false,
    })
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_found', status: 409 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf({ code: 'model_error', status: 409 })).toBeNull()
    expect(chatRunContinueHttpRefusalOf(null)).toBeNull()
  })

  it('does not treat a frame code without approval_spent true as spent', () => {
    expect(chatRunContinueFrameRefusalOf({ code: 'not_releasable', retryable: false })).toBeNull()
    expect(chatRunContinueFrameRefusalOf({ code: 'not_releasable', approval_spent: false })).toBeNull()
    expect(chatRunContinueFrameRefusalOf({ code: 'not_releasable', approval_spent: true })).toMatchObject({
      regime: 1,
      code: 'not_releasable',
      approvalSpent: 'yes',
      retryable: false,
      followRun: true,
    })
  })

  it('reads an unflagged tuple code as unknown, not retryable and to be re-read', () => {
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'continuation_already_consumed', retryable: false })).toEqual({
      regime: 2,
      code: 'continuation_already_consumed',
      approvalSpent: 'unknown',
      retryable: false,
      followRun: false,
      reread: true,
    })
  })

  it('gives null for a non-tuple code and for a flagged frame on the unflagged guard', () => {
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'agent_dispatch_unavailable' })).toBeNull()
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'not_releasable', approval_spent: true })).toBeNull()
    expect(chatRunContinueFrameRefusalOf({ code: 'agent_dispatch_unavailable', approval_spent: true })).toBeNull()
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'unheard_of' })).toBeNull()
  })

  it('reads the connected frame of a pre-consumed continuation as spent', () => {
    expect(
      chatRunContinueSpentConnected({ ok: true, run_id: 'run_1', thread_id: 'thr_1', request_id: 'req_1', approval_spent: true }),
    ).toMatchObject({ approvalSpent: 'yes', retryable: false, followRun: true })
    expect(chatRunContinueSpentConnected({ ok: true, run_id: 'run_1', thread_id: 'thr_1', request_id: 'req_1' })).toBeNull()
  })

  it('relays a spent frame through continueRun untouched, so the SPA reads the flag from the raw data', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          'event: connected\ndata: {"ok":true,"run_id":"run_1","approval_spent":true}\n\n'
            + 'event: error\ndata: {"code":"continue_unreachable","retryable":false,"run_id":"run_1","approval_spent":true}\n\n',
          { status: 200, headers: { 'Content-Type': 'text/event-stream' } },
        ),
      ),
    )
    const frames: Array<[string, unknown]> = []

    await continueRun('run_1', 'appr_1', 'claim_1', (event) => {
      frames.push([event.event ?? '', JSON.parse(event.data)])
    })

    expect(frames.map(([name]) => name)).toEqual(['connected', 'error'])
    const [connected, refusal] = frames.map(([, data]) => data)
    expect(chatRunContinueSpentConnected(connected)).not.toBeNull()
    expect(chatRunContinueFrameRefusalOf(refusal)).toMatchObject({ regime: 1, code: 'continue_unreachable' })
  })
})
