import {
  Download,
  ExternalLink,
  FileText,
  Image as ImageIcon,
  Table2,
} from '@/shared/icons'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createSignal,
} from 'solid-js'
import type { ChatTurnAttachment } from './chat-types'
import { sandboxHtmlDocument } from '@/shared/lib/sandbox-html'
import { useI18n } from '@/shared/i18n'

/**
 * Viewer for files the user attached to the current conversation. Object URLs
 * belong to the composer, so the controller supplies a bounded data URL when a
 * turn is submitted. Persisted transcripts retain only metadata and therefore
 * render an honest "available in this session" note instead of a broken frame.
 */
export function ChatAttachmentCanvas(props: {
  attachments: ChatTurnAttachment[]
  selectedId?: string | null
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
            />
          )}
        </Show>
      </div>
    </Show>
  )
}

function AttachmentViewer(props: { attachment: ChatTurnAttachment; panelId: string; tabId: string }) {
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
            {isImage() ? 'Sendt til modellen' : 'Kun forhåndsvisning i denne chatten'}
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
        </div>
      </header>
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
