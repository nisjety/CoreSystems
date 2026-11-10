import { For, Match, Show, Switch, createEffect, createSignal } from 'solid-js'
import type {
  AutonomyRung,
  ChatQuestionAnswer,
  ChatResearchPlanEvent,
  ChatUserQuestionEvent,
  ResearchPlanEdit,
  WideResearchStart as WideResearchChoice,
} from '@/shared/api/chat-client'
import { listDeckTemplates, type DeckTemplate } from '@/shared/api/chat-client'
import type { ApprovalDecision, ApprovalEdit, PlanStepEdit } from '@/shared/api/orchestration-client'
import { QuestionCard } from '@/shared/chat-nodes/QuestionCard'
import { X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { DockedTray, DockedTrayKind } from '../lib/composer-trays'
import type { GoalAutoContinueView } from '../lib/goal-pump'
import { ApprovalRequests, PlanApprovalControl, QueuedInputStrip } from './ChatMessages'
import { DeckTemplatePicker } from './DeckTemplatePicker'
import { GoalBar, type GoalAwayView } from './GoalBar'
import { GoalDialog, type GoalDraft } from './GoalDialog'
import { ResearchPlanCard } from './ResearchPlanCard'
import { WideResearchStart } from './WideResearchStart'

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
  /** Mark the open goal done, or reopen a met or blocked one (EP-7 S3): the person's own override of the judge. */
  onMarkGoalDone?: () => void
  onReopenGoal?: () => void
  /** "Keep going" on the goal bar (EP-7 S4). */
  goalAutoContinue?: GoalAutoContinueView
  /** "Keep working while away" on the goal bar (EP-7 S6); hidden unless the server reports it available. */
  goalAway?: GoalAwayView
  /**
   * The composer's "Bred forskning" entry (EP-2 S8). Offered only when the page says the gateway serves it and the
   * chat may send a wide-research turn. `wideResearchModel` is the model its estimate names; `onStartWideResearch`
   * takes the person's choice once they press Start, and answers `false` when it sent nothing (the card then stays).
   */
  wideResearchEntry?: boolean
  wideResearchModel?: string
  onStartWideResearch?: (choice: WideResearchChoice) => boolean | void
  /**
   * The composer's deck template chip (EP-5 S7). Offered while the draft talks about a deck and no run streams; the
   * gateway's list decides whether it shows at all. `deckTemplate` is the pick the next send carries, and
   * `onDeckTemplateChange` sets it (`null` is Standard, which carries none).
   */
  deckTemplateEntry?: boolean
  deckTemplate?: DeckTemplate | null
  onDeckTemplateChange?: (template: DeckTemplate | null) => void
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
  // The wide-research card is the person's own click too: it takes the dock until it is started or cancelled.
  const [wideResearchOpen, setWideResearchOpen] = createSignal(false)
  const wideResearchEntryShown = () =>
    props.wideResearchEntry === true
    && props.trays.length === 0
    && !props.question
    && !props.researchPlan
    && !goalDialogOpen()
    && !wideResearchOpen()
  // The deck template picker is the person's own click as well: its card takes the dock until a pick or Close. The
  // entry hides once a pick is made, and the pick shows as its own row below the dock.
  const [deckPickerOpen, setDeckPickerOpen] = createSignal(false)
  // The gateway's list decides whether the entry exists: it is asked once the draft first talks about a deck, and it is
  // offered only with at least one template (the list route answers 404 while the templates are off, and null in a
  // zero-retention session). A reply that arrives after the hint has gone is dropped.
  const [deckListed, setDeckListed] = createSignal(false)
  let deckListing = 0
  createEffect(
    () => props.deckTemplateEntry === true,
    (wanted) => {
      const request = ++deckListing
      if (!wanted) return
      listDeckTemplates()
        .then((listed) => {
          if (request === deckListing) setDeckListed((listed?.length ?? 0) > 0)
        })
        .catch(() => {
          if (request === deckListing) setDeckListed(false)
        })
    },
  )
  const deckEntryShown = () =>
    props.deckTemplateEntry === true
    && deckListed()
    && !props.deckTemplate
    && props.trays.length === 0
    && !props.question
    && !props.researchPlan
    && !goalDialogOpen()
    && !wideResearchOpen()
    && !deckPickerOpen()
  const saveGoal = async (draft: GoalDraft) => {
    if (await props.onSaveGoal?.(draft)) setGoalDialogOpen(false)
  }
  const others = () =>
    questionShown() || researchPlanShown() || goalDialogOpen() ? props.trays : props.trays.slice(1)

  return (
    <div
      class="verevon-chat-composer-trays"
      data-empty={
        props.trays.length === 0
        && !props.question
        && !props.researchPlan
        && !goalEntryShown()
        && !goalDialogOpen()
        && !wideResearchEntryShown()
        && !wideResearchOpen()
        && !deckEntryShown()
        && !deckPickerOpen()
        && !props.deckTemplate
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
              onMarkDone={() => props.onMarkGoalDone?.()}
              onReopen={() => props.onReopenGoal?.()}
              autoContinue={props.goalAutoContinue}
              away={props.goalAway}
            />
          )}
        </Match>
        <Match when={queue()}>{(tray) => <QueuedInputStrip entries={tray().entries} />}</Match>
        <Match when={wideResearchOpen()}>
          <WideResearchStart
            model={props.wideResearchModel}
            onStart={(choice) => {
              if (props.onStartWideResearch?.(choice) !== false) setWideResearchOpen(false)
            }}
            onCancel={() => setWideResearchOpen(false)}
          />
        </Match>
        <Match when={deckPickerOpen()}>
          <div class="verevon-chat-composer-trays__entries">
            <DeckTemplatePicker
              selected={props.deckTemplate?.skillId ?? null}
              onSelect={(template) => {
                props.onDeckTemplateChange?.(template)
                setDeckPickerOpen(false)
              }}
            />
            <button type="button" class="verevon-chat-goal-entry" onClick={() => setDeckPickerOpen(false)}>
              {i18n.tr('Lukk', 'Close')}
            </button>
          </div>
        </Match>
        <Match when={goalEntryShown() || wideResearchEntryShown() || deckEntryShown()}>
          <div class="verevon-chat-composer-trays__entries">
            <Show when={goalEntryShown()}>
              <button type="button" class="verevon-chat-goal-entry" onClick={() => setGoalDialogOpen(true)}>
                {i18n.tr('Sett et mål for samtalen', 'Set a goal for this conversation')}
              </button>
            </Show>
            <Show when={wideResearchEntryShown()}>
              <button type="button" class="verevon-chat-goal-entry" onClick={() => setWideResearchOpen(true)}>
                {i18n.tr('Bred forskning', 'Wide research')}
              </button>
            </Show>
            <Show when={deckEntryShown()}>
              <button type="button" class="verevon-chat-goal-entry" onClick={() => setDeckPickerOpen(true)}>
                {i18n.tr('Velg mal', 'Choose template')}
              </button>
            </Show>
          </div>
        </Match>
      </Switch>
      {/* The pick the next send carries. It stays in view while a tray waits, so a template is never applied unseen. */}
      <Show when={props.deckTemplate}>
        {(template) => (
          <div class="verevon-chat-composer-trays__entries">
            <button
              type="button"
              class="verevon-chat-goal-entry"
              aria-expanded={deckPickerOpen() ? 'true' : 'false'}
              onClick={() => setDeckPickerOpen(true)}
            >
              {i18n.tr('Mal', 'Template')}: {template().name}
            </button>
            <button
              type="button"
              class="verevon-chat-goal-entry"
              aria-label={i18n.tr('Fjern malen', 'Remove the template')}
              onClick={() => props.onDeckTemplateChange?.(null)}
            >
              <X class="size-3" aria-hidden="true" />
            </button>
          </div>
        )}
      </Show>
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
