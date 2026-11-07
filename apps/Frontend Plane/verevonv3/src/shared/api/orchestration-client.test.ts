import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './http'
import {
  answerQuestion,
  approvalEditRefusalOf,
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueParkFailureOf,
  chatRunContinueSpentConnected,
  chatRunContinueTransportFailureOf,
  chatRunContinueUnflaggedRefusalOf,
  continueParkedRun,
  continueRun,
  decideApproval,
  getApprovalPreview,
  listApprovals,
  listRunQuestions,
  normalizePlan,
  normalizeRunQuestion,
  planStepEditToWire,
  runQuestionErrorCodeOf,
} from './orchestration-client'
import clientSource from './orchestration-client.ts?raw'

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

  it('reads the allowlisted cause model-gateway projects, and nothing it does not project', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        approvals: [
          {
            id: 'appr_1',
            run_id: 'run_1',
            kind: 'APPROVAL_KIND_DESTRUCTIVE',
            state: 'APPROVAL_STATE_REQUESTED',
            cause: {
              code: 'browser_action_risk',
              tool: null,
              effect_class: null,
              risk_category: 'checkout',
              rule_id: null,
              scope_options: ['once'],
              step_up_required: false,
            },
          },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const [approval] = await listApprovals('run_1')

    expect(approval?.cause).toEqual({
      code: 'browser_action_risk',
      riskCategory: 'checkout',
      scopeOptions: ['once'],
      stepUpRequired: false,
    })
  })

  it('reads a code it does not know as unclassified_gate and an absent or malformed cause as none', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        approvals: [
          { id: 'a', cause: { code: 'a_code_from_a_newer_server' } },
          { id: 'b' },
          { id: 'c', cause: null },
          { id: 'd', cause: 'provider_write' },
          { id: 'e', cause: ['provider_write'] },
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const approvals = await listApprovals('r')

    expect(approvals[0]?.cause?.code).toBe('unclassified_gate')
    expect(approvals[0]?.cause?.scopeOptions).toEqual([])
    expect(approvals[0]?.cause?.stepUpRequired).toBe(false)
    expect(approvals.slice(1).map((approval) => approval.cause)).toEqual([
      undefined,
      undefined,
      undefined,
      undefined,
    ])
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

  const edit = { patch: { subject: 'Revised', to: ['a@example.com'] }, baseFingerprint: 'f'.repeat(64) }

  it('sends an edit as {patch, baseFingerprint} with a once approval and nothing else (EP-10-S6)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ approval: { id: 'appr_1', run_id: 'run_1', state: 'APPROVAL_STATE_GRANTED' } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await decideApproval('appr_1', 'approve', undefined, undefined, 'once', false, edit)

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toEqual({ decision: 'approve', reason: '', edit })
  })

  it('refuses an edit with a reject, a standing scope, Always or a broken limit before sending', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const refusals = [
      decideApproval('appr_1', 'reject', undefined, undefined, 'once', false, edit),
      decideApproval('appr_1', 'approve', undefined, undefined, 'thread', false, edit),
      decideApproval('appr_1', 'approve', undefined, undefined, 'thread', true, edit),
      decideApproval('appr_1', 'approve', undefined, undefined, 'once', false, {
        ...edit,
        patch: { subject: 'a\r\nBcc: x@example.com' },
      }),
      decideApproval('appr_1', 'approve', undefined, undefined, 'once', false, { ...edit, baseFingerprint: 'abc' }),
      decideApproval('appr_1', 'approve', undefined, undefined, 'once', false, { ...edit, patch: {} }),
    ]
    for (const refusal of refusals) {
      await expect(refusal).rejects.toMatchObject({ status: 400, code: 'invalid_approval_decision' })
    }
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('surfaces a changed draft and a refused patch as typed edit refusals', async () => {
    for (const [status, code] of [
      [409, 'approval_changed'],
      [422, 'invalid_recipient'],
      [403, 'approval_edit_disabled'],
    ] as const) {
      vi.stubGlobal(
        'fetch',
        vi.fn().mockResolvedValue(
          new Response(JSON.stringify({ error: { code, message: code, details: {} } }), {
            status,
            headers: { 'Content-Type': 'application/json' },
          }),
        ),
      )
      const error = await decideApproval('appr_1', 'approve', undefined, undefined, 'once', false, edit).catch(
        (failure: unknown) => failure,
      )
      expect(error).toBeInstanceOf(ApiError)
      expect(approvalEditRefusalOf(error)).toBe(code)
    }
    expect(approvalEditRefusalOf(new ApiError('x', 500, 'internal'))).toBeNull()
    expect(approvalEditRefusalOf(new Error('x'))).toBeNull()
  })
})

