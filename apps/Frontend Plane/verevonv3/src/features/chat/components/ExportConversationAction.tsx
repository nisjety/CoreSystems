import { createSignal, createUniqueId, lazy, Loading, onCleanup, Show } from 'solid-js'
import { exportThread, type ThreadExportFormat, type ThreadExportProfile } from '@/shared/api/chat-client'
import { downloadBytes } from '@/features/chat/lib/chat-export/download'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { isZdrCode, zdrRefusalPresentation, type Translate } from '@/shared/api/zdr-refusals'
import { Download, Play } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { readThreadExport, type ThreadExport } from '../replay/thread-export-schema'
import './ExportConversationAction.css'

// The replay is the heavier half of this menu, so its code loads only when someone opens it.
const ThreadReplayView = lazy(() =>
  import('../replay/ThreadReplayView').then((module) => ({ default: module.ThreadReplayView })),
)

/** Whether this build offers the conversation export (`VITE_THREAD_EXPORT`, read with the strict literal `true`). */
export const threadExportEnabled = () => import.meta.env.VITE_THREAD_EXPORT === 'true'

/**
 * "Export conversation" beside "Fork from here" on the latest answer (EP-12 S12). The menu takes a format and a
 * content choice, neither preselected, and downloads the whole active branch as a file. "Replay conversation" reads
 * the redacted export into the read-only viewer. The control stays out of sight for a temporary chat, a Space
 * thread and a zero-retention caller, and goes away once the deployment answers that exporting is off, as fork does.
 */
export function ExportConversationAction(props: {
  threadId: string
  temporaryChat?: boolean
  spaceScoped?: boolean
}) {
  const i18n = useI18n()
  const panelId = createUniqueId()
  // The zero-retention posture of this caller can change while the page is open, so the menu follows it.
  const [retentionAllowed, setRetentionAllowed] = createSignal(isLocalRetentionAllowed())
  onCleanup(subscribeLocalRetention((allowed) => setRetentionAllowed(allowed)))
  const [off, setOff] = createSignal(false)
  const [open, setOpen] = createSignal(false)
  const [format, setFormat] = createSignal<ThreadExportFormat | null>(null)
  const [profile, setProfile] = createSignal<ThreadExportProfile | null>(null)
  const [busy, setBusy] = createSignal<'download' | 'replay' | null>(null)
  const [refusal, setRefusal] = createSignal<string | null>(null)
  const [replay, setReplay] = createSignal<ThreadExport | null>(null)

  const hidden = () =>
    off() || !props.threadId || Boolean(props.temporaryChat) || Boolean(props.spaceScoped) || !retentionAllowed()

  const refused = (error: unknown) => {
    setRefusal(refusalText(error, i18n.tr))
    if (failureOf(error).code === 'thread_export_disabled') setOff(true)
  }

  async function download() {
    const chosenFormat = format()
    const chosenProfile = profile()
    if (!chosenFormat || !chosenProfile || busy()) return
    setBusy('download')
    setRefusal(null)
    try {
      const file = await exportThread(props.threadId, chosenFormat, chosenProfile)
      downloadBytes(new TextEncoder().encode(file.body), file.fileName, file.mediaType)
    } catch (error) {
      refused(error)
    } finally {
      setBusy(null)
    }
  }

  // The replay always reads the redacted copy: it is the one the viewer is built to show (EP-12 D9).
  async function openReplay() {
    if (busy()) return
    setBusy('replay')
    setRefusal(null)
    try {
      const reading = readThreadExport((await exportThread(props.threadId, 'json', 'redacted')).body)
      if (reading.status === 'ok') {
        setOpen(false)
        setReplay(reading.document)
      } else {
        setRefusal(
          reading.status === 'unsupported'
            ? i18n.tr('Denne eksportversjonen kan ikke vises her.', 'This export version cannot be shown here.')
            : i18n.tr('Eksporten kunne ikke leses.', 'The export could not be read.'),
        )
      }
    } catch (error) {
      refused(error)
    } finally {
      setBusy(null)
    }
  }

  return (
    <Show when={!hidden()}>
      <div class="verevon-chat-export">
        <button
          type="button"
          class="verevon-chat-export__trigger"
          aria-expanded={open() ? 'true' : 'false'}
          aria-controls={open() ? panelId : undefined}
          onClick={() => setOpen(!open())}
        >
          <Download size={14} />
          {i18n.tr('Eksporter samtalen', 'Export conversation')}
        </button>
        <Show when={open()}>
          <div id={panelId} class="verevon-chat-export__panel">
            <fieldset class="verevon-chat-export__choices">
              <legend>{i18n.tr('Format', 'Format')}</legend>
              <label class="verevon-chat-export__option">
                <input
                  type="radio"
                  name={`${panelId}-format`}
                  checked={format() === 'json'}
                  onChange={() => setFormat('json')}
                />
                <span>JSON</span>
              </label>
              <label class="verevon-chat-export__option">
                <input
                  type="radio"
                  name={`${panelId}-format`}
                  checked={format() === 'markdown'}
                  onChange={() => setFormat('markdown')}
                />
                <span>Markdown</span>
              </label>
            </fieldset>
            <fieldset class="verevon-chat-export__choices">
              <legend>{i18n.tr('Innhold', 'Content')}</legend>
              <label class="verevon-chat-export__option">
                <input
                  type="radio"
                  name={`${panelId}-profile`}
                  checked={profile() === 'full'}
                  onChange={() => setProfile('full')}
                />
                <span>{i18n.tr('Fullstendig', 'Full')}</span>
              </label>
              <label class="verevon-chat-export__option">
                <input
                  type="radio"
                  name={`${panelId}-profile`}
                  checked={profile() === 'redacted'}
                  onChange={() => setProfile('redacted')}
                />
                <span>{i18n.tr('Redigert', 'Redacted')}</span>
              </label>
            </fieldset>
            <button
              type="button"
              class="verevon-chat-export__action"
              disabled={!format() || !profile() || busy() !== null}
              onClick={() => void download()}
            >
              {busy() === 'download' ? i18n.tr('Laster ned …', 'Downloading …') : i18n.tr('Last ned', 'Download')}
            </button>
            <button
              type="button"
              class="verevon-chat-export__action"
              disabled={busy() !== null}
              onClick={() => void openReplay()}
            >
              <Play size={14} />
              {busy() === 'replay' ? i18n.tr('Henter …', 'Loading …') : i18n.tr('Spill av samtalen', 'Replay conversation')}
            </button>
            <Show when={refusal()}>
              {(message) => <p class="verevon-chat-export__refusal" role="alert">{message()}</p>}
            </Show>
          </div>
        </Show>
        <Show when={replay()}>
          {(conversation) => (
            <Loading fallback={<span class="verevon-chat-export__loading">{i18n.tr('Åpner gjenspillingen …', 'Opening the replay …')}</span>}>
              <ThreadReplayView conversation={conversation()} onClose={() => setReplay(null)} />
            </Loading>
          )}
        </Show>
      </div>
    </Show>
  )
}

