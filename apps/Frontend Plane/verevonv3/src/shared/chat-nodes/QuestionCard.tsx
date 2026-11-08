import { For, Show, createSignal, onCleanup } from 'solid-js'
import type { ChatQuestionAnswer, ChatQuestionItem, ChatUserQuestionEvent } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

/** The countdown appears only inside this window before the question expires (WS14 S4). */
const COUNTDOWN_WINDOW_MS = 60 * 60 * 1000

type Picked = { optionIds: string[]; text: string }

/** Whether one item carries an answer the gateway will accept: a choice, or words where words are allowed. */
function isAnswered(item: ChatQuestionItem, picked: Picked | undefined): boolean {
  if (!picked) return false
  if (item.kind === 'text') return picked.text.trim().length > 0
  if (item.kind === 'single') return picked.optionIds.length === 1 || (item.allowOther && picked.text.trim().length > 0)
  return picked.optionIds.length > 0 || (item.allowOther && picked.text.trim().length > 0)
}

function minutesAndSeconds(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const minutes = Math.floor(total / 60)
  const seconds = total % 60
  return `${minutes}:${seconds.toString().padStart(2, '0')}`
}

/**
 * A question a run put to the person who started it (WS14 S4), answered from the composer's tray.
 *
 * The answer is not an approval: it goes out as the person's next message, and the run's next turn works
 * from it. Each item is answered once, with its choices or, where the item allows it, the person's own words.
 * Skipping sends nothing but the skip. The card asks for no credentials and says so.
 * A question an MCP server sent names that server above the items and again where the answer goes (EP-4 S6).
 */
