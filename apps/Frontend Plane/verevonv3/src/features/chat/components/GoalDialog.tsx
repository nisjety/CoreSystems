import { Show, createMemo, createSignal } from 'solid-js'
import { THREAD_GOAL_LIMITS, type ThreadGoal } from '@/shared/api/thread-goal-client'
import { useI18n } from '@/shared/i18n'

/**
 * What the dialog saves: the objective and the criteria, one per line, blank lines dropped, and the
 * caps (EP-7 S2) when the person set them: the turn cap and an optional metered-token budget.
 */
export type GoalDraft = { objective: string; successCriteria: string[]; maxTurns?: number; tokenBudget?: number }

/** D11: the largest metered-token budget the server accepts. */
export const MAX_GOAL_TOKEN_BUDGET = 5_000_000

/** The caps as typed: empty means "the default turn cap" and "no token budget". */
export type GoalBudgetText = { maxTurns: string; tokenBudget: string }

/** A whole number typed into a cap field; `undefined` when empty, `NaN` when not a whole number. */
function capValue(raw: string): number | undefined {
  const trimmed = raw.trim()
  if (!trimmed) return undefined
  return /^\d+$/.test(trimmed) ? Number(trimmed) : Number.NaN
}

/** An unset cap, or a whole number from 1 to `max`. */
function withinCap(value: number | undefined, max: number): boolean {
  return value === undefined || (Number.isInteger(value) && value >= 1 && value <= max)
}

/**
 * The criteria field's lines as the server will store them, and whether they fit D11's limits. Pure,
 * so the dialog and its test read one rule. The caps are read only when the dialog shows them.
 */
export function readGoalDraft(
  objective: string,
  criteriaText: string,
  budget?: GoalBudgetText,
): { draft: GoalDraft; valid: boolean } {
  const trimmed = objective.trim()
  const criteria = criteriaText
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
  const maxTurns = budget ? capValue(budget.maxTurns) : undefined
  const tokenBudget = budget ? capValue(budget.tokenBudget) : undefined
  const capsValid = withinCap(maxTurns, THREAD_GOAL_LIMITS.maxTurns) && withinCap(tokenBudget, MAX_GOAL_TOKEN_BUDGET)
  const valid =
    trimmed.length > 0 &&
    trimmed.length <= THREAD_GOAL_LIMITS.objectiveChars &&
    criteria.length <= THREAD_GOAL_LIMITS.criteria &&
    criteria.every((line) => line.length <= THREAD_GOAL_LIMITS.criterionChars) &&
    capsValid
  return {
    draft: {
      objective: trimmed,
      successCriteria: criteria,
      ...(maxTurns !== undefined && capsValid ? { maxTurns } : {}),
      ...(tokenBudget !== undefined && capsValid ? { tokenBudget } : {}),
    },
    valid,
  }
}

/**
 * Set or edit the thread's goal (EP-7 S1): an objective and up to 8 success criteria, one per line,
 * and its caps (EP-7 S2): a turn cap (default 10, at most 50) and an optional metered-token budget.
 * The limits are checked here so the server's refusal is the rare case; the server checks them
 * again. Opened only by the person's own click, so it may take focus; Cancel hands nothing to the
 * server.
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
  const [maxTurns, setMaxTurns] = createSignal(props.goal?.maxTurns ? String(props.goal.maxTurns) : '')
  const [tokenBudget, setTokenBudget] = createSignal(props.goal?.tokenBudget ? String(props.goal.tokenBudget) : '')
  const read = createMemo(() =>
    readGoalDraft(objective(), criteria(), { maxTurns: maxTurns(), tokenBudget: tokenBudget() }),
  )

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
      <div class="verevon-chat-goal-dialog__caps">
        <label class="verevon-chat-goal-dialog__field">
          <span>{i18n.tr('Høyst antall runder (1-50, standard 10)', 'Most turns (1-50, default 10)')}</span>
          <input
            type="text"
            inputmode="numeric"
            value={maxTurns()}
            onInput={(event) => setMaxTurns(event.currentTarget.value)}
            placeholder="10"
          />
        </label>
        <label class="verevon-chat-goal-dialog__field">
          <span>{i18n.tr('Tokenbudsjett (valgfritt)', 'Token budget (optional)')}</span>
          <input
            type="text"
            inputmode="numeric"
            value={tokenBudget()}
            onInput={(event) => setTokenBudget(event.currentTarget.value)}
            placeholder={i18n.tr('Ingen grense', 'No limit')}
          />
        </label>
      </div>
      <p class="verevon-chat-goal-dialog__note">
        {i18n.tr(
          'Bare målte tokens telles mot budsjettet. Med ChatGPT-abonnementet brukes ingen målte tokens, så der gjelder bare grensen for runder.',
          'Only metered tokens count against the budget. The ChatGPT subscription spends no metered tokens, so only the turn cap applies there.',
        )}
      </p>
      <Show
        when={
          !read().valid &&
          (objective().trim().length > 0 ||
            criteria().trim().length > 0 ||
            maxTurns().trim().length > 0 ||
            tokenBudget().trim().length > 0)
        }
      >
        <p class="verevon-chat-goal-dialog__hint" role="status">
          {i18n.tr(
            'Målet må ha tekst, høyst 2000 tegn, høyst 8 kriterier på 500 tegn, 1 til 50 runder og et budsjett på høyst 5 000 000 tokens.',
            'The goal needs text, at most 2000 characters, at most 8 criteria of 500 characters, 1 to 50 turns and a budget of at most 5,000,000 tokens.',
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
