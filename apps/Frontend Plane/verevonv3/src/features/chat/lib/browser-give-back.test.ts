/**
 * EP-1 give-back, the chat side (C6 a). The panel hands the browser back; this decides the takeover
 * approval the run waits on with the person's note, then continues the run under chat.run.continue.
 * The tests pin what is decided and in which order, what a refused or ambiguous continue means, and
 * that a continue is never sent again by this path.
 */

import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import type { Approval } from '@/shared/api/orchestration-client'
import { runBrowserGiveBack, takeoverApprovalOf, type BrowserGiveBackDeps } from './browser-give-back'

const takeover: Approval = {
  id: 'apr-takeover',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  status: 'PENDING',
  cause: {
    code: 'browser_human_assist',
    scopeOptions: ['once'],
    stepUpRequired: false,
    assistKind: 'user_takeover',
    assistOrigin: 'person',
  },
}

const helpRequest: Approval = {
  id: 'apr-help',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  status: 'PENDING',
  cause: {
    code: 'browser_human_assist',
    scopeOptions: ['once'],
    stepUpRequired: false,
    assistKind: 'login',
    assistOrigin: 'agent',
    host: 'example.com',
  },
}

type DepsOverrides = { [K in keyof BrowserGiveBackDeps]?: BrowserGiveBackDeps[K] }

function deps(overrides: DepsOverrides = {}) {
  return {
    listApprovals: vi.fn(overrides.listApprovals ?? (async () => [takeover])),
    decideApproval: vi.fn(overrides.decideApproval ?? (async () => null)),
    continueRun: vi.fn(overrides.continueRun ?? (async () => undefined)),
    newClaimToken: vi.fn(overrides.newClaimToken ?? (() => 'claim-1')),
  }
}

describe('takeoverApprovalOf', () => {
  it('finds the pending takeover the person raised, and no help request or decided one', () => {
    expect(takeoverApprovalOf([helpRequest, takeover])?.id).toBe('apr-takeover')
    expect(takeoverApprovalOf([helpRequest])).toBeUndefined()
    expect(takeoverApprovalOf([{ ...takeover, status: 'GRANTED' }])).toBeUndefined()
    expect(takeoverApprovalOf([{ ...takeover, cause: undefined }])).toBeUndefined()
  })
})

describe('runBrowserGiveBack', () => {
  it('decides the takeover granted with the note as its reason, then continues the run under a new claim', async () => {
    const d = deps()

    const outcome = await runBrowserGiveBack('run-1', 'use the second result', d)

    expect(outcome).toEqual({ kind: 'continued' })
    expect(d.decideApproval).toHaveBeenCalledWith('apr-takeover', 'approve', 'use the second result')
    expect(d.continueRun).toHaveBeenCalledWith('run-1', 'apr-takeover', 'claim-1', expect.any(Function))
    const decidedAt = d.decideApproval.mock.invocationCallOrder[0] ?? Number.NaN
    const continuedAt = d.continueRun.mock.invocationCallOrder[0] ?? Number.NaN
    expect(decidedAt).toBeLessThan(continuedAt)
  })

  it('sends an empty reason when the person wrote no note', async () => {
    const d = deps()

    await runBrowserGiveBack('run-1', '', d)

    expect(d.decideApproval).toHaveBeenCalledWith('apr-takeover', 'approve', '')
  })

  it('does nothing when the run has no open takeover to decide', async () => {
    const d = deps({ listApprovals: vi.fn(async () => [helpRequest, { ...takeover, status: 'GRANTED' }]) })

    expect(await runBrowserGiveBack('run-1', 'note', d)).toEqual({ kind: 'nothing_to_decide' })
    expect(d.decideApproval).not.toHaveBeenCalled()
    expect(d.continueRun).not.toHaveBeenCalled()
  })

  it('checks the continue against its contract before it decides anything', async () => {
    const d = deps({ newClaimToken: () => '' })

    await expect(runBrowserGiveBack('run-1', 'note', d)).rejects.toThrow('chat.run.continue input is refused')
    expect(d.decideApproval).not.toHaveBeenCalled()
    expect(d.continueRun).not.toHaveBeenCalled()
  })

  it('never continues a run whose takeover was not decided', async () => {
    const d = deps({ decideApproval: vi.fn(async () => { throw new ApiError('gone', 409, 'approval_changed') }) })

    expect(await runBrowserGiveBack('run-1', 'note', d)).toEqual({ kind: 'decision_failed' })
    expect(d.continueRun).not.toHaveBeenCalled()
  })

  it('reads a refusal before the spend as not spent, and sends no second continue', async () => {
    const d = deps({ continueRun: vi.fn(async () => { throw new ApiError('not granted', 409, 'approval_not_granted') }) })

    const outcome = await runBrowserGiveBack('run-1', 'note', d)

    expect(outcome).toMatchObject({ kind: 'continue_refused', effect: { approvalSpent: 'no', followRun: false } })
    expect(d.continueRun).toHaveBeenCalledTimes(1)
  })

  it('reads a refusal of a consumed approval as spent, and follows the run', async () => {
    const d = deps({ continueRun: vi.fn(async () => { throw new ApiError('consumed', 409, 'continuation_already_consumed') }) })

    const outcome = await runBrowserGiveBack('run-1', 'note', d)

    expect(outcome).toMatchObject({ kind: 'continue_refused', effect: { approvalSpent: 'yes', retryable: false } })
    expect(d.continueRun).toHaveBeenCalledTimes(1)
  })

  it('reads a network failure as an unknown spend, and does not retry it', async () => {
    const d = deps({ continueRun: vi.fn(async () => { throw new TypeError('Failed to fetch') }) })

    const outcome = await runBrowserGiveBack('run-1', 'note', d)

    expect(outcome).toMatchObject({ kind: 'continue_refused', effect: { approvalSpent: 'unknown', followRun: true } })
    expect(d.continueRun).toHaveBeenCalledTimes(1)
  })

  it('reads an error frame of a spent continuation from the stream as refused, and final', async () => {
    const d = deps({
      continueRun: vi.fn(async (_run, _approval, _claim, onEvent) => {
        onEvent({ event: 'connected', data: JSON.stringify({ ok: true, approval_spent: true }) })
        onEvent({ event: 'error', data: JSON.stringify({ code: 'continue_unreachable', approval_spent: true }) })
      }),
    })

    expect(await runBrowserGiveBack('run-1', 'note', d)).toMatchObject({
      kind: 'continue_refused',
      effect: { approvalSpent: 'yes', retryable: false },
    })
  })

  it('reads an unflagged refusal frame from the stream as an unknown spend', async () => {
    const d = deps({
      continueRun: vi.fn(async (_run, _approval, _claim, onEvent) => {
        onEvent({ event: 'error', data: JSON.stringify({ code: 'approval_not_granted' }) })
      }),
    })

    expect(await runBrowserGiveBack('run-1', 'note', d)).toMatchObject({
      kind: 'continue_refused',
      effect: { approvalSpent: 'unknown', retryable: false },
    })
  })

  it('reports a stream with no refusal frame as continued, and ignores a frame that is not JSON', async () => {
    const d = deps({
      continueRun: vi.fn(async (_run, _approval, _claim, onEvent) => {
        onEvent({ event: 'message', data: 'not json' })
        onEvent({ event: 'connected', data: JSON.stringify({ ok: true, run_id: 'run-1' }) })
      }),
    })

    expect(await runBrowserGiveBack('run-1', 'note', d)).toEqual({ kind: 'continued' })
  })
})
