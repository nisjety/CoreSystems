/**
 * Recover a plan-mode turn's reviewable plan from the server after a reload
 * (plan-gate §2.1, "proposedPlan is session-only").
 *
 * `ChatTurn.proposedPlan` arrives on the `plan_proposed` SSE frame and is never
 * persisted into thread history — the transcript stores messages, not run
 * metadata. The plan itself, though, is durable: session-core holds it as a
 * `PROPOSED` plan row with its steps until somebody decides it. Without reading
 * it back, a reload between "the agent proposed a plan" and "the person decided"
 * leaves the rung-only control on a message that (by `plan.propose`'s own
 * contract — the call IS the plan, the answer does not repeat it) shows nothing
 * to review. Approving that grants authority against steps the person never saw
 * and leaves the plan itself undecided, so the executing run is never told what
 * it was approved to do.
 *
 * This is the pure decision half: given a run's plans (oldest first, as
 * `listPlans` returns them), what — if anything — does the card need restored?
 * The controller does the fetch and the store write.
 */

import type { ChatPlanStep } from '@/shared/api/chat-client'
import type { Plan } from '@/shared/api/orchestration-client'

export type HydratedPlan =
  /** Still awaiting a decision: restore the editable card. */
  | { kind: 'proposed'; plan: { planId: string; summary: string; steps: ChatPlanStep[] } }
  /** Already rejected: say so instead of offering to approve it again. */
  | { kind: 'rejected' }

/** `PLAN_STATE_PROPOSED` / `proposed` / `Proposed` → `PROPOSED`. */
function planStateName(state: string | undefined): string {
  return (state ?? '').trim().toUpperCase().replace(/^PLAN_STATE_/, '')
}

/**
 * The newest plan of the run that is in a state this card cares about decides
 * the outcome; superseded and archived plans are passed over, since a later
 * proposal replaced them.
 *
 * Deliberately conservative — anything else yields `null` and the turn keeps
 * the rung-only control it always had:
 *   - an APPROVED (or already executing/completed) plan is a decision that was
 *     made, and the plan row does not record which rung was granted, so there is
 *     nothing truthful to restore;
 *   - the run's own execution log is also a plan of the run, but it is never
 *     PROPOSED or REJECTED (it goes draft -> executing -> terminal), so it
 *     cannot be mistaken for a proposal;
 *   - a PROPOSED plan with no readable steps has nothing to edit.
 */
export function hydratablePlan(plans: readonly Plan[]): HydratedPlan | null {
  for (let index = plans.length - 1; index >= 0; index -= 1) {
    const plan = plans[index]
    if (!plan) continue
    const state = planStateName(plan.state)
    if (state === 'SUPERSEDED' || state === 'ARCHIVED') continue
    if (state === 'REJECTED') return { kind: 'rejected' }
    if (state !== 'PROPOSED') return null
    const steps: ChatPlanStep[] = []
    for (const step of plan.steps ?? []) {
      // A removed step is marked skipped, never deleted; a proposal has none,
      // but a card must never offer a step the person already struck out.
      if (planStateName(step.state).endsWith('SKIPPED')) continue
      steps.push({
        id: step.id,
        title: step.title,
        ...(step.operation ? { operation: step.operation } : {}),
      })
    }
    if (!plan.id || steps.length === 0) return null
    return { kind: 'proposed', plan: { planId: plan.id, summary: plan.summary ?? '', steps } }
  }
  return null
}
