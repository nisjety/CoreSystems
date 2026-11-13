import {
  Download,
  ExternalLink,
  FileText,
  Image as ImageIcon,
  Table2,
} from '@/shared/icons'
import { For, Match, Show, Switch, createEffect, createSignal } from 'solid-js'
import type { ChatTurnAttachment } from './chat-types'
import { sandboxHtmlDocument } from '@/shared/lib/sandbox-html'
import { useI18n } from '@/shared/i18n'
import { mediaTranscriptsEnabled, transcribeToDocument } from '@/shared/api/chat-client'
import { ApiError, hasErrorCode } from '@/shared/api/http'
import { REFUSAL_TEXT, isZdrCode } from '@/shared/api/zdr-refusals'
import { transcriptAudioType } from '@/shared/actions/chat-transcript'

/**
 * Viewer for files the user attached to the current conversation. Object URLs
 * belong to the composer, so the controller supplies a bounded data URL when a
 * turn is submitted. Durable text sources are restored from the thread; an
 * unavailable original binary keeps an honest session-only preview note.
 */
export function ChatAttachmentCanvas(props: {
  attachments: ChatTurnAttachment[]
  selectedId?: string | null
  /**
   * The conversation a transcript of an audio attachment is saved from (W5-090-b). Absent in a temporary or
   * zero-retention chat, where no transcript is offered: a transcript is a durable document.
   */
  transcriptThreadId?: string
}) {
  const i18n = useI18n()
  const [selectedId, setSelectedId] = createSignal<string | null>(props.selectedId ?? null)
  createEffect(
    () => props.selectedId,
    (value) => {
      setSelectedId(value ?? null)
    },
  )
  const selected = () => {
    const requested = selectedId()
    return props.attachments.find((attachment) => attachment.id === requested)
      ?? props.attachments.at(-1)
      ?? null
  }

  // Each attachment's transcript state, kept by id so switching tabs keeps what its last request left.
  const [transcripts, setTranscripts] = createSignal<Record<string, TranscriptOutcome | undefined>>({})
  const setOutcome = (id: string, outcome: TranscriptOutcome) => {
    setTranscripts((current) => ({ ...current, [id]: outcome }))
  }
  const transcribeOffered = (attachment: ChatTurnAttachment): boolean => Boolean(props.transcriptThreadId)
    && mediaTranscriptsEnabled()
    && transcriptAudioType(attachment.type) !== null
    && (attachment.previewUrl ?? '').startsWith('data:')
  const transcribe = async (attachment: ChatTurnAttachment) => {
    const threadId = props.transcriptThreadId
    const phase = transcripts()[attachment.id]?.phase
    if (!threadId || phase === 'working' || phase === 'saved') return
    const file = attachment.previewUrl ? audioBlobFromDataUrl(attachment.previewUrl, attachment.type) : null
    if (!file) {
      setOutcome(attachment.id, { phase: 'failed', failure: 'unavailable' })
      return
    }
    setOutcome(attachment.id, { phase: 'working' })
    try {
      const saved = await transcribeToDocument({
        file,
        title: i18n.tr(`Transkripsjon av ${attachment.name}`, `Transcript of ${attachment.name}`),
        threadId,
      })
      setOutcome(attachment.id, { phase: 'saved', documentId: saved.documentId })
    } catch (error) {
      setOutcome(attachment.id, { phase: 'failed', failure: transcriptFailureOf(error) })
    }
  }

  return (
    <Show
      when={props.attachments.length > 0}
      fallback={(
        <div class="verevon-chat-empty-panel">
          <div>
            <FileText size={20} />
            <h2>{i18n.tr('Ingen vedlegg', 'No attachments')}</h2>
            <p>{i18n.tr(
              'Filer du legger til i samtalen blir tilgjengelige her mens du arbeider.',
              'Files you add to the conversation become available here while you work.',
            )}</p>
          </div>
        </div>
      )}
    >
      <div class="verevon-chat-attachment-workspace">
        <div class="verevon-chat-attachment-workspace__list" role="tablist" aria-label={i18n.tr('Vedlegg i samtalen', 'Attachments in this conversation')}>
          <For each={props.attachments}>
            {(attachment) => {
              const tabId = attachmentTabId(attachment.id)
              const panelId = `${tabId}-panel`
              return (
                <button
                  type="button"
                  role="tab"
                  id={tabId}
                  aria-controls={panelId}
                  aria-selected={selected()?.id === attachment.id ? 'true' : 'false'}
                  class={{
                    'verevon-chat-attachment-workspace__item': true,
                    'verevon-chat-attachment-workspace__item--active': selected()?.id === attachment.id,
                  }}
                  onClick={() => setSelectedId(attachment.id)}
                >
                  <span class="verevon-chat-attachment-workspace__item-icon">
                    <Show when={isCsvAttachment(attachment)} fallback={(
                      <Show when={attachment.type.startsWith('image/')} fallback={<FileText size={14} />}>
                        <ImageIcon size={14} />
                      </Show>
                    )}>
                      <Table2 size={14} />
                    </Show>
                  </span>
                  <span>
                    <strong>{attachment.name}</strong>
                    <small>{formatBytes(attachment.size)}</small>
                  </span>
                </button>
              )
            }}
          </For>
        </div>
        <Show when={selected()}>
          {(attachment) => (
            <AttachmentViewer
              attachment={attachment()}
              panelId={`${attachmentTabId(attachment().id)}-panel`}
              tabId={attachmentTabId(attachment().id)}
              transcribeOffered={transcribeOffered(attachment())}
              transcript={transcripts()[attachment().id]}
              onTranscribe={() => void transcribe(attachment())}
            />
          )}
        </Show>
      </div>
    </Show>
  )
}

