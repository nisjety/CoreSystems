import { Show, createMemo, createSignal } from 'solid-js'
import { THREAD_GOAL_LIMITS, type ThreadGoal } from '@/shared/api/thread-goal-client'
import { useI18n } from '@/shared/i18n'

/** What the dialog saves: the objective and the criteria, one per line, blank lines dropped. */
export type GoalDraft = { objective: string; successCriteria: string[] }

/**
 * The criteria field's lines as the server will store them, and whether they fit D11's limits. Pure,
 * so the dialog and its test read one rule.
 */
export function readGoalDraft(objective: string, criteriaText: string): { draft: GoalDraft; valid: boolean } {
  const trimmed = objective.trim()
  const criteria = criteriaText
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
  const valid =
    trimmed.length > 0 &&
    trimmed.length <= THREAD_GOAL_LIMITS.objectiveChars &&
    criteria.length <= THREAD_GOAL_LIMITS.criteria &&
    criteria.every((line) => line.length <= THREAD_GOAL_LIMITS.criterionChars)
  return { draft: { objective: trimmed, successCriteria: criteria }, valid }
}

/**
 * Set or edit the thread's goal (EP-7 S1): an objective and up to 8 success criteria, one per line.
 * Budget fields arrive with the budget ledger (EP-7 S2). The limits are checked here so the server's
 * refusal is the rare case; the server checks them again. Opened only by the person's own click, so
 * it may take focus; Cancel hands nothing to the server.
 */
export function GoalDialog(props: {
  goal?: ThreadGoal | null
  pending?: boolean
  error?: string | null
  onSave: (draft: GoalDraft) => void
  onCancel: () => void
}) {
  const i18n = useI18n()
  const [objective, setObjective] = createSignal(props.goal?.objective ?? '')
  const [criteria, setCriteria] = createSignal((props.goal?.successCriteria ?? []).join('\n'))
  const read = createMemo(() => readGoalDraft(objective(), criteria()))

  return (
    <form
      class="verevon-chat-goal-dialog"
      aria-label={props.goal ? i18n.tr('Rediger målet', 'Edit the goal') : i18n.tr('Sett et mål', 'Set a goal')}
      onSubmit={(event) => {
        event.preventDefault()
        if (read().valid && !props.pending) props.onSave(read().draft)
      }}
    >
      <label class="verevon-chat-goal-dialog__field">
        <span>{i18n.tr('Mål', 'Objective')}</span>
        <textarea
          autofocus
          rows={3}
          maxlength={THREAD_GOAL_LIMITS.objectiveChars}
          value={objective()}
          onInput={(event) => setObjective(event.currentTarget.value)}
          placeholder={i18n.tr('Hva skal være gjort?', 'What should be done?')}
        />
      </label>
      <label class="verevon-chat-goal-dialog__field">
        <span>
          {i18n.tr('Kriterier (ett per linje, høyst 8)', 'Success criteria (one per line, at most 8)')}
        </span>
        <textarea
          rows={4}
          value={criteria()}
          onInput={(event) => setCriteria(event.currentTarget.value)}
          placeholder={i18n.tr('Hvordan vet du at det er gjort?', 'How will you know it is done?')}
        />
      </label>
      <Show when={!read().valid && (objective().trim().length > 0 || criteria().trim().length > 0)}>
        <p class="verevon-chat-goal-dialog__hint" role="status">
          {i18n.tr(
            'Målet må ha tekst, høyst 2000 tegn, og høyst 8 kriterier på 500 tegn.',
            'The goal needs text, at most 2000 characters, and at most 8 criteria of 500 characters.',
          )}
        </p>
      </Show>
      <Show when={props.error}>
        {(message) => (
          <p class="verevon-chat-goal-dialog__error" role="status">
            {message()}
          </p>
        )}
      </Show>
      <p class="verevon-chat-goal-dialog__note">
        {i18n.tr(
          'Målet vises for agenten som data i hver Utfør-runde i denne samtalen.',
          'The goal is shown to the agent as data on every Do turn in this conversation.',
        )}
      </p>
      <div class="verevon-chat-goal-dialog__actions">
        <button type="submit" disabled={!read().valid || props.pending}>
          {i18n.tr('Lagre mål', 'Save goal')}
        </button>
        <button type="button" onClick={() => props.onCancel()}>
          {i18n.tr('Avbryt', 'Cancel')}
        </button>
      </div>
    </form>
  )
}
