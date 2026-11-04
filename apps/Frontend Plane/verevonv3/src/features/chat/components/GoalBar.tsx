import { For, Show, createSignal } from 'solid-js'
import type { ChatGoalProgressEvent } from '@/shared/api/chat-client'
import type { CronScheduleFire, CronTaskTemplate } from '@/shared/api/cron-client'
import {
  SCHEDULE_CONTRACT_MAX_DAYS,
  buildScheduleContractText,
  scheduleContractExpiry,
  type ScheduleContractTool,
} from '@/shared/api/schedule-from-chat'
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

/** EP-7 S6 limits, the same as the gateway's admission: at most 20 runs, at most one every 30 minutes. */
export const GOAL_AWAY_MAX_RUNS = 20
export const GOAL_AWAY_DEFAULT_RUNS = 3
export const GOAL_AWAY_DEFAULT_DAYS = 7

/** The intervals the away dialog offers, each with the cron expression the gateway accepts. */
export const GOAL_AWAY_INTERVALS: readonly { minutes: number; cron: string; no: string; en: string }[] = [
  { minutes: 30, cron: '*/30 * * * *', no: 'Hver halvtime', en: 'Every 30 minutes' },
  { minutes: 60, cron: '0 * * * *', no: 'Hver time', en: 'Every hour' },
  { minutes: 240, cron: '0 */4 * * *', no: 'Hver fjerde time', en: 'Every 4 hours' },
  { minutes: 1440, cron: '0 8 * * *', no: 'Hver dag kl. 08:00', en: 'Every day at 08:00' },
]

/** What the person chose in "Keep working while away". */
export interface GoalAwayForm {
  readonly tools: readonly ScheduleContractTool[]
  readonly maxRuns: number
  readonly everyMinutes: number
  readonly days: number
}

/** One run a goal-bound schedule made while the person was away, linked to its scheduled thread (D9). */
export interface GoalAwayRun {
  readonly runId: string
  readonly threadId: string
  readonly outcome: string
  readonly withheldEffects: number
}

/**
 * "Keep working while away" (EP-7 S6), supplied by the controller. Hidden unless the server reports it
 * available; `tools` is the delegable read-only list, and with none the dialog cannot start.
 */
export interface GoalAwayView {
  available: () => boolean
  tools: () => readonly ScheduleContractTool[]
  /** A goal-bound schedule is set for this goal and still enabled. */
  scheduled: () => boolean
  runs: () => readonly GoalAwayRun[]
  threadHref: (threadId: string) => string
  onStart: (form: GoalAwayForm) => void
}

/** The task template of a goal-bound schedule (EP-7 S6): the goal text, its id and the read-only contract. */
export interface GoalScheduleTemplate extends CronTaskTemplate {
  purpose: 'goal'
  goal_id: string
  goal_max_runs: number
  approval_contract: string
}

/**
 * The template "Keep working while away" sends through the ordinary schedule create, or null while the
 * form may not start (no tool picked, a limit out of range, a goal that is not active).
 */
export function goalScheduleTemplate(
  goal: ThreadGoal,
  form: GoalAwayForm,
  now: Date = new Date(),
): GoalScheduleTemplate | null {
  const interval = GOAL_AWAY_INTERVALS.find((candidate) => candidate.minutes === form.everyMinutes)
  const runsValid = Number.isInteger(form.maxRuns) && form.maxRuns >= 1 && form.maxRuns <= GOAL_AWAY_MAX_RUNS
  const daysValid = Number.isInteger(form.days) && form.days >= 1 && form.days <= SCHEDULE_CONTRACT_MAX_DAYS
  if (goal.status !== 'active' || !interval || !runsValid || !daysValid || form.tools.length === 0) return null
  const approvedAt = now.toISOString().replace(/\.\d{3}Z$/, 'Z')
  return {
    kind: 'cron',
    title: goal.objective.slice(0, 120),
    description: goal.objective,
    purpose: 'goal',
    goal_id: goal.id,
    goal_max_runs: form.maxRuns,
    approval_contract: buildScheduleContractText(
      { tools: form.tools, expiresAt: scheduleContractExpiry(form.days, now) },
      approvedAt,
    ),
  }
}

/** The cron expression of an interval the dialog offers. */
export function goalAwayCron(everyMinutes: number): string | null {
  return GOAL_AWAY_INTERVALS.find((candidate) => candidate.minutes === everyMinutes)?.cron ?? null
}