function AttachmentViewer(props: {
  attachment: ChatTurnAttachment
  panelId: string
  tabId: string
  /** Whether "Transcribe to document" is offered for this attachment (W5-090-b). */
  transcribeOffered: boolean
  /** What the last transcript request for this attachment left on screen. */
  transcript: TranscriptOutcome | undefined
  onTranscribe: () => void
}) {
  const i18n = useI18n()
  const previewUrl = () => props.attachment.previewUrl
  const isImage = () => props.attachment.type.startsWith('image/')
  const isPdf = () => props.attachment.type === 'application/pdf' || props.attachment.name.toLowerCase().endsWith('.pdf')
  const isHtml = () => props.attachment.type === 'text/html' || /\.html?$/i.test(props.attachment.name)
  const isCsv = () => isCsvAttachment(props.attachment)
  const isText = () => {
    const mime = props.attachment.type.toLowerCase()
    return mime.startsWith('text/')
      || ['application/json', 'application/javascript', 'application/xml'].includes(mime)
      || /\.(csv|css|js|json|md|markdown|txt|ts|tsx|xml)$/i.test(props.attachment.name)
  }
  const textPreview = () => {
    const url = previewUrl()
    return isText() && url ? decodeDataUrl(url) : null
  }
  const csvRows = () => {
    const text = textPreview()
    return isCsv() && text ? parseCsv(text) : null
  }

  return (
    <article
      id={props.panelId}
      class="verevon-chat-attachment-workspace__viewer"
      role="tabpanel"
      aria-labelledby={props.tabId}
      tabindex="-1"
    >
      <header>
        <div>
          <strong>{props.attachment.name}</strong>
          <small>{formatBytes(props.attachment.size)} · {props.attachment.type || 'fil'}</small>
          <small class="verevon-chat-attachment-workspace__provenance">
            {isImage()
              ? i18n.tr('Sendt til modellen', 'Sent to the model')
              : props.attachment.extractedText
                ? i18n.tr('Teksten er inkludert i samtalegrunnlaget', 'Text is included in the conversation context')
                : i18n.tr('Kun forhåndsvisning i denne chatten', 'Preview only in this chat')}
          </small>
        </div>
        <div class="verevon-chat-attachment-workspace__actions">
          <Show when={previewUrl()}>
            {(url) => (
              <>
                <a href={url()} download={props.attachment.name} aria-label={i18n.tr(`Last ned ${props.attachment.name}`, `Download ${props.attachment.name}`)} title={i18n.tr('Last ned', 'Download')}>
                  <Download size={14} />
                </a>
                <a href={url()} target="_blank" rel="noopener noreferrer" aria-label={i18n.tr(`Åpne ${props.attachment.name}`, `Open ${props.attachment.name}`)} title={i18n.tr('Åpne i ny fane', 'Open in a new tab')}>
                  <ExternalLink size={14} />
                </a>
              </>
            )}
          </Show>
          <Show when={props.transcribeOffered && props.transcript?.phase !== 'saved'}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              disabled={props.transcript?.phase === 'working'}
              aria-busy={props.transcript?.phase === 'working' ? 'true' : undefined}
              onClick={() => props.onTranscribe()}
            >
              <FileText size={14} />
              {props.transcript?.phase === 'working'
                ? i18n.tr('Transkriberer…', 'Transcribing…')
                : i18n.tr('Transkriber til dokument', 'Transcribe to document')}
            </button>
          </Show>
        </div>
      </header>
      <Show when={props.transcript}>
        {(outcome) => <TranscriptNote outcome={outcome()} />}
      </Show>
      <Switch>
        <Match when={!previewUrl()}>
          <div class="verevon-chat-attachment-workspace__unavailable">
            <FileText size={18} />
            <p>{i18n.tr('Forhåndsvisning finnes bare i denne nettleserøkten.', 'This preview exists only in the current browser session.')}</p>
          </div>
        </Match>
        <Match when={isImage()}>
          <img class="verevon-chat-attachment-workspace__image" src={previewUrl()} alt={props.attachment.name} />
        </Match>
        <Match when={isPdf()}>
          <iframe class="verevon-chat-attachment-workspace__frame" src={previewUrl()} title={props.attachment.name} />
        </Match>
        <Match when={isHtml()}>
          <iframe
            class="verevon-chat-attachment-workspace__frame"
            srcdoc={sandboxHtmlDocument(textPreview() ?? '')}
            title={props.attachment.name}
            sandbox="allow-scripts"
            referrerpolicy="no-referrer"
          />
        </Match>
        <Match when={csvRows() && csvRows()!.length > 0}>
          <CsvTablePreview rows={csvRows()!} />
        </Match>
        <Match when={isText() && textPreview() != null}>
          <pre class="verevon-chat-attachment-workspace__text">{textPreview()}</pre>
        </Match>
        <Match when={true}>
          <div class="verevon-chat-attachment-workspace__unavailable">
            <FileText size={18} />
            <p>Filen kan lastes ned fra kontrollene over.</p>
          </div>
        </Match>
      </Switch>
    </article>
  )
}

