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
import { threadSpinOffAction } from '@/shared/actions/chat-history'
import { ArrowUpRight, MessageSquare, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import './side-chat.css'

/** Only the literal `true` enables the side chat, as for the other chat switches. */
export function isSideChatEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const sideChatEnabled = isSideChatEnabled(import.meta.env.VITE_CHAT_SIDE_CHAT)

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

type SideExchange = {
  question: string
  answer: string
}

export type SideChatPanelProps = {
  /** The open thread's server id. A chat without one shows no trigger, since there is no thread to ask about. */
  threadId: string | null
  /** True while the page hides the side chat: a temporary (zero-retention) thread, or a Space thread. */
  hidden: boolean
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

  // Asks one question of the open thread. A failed question keeps its text, so the person can send it again.
  const ask = async () => {
    const threadId = props.threadId
    const text = question().trim()
    if (!threadId || !text || pending()) return
    const token = generation
    setPending(true)
    setFailed(false)
    try {
      const reply = await askSideChat(threadId, text)
      if (token !== generation) return
      // The gateway's side chat is switched off: the panel goes, with its question.
      if (reply === null) {
        setUnavailable(true)
        return
      }
      setExchanges((current) => [...current, { question: text, answer: reply.answer }])
      setQuestion('')
    } catch {
      if (token === generation) setFailed(true)
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
