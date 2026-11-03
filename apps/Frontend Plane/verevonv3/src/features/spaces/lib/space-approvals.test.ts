import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const orchestration = vi.hoisted(() => ({
  decideApproval: vi.fn(),
  listApprovals: vi.fn(),
  resumeRun: vi.fn(),
  cancelRun: vi.fn(),
}))

const spaceClient = vi.hoisted(() => ({
  decideSpaceApproval: vi.fn(),
}))

vi.mock('@/shared/api/orchestration-client', () => orchestration)
vi.mock('@/shared/api/space-approvals-client', () => spaceClient)

const { describeApproval, pendingApprovals, readPendingApprovals, settleApproval } = await import(
  './space-approvals'
)

const tr = (_no: string, en: string) => en

beforeEach(() => {
  orchestration.decideApproval.mockReset()
  orchestration.listApprovals.mockReset()
  orchestration.resumeRun.mockReset().mockResolvedValue(undefined)
  orchestration.cancelRun.mockReset().mockResolvedValue({ cancelled: true })
  spaceClient.decideSpaceApproval.mockReset()
})

describe('pendingApprovals', () => {
  it('treats a missing status as pending rather than dropping the row', () => {
    // A decision surface that hides an approval it cannot classify is worse
    // than one that shows it: the run stays paused either way, but nobody sees
    // why.
    const kept = pendingApprovals([
      { id: 'a' },
      { id: 'b', status: 'PENDING' },
      { id: 'c', status: 'GRANTED' },
    ])
    expect(kept.map((approval) => approval.id)).toEqual(['a', 'b'])
  })
})

describe('describeApproval', () => {
  it('prefers the cause, then the server detail, then an honest generic line', () => {
    expect(
      describeApproval(
        { id: 'a', cause: { code: 'provider_write', scopeOptions: ['once'], stepUpRequired: false } },
        tr,
      ),
    ).toBe('The agent wants to change something in a connected service. It may be hard to undo.')
    expect(describeApproval({ id: 'a', detail: 'Delete 3 files' }, tr)).toBe('Delete 3 files')
    // Nothing recorded: say that something is waiting, do not invent what. The kind is a wire
    // token, so it is not dressed up as a description either.
    expect(describeApproval({ id: 'a', kind: 'APPROVAL_KIND_TOOL_CALL' }, tr)).toBe(
      'The agent is waiting for approval before it continues.',
    )
    expect(describeApproval({ id: 'a' }, tr)).toBe(
      'The agent is waiting for approval before it continues.',
    )
  })

  it('words the cause in Norwegian under no', () => {
    const no = (norwegian: string) => norwegian
    expect(
      describeApproval(
        { id: 'a', cause: { code: 'owner_effect_reserved', scopeOptions: ['once'], stepUpRequired: false } },
        no,
      ),
    ).toBe('Dette verktøyet endrer data du eier, så det krever alltid en godkjenning fra deg.')
  })
})

describe('settleApproval', () => {
  // WS10-S1: a granted approval cannot resume an approval-gated run, so the old blind resume is gone.
  it('approving records the decision and never resumes the run', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
    expect(orchestration.cancelRun).not.toHaveBeenCalled()
  })

  it('denying stops the run rather than letting the agent route around the gate', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'DENIED' })
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'reject' })
    expect(outcome).toBe('denied')
    expect(orchestration.cancelRun).toHaveBeenCalledWith('run-1', undefined)
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
  })

  // The room shows other members' work, so being refused is ordinary. It must
  // not cost a reconciliation round trip or read as a transport failure.
  it('reports a refusal without re-reading', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('nope', 403, 'forbidden'))
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('refused')
    expect(orchestration.listApprovals).not.toHaveBeenCalled()
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
  })

  // The expensive mistake at a gate is telling someone their approval failed
  // when it actually landed.
  it('re-reads after an error and reports the grant when the decision did land', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'GRANTED' }])
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
  })

  it('reports someone else deciding it first, and does not touch the run', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'DENIED' }])
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('already_decided')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
    expect(orchestration.cancelRun).not.toHaveBeenCalled()
  })

  it.each([
    ['the re-read also failed', null],
    ['it is still pending', [{ id: 'ap-1', status: 'PENDING' }]],
    ['it is gone from the listing', []],
  ])('stays unconfirmed when %s, and never acts on the run', async (_case, fresh) => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    if (fresh === null) orchestration.listApprovals.mockRejectedValue(new Error('down'))
    else orchestration.listApprovals.mockResolvedValue(fresh)
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('unconfirmed')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
    expect(orchestration.cancelRun).not.toHaveBeenCalled()
  })

  // A resume that fails leaves a decided approval and a paused run. That is a
  // truthful state the next read shows; it must not undo the decision.
  it('keeps the decision when the follow-through call fails', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    orchestration.resumeRun.mockRejectedValue(new Error('resume unavailable'))
    await expect(
      settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' }),
    ).resolves.toBe('granted')
  })
})