describe('getApprovalPreview can_edit (EP-10-S6)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads can_edit only as the literal true', async () => {
    for (const [value, expected] of [
      [true, true],
      [false, false],
      ['true', false],
      [undefined, false],
    ] as const) {
      vi.stubGlobal(
        'fetch',
        vi.fn().mockResolvedValue(
          jsonResponse({ preview: { kind: 'mail_send', to: ['a@example.com'], can_edit: value } }),
        ),
      )
      expect((await getApprovalPreview('appr_1'))?.canEdit).toBe(expected)
    }
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

  it('does not read a BFF send failure as a refusal: a 502 upstream_unavailable is a transport failure, spent unknown', async () => {
    refusedWith(502, 'upstream_unavailable')

    const error = await rejectionOf()

    expect(error).toMatchObject({ status: 502, code: 'upstream_unavailable' })
    expect(chatRunContinueHttpRefusalOf(error)).toBeNull()
    expect(chatRunContinueTransportFailureOf(error)).toEqual({
      kind: 'transport',
      reason: 'upstream_unavailable',
      approvalSpent: 'unknown',
      retryable: true,
      followRun: true,
    })
  })

  it('reads a 504 and a model_error the BFF made of an upstream failure as transport failures too', async () => {
    for (const [status, code, reason] of [
      [504, 'gateway_timeout', 'gateway_timeout'],
      [500, 'model_error', 'unlisted'],
      [401, 'unauthorized', 'unlisted'],
    ] as const) {
      refusedWith(status, code)
      const error = await rejectionOf()
      expect(chatRunContinueHttpRefusalOf(error)).toBeNull()
      expect(chatRunContinueTransportFailureOf(error)).toMatchObject({ kind: 'transport', reason, approvalSpent: 'unknown', followRun: true })
    }
  })

  it('rejects with the network error itself when fetch fails, and the contract reads it as a transport failure', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))

    const error = await rejectionOf()

    expect(error).toBeInstanceOf(TypeError)
    expect(chatRunContinueHttpRefusalOf(error)).toBeNull()
    expect(chatRunContinueTransportFailureOf(error)).toMatchObject({
      kind: 'transport',
      reason: 'network',
      approvalSpent: 'unknown',
      retryable: true,
      followRun: true,
    })
  })

  it('reads not_resumable as unknown, not as unspent: it also answers a retry for a spent approval whose run has finished', async () => {
    refusedWith(409, 'not_resumable')

    const error = await rejectionOf()

    expect(chatRunContinueHttpRefusalOf(error)).toMatchObject({ approvalSpent: 'unknown', followRun: true, retryable: false })
  })
})