export function QuestionCard(props: {
  question: ChatUserQuestionEvent
  pending?: boolean
  error?: string
  /** Epoch milliseconds; injectable so the countdown is testable. */
  now?: () => number
  onAnswer: (answer: ChatQuestionAnswer) => void
  onSkip: () => void
}) {
  const i18n = useI18n()
  const clock = props.now ?? (() => Date.now())
  const [now, setNow] = createSignal(clock())
  const timer = setInterval(() => setNow(clock()), 1000)
  onCleanup(() => clearInterval(timer))

  const [picked, setPicked] = createSignal<Record<string, Picked>>({})
  const current = (itemId: string): Picked => picked()[itemId] ?? { optionIds: [], text: '' }
  const update = (item: ChatQuestionItem, next: Partial<Picked>) => {
    setPicked((all) => ({ ...all, [item.id]: { ...current(item.id), ...next } }))
  }
  // A single choice is one answer: picking an option drops the words typed for it, and typing words drops the
  // option picked, because the gateway counts both as the answer and refuses two.
  // The toggle is computed inside the state update, from the choices held at that moment.
  const choose = (item: ChatQuestionItem, optionId: string) => {
    setPicked((all) => {
      const held = all[item.id] ?? { optionIds: [], text: '' }
      if (item.kind !== 'multi') return { ...all, [item.id]: { optionIds: [optionId], text: '' } }
      const optionIds = held.optionIds.includes(optionId)
        ? held.optionIds.filter((id) => id !== optionId)
        : [...held.optionIds, optionId]
      return { ...all, [item.id]: { ...held, optionIds } }
    })
  }
  const type = (item: ChatQuestionItem, text: string) => {
    update(item, { text, optionIds: item.kind === 'single' && text.trim() ? [] : current(item.id).optionIds })
  }

  const expiresAt = () => (props.question.expiresAt ? Date.parse(props.question.expiresAt) : Number.NaN)
  const expired = () => Number.isFinite(expiresAt()) && expiresAt() <= now()
  const countdown = () => {
    const remaining = expiresAt() - now()
    return Number.isFinite(remaining) && remaining > 0 && remaining <= COUNTDOWN_WINDOW_MS ? remaining : undefined
  }
  const complete = () => props.question.items.every((item) => isAnswered(item, picked()[item.id]))
  const locked = () => props.pending === true || expired()
  // The server that asked, when an MCP server did (EP-4 S6): the card names it, and the answer goes back to it.
  // A model question names nobody, so this is empty and the card reads as it always has.
  const server = () => (props.question.origin === 'mcp_elicitation' ? (props.question.originLabel ?? '') : '')

  const submit = () => {
    if (locked() || !complete()) return
    props.onAnswer({
      answers: props.question.items.map((item) => {
        const answer = current(item.id)
        const words = answer.text.trim()
        if (item.kind === 'text') return { itemId: item.id, optionIds: [], text: words }
        if (item.kind === 'single' && words) return { itemId: item.id, optionIds: [], text: words }
        return { itemId: item.id, optionIds: answer.optionIds, text: item.allowOther ? words : '' }
      }),
    })
  }

  return (
    <section class="verevon-question-card" aria-labelledby="verevon-question-card-title">
      <h3 id="verevon-question-card-title" class="verevon-question-card__title">
        {i18n.tr('Agenten trenger et svar', 'The agent needs an answer')}
      </h3>
      <p class="verevon-question-card__note">
        {i18n.tr(
          'Svar med de valgene som vises. Ikke skriv inn passord, koder eller kortnummer her.',
          'Answer with the choices shown. Do not enter passwords, codes or card numbers here.',
        )}
      </p>
      <Show when={server()}>
        <p class="verevon-question-card__origin">
          {i18n.tr(`Spørsmål fra ${server()}`, `Question from ${server()}`)}
        </p>
      </Show>
      <For each={props.question.items}>
        {(item) => (
          <fieldset class="verevon-question-card__item">
            <legend class="verevon-question-card__prompt">{item.prompt}</legend>
            <Show when={item.kind === 'text'}>
              <textarea
                class="verevon-question-card__text"
                maxlength={2000}
                disabled={locked()}
                aria-label={item.prompt}
                value={current(item.id).text}
                onInput={(event) => update(item, { text: event.currentTarget.value })}
              />
            </Show>
            <Show when={item.kind !== 'text'}>
              <For each={item.options}>
                {(option) => (
                  <label class="verevon-question-card__option">
                    <input
                      type={item.kind === 'multi' ? 'checkbox' : 'radio'}
                      name={`verevon-question-${item.id}`}
                      value={option.id}
                      disabled={locked()}
                      checked={current(item.id).optionIds.includes(option.id)}
                      onChange={() => choose(item, option.id)}
                    />
                    <span>{option.label}</span>
                    <Show when={option.description}>
                      <span class="verevon-question-card__hint">{option.description}</span>
                    </Show>
                  </label>
                )}
              </For>
              <Show when={item.allowOther}>
                <label class="verevon-question-card__option">
                  <span>{i18n.tr('Annet', 'Other')}</span>
                  <input
                    type="text"
                    maxlength={2000}
                    disabled={locked()}
                    aria-label={`${item.prompt} — ${i18n.tr('annet', 'other')}`}
                    value={current(item.id).text}
                    onInput={(event) => type(item, event.currentTarget.value)}
                  />
                </label>
              </Show>
            </Show>
          </fieldset>
        )}
      </For>
      <Show when={server()}>
        <p class="verevon-question-card__notice">
          {i18n.tr(`Svaret sendes til ${server()}`, `The answer goes to ${server()}`)}
        </p>
      </Show>
      <Show when={countdown() !== undefined}>
        <p class="verevon-question-card__countdown" role="timer">
          {i18n.tr(
            `Svar innen ${minutesAndSeconds(countdown() ?? 0)}`,
            `Answer within ${minutesAndSeconds(countdown() ?? 0)}`,
          )}
        </p>
      </Show>
      <Show when={expired()}>
        <p class="verevon-question-card__error" role="status">
          {i18n.tr('Spørsmålet er utløpt, og kan ikke besvares lenger.', 'This question has expired and can no longer be answered.')}
        </p>
      </Show>
      <Show when={props.error}>
        <p class="verevon-question-card__error" role="alert">
          {props.error}
        </p>
      </Show>
      <div class="verevon-question-card__actions">
        <button type="button" disabled={locked()} onClick={() => props.onSkip()}>
          {i18n.tr('Hopp over', 'Skip')}
        </button>
        <button type="button" disabled={locked() || !complete()} onClick={submit}>
          {props.pending ? i18n.tr('Sender …', 'Sending …') : i18n.tr('Send svar', 'Send answer')}
        </button>
      </div>
    </section>
  )
}
