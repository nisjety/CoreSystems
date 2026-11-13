/**
 * The side chat (F-t3-069): a read-only question about the open conversation, asked in a drawer beside the transcript.
 * Model Gateway answers from a snapshot of the thread and keeps nothing, so an answer exists only here. It is never
 * written to local storage, the thread history or a draft, and it goes when the drawer closes, when the thread changes,
 * or when the panel is hidden.
 *
 * "Spin off as new chat" opens a new draft chat with the answer's first characters in its composer, through the page's
 * new-chat path. Nothing is sent, so the chat exists on the server only once the person sends it.
 *
 * The page mounts this only with `VITE_CHAT_SIDE_CHAT` on, and passes `hidden` for a temporary (zero-retention) thread
 * and for a Space thread. The panel knows nothing of those rules beyond that flag.
 */

import { createEffect, createSignal, createUniqueId, For, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { askSideChat } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { REFUSAL_TEXT, isZdrRefusalCode } from '@/shared/api/zdr-refusals'
import { threadSpinOffAction } from '@/shared/actions/chat-history'
import { ArrowUpRight, MessageSquare, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { streamFailurePresentation, type StreamFailureDetails } from './chat-normalizers'
import type { ChatTurn } from './chat-types'
import './side-chat.css'

export { isSideChatEnabled, sideChatEnabled } from '../lib/side-chat-switch'

/** The most a question takes, as the `thread.side_chat` action contract bounds it. */
export const SIDE_CHAT_QUESTION_MAX = 2000

/** How much of an answer a spin-off carries into the new draft. */
export const SIDE_CHAT_SPIN_OFF_CHARS = 400

/**
 * The composer text a spin-off opens with: the lead-in, then the answer's first characters. Characters are counted as
 * code points, so a character outside the basic plane is never cut in half.
 */
export function sideChatSpinOffDraft(answer: string, leadIn: string): string {
  return `${leadIn}${Array.from(answer).slice(0, SIDE_CHAT_SPIN_OFF_CHARS).join('')}`
}

/**
 * The connection a turn was written under, by its own id: a ChatGPT subscription or a customer gateway. Null for a platform
 * turn, and for a turn whose connection id is blank.
 */
function turnConnection(turn: ChatTurn): string | null {
  const subscription = turn.subscriptionConnectionId?.trim()
  if (subscription) return `subscription:${subscription}`
  const gateway = turn.modelConnectionId?.trim()
  return gateway ? `gateway:${gateway}` : null
}

/**
 * The model a side question names for an open thread (W4-BYO, DEC-V-R8): the model its connection serves. A thread is written
 * under one connection, so that is the connection of its newest connection turn; the model is that turn's, or else the newest
 * turn on the same connection that names one. A turn on another connection is never read. Null when no turn names a
 * connection (a platform thread), or when none of that connection's turns names a model. The turns are the chat page's own,
 * and they keep their connection across a reload.
 */
export function sideChatConnectionModel(turns: readonly ChatTurn[]): string | null {
  let connection: string | null = null
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    const written = turn ? turnConnection(turn) : null
    if (!turn || written === null) continue
    if (connection === null) connection = written
    if (written !== connection) continue
    const model = turn.model?.trim()
    if (model) return model
  }
  return null
}

/**
 * The typed refusals a side question cannot get past by asking again (DEC-V-R8): the same question is refused the same way.
 * The zero-retention refusals are judged by the catalogue, in {@link isPermanentSideChatRefusal}.
 */
const SIDE_CHAT_PERMANENT_CODES: ReadonlySet<string> = new Set([
  'model_not_entitled',
  'side_chat_model_required',
  'connection_ambiguous',
  'side_chat_thread_retention_refused',
  'scope_not_available_in_space',
])

/**
 * The ChatGPT subscription's refusals a side question cannot get past by asking again: the connection must be renewed, the
 * plan's limit must reset, or the request must change. Model Gateway answers each with its own code and `retryable: false`,
 * and the panel shows the chat stream's words for it, with no retry line. `prompt_too_long` is the gateway's code for a prompt
 * still too long after compaction, and the same transcript is too long again. The retryable codes
 * (`subscription_upstream_unavailable`, `subscription_route_unavailable`) and `rate_limited` are not here: they keep the retry.
 */
const SIDE_CHAT_SUBSCRIPTION_PERMANENT_CODES: ReadonlySet<string> = new Set([
  'subscription_reauth_required',
  'subscription_usage_limit',
  'subscription_bad_request',
  'prompt_too_long',
])

/**
 * The side chat's own words for the two refusals the global table does not word. `connection_ambiguous` and
 * `scope_not_available_in_space` come from other routes too (a chat turn, a Space stream), where "side questions" would be
 * wrong, so they are worded here and not in ERROR_COPY. Each says that no model was sent to. Null for any other code.
 */
function sideChatOwnRefusalWords(code: string, i18n: { tr: (noText: string, enText: string) => string }): string | null {
  switch (code) {
    case 'connection_ambiguous':
      return i18n.tr(
        'Sidespørsmål er ikke tilgjengelig i en samtale som er skrevet under mer enn én tilkobling. Forespørselen ble ikke sendt til en modell.',
        'Side questions are not available in a chat written under more than one connection. The request was not sent to a model.',
      )
    case 'scope_not_available_in_space':
      return i18n.tr(
        'Sidespørsmål er ikke tilgjengelig i en samtale i et delt Space. Forespørselen ble ikke sendt til en modell.',
        'Side questions are not available in a chat in a shared Space. The request was not sent to a model.',
      )
    default:
      return null
  }
}

/**
 * Whether a side question's failure is permanent: a typed refusal that asking again cannot change, so the panel shows its
 * words and offers no retry. Every other failure is transient, and keeps the retry.
 */
export function isPermanentSideChatRefusal(error: unknown): boolean {
  if (!(error instanceof ApiError) || error.code === null) return false
  const { code } = error
  if (SIDE_CHAT_PERMANENT_CODES.has(code) || SIDE_CHAT_SUBSCRIPTION_PERMANENT_CODES.has(code)) return true
  // A zero-retention refusal is permanent when the catalogue says so. The retryable ones (its settings or its inference could
  // not be read right now) are transient, and so is a code this SPA does not know: that one has no catalogue words to show,
  // so the neutral retry line stands in for it.
  return isZdrRefusalCode(code) && !REFUSAL_TEXT[code].retryable
}

/**
 * The words a permanent refusal shows, in the reader's language. A ChatGPT subscription refusal takes the chat stream's own
 * words, with the reset or wait time when the error carries one (`retryAfterSeconds`); without one, the neutral form stands.
 * The side chat's own words come next, for the two codes {@link sideChatOwnRefusalWords} words; every other refusal takes
 * the catalogue's words (`translateApiError`).
 */
export function sideChatRefusalMessage(error: ApiError, i18n: { tr: (noText: string, enText: string) => string }): string {
  const { code } = error
  if (code !== null && SIDE_CHAT_SUBSCRIPTION_PERMANENT_CODES.has(code)) {
    const details: StreamFailureDetails = error.retryAfterSeconds === null ? {} : { retryAfterMs: error.retryAfterSeconds * 1000 }
    return streamFailurePresentation(code, '', i18n.tr, details).message
  }
  const own = code === null ? null : sideChatOwnRefusalWords(code, i18n)
  if (own !== null) return own
  return translateApiError(error, i18n.tr)
}

type SideExchange = {
  question: string
  answer: string
}

export type SideChatPanelProps = {
  /** The open thread's server id. A chat without one shows no trigger, since there is no thread to ask about. */
  threadId: string | null
  /** True while the page hides the side chat: a temporary (zero-retention) thread, or a Space thread. */
  hidden: boolean
  /**
   * The model the open thread's connection serves (W4-BYO, DEC-V-R8), from {@link sideChatConnectionModel}. It goes with the
   * question only on a connection thread; null (a platform thread, or none named) sends the question alone.
   */
  connectionModel: string | null
  /** Opens a new draft chat with this text in its composer. Nothing is sent. */
  onSpinOff: (draft: string) => void
}

export function SideChatPanel(props: SideChatPanelProps) {
  const i18n = useI18n()
  const drawerId = createUniqueId()
  const titleId = createUniqueId()
  const [open, setOpen] = createSignal(false)
  const [question, setQuestion] = createSignal('')
  const [exchanges, setExchanges] = createSignal<readonly SideExchange[]>([])
  const [pending, setPending] = createSignal(false)
  const [failed, setFailed] = createSignal(false)
  // A permanent refusal's words. They take the place of the retry line: nothing is offered to retry.
  const [refusal, setRefusal] = createSignal<string | null>(null)
  // A "not enabled" answer takes the panel away for the rest of this page: the switch is not per thread.
  const [unavailable, setUnavailable] = createSignal(false)
  const shown = () => !props.hidden && !unavailable() && Boolean(props.threadId)
  const canAsk = () => Boolean(props.threadId) && !pending() && question().trim().length > 0
  let trigger: HTMLButtonElement | undefined
  let box: HTMLTextAreaElement | undefined
  // Bumped whenever the drawer's content is dropped, so an answer still on its way lands nowhere.
  let generation = 0

  // Drops everything the drawer holds: the answers, the question and any answer still on its way.
  const dropContent = () => {
    generation += 1
    setPending(false)
    setFailed(false)
    setRefusal(null)
    setQuestion('')
    setExchanges([])
  }

  // Escape and the close button return the focus to the trigger. A change of thread or a hidden panel does not: there
  // is nothing on screen to return to.
  const closeDrawer = (returnFocus: boolean) => {
    dropContent()
    setOpen(false)
    if (returnFocus) trigger?.focus()
  }

  const toggleDrawer = () => {
    if (open()) {
      closeDrawer(true)
      return
    }
    setOpen(true)
    // The drawer mounts with this click, so its question box takes the focus on the next tick.
    setTimeout(() => box?.focus(), 0)
  }

  // The drawer belongs to one thread: a change of thread drops its answers and closes it.
  createEffect(
    () => props.threadId,
    () => closeDrawer(false),
    { defer: true },
  )
  createEffect(
    () => shown(),
    (visible) => {
      if (!visible) closeDrawer(false)
    },
    { defer: true },
  )

  // Asks one question of the open thread. A failed question keeps its text, so the person can send it again: a transient
  // failure keeps the retry, and a permanent refusal says why in its own words instead, with no retry.
  const ask = async () => {
    const threadId = props.threadId
    const text = question().trim()
    if (!threadId || !text || pending()) return
    const token = generation
    setPending(true)
    setFailed(false)
    setRefusal(null)
    try {
      // The thread's connection model goes with the question only on a connection thread: null sends none.
      const reply = await askSideChat(threadId, text, props.connectionModel ?? undefined)
      if (token !== generation) return
      // The gateway's side chat is switched off: the panel goes, with its question.
      if (reply === null) {
        setUnavailable(true)
        return
      }
      setExchanges((current) => [...current, { question: text, answer: reply.answer }])
      setQuestion('')
    } catch (error) {
      if (token !== generation) return
      if (error instanceof ApiError && isPermanentSideChatRefusal(error)) setRefusal(sideChatRefusalMessage(error, i18n))
      else setFailed(true)
    } finally {
      if (token === generation) setPending(false)
    }
  }

  // "Spin off as new chat": the answer opens a new draft chat, with its first characters in the composer.
  const spinOff = (answer: string) => {
    const threadId = props.threadId
    if (!threadId) return
    const draft = sideChatSpinOffDraft(answer, i18n.tr('Følg opp på: ', 'Follow up on: '))
    const request = threadSpinOffAction.safeParse({ type: 'thread.spin_off', threadId, draft })
    if (!request.success) return
    props.onSpinOff(request.data.draft)
    closeDrawer(false)
  }

  const onDrawerKeyDown = (event: KeyboardEvent) => {
    if (event.key !== 'Escape') return
    // The drawer takes its Escape, so the page's own Escape handlers do not also act on it.
    event.stopPropagation()
    closeDrawer(true)
  }

  const onQuestionKeyDown = (event: KeyboardEvent) => {
    // Enter sends and Shift+Enter starts a new line, as in the composer. A key that an IME is composing is left alone.
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
    event.preventDefault()
    void ask()
  }

  return (
    <Show when={shown()}>
      <div class="verevon-side-chat">
        <button
          ref={(element) => {
            trigger = element
          }}
          type="button"
          class="verevon-side-chat__trigger"
          aria-expanded={open() ? 'true' : 'false'}
          aria-controls={open() ? drawerId : undefined}
          onClick={toggleDrawer}
        >
          <MessageSquare size={14} aria-hidden="true" />
          <span>{i18n.tr('Spør ved siden av', 'Ask aside')}</span>
        </button>
      </div>
      <Show when={open()}>
        <Portal>
          <div
            id={drawerId}
            class="verevon-side-chat__drawer"
            role="dialog"
            aria-labelledby={titleId}
            onKeyDown={onDrawerKeyDown}
          >
            <header class="verevon-side-chat__head">
              <h2 id={titleId} class="verevon-side-chat__title">
                {i18n.tr('Spør ved siden av', 'Ask aside')}
              </h2>
              <button
                type="button"
                class="verevon-side-chat__close"
                aria-label={i18n.tr('Lukk', 'Close')}
                onClick={() => closeDrawer(true)}
              >
                <X size={16} aria-hidden="true" />
              </button>
            </header>
            <p class="verevon-side-chat__note">
              {i18n.tr(
                'Spør om denne samtalen. Svarene vises bare her, lagres ikke og forsvinner når du lukker panelet.',
                'Ask about this conversation. Answers show here only, are not saved, and go when you close the panel.',
              )}
            </p>
            <div
              class="verevon-side-chat__body"
              role="log"
              aria-label={i18n.tr('Svar på sidespørsmål', 'Answers to side questions')}
            >
              <For each={exchanges()}>
                {(entry) => (
                  <article class="verevon-side-chat__entry">
                    <p class="verevon-side-chat__question">{entry.question}</p>
                    <p class="verevon-side-chat__answer">{entry.answer}</p>
                    <button type="button" class="verevon-side-chat__chip" onClick={() => spinOff(entry.answer)}>
                      <ArrowUpRight size={14} aria-hidden="true" />
                      <span>{i18n.tr('Fortsett i ny samtale', 'Spin off as new chat')}</span>
                    </button>
                  </article>
                )}
              </For>
              <Show when={pending()}>
                <p class="verevon-side-chat__status">{i18n.tr('Svarer …', 'Answering …')}</p>
              </Show>
              <Show when={refusal() !== null}>
                <p class="verevon-side-chat__error" role="alert">{refusal()}</p>
              </Show>
              <Show when={failed()}>
                <p class="verevon-side-chat__error" role="alert">
                  {i18n.tr('Kunne ikke svare nå. Prøv igjen.', 'Could not answer right now. Try again.')}
                </p>
              </Show>
            </div>
            <form
              class="verevon-side-chat__ask"
              onSubmit={(event) => {
                event.preventDefault()
                void ask()
              }}
            >
              <label class="verevon-side-chat__label">
                {i18n.tr('Spørsmål', 'Question')}
                <textarea
                  ref={(element) => {
                    box = element
                  }}
                  class="verevon-side-chat__input"
                  rows={2}
                  maxlength={SIDE_CHAT_QUESTION_MAX}
                  value={question()}
                  onInput={(event) => setQuestion(event.currentTarget.value)}
                  onKeyDown={onQuestionKeyDown}
                />
              </label>
              <button type="submit" class="verevon-side-chat__send" disabled={!canAsk()}>
                {i18n.tr('Spør', 'Ask')}
              </button>
            </form>
          </div>
        </Portal>
      </Show>
    </Show>
  )
}
