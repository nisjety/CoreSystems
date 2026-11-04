/**
 * The plan card of a two-turn deep research (EP-2 S5). A "Dyp research" send first gets the plan:
 * the sub-queries model-gateway would search and up to three clarifying questions. The person edits,
 * removes or adds sub-queries up to the gateway's cap and answers what they like; Start sends the run
 * turn (`chat.research_plan_start`) with exactly that plan, and Cancel closes the card
 * (`chat.research_plan_cancel`). Nothing waits on the server between the two turns, so a dismissed
 * plan leaves nothing behind, and the gateway re-clamps whatever Start sends.
 *
 * Docked with the composer trays, behind an approval and a run's question. Shown only while
 * `VITE_DEEP_RESEARCH_PLAN_TURN` is the literal `true`, flipped together with the gateway's
 * `DEEP_RESEARCH_PLAN_TURN`.
 */

import { createSignal, For, Show } from 'solid-js'
import type { ChatResearchPlanEvent, ResearchPlanEdit } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

/** Longest sub-query the card accepts: the gateway cuts at the same length. */
export const RESEARCH_PLAN_MAX_QUERY_CHARS = 200

/** Longest answer the card accepts: the gateway bounds the whole answers line at this. */
export const RESEARCH_PLAN_MAX_ANSWER_CHARS = 1000

/** Whether deep research asks for a plan first. Read at call time so a test can pin it. */
export const researchPlanTurnEnabled = (): boolean =>
  import.meta.env.VITE_DEEP_RESEARCH_PLAN_TURN === 'true'

/**
 * The plan Start sends: non-blank sub-queries, trimmed, cut to the gateway's length and at most
 * `maxSubQueries`, and the answered questions only.
 */
export function researchPlanEdit(
  subQueries: readonly string[],
  questions: readonly string[],
  answers: readonly string[],
  maxSubQueries: number,
): ResearchPlanEdit {
  return {
    subQueries: subQueries
      .map((query) => query.trim().slice(0, RESEARCH_PLAN_MAX_QUERY_CHARS))
      .filter((query) => query.length > 0)
      .slice(0, maxSubQueries),
    answers: questions
      .map((question, index) => ({
        question,
        answer: (answers[index] ?? '').trim().slice(0, RESEARCH_PLAN_MAX_ANSWER_CHARS),
      }))
      .filter((entry) => entry.answer.length > 0),
  }
}

export function ResearchPlanCard(props: {
  plan: ChatResearchPlanEvent
  /** A plan whose run turn already went out: shown, not edited. */
  readOnly?: boolean
  onStart: (edit: ResearchPlanEdit) => void
  onCancel: () => void
}) {
  const i18n = useI18n()
  // Rows are keyed by a stable id so editing one never rebuilds its field (and loses the cursor).
  let nextRow = 0
  const seed = props.plan.subQueries.slice(0, props.plan.maxSubQueries)
  const [rows, setRows] = createSignal<number[]>(seed.map(() => nextRow++))
  const [texts, setTexts] = createSignal<Record<number, string>>(
    Object.fromEntries(seed.map((query, index) => [index, query])),
  )
  const [answers, setAnswers] = createSignal<string[]>(props.plan.questions.map(() => ''))

  const queries = () => rows().map((row) => texts()[row] ?? '')
  const edit = () => researchPlanEdit(queries(), props.plan.questions, answers(), props.plan.maxSubQueries)
  const full = () => rows().length >= props.plan.maxSubQueries
  const locked = () => props.readOnly === true

  const setText = (row: number, value: string) => setTexts((current) => ({ ...current, [row]: value }))
  const remove = (row: number) => setRows((current) => current.filter((candidate) => candidate !== row))
  const add = () => {
    if (full()) return
    const row = nextRow++
    setTexts((current) => ({ ...current, [row]: '' }))
    setRows((current) => [...current, row])
  }
  const setAnswer = (index: number, value: string) =>
    setAnswers((current) => current.map((answer, at) => (at === index ? value : answer)))

  return (
    <section
      class="verevon-research-plan"
      aria-label={i18n.tr('Plan for dyp research', 'Deep research plan')}
      data-readonly={props.readOnly ? '' : undefined}
    >
      <h3 class="verevon-research-plan__title">{i18n.tr('Plan for dyp research', 'Deep research plan')}</h3>
      <ol class="verevon-research-plan__queries">
        <For each={rows()}>
          {(row, index) => (
            <li class="verevon-research-plan__query">
              <input
                type="text"
                aria-label={i18n.tr(`Delspørsmål ${index() + 1}`, `Sub-query ${index() + 1}`)}
                maxlength={RESEARCH_PLAN_MAX_QUERY_CHARS}
                value={texts()[row] ?? ''}
                readonly={locked()}
                onInput={(event) => setText(row, event.currentTarget.value)}
              />
              <Show when={!locked()}>
                <button
                  type="button"
                  class="verevon-research-plan__remove"
                  aria-label={i18n.tr(`Fjern delspørsmål ${index() + 1}`, `Remove sub-query ${index() + 1}`)}
                  onClick={() => remove(row)}
                >
                  {i18n.tr('Fjern', 'Remove')}
                </button>
              </Show>
            </li>
          )}
        </For>
      </ol>
      <Show when={!locked()}>
        <button type="button" class="verevon-research-plan__add" disabled={full()} onClick={add}>
          {full()
            ? i18n.tr(
                `Maks ${props.plan.maxSubQueries} delspørsmål`,
                `At most ${props.plan.maxSubQueries} sub-queries`,
              )
            : i18n.tr('Legg til delspørsmål', 'Add a sub-query')}
        </button>
      </Show>
      <Show when={props.plan.questions.length > 0}>
        <div class="verevon-research-plan__questions">
          <For each={props.plan.questions}>
            {(question, index) => (
              <label class="verevon-research-plan__question">
                <span>{question}</span>
                <input
                  type="text"
                  maxlength={RESEARCH_PLAN_MAX_ANSWER_CHARS}
                  value={answers()[index()] ?? ''}
                  readonly={locked()}
                  onInput={(event) => setAnswer(index(), event.currentTarget.value)}
                />
              </label>
            )}
          </For>
        </div>
      </Show>
      <Show when={!locked()}>
        <div class="verevon-research-plan__actions">
          <button
            type="button"
            class="verevon-research-plan__start"
            disabled={edit().subQueries.length === 0}
            onClick={() => props.onStart(edit())}
          >
            {i18n.tr('Start research', 'Start research')}
          </button>
          <button type="button" class="verevon-research-plan__cancel" onClick={() => props.onCancel()}>
            {i18n.tr('Avbryt', 'Cancel')}
          </button>
        </div>
      </Show>
    </section>
  )
}