// WS10-S4: inside a room the decision goes through the Space route, which is the one that carries
// a scope; outside a room the owner-path call is unchanged.
describe('settleApproval inside a Space', () => {
  it('uses the Space route with the scope and reports the grant, never the owner path', async () => {
    spaceClient.decideSpaceApproval.mockResolvedValue({
      approval: { id: 'ap-1', status: 'GRANTED' },
      grant: { status: 'recorded' },
    })
    const onGrant = vi.fn()
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
      spaceRef: 'space-1',
      scope: 'always',
      onGrant,
    })
    expect(outcome).toBe('granted')
    expect(spaceClient.decideSpaceApproval).toHaveBeenCalledWith(
      'space-1',
      'ap-1',
      { decision: 'approve', scope: 'always' },
      undefined,
    )
    expect(orchestration.decideApproval).not.toHaveBeenCalled()
    expect(onGrant).toHaveBeenCalledWith({ status: 'recorded' })
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
  })

  it('reports a standing rule that was not recorded while the decision stands', async () => {
    spaceClient.decideSpaceApproval.mockResolvedValue({
      approval: { id: 'ap-1', status: 'GRANTED' },
      grant: { status: 'not_recorded', reason: 'approval_projection_incomplete' },
    })
    const onGrant = vi.fn()
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
      spaceRef: 'space-1',
      scope: 'always',
      onGrant,
    })
    expect(outcome).toBe('granted')
    expect(onGrant).toHaveBeenCalledWith({
      status: 'not_recorded',
      reason: 'approval_projection_incomplete',
    })
  })

  it('maps the derived continuation of the decided approval from the Space answer', async () => {
    spaceClient.decideSpaceApproval.mockResolvedValue({
      approval: { id: 'ap-1', status: 'GRANTED', continuation: { state: 'queued' } },
      grant: { status: 'none' },
    })
    await expect(
      settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve', spaceRef: 'space-1' }),
    ).resolves.toBe('granted_action')
  })

  it('stops the run on a denial, once, and sends no standing rule', async () => {
    spaceClient.decideSpaceApproval.mockResolvedValue({
      approval: { id: 'ap-1', status: 'DENIED' },
      grant: { status: 'none' },
    })
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'reject',
      spaceRef: 'space-1',
    })
    expect(outcome).toBe('denied')
    expect(orchestration.cancelRun).toHaveBeenCalledWith('run-1', undefined)
    expect(spaceClient.decideSpaceApproval).toHaveBeenCalledWith(
      'space-1',
      'ap-1',
      { decision: 'reject', scope: undefined },
      undefined,
    )
  })

  it('maps 409 to already_decided without re-reading or touching the run', async () => {
    spaceClient.decideSpaceApproval.mockRejectedValue(new ApiError('conflict', 409, 'approval_already_decided'))
    const onGrant = vi.fn()
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
      spaceRef: 'space-1',
      scope: 'always',
      onGrant,
    })
    expect(outcome).toBe('already_decided')
    expect(orchestration.listApprovals).not.toHaveBeenCalled()
    expect(orchestration.cancelRun).not.toHaveBeenCalled()
    expect(onGrant).not.toHaveBeenCalled()
  })

  it.each([403, 404])('maps %i to refused without re-reading', async (status) => {
    spaceClient.decideSpaceApproval.mockRejectedValue(new ApiError('nope', status, 'approval_not_addressed_to_you'))
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
      spaceRef: 'space-1',
    })
    expect(outcome).toBe('refused')
    expect(orchestration.listApprovals).not.toHaveBeenCalled()
  })

  it('re-reads after a transport error and never double-decides', async () => {
    spaceClient.decideSpaceApproval.mockRejectedValue(new ApiError('gateway', 503, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'GRANTED' }])
    const outcome = await settleApproval({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
      spaceRef: 'space-1',
    })
    expect(outcome).toBe('granted')
    expect(spaceClient.decideSpaceApproval).toHaveBeenCalledTimes(1)
  })
})

describe('readPendingApprovals', () => {
  it('answers "not yours" for a refusal instead of throwing', async () => {
    orchestration.listApprovals.mockRejectedValue(new ApiError('nope', 403, 'forbidden'))
    await expect(readPendingApprovals('run-1')).resolves.toEqual({ approvals: [], refused: true })
  })

  // An unreachable Model Plane must never render as "nothing to approve".
  it('rethrows a real failure', async () => {
    orchestration.listApprovals.mockRejectedValue(new ApiError('boom', 500, null))
    await expect(readPendingApprovals('run-1')).rejects.toThrow()
  })

  it('returns only what is still pending', async () => {
    orchestration.listApprovals.mockResolvedValue([
      { id: 'a', status: 'PENDING' },
      { id: 'b', status: 'GRANTED' },
    ])
    const result = await readPendingApprovals('run-1')
    expect(result.refused).toBe(false)
    expect(result.approvals.map((approval) => approval.id)).toEqual(['a'])
  })
})
