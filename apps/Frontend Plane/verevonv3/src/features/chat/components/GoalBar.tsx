import { For, Show, createSignal } from 'solid-js'
import type { ChatGoalProgressEvent } from '@/shared/api/chat-client'
import type { ThreadGoal, ThreadGoalStatus } from '@/shared/api/thread-goal-client'
import { useI18n } from '@/shared/i18n'
import type { GoalAutoContinueView, GoalPumpStopReason } from '../lib/goal-pump'

const STATUS_LABEL: Record<ThreadGoalStatus, { no: string; en: string }> = {
  active: { no: 'Aktivt', en: 'Active' },
  paused: { no: 'Satt på pause', en: 'Paused' },
  met: { no: 'Nådd', en: 'Met' },
  budget_reached: { no: 'Budsjett brukt opp', en: 'Budget reached' },
  turns_reached: { no: 'Ingen runder igjen', en: 'Out of turns' },
  blocked: { no: 'Stoppet', en: 'Blocked' },
}

/** Why "Keep going" stopped (EP-7 S4), one closed reason each. */
const STOP_LABEL: Record<GoalPumpStopReason, { no: string; en: string }> = {
  pending_approval: { no: 'Stoppet: venter på godkjenning', en: 'Stopped: waiting for an approval' },
  pending_question: { no: 'Stoppet: venter på svar', en: 'Stopped: waiting for an answer' },
  plan_proposed: { no: 'Stoppet: en plan venter', en: 'Stopped: a plan is waiting' },
  parked: { no: 'Stoppet: kjøringen er parkert', en: 'Stopped: the run is parked' },
  failed: { no: 'Stoppet: runden feilet', en: 'Stopped: the turn failed' },
  budget_reached: { no: 'Stoppet: budsjettet er brukt opp', en: 'Stopped: budget reached' },
  turns_reached: { no: 'Stoppet: ingen runder igjen', en: 'Stopped: out of turns' },
  met: { no: 'Stoppet: målet er nådd', en: 'Stopped: the goal is met' },
  blocked: { no: 'Stoppet: målet står fast', en: 'Stopped: the goal is blocked' },
  user_stopped: { no: 'Stoppet av deg', en: 'Stopped by you' },
  hidden_tab: { no: 'Venter til fanen vises', en: 'Waiting until this tab is shown' },
}

/** How much of the objective the collapsed bar shows; the rest is one click away. */
const COLLAPSED_CHARS = 120

/** Why a goal stopped on its own (EP-7 S2), by the closed reason the server wrote. */
const STOP_REASON_LABEL: Record<string, { no: string; en: string }> = {
  token_budget: { no: 'Tokenbudsjettet er brukt opp', en: 'Token budget spent' },
  cost_budget: { no: 'Kostnadsbudsjettet er brukt opp', en: 'Cost budget spent' },
  max_turns: { no: 'Alle runder er brukt', en: 'All turns used' },
}

/** A token count as the usage line shows it: 950, 41k, 1.5M. */
export function compactTokens(value: number): string {
  if (value < 1000) return String(value)
  if (value < 1_000_000) return `${Math.round(value / 1000)}k`
  return `${Math.round(value / 100_000) / 10}M`
}

/**
 * The goal's spend against its caps (EP-7 S2), as one line: "41k of 200k tokens - turn 3 of 10".
 * `null` when the goal has spent nothing and sets no cap to show.
 */
export function goalUsageLine(goal: ThreadGoal): { no: string; en: string } | null {
  const parts: { no: string; en: string }[] = []
  if (goal.tokenBudget) {
    const used = compactTokens(goal.tokensUsed)
    const budget = compactTokens(goal.tokenBudget)
    parts.push({ no: `${used} av ${budget} tokens`, en: `${used} of ${budget} tokens` })
  } else if (goal.tokensUsed > 0) {
    const used = compactTokens(goal.tokensUsed)
    parts.push({ no: `${used} tokens`, en: `${used} tokens` })
  }
  if (goal.maxTurns) {
    parts.push({
      no: `runde ${goal.turnsUsed} av ${goal.maxTurns}`,
      en: `turn ${goal.turnsUsed} of ${goal.maxTurns}`,
    })
  }
  if (parts.length === 0) return null
  return { no: parts.map((part) => part.no).join(' - '), en: parts.map((part) => part.en).join(' - ') }
}

