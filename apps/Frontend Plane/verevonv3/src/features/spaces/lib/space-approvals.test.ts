import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const orchestration = vi.hoisted(() => ({
  decideApproval: vi.fn(),
  listApprovals: vi.fn(),
  resumeRun: vi.fn(),
  cancelRun: vi.fn(),
}))

vi.mock('@/shared/api/orchestration-client', () => orchestration)

const { describeApproval, pendingApprovals, readPendingApprovals, settleApproval } = await import(
  './space-approvals'
)

const tr = (_no: string, en: string) => en

beforeEach(() => {
  orchestration.decideApproval.mockReset()
  orchestration.listApprovals.mockReset()
  orchestration.resumeRun.mockReset().mockResolvedValue(undefined)
  orchestration.cancelRun.mockReset().mockResolvedValue({ cancelled: true })
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
  it('prefers the server detail, then the kind, then an honest generic line', () => {
    expect(describeApproval({ id: 'a', detail: 'Delete 3 files' }, tr)).toBe('Delete 3 files')
    expect(describeApproval({ id: 'a', kind: 'shell' }, tr)).toContain('shell')
    // Nothing recorded: say that something is waiting, do not invent what.
    expect(describeApproval({ id: 'a' }, tr)).toBe(
      'The agent is waiting for approval before it continues.',
    )
  })
})

describe('settleApproval', () => {
  it('approving records the decision and resumes the run', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)
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
  it('re-reads after an error and follows through when the decision did land', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'GRANTED' }])
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)
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
