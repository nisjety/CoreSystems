import { For, Match, Show, Switch, createSignal } from 'solid-js'
import type {
  AutonomyRung,
  ChatQuestionAnswer,
  ChatResearchPlanEvent,
  ChatUserQuestionEvent,
  ResearchPlanEdit,
} from '@/shared/api/chat-client'
import type { ApprovalDecision, ApprovalEdit, PlanStepEdit } from '@/shared/api/orchestration-client'
import { QuestionCard } from '@/shared/chat-nodes/QuestionCard'
import { useI18n } from '@/shared/i18n'
import type { DockedTray, DockedTrayKind } from '../lib/composer-trays'
import { ApprovalRequests, PlanApprovalControl, QueuedInputStrip } from './ChatMessages'
import { GoalBar } from './GoalBar'
import { GoalDialog, type GoalDraft } from './GoalDialog'
import { ResearchPlanCard } from './ResearchPlanCard'

const TRAY_NAME: Record<DockedTrayKind, { no: string; en: string }> = {
  approval: { no: 'godkjenning', en: 'approval' },
  plan: { no: 'plan', en: 'plan' },
  goal: { no: 'mål', en: 'goal' },
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
  onApprovalEditedDecision?: (turnId: string, approvalId: string, edit: ApprovalEdit) => Promise<void>
  onApprovePlan: (turnId: string, rung: AutonomyRung, justification: string, steps?: PlanStepEdit[]) => void
  onRejectPlan: (turnId: string, reason: string) => void
  /** The question a run put to its owner (WS14 S4). It waits behind an approval and ahead of a plan or the queue. */
  question?: ChatUserQuestionEvent
  questionPending?: boolean
  questionError?: string | null
  onAnswerQuestion?: (answer: ChatQuestionAnswer) => void
  onSkipQuestion?: () => void
  /** A deep-research plan waiting for Start (EP-2 S5). It waits behind an approval and a question, ahead of a plan. */
  researchPlan?: ChatResearchPlanEvent
  onStartResearchPlan?: (edit: ResearchPlanEdit) => void
  onCancelResearchPlan?: () => void
  /**
   * The thread goal's controls (EP-7 S1). The goal itself rides in `trays`; `goalEntry` offers "Set a
   * goal" when nothing else is docked and the thread may hold one. `goalBusy` holds Continue while a
   * turn is running. `onSaveGoal` resolves true when the server took the goal, which closes the dialog.
   */
  goalEntry?: boolean
  goalPending?: boolean
  goalError?: string | null
  goalBusy?: boolean
  onSaveGoal?: (draft: GoalDraft) => Promise<boolean>
  onContinueGoal?: () => void
  onPauseGoal?: (paused: boolean) => void
  onClearGoal?: () => void
}) {
  const i18n = useI18n()
  const active = () => props.trays[0]
  const approval = () => {
    const tray = active()
    return tray?.kind === 'approval' ? tray : undefined
  }
  // The question takes the tray unless an approval is waiting first: the order the dock is read in.
  const questionShown = () => props.question !== undefined && approval() === undefined
  const researchPlanShown = () => props.researchPlan !== undefined && approval() === undefined && !questionShown()
  const plan = () => {
    const tray = active()
    return tray?.kind === 'plan' ? tray : undefined
  }
  const queue = () => {
    const tray = active()
    return tray?.kind === 'queue' ? tray : undefined
  }
  const goalTray = () => {
    const tray = active()
    return tray?.kind === 'goal' ? tray : undefined
  }
  // The goal dialog is the person's own click, so it takes the dock until it is saved or cancelled.
  const [goalDialogOpen, setGoalDialogOpen] = createSignal(false)
  const dockedGoal = () => {
    for (const tray of props.trays) if (tray.kind === 'goal') return tray.goal
    return null
  }
  const goalEntryShown = () =>
    props.goalEntry === true && !dockedGoal() && props.trays.length === 0 && !props.question && !props.researchPlan
  const saveGoal = async (draft: GoalDraft) => {
    if (await props.onSaveGoal?.(draft)) setGoalDialogOpen(false)
  }
  const others = () =>
    questionShown() || researchPlanShown() || goalDialogOpen() ? props.trays : props.trays.slice(1)

  return (
    <div
      class="verevon-chat-composer-trays"
      data-empty={
        props.trays.length === 0 && !props.question && !props.researchPlan && !goalEntryShown() && !goalDialogOpen()
          ? ''
          : undefined
      }
      role="region"
      aria-live="polite"
      aria-label={i18n.tr('Venter på deg', 'Waiting for you')}
    >
      <Switch>
        <Match when={goalDialogOpen()}>
          <GoalDialog
            goal={dockedGoal()}
            pending={props.goalPending}
            error={props.goalError}
            onSave={(draft) => void saveGoal(draft)}
            onCancel={() => setGoalDialogOpen(false)}
          />
        </Match>
        <Match when={approval()}>
          {(tray) => (
            <ApprovalRequests
              approvals={tray().approvals.map((entry) => entry.approval)}
              onDecide={(approvalId, decision) => {
                const entry = tray().approvals.find((candidate) => candidate.approval.id === approvalId)
                if (entry) props.onApprovalDecision(entry.turnId, approvalId, decision)
              }}
              onDecideEdited={props.onApprovalEditedDecision
                ? (approvalId, edit) => {
                    const entry = tray().approvals.find((candidate) => candidate.approval.id === approvalId)
                    return entry && props.onApprovalEditedDecision
                      ? props.onApprovalEditedDecision(entry.turnId, approvalId, edit)
                      : Promise.resolve()
                  }
                : undefined}
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
        <Match when={researchPlanShown() && props.researchPlan}>
          {(researchPlan) => (
            // Keyed on the frame, so a new plan seeds a fresh card instead of keeping the last one's edits.
            <Show when={researchPlan()} keyed>
              {(current) => (
                <ResearchPlanCard
                  plan={current}
                  onStart={(edit) => props.onStartResearchPlan?.(edit)}
                  onCancel={() => props.onCancelResearchPlan?.()}
                />
              )}
            </Show>
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
        <Match when={goalTray()}>
          {(tray) => (
            <GoalBar
              goal={tray().goal}
              pending={props.goalPending}
              error={props.goalError}
              busy={props.goalBusy}
              onContinue={() => props.onContinueGoal?.()}
              onEdit={() => setGoalDialogOpen(true)}
              onPause={() => props.onPauseGoal?.(true)}
              onResume={() => props.onPauseGoal?.(false)}
              onClear={() => props.onClearGoal?.()}
            />
          )}
        </Match>
        <Match when={queue()}>{(tray) => <QueuedInputStrip entries={tray().entries} />}</Match>
        <Match when={goalEntryShown()}>
          <button type="button" class="verevon-chat-goal-entry" onClick={() => setGoalDialogOpen(true)}>
            {i18n.tr('Sett et mål for samtalen', 'Set a goal for this conversation')}
          </button>
        </Match>
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
