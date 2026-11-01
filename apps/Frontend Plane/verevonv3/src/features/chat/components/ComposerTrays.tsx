import { For, Match, Show, Switch } from 'solid-js'
import type { AutonomyRung, ChatQuestionAnswer, ChatUserQuestionEvent } from '@/shared/api/chat-client'
import type { ApprovalDecision, PlanStepEdit } from '@/shared/api/orchestration-client'
import { QuestionCard } from '@/shared/chat-nodes/QuestionCard'
import { useI18n } from '@/shared/i18n'
import type { DockedTray, DockedTrayKind } from '../lib/composer-trays'
import { ApprovalRequests, PlanApprovalControl, QueuedInputStrip } from './ChatMessages'

const TRAY_NAME: Record<DockedTrayKind, { no: string; en: string }> = {
  approval: { no: 'godkjenning', en: 'approval' },
  plan: { no: 'plan', en: 'plan' },
  queue: { no: 'meldinger', en: 'messages' },
}

/**
 * The controls that wait on the person, docked above the composer.
 *
 * One tray at a time, the first of `trays` (already in priority order, see
 * `selectDockedTrays`); the others are named in a line under it so nothing waiting
 * is hidden. The tray is a polite live region that never takes focus: a run that
 * needs an answer must not pull the cursor out of the field the person is typing
 * in. Its height is capped so a long plan scrolls inside it instead of pushing the
 * composer off screen.
 *
 * The container is always mounted: a live region that appears together with its
 * content is not reliably announced.
 */
export function ComposerTrays(props: {
  trays: readonly DockedTray[]
  onApprovalDecision: (turnId: string, approvalId: string, decision: ApprovalDecision) => void
  onApprovePlan: (turnId: string, rung: AutonomyRung, justification: string, steps?: PlanStepEdit[]) => void
  onRejectPlan: (turnId: string, reason: string) => void
  /** The question a run put to its owner (WS14 S4). It waits behind an approval and ahead of a plan or the queue. */
  question?: ChatUserQuestionEvent
  questionPending?: boolean
  questionError?: string | null
  onAnswerQuestion?: (answer: ChatQuestionAnswer) => void
  onSkipQuestion?: () => void
}) {
  const i18n = useI18n()
  const active = () => props.trays[0]
  const approval = () => {
    const tray = active()
    return tray?.kind === 'approval' ? tray : undefined
  }
  // The question takes the tray unless an approval is waiting first: the order the dock is read in.
  const questionShown = () => props.question !== undefined && approval() === undefined
  const plan = () => {
    const tray = active()
    return tray?.kind === 'plan' ? tray : undefined
  }
  const queue = () => {
    const tray = active()
    return tray?.kind === 'queue' ? tray : undefined
  }
  const others = () => (questionShown() ? props.trays : props.trays.slice(1))

  return (
    <div
      class="verevon-chat-composer-trays"
      data-empty={props.trays.length === 0 && !props.question ? '' : undefined}
      role="region"
      aria-live="polite"
      aria-label={i18n.tr('Venter på deg', 'Waiting for you')}
    >
      <Switch>
        <Match when={approval()}>
          {(tray) => (
            <ApprovalRequests
              approvals={tray().approvals.map((entry) => entry.approval)}
              onDecide={(approvalId, decision) => {
                const entry = tray().approvals.find((candidate) => candidate.approval.id === approvalId)
                if (entry) props.onApprovalDecision(entry.turnId, approvalId, decision)
              }}
            />
          )}
        </Match>
        <Match when={questionShown() && props.question}>
          {(question) => (
            <QuestionCard
              question={question()}
              pending={props.questionPending}
              error={props.questionError ?? undefined}
              onAnswer={(answer) => props.onAnswerQuestion?.(answer)}
              onSkip={() => props.onSkipQuestion?.()}
            />
          )}
        </Match>
        <Match when={plan()}>
          {(tray) => (
            // Keyed on the plan, so a different proposal starts a fresh control
            // (its edits are seeded once) while a status change updates this one.
            // The child takes the key because a keyed <Show> rebuilds only a
            // child that does.
            <Show when={`${tray().turnId}:${tray().plan.planId ?? ''}`} keyed>
              {(_planKey) => (
                <PlanApprovalControl
                  planId={tray().plan.planId}
                  summary={tray().plan.summary}
                  steps={tray().plan.steps}
                  grantedRung={tray().plan.grantedRung}
                  rejected={tray().plan.rejected}
                  pending={tray().plan.pending}
                  error={tray().plan.error}
                  onApprove={(rung, justification, steps) =>
                    props.onApprovePlan(tray().turnId, rung, justification, steps)
                  }
                  onReject={(_planId, reason) => props.onRejectPlan(tray().turnId, reason)}
                />
              )}
            </Show>
          )}
        </Match>
        <Match when={queue()}>{(tray) => <QueuedInputStrip entries={tray().entries} />}</Match>
      </Switch>
      <Show when={others().length > 0}>
        <p class="verevon-chat-composer-trays__more">
          {i18n.tr('Venter også:', 'Also waiting:')}{' '}
          <For each={others()}>
            {(tray, index) => (
              <>
                {index() > 0 ? ', ' : ''}
                {i18n.tr(TRAY_NAME[tray.kind].no, TRAY_NAME[tray.kind].en)}
              </>
            )}
          </For>
        </p>
      </Show>
    </div>
  )
}