type CsvRow = string[]

/**
 * Bounded, dependency-free CSV preview for the contextual canvas. The source
 * stays a plain data URL and every cell is rendered as text, so a spreadsheet
 * attachment cannot turn into executable markup. Large files remain useful
 * without making a chat tab allocate an unbounded table.
 */
function CsvTablePreview(props: { rows: CsvRow[] }) {
  const [header, ...body] = props.rows
  const columns = Math.max(header?.length ?? 0, ...body.map((row) => row.length), 0)
  const cells = (row: CsvRow) => Array.from({ length: columns }, (_, index) => row[index] ?? '')
  return (
    <div class="verevon-chat-attachment-workspace__table-wrap">
      <table class="verevon-chat-table">
        <caption>{props.rows.length} rader · {columns} kolonner</caption>
        <thead>
          <tr><For each={cells(header ?? [])}>{(cell) => <th scope="col">{cell || '—'}</th>}</For></tr>
        </thead>
        <tbody>
          <For each={body}>
            {(row) => <tr><For each={cells(row)}>{(cell) => <td>{cell}</td>}</For></tr>}
          </For>
        </tbody>
      </table>
    </div>
  )
}

/** Decode only the data URLs materialized by the chat controller. Text is
 * rendered in a <pre>, never interpreted as markup or executable content. */
function decodeDataUrl(url: string): string | null {
  const comma = url.indexOf(',')
  if (!url.startsWith('data:') || comma < 0) return null
  const metadata = url.slice(5, comma)
  const payload = url.slice(comma + 1)
  try {
    if (/;base64/i.test(metadata)) {
      const binary = atob(payload)
      const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0))
      return new TextDecoder().decode(bytes)
    }
    return decodeURIComponent(payload)
  } catch {
    return null
  }
}

function isCsvAttachment(attachment: ChatTurnAttachment): boolean {
  return attachment.type.toLowerCase() === 'text/csv' || /\.csv$/i.test(attachment.name)
}

function attachmentTabId(id: string): string {
  // Attachment ids are generated client-side or returned by the gateway. Keep
  // arbitrary provider ids out of CSS/DOM syntax while preserving a stable
  // relationship between each tab and its panel.
  return `verevon-chat-attachment-tab-${id.replace(/[^a-zA-Z0-9_-]/g, '-')}`
}

