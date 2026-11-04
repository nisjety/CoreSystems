import { For, Show, createSignal } from 'solid-js'
import type { ThreadGoal, ThreadGoalStatus } from '@/shared/api/thread-goal-client'
import { useI18n } from '@/shared/i18n'

const STATUS_LABEL: Record<ThreadGoalStatus, { no: string; en: string }> = {
  active: { no: 'Aktivt', en: 'Active' },
  paused: { no: 'Satt på pause', en: 'Paused' },
  met: { no: 'Nådd', en: 'Met' },
  budget_reached: { no: 'Budsjett brukt opp', en: 'Budget reached' },
  turns_reached: { no: 'Ingen runder igjen', en: 'Out of turns' },
  blocked: { no: 'Stoppet', en: 'Blocked' },
}

/** How much of the objective the collapsed bar shows; the rest is one click away. */
const COLLAPSED_CHARS = 120

/**
 * The thread goal, docked above the composer (EP-7 S1, decision D1): one line with the objective,
 * the number of success criteria and a status chip, and the actions Continue, Edit, Pause/Resume
 * and Clear. Continue sends one turn toward the goal (`chat.continue_goal`); nothing here sends a
 * turn on its own. The bar never takes focus: it sits in the dock's polite live region, and a goal
 * must not pull the cursor out of the field the person is typing in.
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
}) {
  const i18n = useI18n()
  const [expanded, setExpanded] = createSignal(false)
  const open = () => props.goal.status === 'active' || props.goal.status === 'paused'
  const long = () => props.goal.objective.length > COLLAPSED_CHARS
  const objective = () =>
    expanded() || !long() ? props.goal.objective : `${props.goal.objective.slice(0, COLLAPSED_CHARS).trimEnd()}…`
  const status = () => STATUS_LABEL[props.goal.status]
  const criteriaCount = () => props.goal.successCriteria.length

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
      </div>
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
