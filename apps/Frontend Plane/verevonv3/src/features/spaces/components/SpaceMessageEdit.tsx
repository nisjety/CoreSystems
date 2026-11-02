import { createSignal, Show } from 'solid-js'

import { translateApiError, useI18n } from '@/shared/i18n'
import './space-message-lifecycle.css'

/**
 * Inline editor for one message (WS12-S7). Saving calls `onSave` with the trimmed text; a refusal
 * stays in the editor, worded by `translateApiError`, with the typed text kept so nothing is lost.
 * Enter with Ctrl or Cmd saves, Escape cancels.
 */
export function SpaceMessageEdit(props: {
  readonly initial: string
  readonly maxChars: number
  readonly onSave: (content: string) => Promise<void>
  readonly onCancel: () => void
}) {
  const i18n = useI18n()
  const [text, setText] = createSignal(props.initial)
  const [busy, setBusy] = createSignal(false)
  const [failure, setFailure] = createSignal<string | undefined>(undefined)
  const trimmed = () => text().trim()
  const unchanged = () => trimmed() === props.initial.trim()
  const invalid = () => trimmed().length === 0 || trimmed().length > props.maxChars
  const save = async () => {
    if (busy() || invalid()) return
    if (unchanged()) {
      props.onCancel()
      return
    }
    setBusy(true)
    setFailure(undefined)
    try {
      await props.onSave(trimmed())
    } catch (error) {
      setFailure(
        translateApiError(error, i18n.tr, {
          no: 'Meldingen kunne ikke endres. Ingenting er endret. Prøv igjen om litt.',
          en: 'The message could not be changed. Nothing was changed. Try again shortly.',
        }),
      )
      setBusy(false)
    }
  }
  return (
    <form
      class="verevon-message-edit"
      onSubmit={(event) => {
        event.preventDefault()
        void save()
      }}
    >
      <textarea
        class="verevon-message-edit__input"
        rows={3}
        value={text()}
        disabled={busy()}
        aria-label={i18n.tr('Rediger melding', 'Edit message')}
        ref={(element) => queueMicrotask(() => element.focus())}
        onInput={(event) => setText(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault()
            props.onCancel()
          } else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
            event.preventDefault()
            void save()
          }
        }}
      />
      <Show when={failure()}>
        {(message) => (
          <p class="verevon-message-edit__error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <div class="verevon-message-edit__actions">
        <button type="submit" class="verevon-space-primary-action" disabled={busy() || invalid()}>
          {busy() ? i18n.tr('Lagrer ...', 'Saving ...') : i18n.tr('Lagre', 'Save')}
        </button>
        <button type="button" class="verevon-space-secondary-action" disabled={busy()} onClick={() => props.onCancel()}>
          {i18n.tr('Avbryt', 'Cancel')}
        </button>
        <span class="verevon-message-edit__hint">
          {i18n.tr('Ctrl+Enter lagrer, Esc avbryter', 'Ctrl+Enter saves, Esc cancels')}
        </span>
      </div>
    </form>
  )
}
