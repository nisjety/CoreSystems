import { ApiError } from '@/shared/api/http'
import {
  cancelRun,
  decideApproval,
  listApprovals,
  resumeRun,
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
  /** Recorded, and the run was told to continue. */
  | 'granted'
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
 * Model Plane fills `detail` for some approval kinds and not others; when it
 * says nothing we say the honest generic thing rather than dressing the kind
 * up as a description of the action.
 */
export function describeApproval(
  approval: Approval,
  tr: (no: string, en: string) => string,
): string {
  const detail = approval.detail?.trim()
  if (detail) return detail
  const kind = approval.kind?.trim()
  if (kind) {
    return tr(`Agenten ber om å få utføre «${kind}».`, `The agent is asking to run “${kind}”.`)
  }
  return tr(
    'Agenten venter på godkjenning før den fortsetter.',
    'The agent is waiting for approval before it continues.',
  )
}

function isRefusal(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 403 || error.status === 404)
}

/**
 * Record one decision and carry it through to the run.
 *
 * Approving resumes the run; denying cancels it, because the point of denying
 * a gated step is to stop that path rather than to let the agent try to route
 * around it. Both follow-throughs are best-effort by design — the decision is
 * the durable part, and a resume that fails leaves a decided approval and a
 * paused run, which the next read shows truthfully.
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
  try {
    await decideApproval(input.approvalId, input.decision, undefined, input.signal)
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
  }

  if (input.decision === 'approve') {
    await resumeRun(input.runId, input.signal).catch(() => undefined)
    return 'granted'
  }
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
