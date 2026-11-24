import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { Lock } from '@/shared/icons'
import { createResource } from '@/shared/lib/create-resource-compat'
import { chatThreadHref } from '@/features/chat/lib/chat-thread-deep-link'
import { createId } from '@/features/chat/components/chat-normalizers'
import { getChatThreadTranscript, streamChat, type ChatErrorEvent } from '@/shared/api/chat-client'
import { hasErrorCode } from '@/shared/api/api-error'
import { getSpacePrivatePanel, openSpacePrivatePanel } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import '../space-private-panel.css'

/**
 * The owner's private assistant for this room (WS17-S6), shown in the room's side pane.
 *
 * It reads the caller's panel pointer and the owner's own thread through the chat transcript route,
 * which enforces the owner. Sharing is the only write, and it never happens here: a chosen turn
 * opens the reviewed share dialog, and a person posts the text to the room as themselves. Nothing
 * in this pane is visible to the room, and the room's memory is not read into it (SD-22 v1).
 *
 * The pane has its own composer (D21e): a turn written here goes over the ordinary private-turn path, the owner's chat
 * stream on the panel's own thread with conversation scope (SD-22 v1), and carries no room. The reply lands in that
 * thread only, so it stays private to the member; the room sees it only if the person shares it. Under Zero Data
 * Retention the pointer is refused first, so there is no thread and no composer. A send carries a client message id, so a
 * retry of a refused send is the same send, and Stop cancels a streaming reply. "Continue in Chat" is a secondary link
 * under the composer, the way to the full chat: the thread carries no title, so a person could not find it by name.
 *
 * `features.private_panel` decides whether the room offers the pane at all; the gateway decides what
 * the pointer and the thread may show. Presentation only.
 */

export interface SpacePrivatePanelProps {
  readonly spaceRef: string
  /** Whether this viewer may post to the room now (not a viewer, posting on, not archived). Read on each render. */
  readonly canShare: () => boolean
  /** The person picked a turn to share. The dialog shows it as an editable draft; nothing is sent here. */
  readonly onShare: (text: string) => void
}

type PanelTurn = { readonly key: string; readonly role: 'user' | 'assistant'; readonly text: string }

/** The owner's turns reduced to who said what. Other roles and empty turns are not shown. */
export function privatePanelTurns(snapshot: { readonly turns?: unknown } | null | undefined): PanelTurn[] {
  const turns = Array.isArray(snapshot?.turns) ? snapshot.turns : []
  const out: PanelTurn[] = []
  turns.forEach((turn, index) => {
    if (!turn || typeof turn !== 'object') return
    const record = turn as Record<string, unknown>
    const role = record.role === 'user' || record.role === 'assistant' ? record.role : null
    const text = typeof record.content === 'string' ? record.content.trim() : ''
    if (role && text) out.push({ key: String(index), role, text })
  })
  return out
}

