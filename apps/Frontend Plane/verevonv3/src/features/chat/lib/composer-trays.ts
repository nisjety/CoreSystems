/**
 * What is waiting on the person, as the trays docked above the composer.
 *
 * An approval, a plan awaiting a grant and a message queued into a live run each
 * used to render inside the transcript, so the control that blocked the run sat
 * wherever its turn happened to be: scrolled away, or under a long answer. The
 * composer is where the person is looking when the run needs them, so these live
 * there, and the transcript keeps a one-line pointer instead.
 *
 * This module is the single source of "what is waiting on me": pure data in, an
 * ordered list out, no rendering. `ComposerTrays` shows the first tray, and the
 * transcript asks `dockedNodesFor` whether a turn's control lives in the dock, so
 * a control is never in both places and never in neither.
 *
 * # Priority
 *
 * approval, then plan, then goal, then queue. An approval blocks a run that is
 * already going, a plan blocks one that has not started, the goal (EP-7 S1) is
 * standing context the person may act on, and the queue is only a status.
 * Pending decisions come first, standing context second. The question tray
 * (`ask_user`, WS14-S4) goes between approval and plan when it exists.
 */

import { deriveConversationNodes } from '@/shared/chat-nodes'
import type { ConversationNode, PlanApprovalStatus } from '@/shared/chat-nodes'
import type { DockedNodes } from '@/shared/chat-nodes/types'
import type { Approval } from '@/shared/api/orchestration-client'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import type { ChatTurn, QueuedInput } from '../components/chat-types'

export type PlanApprovalNode = Extract<ConversationNode, { kind: 'plan-approval' }>

export type DockedTray =
  /** Every undecided approval, oldest first. The turn id is what a decision is recorded against. */
  | { kind: 'approval'; approvals: { turnId: string; approval: Approval }[] }
  /** The latest assistant turn's plan, while it has neither been granted nor rejected. */
  | { kind: 'plan'; turnId: string; plan: PlanApprovalNode }
  /** The thread's goal (EP-7 S1): open, met or stopped, until the person clears it. */
  | { kind: 'goal'; goal: ThreadGoal }
  /** Messages sent into the live run, in every state the strip shows. */
  | { kind: 'queue'; entries: QueuedInput[] }

export type DockedTrayKind = DockedTray['kind']

export type DockedTrayInput = {
  turns: readonly ChatTurn[]
  queuedInputs: readonly QueuedInput[]
  /** Live status of a plan approval in flight, by turn id. */
  planApproval?: (turnId: string) => PlanApprovalStatus | undefined
  /** The thread's goal, when it has one (EP-7 S1). */
  goal?: ThreadGoal | null
}

export function selectDockedTrays(input: DockedTrayInput): DockedTray[] {
  const trays: DockedTray[] = []

  const approvals = input.turns.flatMap((turn) =>
    (turn.pendingApprovals ?? []).map((approval) => ({ turnId: turn.id, approval })),
  )
  if (approvals.length > 0) trays.push({ kind: 'approval', approvals })

  // Only the latest assistant turn: an older plan that was never decided keeps
  // its control in its own turn, where it was proposed.
  const latest = latestAssistantTurn(input.turns)
  // A plan is proposed by a finished plan-mode turn; the derivation is skipped for
  // every other turn, which includes each token of an answer still streaming.
  if (latest?.planMode === true && !latest.streaming) {
    const plan = deriveConversationNodes(latest, input.planApproval?.(latest.id)).find(
      (node): node is PlanApprovalNode => node.kind === 'plan-approval',
    )
    if (plan && !plan.grantedRung && !plan.rejected) {
      trays.push({ kind: 'plan', turnId: latest.id, plan })
    }
  }

  if (input.goal) trays.push({ kind: 'goal', goal: input.goal })

  if (input.queuedInputs.length > 0) trays.push({ kind: 'queue', entries: [...input.queuedInputs] })
  return trays
}

// Shared, so a turn whose docking did not change keeps the same object across rows.
const DOCKED: Record<'none' | 'approvals' | 'plan' | 'both', DockedNodes> = {
  none: { approvals: false, plan: false },
  approvals: { approvals: true, plan: false },
  plan: { approvals: false, plan: true },
  both: { approvals: true, plan: true },
}

export function dockedNodesFor(trays: readonly DockedTray[], turnId: string): DockedNodes {
  let approvals = false
  let plan = false
  for (const tray of trays) {
    if (tray.kind === 'approval' && tray.approvals.some((entry) => entry.turnId === turnId)) approvals = true
    if (tray.kind === 'plan' && tray.turnId === turnId) plan = true
  }
  if (approvals && plan) return DOCKED.both
  return approvals ? DOCKED.approvals : plan ? DOCKED.plan : DOCKED.none
}

function latestAssistantTurn(turns: readonly ChatTurn[]): ChatTurn | undefined {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (turn?.role === 'assistant') return turn
  }
  return undefined
}