type Failure = { code?: string; feature?: string; reason?: string; message?: string }

/** The parts of a refusal this menu reads. Anything it does not recognise is left out, never guessed. */
function failureOf(error: unknown): Failure {
  if (typeof error !== 'object' || error === null) return {}
  const record = error as { code?: unknown; feature?: unknown; message?: unknown; details?: unknown }
  const details =
    typeof record.details === 'object' && record.details !== null && !Array.isArray(record.details)
      ? (record.details as Record<string, unknown>)
      : {}
  return {
    code: typeof record.code === 'string' ? record.code : undefined,
    feature: typeof record.feature === 'string' ? record.feature : undefined,
    reason: typeof details.reason === 'string' ? details.reason : undefined,
    message: typeof record.message === 'string' ? record.message : undefined,
  }
}

/**
 * What the person is told when an export is refused. A zero-retention refusal and the refusals of a conversation
 * are said in the reader's language; any other refusal keeps the server's own sentence.
 */
function refusalText(error: unknown, tr: Translate): string {
  const failure = failureOf(error)
  if (failure.code && isZdrCode(failure.code)) {
    const presented = zdrRefusalPresentation(failure.code, failure.feature, tr)
    return presented.next ? `${presented.message} ${presented.next}` : presented.message
  }
  if (failure.code === 'thread_not_exportable') {
    if (failure.reason === 'space_thread') {
      return tr('En Space-samtale kan ikke eksporteres herfra.', 'A Space conversation cannot be exported from here.')
    }
    if (failure.reason === 'non_persistent_run') {
      return tr(
        'En kjøring i samtalen ble ikke lagret, så samtalen kan ikke eksporteres i sin helhet.',
        'A run in this conversation was not kept, so it cannot be exported in full.',
      )
    }
    return tr('Samtalen kan ikke eksporteres i sin helhet.', 'This conversation cannot be exported in full.')
  }
  if (failure.code === 'thread_not_found') return tr('Fant ikke samtalen.', 'The conversation was not found.')
  if (failure.code === 'thread_export_disabled') return tr('Eksport er ikke slått på her.', 'Export is not turned on here.')
  if (failure.code && failure.message) return failure.message
  return tr('Samtalen kunne ikke eksporteres. Prøv igjen.', 'The conversation could not be exported. Try again.')
}