describe('the chat.run.continue contract guards, re-exported by the client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads an HTTP refusal only for a listed code and status pair', () => {
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_granted', status: 409 })).toMatchObject({
      approvalSpent: 'no',
      retryable: false,
      followRun: false,
    })
    expect(chatRunContinueHttpRefusalOf({ code: 'approval_not_found', status: 404 })).toMatchObject({
      approvalSpent: 'unknown',
      retryable: false,
      followRun: true,
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

  it('reads an unflagged tuple code as unknown, not retried automatically and resolved by following the run', () => {
    expect(chatRunContinueUnflaggedRefusalOf({ code: 'continuation_already_consumed', retryable: false })).toEqual({
      regime: 2,
      code: 'continuation_already_consumed',
      approvalSpent: 'unknown',
      retryable: false,
      followRun: true,
    })
  })

  it('documents continueRun with guidance a client can follow, and never with an instruction to re-read an approval', () => {
    // The doc comment with its line wrapping folded away.
    const doc = (clientSource.match(/\/\*\*(?:[^*]|\*(?!\/))*\*\/\s*export async function continueRun/)?.[0] ?? '')
      .replace(/\s*\n\s*\*\s?/g, ' ')
    expect(doc).toContain('chatRunContinueTransportFailureOf')
    expect(doc).toContain('do not assume either way')
    expect(doc).toContain('run by its id')
    expect(doc).toContain('same claim token')
    expect(doc).toContain('409')
    expect(doc).toContain('answered either with the continuation or with a refusal')
    expect(doc).toContain('Only 409 `continuation_already_consumed` means the click was spent')
    expect(doc).toContain('`not_resumable`, `approval_not_found` and `continue_disabled` leave the spend unknown')
    expect(doc).toContain('the run read is the arbiter')
    expect(doc).not.toMatch(/answers 409 `continuation_already_consumed`/)
    expect(doc).not.toMatch(/otherwise it continues/i)
    expect(clientSource).not.toMatch(/re-?read\s+(the\s+)?approval/i)
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

/** The non-test source files under `src`, as [path, text] pairs. */
function sourceFiles(dir: string): Array<[string, string]> {
  const found: Array<[string, string]> = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules') found.push(...sourceFiles(path))
    } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.test\.(ts|tsx)$/.test(entry.name)) {
      found.push([path, readFileSync(path, 'utf8')])
    }
  }
  return found
}

describe('continueParkedRun (WS03 S8-A, no caller)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts an exactly empty approval_id and the claim token, and relays the frames', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response('event: connected\ndata: {"ok":true,"run_id":"run_1"}\n\n', {
        status: 200,
        headers: { 'Content-Type': 'text/event-stream' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const events: string[] = []

    await continueParkedRun('run_1', 'claim_1', (event) => {
      events.push(event.event ?? '')
    })

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toContain('/api/v1/orchestration/runs/run_1/continue')
    expect(init.method).toBe('POST')
    // Snake_case, and the approval id is present and empty: the BFF's body has no absent-key form.
    expect(String(init.body)).toBe('{"approval_id":"","claim_token":"claim_1"}')
    expect(events).toEqual(['connected'])
  })

  it('passes the abort signal to the request', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response('', { status: 200, headers: { 'Content-Type': 'text/event-stream' } }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const controller = new AbortController()

    await continueParkedRun('run_1', 'claim_1', () => {}, controller.signal)

    expect((fetchMock.mock.calls[0] as [string, RequestInit])[1].signal).toBe(controller.signal)
  })

  it('encodes the run id into the path', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response('', { status: 200, headers: { 'Content-Type': 'text/event-stream' } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await continueParkedRun('run/1 x', 'claim_1', () => {})

    expect((fetchMock.mock.calls[0] as [string])[0]).toContain('/runs/run%2F1%20x/continue')
  })

  it('rejects with the typed error when the gateway refuses before a stream opens', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: { code: 'not_resumable', message: 'not resumable' } }), {
          status: 409,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )

    await expect(continueParkedRun('run_1', 'claim_1', () => {})).rejects.toMatchObject({
      status: 409,
      code: 'not_resumable',
    })
  })

  it('relays a park failure frame untouched, and the park reader (only) reads it', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          'event: error\ndata: {"code":"park_not_due","message":"park not due","retryable":false,'
            + '"run_id":"run_1","park_continue":true}\n\n',
          { status: 200, headers: { 'Content-Type': 'text/event-stream' } },
        ),
      ),
    )
    const frames: unknown[] = []

    await continueParkedRun('run_1', 'claim_1', (event) => {
      frames.push(JSON.parse(event.data))
    })

    expect(frames).toHaveLength(1)
    expect(chatRunContinueParkFailureOf(frames[0])).toMatchObject({
      regime: 'park',
      code: 'park_not_due',
      approvalSpent: 'no',
      follow: true,
      runStaysParked: 'yes',
    })
    expect(chatRunContinueFrameRefusalOf(frames[0])).toBeNull()
    expect(chatRunContinueUnflaggedRefusalOf(frames[0])).toBeNull()
  })

  it('has no caller: nothing under src but the client itself names it', () => {
    const here = dirname(fileURLToPath(import.meta.url))
    const src = join(here, '..', '..')
    const naming = sourceFiles(src)
      .filter(([, text]) => text.includes('continueParkedRun'))
      .map(([path]) => path.slice(src.length + 1).replaceAll('\\', '/'))
    expect(naming).toEqual(['shared/api/orchestration-client.ts'])
  })

  it('documents the park form without telling a caller to retry', () => {
    const doc = (clientSource.match(/\/\*\*(?:[^*]|\*(?!\/))*\*\/\s*export async function continueParkedRun/)?.[0] ?? '')
      .replace(/\s*\n\s*\*\s?/g, ' ')
    expect(doc).toContain('no caller')
    expect(doc).toContain('chatRunContinueParkFailureOf')
    expect(doc).toContain('follow the run')
    expect(doc).not.toMatch(/retry|try again|send (it|the request) again/i)
  })
})

