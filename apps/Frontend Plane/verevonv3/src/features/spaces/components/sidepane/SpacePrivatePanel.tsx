import { createMemo, createSignal, For, Show } from 'solid-js'

import { Lock } from '@/shared/icons'
import { createResource } from '@/shared/lib/create-resource-compat'
import { chatThreadHref } from '@/features/chat/lib/chat-thread-deep-link'
import { getChatThreadTranscript } from '@/shared/api/chat-client'
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
 * The pane reads and shares; the conversation itself continues in Chat. Once the thread is open, "Continue in Chat"
 * is the way there: the thread carries no title, so a person could not find it in the chat history by name.
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

      <Show when={!pointer.error ? pointer()?.threadId : undefined}>
        {(threadId) => (
          <p class="verevon-space-private-panel__continue">
            <a link href={chatThreadHref(threadId())}>
              {i18n.tr('Fortsett i Chat', 'Continue in Chat')}
            </a>
          </p>
        )}
      </Show>

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
              <Show when={!transcript.loading} fallback={
                <p class="verevon-space-private-panel__status" role="status">
                  {i18n.tr('Laster samtalen …', 'Loading the conversation …')}
                </p>
              }>
                <Show when={turns().length > 0} fallback={
                  <p class="verevon-space-private-panel__status">
                    {i18n.tr(
                      'Ingen meldinger ennå. Fortsett i Chat for å skrive til assistenten; samtalen ligger i Chat-historikken din.',
                      'No messages yet. Continue in Chat to write to the assistant; the conversation is kept in your chat history.',
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
                  </ol>
                </Show>
              </Show>
            </Show>
          </Show>
        </Show>
      </Show>

      <Show when={openFailureMessage()}>
        {(message) => (
          <p class="verevon-space-private-panel__error" role="alert">{message()}</p>
        )}
      </Show>
    </section>
  )
}
