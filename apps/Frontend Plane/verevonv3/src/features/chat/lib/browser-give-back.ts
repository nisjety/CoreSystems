// The chat side of the live panel's Give back (EP-1). The panel has already handed the browser back
// (its control flip); this decides the takeover approval the run waits on, with the person's note as
// its reason, and continues the run under `chat.run.continue`. It never sends a continue again on its
// own: a refused or ambiguous continue is followed by the live run, see the contract's guidance.

import { getActionDescriptor } from '@/shared/actions/action-registry'
import {
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueTransportFailureOf,
  chatRunContinueUnflaggedRefusalOf,
  type ChatRunContinueEffect,
} from '@/shared/actions/chat-run-continue-contract'
import type { Approval, ApprovalDecision } from '@/shared/api/orchestration-client'
import type { SseEvent } from '@/shared/api/sse'

/** The approval a person's takeover paused the run on, while the person still has it open. */
export function takeoverApprovalOf(approvals: readonly Approval[]): Approval | undefined {
  return approvals.find((approval) => (
    approval.cause?.code === 'browser_human_assist'
    && approval.cause.assistOrigin === 'person'
    && (approval.status ?? 'PENDING').toUpperCase() === 'PENDING'
  ))
}

export type BrowserGiveBackDeps = {
  listApprovals: (runId: string) => Promise<Approval[]>
  decideApproval: (approvalId: string, decision: ApprovalDecision, reason: string) => Promise<unknown>
  continueRun: (
    runId: string,
    approvalId: string,
    claimToken: string,
    onEvent: (event: SseEvent) => void,
  ) => Promise<void>
  newClaimToken: () => string
}

/** What became of a give-back. Only `continued` and `continue_refused` mean a continue was sent. */
export type BrowserGiveBackOutcome =
  | { readonly kind: 'nothing_to_decide' }
  | { readonly kind: 'decision_failed' }
  | { readonly kind: 'continued' }
  | { readonly kind: 'continue_refused'; readonly effect: ChatRunContinueEffect }

const UNKNOWN_SPEND: ChatRunContinueEffect = { approvalSpent: 'unknown', retryable: false, followRun: true }

function frameOf(data: string): unknown {
  try {
    return JSON.parse(data) as unknown
  } catch {
    return null
  }
}

export async function runBrowserGiveBack(
  runId: string,
  note: string,
  deps: BrowserGiveBackDeps,
): Promise<BrowserGiveBackOutcome> {
  const approval = takeoverApprovalOf(await deps.listApprovals(runId))
  if (!approval) return { kind: 'nothing_to_decide' }

  // Checked before the decision: a continue the contract refuses must not leave a granted approval behind.
  const input = { runId, approvalId: approval.id, claimToken: deps.newClaimToken() }
  if (!getActionDescriptor('chat.run.continue')?.inputSchema.safeParse(input).success) {
    throw new Error('chat.run.continue input is refused by its action contract')
  }
  try {
    await deps.decideApproval(approval.id, 'approve', note)
  } catch {
    return { kind: 'decision_failed' }
  }

  const refused: { effect: ChatRunContinueEffect | null } = { effect: null }
  try {
    await deps.continueRun(runId, approval.id, input.claimToken, (event) => {
      const frame = frameOf(event.data)
      refused.effect ??= chatRunContinueFrameRefusalOf(frame) ?? chatRunContinueUnflaggedRefusalOf(frame)
    })
  } catch (error) {
    const effect = chatRunContinueHttpRefusalOf(error) ?? chatRunContinueTransportFailureOf(error)
    return { kind: 'continue_refused', effect: effect ?? UNKNOWN_SPEND }
  }
  return refused.effect ? { kind: 'continue_refused', effect: refused.effect } : { kind: 'continued' }
}
