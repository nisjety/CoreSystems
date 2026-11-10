import { createSignal, createUniqueId, onCleanup, Show } from 'solid-js'
import { importThread, type ChatThreadImport } from '@/shared/api/chat-client'
import { THREAD_IMPORT_MAX_BYTES } from '@/shared/actions/chat-history'
import { selectChatThread } from '@/features/chat/lib/chat-thread-history'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { isZdrCode, zdrRefusalPresentation, type Translate } from '@/shared/api/zdr-refusals'
import { Upload } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import './ImportConversationAction.css'

/** Whether this build offers importing a conversation export (`VITE_THREAD_IMPORT`, read with the strict literal `true`). */
export const threadImportEnabled = () => import.meta.env.VITE_THREAD_IMPORT === 'true'

/**
 * "Import conversation" beside the export and fork on the latest answer (EP-12 S15). The person picks a JSON file the
 * export made; it is read whole and sent to become a new conversation of theirs. The counts of what was kept and what
 * was left out stay in view beside a control that opens the new conversation. The control stays out of sight for a
 * temporary chat, a Space thread and a zero-retention caller, and goes away once the deployment answers that importing
 * is off, as the export does. A refusal shows the server's message.
 */
export function ImportConversationAction(props: { temporaryChat?: boolean; spaceScoped?: boolean }) {
  const i18n = useI18n()
  const inputId = createUniqueId()
  // The zero-retention posture of this caller can change while the page is open, so the control follows it.
  const [retentionAllowed, setRetentionAllowed] = createSignal(isLocalRetentionAllowed())
  onCleanup(subscribeLocalRetention((allowed) => setRetentionAllowed(allowed)))
  const [off, setOff] = createSignal(false)
  const [busy, setBusy] = createSignal(false)
  const [refusal, setRefusal] = createSignal<string | null>(null)
  const [made, setMade] = createSignal<ChatThreadImport | null>(null)

  const hidden = () => off() || Boolean(props.temporaryChat) || Boolean(props.spaceScoped) || !retentionAllowed()

  const refused = (error: unknown) => {
    setRefusal(refusalText(error, i18n.tr))
    const code = failureOf(error).code
    if (code === 'thread_import_disabled' || code === 'thread_export_disabled') setOff(true)
  }

  // The file is read and sent as soon as it is picked. The picker is cleared afterwards, so the same file can be picked again.
  async function chooseFile(input: HTMLInputElement) {
    const file = input.files?.[0]
    input.value = ''
    if (!file || busy()) return
    setRefusal(null)
    setMade(null)
    if (file.size > THREAD_IMPORT_MAX_BYTES) {
      setRefusal(tooLargeText(i18n.tr))
      return
    }
    setBusy(true)
    try {
      const text = await file.text()
      if (text.length === 0) {
        setRefusal(i18n.tr('Filen er tom.', 'The file is empty.'))
        return
      }
      if (new TextEncoder().encode(text).length > THREAD_IMPORT_MAX_BYTES) {
        setRefusal(tooLargeText(i18n.tr))
        return
      }
      setMade(await importThread(text))
    } catch (error) {
      refused(error)
    } finally {
      setBusy(false)
    }
  }

  function openMade() {
    const result = made()
    if (result) selectChatThread(result.threadId)
  }

  return (
    <Show when={!hidden()}>
      <div class="verevon-chat-import">
        <input
          id={inputId}
          class="verevon-chat-import__input"
          type="file"
          accept=".json,application/json"
          disabled={busy()}
          onChange={(event) => void chooseFile(event.currentTarget)}
        />
        <label class="verevon-chat-import__trigger" for={inputId}>
          <Upload size={14} />
          {busy() ? i18n.tr('Importerer …', 'Importing …') : i18n.tr('Importer samtale', 'Import conversation')}
        </label>
        <Show when={made()}>
          {(result) => (
            <p class="verevon-chat-import__result" role="status">
              <span>{countsText(result(), i18n.tr)}</span>
              <button type="button" class="verevon-chat-import__open" onClick={openMade}>
                {i18n.tr('Åpne samtalen', 'Open the conversation')}
              </button>
            </p>
          )}
        </Show>
        <Show when={refusal()}>
          {(message) => <p class="verevon-chat-import__refusal" role="alert">{message()}</p>}
        </Show>
      </div>
    </Show>
  )
}

type Failure = { code?: string; feature?: string; status?: number; message?: string }

/** The parts of a refusal this control reads. Anything it does not recognise is left out, never guessed. */
function failureOf(error: unknown): Failure {
  if (typeof error !== 'object' || error === null) return {}
  const record = error as { code?: unknown; feature?: unknown; status?: unknown; message?: unknown }
  return {
    code: typeof record.code === 'string' ? record.code : undefined,
    feature: typeof record.feature === 'string' ? record.feature : undefined,
    status: typeof record.status === 'number' ? record.status : undefined,
    message: typeof record.message === 'string' ? record.message : undefined,
  }
}

/**
 * What the person is told when an import is refused. A zero-retention refusal is said in the reader's language; a
 * file that is too large is said here, since the gateway answers that size without a typed body; any other refusal
 * keeps the server's own sentence.
 */
function refusalText(error: unknown, tr: Translate): string {
  const failure = failureOf(error)
  if (failure.code && isZdrCode(failure.code)) {
    const presented = zdrRefusalPresentation(failure.code, failure.feature, tr)
    return presented.next ? `${presented.message} ${presented.next}` : presented.message
  }
  if (failure.status === 413) return tooLargeText(tr)
  if (failure.code === 'thread_import_disabled' || failure.code === 'thread_export_disabled') {
    return tr('Import er ikke slått på her.', 'Import is not turned on here.')
  }
  if (failure.code && failure.message) return failure.message
  return tr('Samtalen kunne ikke importeres. Prøv igjen.', 'The conversation could not be imported. Try again.')
}

function tooLargeText(tr: Translate): string {
  return tr('Filen er større enn det en import tar.', 'The file is larger than an import takes.')
}

/** The two counts of an import, said in the reader's language: the messages kept and the things left out. */
function countsText(result: ChatThreadImport, tr: Translate): string {
  const kept = result.importedMessages
  const left = result.dropped
  const messages = tr(kept === 1 ? '1 melding' : `${kept} meldinger`, kept === 1 ? '1 message' : `${kept} messages`)
  const things = tr(
    left === 1 ? '1 ting ble ikke tatt med.' : `${left} ting ble ikke tatt med.`,
    left === 1 ? '1 thing was not included.' : `${left} things were not included.`,
  )
  return `${tr('Importerte', 'Imported')} ${messages}. ${things}`
}
