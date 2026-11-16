import { createSignal, onMount, Show } from 'solid-js'

import { X } from '@/shared/icons'
import { createSpacePost, SPACE_POST_MAX_CHARS_DEFAULT } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import './space-private-panel.css'

/**
 * Share a reviewed reply from the private panel to the room (WS17-S6).
 *
 * The starting text is the person's choice from their private conversation. They read it, may edit
 * it, and post it. The post is a human post as them, through WS01's post route; the agent never
 * writes to the room from the panel. Nothing is sent until they press the button. A retry of the
 * same text reuses its send id, so the room holds the post once; changed text is a new send.
 */

export interface SpaceShareToRoomDialogProps {
  readonly spaceRef: string
  /** The reviewed draft to start from. Editable. */
  readonly initialText: string
  /** The room's post limit (`data.limits.post_max_chars`); the WS01 default when absent. */
  readonly maxChars?: number
  readonly onClose: () => void
  /** Called once the room holds the post (accepted now, or already stored by an earlier attempt). */
  readonly onPosted?: () => void
}

/** A fresh send id for one send: a UUID is 36 characters from the set the gateway accepts. */
function newSendId(): string {
  return crypto.randomUUID()
}

export function SpaceShareToRoomDialog(props: SpaceShareToRoomDialogProps) {
  const i18n = useI18n()
  const limit = () => props.maxChars ?? SPACE_POST_MAX_CHARS_DEFAULT
  const [text, setText] = createSignal(props.initialText)
  const [sending, setSending] = createSignal(false)
  const [failure, setFailure] = createSignal<string | null>(null)
  let sendId = newSendId()
  let attemptedText: string | null = null
  let textarea: HTMLTextAreaElement | undefined

  const trimmed = () => text().trim()
  const overLimit = () => text().length > limit()
  const canSend = () => trimmed().length > 0 && !overLimit() && !sending()

  onMount(() => {
    queueMicrotask(() => textarea?.focus())
  })

  async function post(): Promise<void> {
    if (!canSend()) return
    const content = trimmed()
    if (content !== attemptedText) {
      sendId = newSendId()
      attemptedText = content
    }
    setSending(true)
    setFailure(null)
    try {
      await createSpacePost(props.spaceRef, { content, clientSendId: sendId })
      props.onPosted?.()
      props.onClose()
    } catch (error) {
      setFailure(
        translateApiError(error, i18n.tr, {
          no: 'Innlegget ble ikke lagt ut. Teksten din er beholdt; prøv igjen.',
          en: 'The post was not sent. Your text is kept; try again.',
        }),
      )
    } finally {
      setSending(false)
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault()
      props.onClose()
    }
  }

  return (
    <div
      class="verevon-space-share"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose()
      }}
    >
      <section
        class="verevon-space-share__dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="space-share-title"
        onKeyDown={onKeyDown}
      >
        <header class="verevon-space-share__header">
          <h3 id="space-share-title">{i18n.tr('Del i rommet', 'Share to room')}</h3>
          <button type="button" aria-label={i18n.tr('Lukk', 'Close')} onClick={() => props.onClose()}>
            <X size={16} aria-hidden="true" />
          </button>
        </header>

        <p class="verevon-space-share__notice">
          {i18n.tr(
            'Dette legges ut i rommet som deg selv, slik det står her. Det vises som ditt innlegg, ikke assistentens, og alle i rommet kan lese det.',
            'This is posted to the room as you, exactly as written here. It shows as your post, not the assistant’s, and everyone in the room can read it.',
          )}
        </p>

        <label class="verevon-space-share__label" for="space-share-text">
          {i18n.tr('Teksten du deler', 'The text you share')}
        </label>
        <textarea
          id="space-share-text"
          class="verevon-space-share__text"
          rows={8}
          value={text()}
          ref={(element) => {
            textarea = element
          }}
          aria-describedby="space-share-count"
          aria-invalid={overLimit() ? 'true' : undefined}
          onInput={(event) => setText(event.currentTarget.value)}
        />
        <p id="space-share-count" class="verevon-space-share__count" data-over={overLimit() ? 'true' : undefined}>
          {overLimit()
            ? i18n.tr('For lang for rommet. Forkort teksten før du legger den ut.', 'Too long for the room. Shorten the text before posting.')
            : i18n.tr(
                `${limit() - text().length} tegn igjen`,
                `${limit() - text().length} characters left`,
              )}
        </p>

        <Show when={failure()}>
          {(message) => (
            <p class="verevon-space-share__error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <footer class="verevon-space-share__footer">
          <button type="button" class="verevon-space-share__cancel" onClick={() => props.onClose()}>
            {i18n.tr('Avbryt', 'Cancel')}
          </button>
          <button
            type="button"
            class="verevon-space-share__submit"
            disabled={!canSend()}
            onClick={() => void post()}
          >
            {sending() ? i18n.tr('Legger ut …', 'Posting …') : i18n.tr('Legg ut i rommet', 'Post to room')}
          </button>
        </footer>
      </section>
    </div>
  )
}
