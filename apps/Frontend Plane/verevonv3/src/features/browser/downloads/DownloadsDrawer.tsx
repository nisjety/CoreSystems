import { createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Download, FileText, ShieldCheck } from '@/shared/icons'
import type { ActionActor } from '@/shared/actions/types'
import { useI18n } from '@/shared/i18n'
import {
  formatDownloadSize,
  isBrowserDownloadsDrawerEnabled,
  sendDownloadToKnowledge,
  sendRefusalBefore,
  type BrowserSessionDownload,
  type DownloadKind,
  type SendOutcome,
  type SendRefusal,
} from './downloads-model'
import './downloads-drawer.css'

export type DownloadsDrawerProps = {
  /** The browser session the downloads belong to. */
  sessionId: string
  /** What the session reported, read with `readSessionDownloads`. An empty list hides the drawer. */
  downloads: readonly BrowserSessionDownload[]
  /** Zero Data Retention on: the hand-off is off and a line says so. */
  zdr?: boolean
  /** The signed-in person in the active organisation. The owner checks it again. */
  actor: () => ActionActor | undefined
  /** Shows the drawer. Defaults to the build flag `VITE_BROWSER_DOWNLOADS_DRAWER`, which is off. */
  enabled?: boolean
  /** Replaces the send (tests). Defaults to `sendDownloadToKnowledge`. */
  send?: typeof sendDownloadToKnowledge
}

type RowState = { readonly state: 'sending' } | SendOutcome

/**
 * Downloads drawer of a browser session (E3.4-7). Lists the files the session's browser downloaded, with
 * type and size, and sends one at a time to the knowledge base through `browser_download.send_to_knowledge`.
 * Hidden unless the flag is on and the session reports at least one download.
 */
export function DownloadsDrawer(props: DownloadsDrawerProps): JSX.Element {
  const i18n = useI18n()
  const tr = i18n.tr
  const [rows, setRows] = createSignal<Readonly<Record<string, RowState>>>({})
  const enabled = () => props.enabled ?? isBrowserDownloadsDrawerEnabled()
  const zdr = () => props.zdr === true
  const visible = () => enabled() && props.downloads.length > 0

  const setRow = (artifactId: string, state: RowState) => {
    setRows((current) => ({ ...current, [artifactId]: state }))
  }

  const send = async (download: BrowserSessionDownload) => {
    const current = rows()[download.artifactId]
    if (current && (current.state === 'sending' || ('outcome' in current && current.outcome === 'sent'))) return
    setRow(download.artifactId, { state: 'sending' })
    const run = props.send ?? sendDownloadToKnowledge
    const outcome = await run({ sessionId: props.sessionId, download, zdr: zdr(), actor: props.actor() })
    setRow(download.artifactId, outcome)
  }

  const kindLabel = (kind: DownloadKind): string => {
    switch (kind) {
      case 'pdf':
        return 'PDF'
      case 'text':
        return tr('Tekst', 'Text')
      case 'markdown':
        return 'Markdown'
      case 'csv':
        return 'CSV'
      default:
        return tr('Annen filtype', 'Other file type')
    }
  }

  const refusalText = (refusal: SendRefusal): string => {
    switch (refusal) {
      case 'zdr':
        return tr('Ikke tilgjengelig med Zero Data Retention', 'Not available with Zero Data Retention')
      case 'unsupported_type':
        return tr(
          'Bare PDF, tekst, Markdown og CSV kan legges i kunnskapsbasen',
          'Only PDF, text, Markdown and CSV can go to the knowledge base',
        )
      case 'signed_out':
        return tr('Logg inn på nytt for å sende filen', 'Sign in again to send the file')
      case 'not_found':
        return tr('Filen finnes ikke lenger i økten', 'The file is no longer in the session')
      default:
        return tr('Filen ble ikke sendt. Prøv igjen.', 'The file was not sent. Try again.')
    }
  }

  const statusText = (state: RowState | undefined): string => {
    if (!state) return ''
    if ('state' in state) return tr('Sender …', 'Sending …')
    if (state.outcome === 'refused') return refusalText(state.refusal)
    switch (state.indexStatus) {
      case 'indexed':
        return tr('Lagt i kunnskapsbasen', 'Added to the knowledge base')
      case 'failed':
        return tr('Sendt, men indekseringen feilet', 'Sent, but indexing failed')
      case 'skipped':
        return tr('Sendt, ikke indeksert', 'Sent, not indexed')
      default:
        return tr('Sendt, indekseres nå', 'Sent, indexing now')
    }
  }

  const buttonDisabled = (download: BrowserSessionDownload): boolean => {
    if (sendRefusalBefore(download, zdr()) !== undefined) return true
    const state = rows()[download.artifactId]
    if (!state) return false
    if ('state' in state) return true
    return state.outcome === 'sent'
  }

  return (
    <Show when={visible()}>
      <section class="browser-downloads" aria-label={tr('Nedlastinger i økten', 'Downloads in this session')}>
        <header class="browser-downloads__header">
          <Download class="size-4" aria-hidden="true" />
          <span>{tr('Nedlastinger', 'Downloads')}</span>
          <span class="browser-downloads__count">{props.downloads.length}</span>
        </header>
        <Show when={zdr()}>
          <p class="browser-downloads__zdr">
            <ShieldCheck class="size-3" aria-hidden="true" />
            <span>
              {tr(
                'Zero Data Retention: filer fra denne økten kan ikke legges i kunnskapsbasen.',
                'Zero Data Retention: files from this session cannot go to the knowledge base.',
              )}
            </span>
          </p>
        </Show>
        <ul class="browser-downloads__list" aria-live="polite">
          <For each={props.downloads}>
            {(download) => {
              const unsupported = () => sendRefusalBefore(download, false) === 'unsupported_type'
              return (
                <li class="browser-downloads__item" data-kind={download.kind}>
                  <FileText class="size-4" aria-hidden="true" />
                  <span class="browser-downloads__name" title={download.filename}>
                    {download.filename}
                  </span>
                  <span class="browser-downloads__meta">
                    {kindLabel(download.kind)}
                    <Show when={download.sizeBytes !== undefined}>
                      {' · '}
                      {formatDownloadSize(download.sizeBytes, i18n.locale())}
                    </Show>
                  </span>
                  <span class="browser-downloads__status">
                    {unsupported() ? refusalText('unsupported_type') : statusText(rows()[download.artifactId])}
                  </span>
                  <button
                    type="button"
                    class="browser-downloads__button"
                    disabled={buttonDisabled(download)}
                    onClick={() => void send(download)}
                  >
                    {tr('Send til kunnskap', 'Send to knowledge')}
                  </button>
                </li>
              )
            }}
          </For>
        </ul>
      </section>
    </Show>
  )
}