/** Parse RFC-4180-style CSV, retaining newlines inside quoted fields. */
function parseCsv(source: string, maxRows = 200, maxColumns = 24): CsvRow[] {
  const rows: CsvRow[] = []
  const pushRow = (candidate: CsvRow) => {
    if (candidate.some((cell) => cell.length > 0)) rows.push(candidate.slice(0, maxColumns))
  }
  let row: string[] = []
  let field = ''
  let quoted = false
  for (let index = 0; index < source.length && rows.length < maxRows; index += 1) {
    const character = source[index]
    if (quoted) {
      if (character === '"') {
        if (source[index + 1] === '"') {
          field += '"'
          index += 1
        } else {
          quoted = false
        }
      } else {
        field += character
      }
      continue
    }
    if (character === '"' && field.length === 0) {
      quoted = true
    } else if (character === ',') {
      row.push(field)
      field = ''
    } else if (character === '\n' || character === '\r') {
      if (character === '\r' && source[index + 1] === '\n') index += 1
      row.push(field)
      pushRow(row)
      row = []
      field = ''
    } else {
      field += character
    }
  }
  if (rows.length < maxRows && (field.length > 0 || row.length > 0)) {
    row.push(field)
    pushRow(row)
  }
  return rows
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Why a transcript was not saved (W5-090-b). The reader sees the words for the reason, never the server's own text. */
type TranscriptFailure = 'too_large' | 'unsupported' | 'unreadable' | 'empty' | 'zdr' | 'decision_unavailable' | 'unavailable'

/** Where a transcript request for one attachment stands on screen (W5-090-b). */
type TranscriptOutcome =
  | { phase: 'working' }
  | { phase: 'saved'; documentId: string }
  | { phase: 'failed'; failure: TranscriptFailure }

const TRANSCRIPT_FAILURE_COPY: Record<TranscriptFailure, { no: string; en: string }> = {
  too_large: { no: 'Opptaket er for stort. Grensen er 1,4 MB.', en: 'The recording is too large. The limit is 1.4 MB.' },
  unsupported: { no: 'Dette lydformatet kan ikke transkriberes.', en: 'This audio format cannot be transcribed.' },
  unreadable: { no: 'Opptaket kunne ikke transkriberes.', en: 'The recording could not be transcribed.' },
  empty: { no: 'Ingen tale ble gjenkjent i opptaket.', en: 'No speech was recognised in this recording.' },
  zdr: {
    no: 'Zero Data Retention gjelder, så opptaket kan ikke lagres som dokument.',
    en: 'Zero Data Retention applies, so this recording cannot be saved as a document.',
  },
  // The retryable zero-retention outage: the organisation's settings could not be read, so the recording was not sent. Its words
  // are the catalogue's, the same ones the chat stream shows for this code.
  decision_unavailable: {
    no: REFUSAL_TEXT.zdr_decision_unavailable.no,
    en: REFUSAL_TEXT.zdr_decision_unavailable.en,
  },
  unavailable: { no: 'Transkripsjonen kunne ikke lagres. Prøv igjen.', en: 'The transcript could not be saved. Try again.' },
}

function transcriptFailureText(failure: TranscriptFailure, tr: (noText: string, enText: string) => string): string {
  const copy = TRANSCRIPT_FAILURE_COPY[failure]
  return tr(copy.no, copy.en)
}

/** The reason a transcript request failed, read from the typed codes the client and the gateway send. */
function transcriptFailureOf(error: unknown): TranscriptFailure {
  if (hasErrorCode(error, 'transcript_recording_too_large')) return 'too_large'
  if (hasErrorCode(error, 'transcript_type_unsupported', 'unsupported_audio_type')) return 'unsupported'
  if (hasErrorCode(error, 'transcription_failed')) return 'unreadable'
  if (hasErrorCode(error, 'transcript_empty')) return 'empty'
  // A retryable zero-retention answer is an outage, not a refusal of the recording: the route refuses it before anything is sent.
  if (hasErrorCode(error, 'zdr_decision_unavailable')) return 'decision_unavailable'
  if (error instanceof ApiError && isZdrCode(error.code)) return 'zdr'
  return 'unavailable'
}

/** The note under an audio attachment's header: the saved transcript's link, or why it was not saved (W5-090-b). */
function TranscriptNote(props: { outcome: TranscriptOutcome }) {
  const i18n = useI18n()
  const savedId = () => (props.outcome.phase === 'saved' ? props.outcome.documentId : null)
  const failure = () => (props.outcome.phase === 'failed' ? props.outcome.failure : null)
  return (
    <>
      <Show when={savedId()}>
        {(documentId) => (
          <p class="verevon-chat-feedback-reasons__note" role="status">
            {i18n.tr(
              'Transkripsjonen er lagret som dokument. Den blir værende hvis samtalen slettes. Lydopptaket lagres ikke.',
              'The transcript is saved as a document. It stays if the chat is deleted. The recording is not kept.',
            )}
            {' '}
            <a href={`/knowledge?source=${encodeURIComponent(documentId())}`}>
              {i18n.tr('Åpne dokumentet', 'Open the document')}
            </a>
          </p>
        )}
      </Show>
      <Show when={failure()}>
        {(kind) => (
          <p class="verevon-chat-feedback-reasons__note" role="alert">
            {transcriptFailureText(kind(), (noText, enText) => i18n.tr(noText, enText))}
          </p>
        )}
      </Show>
    </>
  )
}

/**
 * The bytes of a base64 data URL the chat controller materialized, as a Blob of the attachment's own audio type (already
 * checked by the caller). Null for any other URL.
 */
function audioBlobFromDataUrl(url: string, type: string): Blob | null {
  const comma = url.indexOf(',')
  if (!url.startsWith('data:') || comma < 0) return null
  if (!/;base64/i.test(url.slice(5, comma))) return null
  try {
    const binary = atob(url.slice(comma + 1))
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0))
    return new Blob([bytes], { type })
  } catch {
    return null
  }
}