/** The finished fires of a goal-bound schedule as away runs: only those that reached a scheduled thread. */
export function goalAwayRuns(fires: readonly CronScheduleFire[]): GoalAwayRun[] {
  return fires.flatMap((fire) =>
    fire.run_id && fire.thread_id && fire.completed_at
      ? [
          {
            runId: fire.run_id,
            threadId: fire.thread_id,
            outcome: fire.outcome ?? fire.status,
            withheldEffects: fire.withheld_effects,
          },
        ]
      : [],
  )
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
  /** "Keep working while away" (EP-7 S6); hidden unless the server reports it available. */
  away?: GoalAwayView
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
  const away = () => (props.away?.available() ? props.away : undefined)

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
      <Show when={away()}>
        {(view) => <GoalAwayControls goal={props.goal} view={view()} pending={props.pending} />}
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

/**
 * "Keep working while away" and what happened meanwhile (EP-7 S6). The form says plainly that the goal
 * continues read only, as the service and not as the person, with changes held as drafts. Each away run
 * links to the service-owned scheduled thread its answer went to (D9).
 */
function GoalAwayControls(props: { goal: ThreadGoal; view: GoalAwayView; pending?: boolean }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [chosen, setChosen] = createSignal<readonly string[]>([])
  const [maxRuns, setMaxRuns] = createSignal(GOAL_AWAY_DEFAULT_RUNS)
  const [everyMinutes, setEveryMinutes] = createSignal(60)
  const [days, setDays] = createSignal(GOAL_AWAY_DEFAULT_DAYS)
  const form = (): GoalAwayForm => ({
    tools: props.view.tools().filter((tool) => chosen().includes(tool.tool)),
    maxRuns: maxRuns(),
    everyMinutes: everyMinutes(),
    days: days(),
  })
  const ready = () => goalScheduleTemplate(props.goal, form()) !== null
  const toggleTool = (name: string) =>
    setChosen((current) => (current.includes(name) ? current.filter((item) => item !== name) : [...current, name]))
  const runs = () => props.view.runs()

  return (
    <div class="verevon-chat-goal-bar__away">
      <Show when={runs().length > 0}>
        <p class="verevon-chat-goal-bar__away-summary">
          {runs().length === 1
            ? i18n.tr('1 kjøring mens du var borte', '1 run while you were away')
            : i18n.tr(`${runs().length} kjøringer mens du var borte`, `${runs().length} runs while you were away`)}
        </p>
        <ul class="verevon-chat-goal-bar__away-runs">
          <For each={runs()}>
            {(run) => (
              <li data-outcome={run.outcome}>
                <a href={props.view.threadHref(run.threadId)}>{i18n.tr('Åpne svaret', 'Open the answer')}</a>
                <Show when={run.withheldEffects > 0}>
                  <span class="verevon-chat-goal-bar__away-drafts">
                    {i18n.tr(`${run.withheldEffects} utkast venter`, `${run.withheldEffects} drafts held`)}
                  </span>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={props.goal.status === 'active' && !props.view.scheduled()}>
        <button
          type="button"
          class="verevon-chat-goal-bar__away-toggle"
          aria-expanded={open() ? 'true' : 'false'}
          disabled={props.pending}
          onClick={() => setOpen((value) => !value)}
        >
          {i18n.tr('Fortsett mens jeg er borte', 'Keep working while away')}
        </button>
        <Show when={open()}>
          <fieldset class="verevon-chat-goal-bar__away-form">
            <legend>{i18n.tr('Kun lesing, som tjenesten', 'Read only, as the service')}</legend>
            <p>
              {i18n.tr(
                'Målet kjører uten deg med verktøyene du velger her. Endringer holdes igjen som utkast til du ser på dem.',
                'The goal runs without you, with the tools you pick here. Changes are held as drafts until you look at them.',
              )}
            </p>
            <Show
              when={props.view.tools().length > 0}
              fallback={
                <p class="verevon-chat-goal-bar__away-none">
                  {i18n.tr(
                    'Ingen leseverktøy kan godkjennes ennå, så dette er ikke tilgjengelig.',
                    'No read-only tools can be approved yet, so this is not available.',
                  )}
                </p>
              }
            >
              <For each={props.view.tools()}>
                {(tool) => (
                  <label>
                    <input
                      type="checkbox"
                      checked={chosen().includes(tool.tool)}
                      onChange={() => toggleTool(tool.tool)}
                    />
                    {tool.tool}
                  </label>
                )}
              </For>
            </Show>
            <label>
              {i18n.tr('Høyst antall kjøringer', 'At most this many runs')}
              <input
                type="number"
                min="1"
                max={GOAL_AWAY_MAX_RUNS}
                value={maxRuns()}
                onInput={(event) => setMaxRuns(Number(event.currentTarget.value))}
              />
            </label>
            <label>
              {i18n.tr('Hvor ofte', 'How often')}
              <select value={everyMinutes()} onChange={(event) => setEveryMinutes(Number(event.currentTarget.value))}>
                <For each={GOAL_AWAY_INTERVALS}>
                  {(interval) => <option value={interval.minutes}>{i18n.tr(interval.no, interval.en)}</option>}
                </For>
              </select>
            </label>
            <label>
              {i18n.tr('Antall dager', 'For this many days')}
              <input
                type="number"
                min="1"
                max={SCHEDULE_CONTRACT_MAX_DAYS}
                value={days()}
                onInput={(event) => setDays(Number(event.currentTarget.value))}
              />
            </label>
            <button
              type="button"
              class="verevon-chat-goal-bar__away-start"
              disabled={props.pending || !ready()}
              onClick={() => {
                props.view.onStart(form())
                setOpen(false)
              }}
            >
              {i18n.tr('Start', 'Start')}
            </button>
          </fieldset>
        </Show>
      </Show>
    </div>
  )
}