// ── Run questions (build plan WS14 S3) ───────────────────────────────────────

function rawQuestion(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'q_1',
    run_id: 'run_1',
    thread_id: 'thread_1',
    step_id: 'step_1',
    state: 'requested',
    delivery: 'next_turn',
    items: [
      {
        id: 'scope',
        kind: 'single',
        prompt: 'Which branch?',
        options: [
          { id: 'main', label: 'main', description: null },
          { id: 'feature', label: 'A feature branch', description: 'New work' },
        ],
        allow_other: false,
      },
    ],
    requested_at: '2026-10-21T10:00:00+00:00',
    expires_at: '2026-10-22T10:00:00+00:00',
    answered_at: null,
    answers: [],
    answered_message_id: null,
    ...overrides,
  }
}

describe('run questions', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists the run questions from the questions route and reads them as camelCase', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ questions: [rawQuestion()] }))
    vi.stubGlobal('fetch', fetchMock)

    const questions = await listRunQuestions('run 1')

    const [listUrl] = fetchMock.mock.calls[0] as [string]
    expect(String(listUrl)).toMatch(/\/api\/v1\/orchestration\/runs\/run%201\/questions$/)
    expect(questions).toEqual([
      {
        id: 'q_1',
        runId: 'run_1',
        threadId: 'thread_1',
        stepId: 'step_1',
        state: 'requested',
        delivery: 'next_turn',
        items: [
          {
            id: 'scope',
            kind: 'single',
            prompt: 'Which branch?',
            options: [
              { id: 'main', label: 'main' },
              { id: 'feature', label: 'A feature branch', description: 'New work' },
            ],
            allowOther: false,
          },
        ],
        requestedAt: '2026-10-21T10:00:00+00:00',
        expiresAt: '2026-10-22T10:00:00+00:00',
        answers: [],
      },
    ])
  })

  it('drops a question it cannot present whole instead of showing half of it', () => {
    const cases: Record<string, unknown>[] = [
      rawQuestion({ state: 'pending' }),
      rawQuestion({ delivery: 'later' }),
      rawQuestion({ items: [] }),
      rawQuestion({ items: [{ id: 'k', kind: 'secret', prompt: 'Your key?', options: [] }] }),
      rawQuestion({ items: [{ id: 'k', kind: 'single', prompt: 'p', options: [{ id: 'a' }] }] }),
      rawQuestion({ answers: [{ option_ids: ['a'] }] }),
    ]
    for (const raw of cases) expect(normalizeRunQuestion(raw)).toBeNull()
    expect(normalizeRunQuestion(null)).toBeNull()
    expect(normalizeRunQuestion([rawQuestion()])).toBeNull()
  })

  it('sends only the answers on the answer route, in snake_case, and reads the stored question back', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        question: rawQuestion({
          state: 'answered',
          answered_at: '2026-10-21T10:05:00+00:00',
          answers: [{ item_id: 'scope', option_ids: ['main'], text: '' }],
        }),
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const stored = await answerQuestion('q_1', {
      answers: [{ itemId: 'scope', optionIds: ['main'], text: '' }],
    })

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(String(url)).toMatch(/\/api\/v1\/orchestration\/questions\/q_1\/answer$/)
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({
      answers: [{ item_id: 'scope', option_ids: ['main'], text: '' }],
      skip: false,
    })
    expect(stored?.state).toBe('answered')
    expect(stored?.answers).toEqual([{ itemId: 'scope', optionIds: ['main'], text: '' }])
    expect(stored?.answeredAt).toBe('2026-10-21T10:05:00+00:00')
  })

  it('sends a skip with no answers', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ question: rawQuestion({ state: 'skipped', answers: [] }) }))
    vi.stubGlobal('fetch', fetchMock)

    const stored = await answerQuestion('q_1', { skip: true })

    const [, skipInit] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(skipInit.body as string)).toEqual({ answers: [], skip: true })
    expect(stored?.state).toBe('skipped')
  })

  it('reads each typed refusal by its code and its status together', async () => {
    const cases: Array<[number, string]> = [
      [404, 'question_not_found'],
      [410, 'question_expired'],
      [409, 'question_already_answered'],
      [422, 'invalid_answer'],
    ]
    for (const [status, code] of cases) {
      vi.stubGlobal(
        'fetch',
        vi.fn().mockResolvedValue(
          new Response(JSON.stringify({ error: { code, message: 'refused', details: {} } }), {
            status,
            headers: { 'Content-Type': 'application/json' },
          }),
        ),
      )
      const failure = await answerQuestion('q_1', { skip: true }).catch((error: unknown) => error)
      expect(runQuestionErrorCodeOf(failure)).toBe(code)
    }
  })

  it('does not read a code under the wrong status, or any other failure, as a question error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: { code: 'question_expired', message: 'x' } }), {
          status: 500,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    const wrongStatus = await answerQuestion('q_1', { skip: true }).catch((error: unknown) => error)
    expect(runQuestionErrorCodeOf(wrongStatus)).toBeNull()
    expect(runQuestionErrorCodeOf(new Error('question_expired'))).toBeNull()
    expect(runQuestionErrorCodeOf(null)).toBeNull()
    expect(runQuestionErrorCodeOf(new ApiError('x', 410, 'toString'))).toBeNull()
  })

  it('is refused typed for a zero-retention credential and the refusal reaches the caller untouched', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            error: { code: 'zdr_durable_mutation_forbidden', message: 'Not available under zero data retention.' },
          }),
          { status: 412, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    const refused = await listRunQuestions('run_1').catch((error: unknown) => error)
    expect(refused).toBeInstanceOf(ApiError)
    expect((refused as ApiError).code).toBe('zdr_durable_mutation_forbidden')
    expect(runQuestionErrorCodeOf(refused)).toBeNull()
  })

  it('has one caller: only the client and the chat controller (WS14-S4) name the question calls', () => {
    const here = dirname(fileURLToPath(import.meta.url))
    const src = join(here, '..', '..')
    const naming = sourceFiles(src)
      .filter(([, text]) => /\b(listRunQuestions|answerQuestion)\b/.test(text))
      .map(([path]) => path.slice(src.length + 1).replaceAll('\\', '/'))
      .filter((path) => !path.endsWith('.test.ts'))
    expect(naming.sort()).toEqual(['features/chat/components/use-chat-controller.ts', 'shared/api/orchestration-client.ts'])
  })
})