export function SpacePrivatePanel(props: SpacePrivatePanelProps) {
  const i18n = useI18n()
  const [pointer, { refetch: refetchPointer }] = createResource(
    () => props.spaceRef,
    (spaceRef: string) => getSpacePrivatePanel(spaceRef),
  )
  const [transcript, { refetch: refetchTranscript }] = createResource(
    () => pointer()?.threadId ?? undefined,
    (threadId: string) => getChatThreadTranscript(threadId),
  )
  const turns = createMemo(() => privatePanelTurns(transcript()))
  const [opening, setOpening] = createSignal(false)
  const [openFailure, setOpenFailure] = createSignal<unknown>(null)
  const [draft, setDraft] = createSignal('')
  // The turn on its way: shown at once, until the refetched transcript carries it.
  const [pendingText, setPendingText] = createSignal<string | null>(null)
  const [replyText, setReplyText] = createSignal('')
  const [sendFailure, setSendFailure] = createSignal<unknown>(null)
  let sendController: AbortController | null = null
  // Stop (the person) and unmount (the pane closed) both abort; only a stop settles the pane afterwards.
  let stopRequested = false
  // A refused send keeps its text and its id: sending the same text again reuses the id, so the gateway reads the repeat
  // as the same send (`idempotency_key`) and the private user turn is stored once, not twice.
  let retryable: { readonly content: string; readonly clientMessageId: string } | null = null
  let composerInput: HTMLTextAreaElement | undefined
  onCleanup(() => sendController?.abort())

  function stop(): void {
    if (!sendController) return
    stopRequested = true
    sendController.abort()
  }

  // ZDR: a durable thread is refused for these callers with the typed catalogue text (412), before any
  // other call. The panel then offers nothing to open.
  const zdrRefused = () =>
    hasErrorCode(pointer.error, 'zdr_durable_mutation_forbidden') || hasErrorCode(openFailure(), 'zdr_durable_mutation_forbidden')

  async function open(): Promise<void> {
    if (opening() || zdrRefused()) return
    setOpening(true)
    setOpenFailure(null)
    try {
      await openSpacePrivatePanel(props.spaceRef)
      void refetchPointer()
    } catch (error) {
      setOpenFailure(error)
    } finally {
      setOpening(false)
    }
  }

  /**
   * One private turn on the panel's thread. Conversation scope is forced (Session Core locks the thread to it, and the
   * chat client sends no workspace or memory sources with it). No room id is sent, so nothing reaches the room.
   */
  async function send(threadId: string): Promise<void> {
    const content = draft().trim()
    if (!content || pendingText() !== null || zdrRefused()) return
    const clientMessageId = retryable?.content === content ? retryable.clientMessageId : createId('msg')
    retryable = null
    stopRequested = false
    setSendFailure(null)
    setPendingText(content)
    setReplyText('')
    setDraft('')
    const controller = new AbortController()
    sendController = controller
    let failed: unknown = null
    try {
      await streamChat(
        {
          content,
          threadId,
          sourceScope: 'conversation',
          locale: i18n.locale() === 'en' ? 'en' : 'no',
          clientMessageId,
        },
        {
          onMessage: (event) => {
            if (typeof event.content === 'string') setReplyText((previous) => previous + event.content)
          },
          onError: (event: ChatErrorEvent) => {
            // Only the code is kept: the typed copy is shown, never the server's text.
            failed = { code: event.code }
          },
        },
        controller.signal,
      )
    } catch (error) {
      failed = error
    } finally {
      if (sendController === controller) sendController = null
    }
    // Unmounted: nothing to settle. A stop is not a failure, and the text is not restored: the server may already hold
    // the turn, so the reread transcript shows what was kept.
    if (controller.signal.aborted && !stopRequested) return
    if (failed && !controller.signal.aborted) {
      setSendFailure(failed)
      setDraft(content)
      retryable = { content, clientMessageId }
    }
    // The sent turn and the reply stay on screen until the reread transcript carries them, so they never blink out.
    try {
      await refetchTranscript()
    } catch {
      // The transcript's own error state shows the failure.
    }
    setPendingText(null)
    setReplyText('')
    composerInput?.focus()
  }

  const sendFailureMessage = () => {
    const failure = sendFailure()
    if (!failure) return null
    return translateApiError(failure, i18n.tr, {
      no: 'Meldingen kunne ikke sendes. Teksten er beholdt, så du kan prøve igjen.',
      en: 'The message could not be sent. Your text is kept, so you can try again.',
    })
  }

  const openFailureMessage = () => {
    const failure = openFailure()
    if (!failure) return null
    return translateApiError(failure, i18n.tr, {
      no: 'Den private assistenten kunne ikke åpnes. Ingenting er endret.',
      en: 'The private assistant could not be opened. Nothing was changed.',
    })
  }

  return (
    <section class="verevon-space-private-panel" aria-labelledby="space-private-panel-title">
      <header class="verevon-space-private-panel__intro">
        <h3 id="space-private-panel-title">{i18n.tr('Privat assistent', 'Private assistant')}</h3>
        <p class="verevon-space-private-panel__notice" role="note">
          <Lock size={14} aria-hidden="true" />
          <span>
            {i18n.tr(
              'Bare du kan se denne samtalen. Den vises ikke i rommet, og rommets minne brukes ikke her. Det du deler med rommet, legger du ut selv.',
              'Only you can see this conversation. It is not shown in the room, and the room’s memory is not used here. What you share with the room, you post yourself.',
            )}
          </span>
        </p>
      </header>

      <Show when={!pointer.loading} fallback={
        <p class="verevon-space-private-panel__status" role="status">
          {i18n.tr('Henter den private samtalen …', 'Loading your private conversation …')}
        </p>
      }>
        <Show
          when={!pointer.error}
          fallback={
            <div class="verevon-space-private-panel__actions">
              <p class="verevon-space-private-panel__error" role="alert">
                {translateApiError(pointer.error, i18n.tr, {
                  no: 'Den private samtalen kunne ikke hentes.',
                  en: 'Your private conversation could not be loaded.',
                })}
              </p>
              <Show when={!zdrRefused()}>
                <button type="button" onClick={() => void refetchPointer()}>
                  {i18n.tr('Prøv igjen', 'Try again')}
                </button>
              </Show>
            </div>
          }
        >
          <Show when={pointer()?.threadId} fallback={
            <div class="verevon-space-private-panel__actions">
              <p class="verevon-space-private-panel__status">
                {i18n.tr(
                  'Den private samtalen for dette rommet er ikke åpnet ennå.',
                  'Your private conversation for this room is not open yet.',
                )}
              </p>
              <Show when={!zdrRefused()}>
                <button type="button" disabled={opening()} onClick={() => void open()}>
                  {opening()
                    ? i18n.tr('Åpner …', 'Opening …')
                    : i18n.tr('Åpne privat assistent', 'Open private assistant')}
                </button>
              </Show>
            </div>
          }>
            <Show when={!transcript.error} fallback={
              <div class="verevon-space-private-panel__actions">
                <p class="verevon-space-private-panel__error" role="alert">
                  {translateApiError(transcript.error, i18n.tr, {
                    no: 'Samtalen kunne ikke hentes.',
                    en: 'The conversation could not be loaded.',
                  })}
                </p>
                <button type="button" onClick={() => void refetchTranscript()}>
                  {i18n.tr('Prøv igjen', 'Try again')}
                </button>
              </div>
            }>
              {/* Only the first read shows the loading line; a reread keeps the turns already shown. */}
              <Show when={!transcript.loading || transcript() !== undefined} fallback={
                <p class="verevon-space-private-panel__status" role="status">
                  {i18n.tr('Laster samtalen …', 'Loading the conversation …')}
                </p>
              }>
                <Show when={turns().length > 0 || pendingText() !== null} fallback={
                  <p class="verevon-space-private-panel__status">
                    {i18n.tr(
                      'Ingen meldinger ennå. Skriv til assistenten nedenfor, eller fortsett i Chat; samtalen ligger i Chat-historikken din.',
                      'No messages yet. Write to the assistant below, or continue in Chat; the conversation is kept in your chat history.',
                    )}
                  </p>
                }>
                  <ol class="verevon-space-private-panel__turns" aria-label={i18n.tr('Den private samtalen', 'Your private conversation')}>
                    <For each={turns()}>
                      {(turn) => (
                        <li class="verevon-space-private-panel__turn" data-role={turn.role}>
                          <span class="verevon-space-private-panel__role">
                            {turn.role === 'user' ? i18n.tr('Du', 'You') : i18n.tr('Assistent', 'Assistant')}
                          </span>
                          <p class="verevon-space-private-panel__text">{turn.text}</p>
                          <Show when={props.canShare()}>
                            <button
                              type="button"
                              class="verevon-space-private-panel__share"
                              onClick={() => props.onShare(turn.text)}
                            >
                              {i18n.tr('Del i rommet …', 'Share to room…')}
                            </button>
                          </Show>
                        </li>
                      )}
                    </For>
                    <Show when={pendingText()}>
                      {(text) => (
                        <li class="verevon-space-private-panel__turn" data-role="user" data-pending="true">
                          <span class="verevon-space-private-panel__role">{i18n.tr('Du', 'You')}</span>
                          <p class="verevon-space-private-panel__text">{text()}</p>
                        </li>
                      )}
                    </Show>
                    <Show when={pendingText() !== null}>
                      <li class="verevon-space-private-panel__turn" data-role="assistant" data-pending="true" aria-live="polite">
                        <span class="verevon-space-private-panel__role">{i18n.tr('Assistent', 'Assistant')}</span>
                        <p class="verevon-space-private-panel__text">
                          {replyText() || i18n.tr('Assistenten svarer …', 'The assistant is replying …')}
                        </p>
                      </li>
                    </Show>
                  </ol>
                </Show>
              </Show>
            </Show>
            <Show when={!zdrRefused() ? pointer()?.threadId : undefined}>
              {(threadId) => (
                <form
                  class="verevon-space-private-panel__composer"
                  aria-label={i18n.tr('Skriv til den private assistenten', 'Write to your private assistant')}
                  onSubmit={(event) => {
                    event.preventDefault()
                    void send(threadId())
                  }}
                >
                  <label class="verevon-space-private-panel__composer-label" for="space-private-panel-input">
                    {i18n.tr('Melding til assistenten (bare du ser den)', 'Message to the assistant (only you see it)')}
                  </label>
                  <textarea
                    id="space-private-panel-input"
                    ref={composerInput}
                    rows={3}
                    value={draft()}
                    // Read-only, not disabled, while a reply streams: a disabled box would drop the keyboard focus.
                    readonly={pendingText() !== null}
                    aria-busy={pendingText() !== null ? 'true' : 'false'}
                    onInput={(event) => setDraft(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
                        event.preventDefault()
                        void send(threadId())
                      }
                    }}
                  />
                  <Show when={sendFailureMessage()}>
                    {(message) => (
                      <p class="verevon-space-private-panel__error" role="alert">{message()}</p>
                    )}
                  </Show>
                  <div class="verevon-space-private-panel__composer-actions">
                    <Show when={pendingText() !== null}>
                      <button type="button" class="verevon-space-private-panel__stop" onClick={() => stop()}>
                        {i18n.tr('Stopp', 'Stop')}
                      </button>
                    </Show>
                    <button type="submit" disabled={pendingText() !== null || !draft().trim()}>
                      {pendingText() !== null ? i18n.tr('Sender …', 'Sending …') : i18n.tr('Send', 'Send')}
                    </button>
                  </div>
                </form>
              )}
            </Show>
          </Show>
        </Show>
      </Show>

      {/* Secondary: the composer above is the way to write here; Chat is the way to the full conversation. */}
      <Show when={!pointer.error ? pointer()?.threadId : undefined}>
        {(threadId) => (
          <p class="verevon-space-private-panel__continue">
            <a link href={chatThreadHref(threadId())}>
              {i18n.tr('Fortsett i Chat', 'Continue in Chat')}
            </a>
          </p>
        )}
      </Show>

      <Show when={openFailureMessage()}>
        {(message) => (
          <p class="verevon-space-private-panel__error" role="alert">{message()}</p>
        )}
      </Show>
    </section>
  )
}