/**
 * The goal after a `goal_progress` frame (EP-7 S2): the frame's counts and status over the goal the
 * bar shows. A frame for another goal leaves it as it is; a goal the frame says was cleared is gone.
 */
export function applyGoalProgress(goal: ThreadGoal | null, event: ChatGoalProgressEvent): ThreadGoal | null {
  if (!goal || goal.id !== event.goalId) return goal
  if (event.status === 'cleared') return null
  const next: ThreadGoal = {
    ...goal,
    status: event.status,
    tokensUsed: event.tokensUsed,
    turnsUsed: event.turnsUsed,
    ...(event.tokenBudget ? { tokenBudget: event.tokenBudget } : {}),
    ...(event.maxTurns ? { maxTurns: event.maxTurns } : {}),
  }
  // The reason belongs to the status the frame states: a goal still active has none.
  if (event.statusReason) next.statusReason = event.statusReason
  else delete next.statusReason
  return next
}

/**
 * The thread goal, docked above the composer (EP-7 S1, decision D1): one line with the objective,
 * the number of success criteria and a status chip, and the actions Continue, Edit, Pause/Resume
 * and Clear. Continue sends one turn toward the goal (`chat.continue_goal`); nothing here sends a
 * turn on its own. The bar never takes focus: it sits in the dock's polite live region, and a goal
 * must not pull the cursor out of the field the person is typing in.
 *
 * With `autoContinue` available (EP-7 S4) an active goal also offers "Keep going": after a turn the
 * judge did not find met, the bar counts down five seconds and the controller sends the next turn;
 * Stop cancels the countdown and turns it off, and the reason the pump stopped stays on the bar. The
 * countdown is plain text in a timer role, which screen readers do not announce on every tick, and
 * nothing in it moves, so there is no motion to reduce.
 */
