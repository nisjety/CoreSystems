import { afterEach, describe, expect, it, vi } from 'vitest'
import { decideApproval, listApprovals } from './orchestration-client'

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