describe('getApprovalPreview (EP-10-S1)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const errorResponse = (status: number, code: string) =>
    new Response(JSON.stringify({ error: { code, message: code, details: {} } }), {
      status,
      headers: { 'Content-Type': 'application/json' },
    })

  it('reads the shaped draft from the preview route, every recipient and the full body', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        preview: {
          kind: 'mail_send',
          provider: 'google',
          operation: 'gmail.send',
          connection_id: 'conn-1',
          to: ['a@example.com', 'b@example.com', 7],
          subject: 'Status',
          body_text: 'Hei,\n\nthe whole body',
          has_html: true,
          is_reply: true,
          raw_only: false,
          editable_fields: ['to', 'subject', 'body_text'],
          action_fingerprint: 'f'.repeat(64),
          revision: 0,
          format: 'plain_text',
        },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const preview = await getApprovalPreview('appr 1')

    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('/api/v1/orchestration/approvals/appr%201/preview')
    expect(preview).toEqual({
      kind: 'mail_send',
      provider: 'google',
      operation: 'gmail.send',
      connectionId: 'conn-1',
      to: ['a@example.com', 'b@example.com'],
      subject: 'Status',
      bodyText: 'Hei,\n\nthe whole body',
      hasHtml: true,
      isReply: true,
      rawOnly: false,
      editableFields: ['to', 'subject', 'body_text'],
      actionFingerprint: 'f'.repeat(64),
      revision: 0,
      canEdit: false,
    })
  })

  it('answers null for every 404 (not the owner, decided, switched off) and for an unknown kind', async () => {
    for (const code of ['approval_not_found', 'preview_unavailable']) {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(errorResponse(404, code)))
      await expect(getApprovalPreview('appr_1')).resolves.toBeNull()
    }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ preview: { kind: 'calendar_event' } })))
    await expect(getApprovalPreview('appr_1')).resolves.toBeNull()
  })

  it('throws a zero-retention refusal typed, so the card can say why', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(errorResponse(412, 'zdr_durable_mutation_forbidden')))
    const refused = await getApprovalPreview('appr_1').catch((error: unknown) => error)
    expect(refused).toBeInstanceOf(ApiError)
    expect((refused as ApiError).code).toBe('zdr_durable_mutation_forbidden')
  })
})