export function GoalBar(props: {
  goal: ThreadGoal
  pending?: boolean
  error?: string | null
  /** A turn is running: Continue waits for it to end. */
  busy?: boolean
  onContinue: () => void
  onEdit: () => void
  onPause: () => void
  onResume: () => void
  onClear: () => void
  /** "Keep going" (EP-7 S4); hidden unless the server reports it available. */
  autoContinue?: GoalAutoContinueView
}) {
  const i18n = useI18n()
  const [expanded, setExpanded] = createSignal(false)
  const open = () => props.goal.status === 'active' || props.goal.status === 'paused'
  const long = () => props.goal.objective.length > COLLAPSED_CHARS
  const objective = () =>
    expanded() || !long() ? props.goal.objective : `${props.goal.objective.slice(0, COLLAPSED_CHARS).trimEnd()}…`
  const status = () => STATUS_LABEL[props.goal.status]
  const criteriaCount = () => props.goal.successCriteria.length
  const usage = () => goalUsageLine(props.goal)
  const stopReason = () =>
    props.goal.status === 'budget_reached' || props.goal.status === 'turns_reached'
      ? STOP_REASON_LABEL[props.goal.statusReason ?? '']
      : undefined
  const auto = () => (props.goal.status === 'active' && props.autoContinue?.available() ? props.autoContinue : undefined)

  return (
    <div class="verevon-chat-goal-bar" data-status={props.goal.status} aria-busy={props.pending ? 'true' : undefined}>
      <div class="verevon-chat-goal-bar__line">
        <span class="verevon-chat-goal-bar__label">{i18n.tr('Mål', 'Goal')}</span>
        <span class="verevon-chat-goal-bar__objective">{objective()}</span>
        <Show when={long()}>
          <button
            type="button"
            class="verevon-chat-goal-bar__more"
            aria-expanded={expanded() ? 'true' : 'false'}
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded() ? i18n.tr('Vis mindre', 'Show less') : i18n.tr('Vis hele', 'Show all')}
          </button>
        </Show>
        <Show when={criteriaCount() > 0}>
          <span class="verevon-chat-goal-bar__criteria">
            {criteriaCount() === 1
              ? i18n.tr('1 kriterium', '1 criterion')
              : i18n.tr(`${criteriaCount()} kriterier`, `${criteriaCount()} criteria`)}
          </span>
        </Show>
        <span class="verevon-chat-goal-bar__status" data-status={props.goal.status}>
          {i18n.tr(status().no, status().en)}
        </span>
        <Show when={stopReason()}>
          {(reason) => (
            <span class="verevon-chat-goal-bar__stop-reason" data-reason={props.goal.statusReason}>
              {i18n.tr(reason().no, reason().en)}
            </span>
          )}
        </Show>
      </div>
      <Show when={usage()}>
        {(line) => <p class="verevon-chat-goal-bar__usage">{i18n.tr(line().no, line().en)}</p>}
      </Show>
      <Show when={expanded() && criteriaCount() > 0}>
        <ul class="verevon-chat-goal-bar__criteria-list" aria-label={i18n.tr('Kriterier for målet', 'Success criteria')}>
          <For each={props.goal.successCriteria}>{(line) => <li>{line}</li>}</For>
        </ul>
      </Show>
      <div class="verevon-chat-goal-bar__actions">
        <Show when={props.goal.status === 'active'}>
          <button
            type="button"
            class="verevon-chat-goal-bar__continue"
            disabled={props.pending || props.busy}
            onClick={() => props.onContinue()}
          >
            {i18n.tr('Fortsett', 'Continue')}
          </button>
        </Show>
        <Show when={open()}>
          <button type="button" disabled={props.pending} onClick={() => props.onEdit()}>
            {i18n.tr('Rediger', 'Edit')}
          </button>
          <Show
            when={props.goal.status === 'active'}
            fallback={
              <button type="button" disabled={props.pending} onClick={() => props.onResume()}>
                {i18n.tr('Gjenoppta', 'Resume')}
              </button>
            }
          >
            <button type="button" disabled={props.pending} onClick={() => props.onPause()}>
              {i18n.tr('Pause', 'Pause')}
            </button>
          </Show>
        </Show>
        <button type="button" disabled={props.pending} onClick={() => props.onClear()}>
          {i18n.tr('Fjern', 'Clear')}
        </button>
      </div>
      <Show when={auto()}>
        {(pump) => (
          <div class="verevon-chat-goal-bar__auto">
            <button
              type="button"
              role="switch"
              class="verevon-chat-goal-bar__auto-toggle"
              aria-checked={pump().on() ? 'true' : 'false'}
              disabled={props.pending}
              onClick={() => pump().setOn(!pump().on())}
            >
              {i18n.tr('Kjør videre', 'Keep going')}
            </button>
            <Show when={pump().countdown()}>
              {(seconds) => (
                <>
                  <span class="verevon-chat-goal-bar__countdown" role="timer">
                    {i18n.tr(`Fortsetter om ${seconds()} s`, `Continuing in ${seconds()} s`)}
                  </span>
                  <button type="button" class="verevon-chat-goal-bar__stop" onClick={() => pump().stop()}>
                    {i18n.tr('Stopp', 'Stop')}
                  </button>
                </>
              )}
            </Show>
            <Show when={pump().stopReason()}>
              {(reason) => (
                <span class="verevon-chat-goal-bar__auto-stopped" data-reason={reason()}>
                  {i18n.tr(STOP_LABEL[reason()].no, STOP_LABEL[reason()].en)}
                </span>
              )}
            </Show>
          </div>
        )}
      </Show>
      <Show when={props.error}>
        {(message) => (
          <p class="verevon-chat-goal-bar__error" role="status">
            {message()}
          </p>
        )}
      </Show>
    </div>
  )
}
