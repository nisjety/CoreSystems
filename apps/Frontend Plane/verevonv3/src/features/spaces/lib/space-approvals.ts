import { ApiError } from '@/shared/api/http'
import { approvalFallbackLine, approvalReason } from '@/shared/approvals/approval-copy'
import {
  cancelRun,
  decideApproval,
  listApprovals,
  type Approval,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'

/**
 * Deciding an approval from inside a room.
 *
 * The mechanism is the same one the Agent Run Console already uses — Model
 * Plane owns the gate, the browser only relays a decision — but the room needs
 * two things the console never did.
 *
 * 1. **A refusal is an ordinary outcome.** A run belongs to the member who
 *    started it, so another member of the same room can see that something is
 *    waiting and still not be the one who may answer it. That has to render as
 *    a sentence, not as a failed request.
 * 2. **Nothing is optimistic.** The console owns its own run and can drop a
 *    decided approval on sight. A room shows other people's work, so a card
 *    that disappears before the server agreed would be the room asserting an
 *    outcome it does not yet have.
 */

/** What actually happened, after reconciliation. Never a guess. */
export type ApprovalOutcome =
  /** Recorded. The server stated nothing about what happens next, so neither do we. */
  | 'granted'
  /** Recorded, and the action is queued or running as the requester; the agent does not
   *  continue by itself. */
  | 'granted_action'
  /** Recorded, but nothing runs: the agent may retry this exact action once. */
  | 'granted_retry'
  /** Recorded, and the running browser step picks it up. */
  | 'granted_live'
  /** Recorded, and the durable loop applies it when the run is continued. */
  | 'granted_continue'
  /** Recorded, and the run was cancelled. */
  | 'denied'
  /** Somebody else got there first; the decision stands, but it is not ours. */
  | 'already_decided'
  /** The caller may not decide this one. Expected in a shared room. */
  | 'refused'
  /** We could not confirm either way. Deliberately distinct from a failure:
   *  the decision may well have landed, so the UI must not invite a blind retry
   *  that could double-decide. */
  | 'unconfirmed'

export function isPending(approval: Approval): boolean {
  return (approval.status ?? 'PENDING').toUpperCase() === 'PENDING'
}

export function pendingApprovals(approvals: readonly Approval[]): Approval[] {
  return approvals.filter(isPending)
}

/**
 * One readable line for what is being asked, without inventing detail.
 *
 * The server states why the pause was raised as a cause code, and the shared approval copy
 * words it in the reader's language. An approval with no cause says the honest generic thing
 * rather than dressing its kind up as a description of the action.
 */
export function describeApproval(
  approval: Approval,
  tr: (no: string, en: string) => string,
): string {
  const i18n = { tr }
  return approvalReason(approval, i18n) ?? approvalFallbackLine(i18n)
}

function isRefusal(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 403 || error.status === 404)
}

/**
 * What a recorded grant leads to, read from the server's derived continuation and never assumed.
 * Nothing here resumes the run: a granted approval cannot resume an approval-gated run (the only
 * continuation is the delivery worker running the one approved action), so the old blind
 * `resumeRun` call after a grant is gone (build plan WS10 S1).
 */
export function grantedOutcome(approval: Approval | null | undefined): ApprovalOutcome {
  switch (approval?.continuation?.state) {
    case 'queued':
    case 'running':
    case 'completed':
    case 'failed':
      return 'granted_action'
    case 'not_resumable':
      return 'granted_retry'
    case 'live':
      return 'granted_live'
    case 'awaiting_continue':
      return 'granted_continue'
    default:
      return 'granted'
  }
}

/**
 * Record one decision and, for a denial, stop the run.
 *
 * Denying cancels the run, because the point of denying a gated step is to stop that path
 * rather than to let the agent try to route around it. The follow-through is best-effort by
 * design — the decision is the durable part. Approving records the decision and reports what
 * the server says happens next ({@link grantedOutcome}); it never calls resume.
 *
 * When the decide call itself errors we re-read before claiming anything. A
 * lost response is not the same as a rejected request, and this is a gate: the
 * expensive mistake is telling someone their approval failed when it landed.
 */
export async function settleApproval(input: {
  readonly approvalId: string
  readonly runId: string
  readonly decision: ApprovalDecision
  readonly signal?: AbortSignal
}): Promise<ApprovalOutcome> {
  const expected = input.decision === 'approve' ? 'GRANTED' : 'DENIED'
  let decided: Approval | null = null
  try {
    decided = await decideApproval(input.approvalId, input.decision, undefined, input.signal)
  } catch (error) {
    if (isRefusal(error)) return 'refused'
    const fresh = await listApprovals(input.runId, input.signal).catch(() => null)
    if (fresh === null) return 'unconfirmed'
    const match = fresh.find((approval) => approval.id === input.approvalId)
    // Gone from the listing entirely: we cannot read a status, so we cannot
    // claim one.
    if (!match) return 'unconfirmed'
    const status = (match.status ?? 'PENDING').toUpperCase()
    if (status === 'PENDING') return 'unconfirmed'
    if (status !== expected) return 'already_decided'
    // It did land despite the error; fall through to the follow-through.
    decided = match
  }

  if (input.decision === 'approve') return grantedOutcome(decided)
  await cancelRun(input.runId, input.signal).catch(() => undefined)
  return 'denied'
}

/**
 * Read a run's pending approvals for the room.
 *
 * A refusal answers "not yours to decide" rather than throwing, so the panel
 * can say that plainly. Every other failure is a real error and stays one —
 * an unreachable Model Plane must not render as "nothing to approve".
 */
export async function readPendingApprovals(
  runId: string,
  signal?: AbortSignal,
): Promise<{ readonly approvals: Approval[]; readonly refused: boolean }> {
  try {
    const approvals = await listApprovals(runId, signal)
    return { approvals: pendingApprovals(approvals), refused: false }
  } catch (error) {
    if (isRefusal(error)) return { approvals: [], refused: true }
    throw error
  }
}
