import { createEffect, createSignal, createUniqueId, For, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { ChevronLeft, ChevronRight, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { ThreadExport, ThreadExportMessage, ThreadExportRun } from './thread-export-schema'
import './ThreadReplayView.css'

type Locale = 'no' | 'en'
type Translate = (noText: string, enText: string) => string
type Wording = { no: string; en: string }

// The run statuses the export names, in the reader's words. Any other status is shown as the server wrote it.
const RUN_STATUS: Record<string, Wording> = {
  completed: { no: 'Fullført', en: 'Completed' },
  failed: { no: 'Feilet', en: 'Failed' },
  cancelled: { no: 'Avbrutt', en: 'Cancelled' },
  running: { no: 'Pågår', en: 'Running' },
  queued: { no: 'I kø', en: 'Queued' },
  awaiting_approval: { no: 'Venter på godkjenning', en: 'Awaiting approval' },
  timed_out: { no: 'Tidsavbrudd', en: 'Timed out' },
  paused: { no: 'Pauset', en: 'Paused' },
}

// The origins a run can carry: the closed vocabulary of the run read model (DEC-17).
const RUN_ORIGIN: Record<string, Wording> = {
  chat: { no: 'Chat', en: 'Chat' },
  api: { no: 'API', en: 'API' },
  schedule: { no: 'Tidsplan', en: 'Schedule' },
  trigger: { no: 'Utløst', en: 'Trigger' },
}

function labelOf(table: Record<string, Wording>, key: string, tr: Translate): string {
  const entry = Object.hasOwn(table, key) ? table[key] : undefined
  return entry ? tr(entry.no, entry.en) : key
}

function speakerOf(role: string, tr: Translate): string {
  if (role === 'user') return tr('Du', 'You')
  if (role === 'assistant') return 'Verevon'
  if (role === 'system') return 'System'
  return tr('Melding', 'Message')
}

function stampOf(value: string, locale: Locale): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return new Intl.DateTimeFormat(locale === 'no' ? 'nb-NO' : 'en', { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function tokensOf(run: ThreadExportRun, tr: Translate, locale: Locale): string {
  const count = (value: number) => value.toLocaleString(locale === 'no' ? 'nb-NO' : 'en')
  return `${tr('inn', 'in')} ${count(run.input_tokens)} · ${tr('ut', 'out')} ${count(run.output_tokens)}`
}

/** Each artifact of a turn as `kind: title`, from what the stored artifact carries. */
function artifactLabelsOf(message: ThreadExportMessage): string[] {
  return message.artifacts.flatMap((artifact) => {
    if (typeof artifact !== 'object' || artifact === null) return []
    const { kind, title } = artifact as { kind?: unknown; title?: unknown }
    const parts = [kind, title].filter((part): part is string => typeof part === 'string' && part.trim() !== '')
    return parts.length > 0 ? [parts.join(': ')] : []
  })
}

/**
 * The read-only replay of one redacted export (EP-12 S12, D9): the conversation one turn at a time, with the runs
 * behind it as summary chips. It reads the document it is given and never calls a model. Every turn is plain
 * text, so nothing in the conversation can become markup here. Opened lazily from the export menu.
 */
export function ThreadReplayView(props: { conversation: ThreadExport; onClose: () => void }) {
  const i18n = useI18n()
  const titleId = createUniqueId()
  const [index, setIndex] = createSignal(0)
  let dialog: HTMLElement | undefined

  const locale = (): Locale => i18n.locale()
  const count = () => props.conversation.messages.length
  const current = () => props.conversation.messages[index()]
  const step = (delta: number) => setIndex(Math.min(Math.max(index() + delta, 0), Math.max(count() - 1, 0)))

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(() => undefined, () => {
    dialog?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  })

  return (
    <Portal>
      <div class="verevon-chat-replay-backdrop">
        <section
          ref={(element) => (dialog = element)}
          class="verevon-chat-replay"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex={-1}
        >
          <header class="verevon-chat-replay__header">
            <div>
              <p class="verevon-chat-replay__kicker">{i18n.tr('Gjenspilling, redigert', 'Replay, redacted')}</p>
              <h2 id={titleId}>
                {props.conversation.thread.title || i18n.tr('Samtale uten tittel', 'Untitled conversation')}
              </h2>
            </div>
            <button
              type="button"
              class="verevon-chat-replay__close"
              aria-label={i18n.tr('Lukk', 'Close')}
              onClick={() => props.onClose()}
            >
              <X size={16} />
            </button>
          </header>

          <Show when={props.conversation.runs.length > 0}>
            <ul class="verevon-chat-replay__runs" aria-label={i18n.tr('Kjøringer i samtalen', 'Runs in this conversation')}>
              <For each={props.conversation.runs}>
                {(run) => (
                  <li class="verevon-chat-replay__chip">
                    <span class="verevon-chat-replay__status">{labelOf(RUN_STATUS, run.status, i18n.tr)}</span>
                    <span>{labelOf(RUN_ORIGIN, run.origin, i18n.tr)}</span>
                    <span>{run.model || i18n.tr('ukjent modell', 'unknown model')}</span>
                    <span>{tokensOf(run, i18n.tr, locale())}</span>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <Show
            when={current()}
            fallback={<p class="verevon-chat-replay__empty">{i18n.tr('Samtalen har ingen meldinger.', 'This conversation has no messages.')}</p>}
          >
            {(message) => (
              <article class="verevon-chat-replay__turn">
                <header class="verevon-chat-replay__turn-head">
                  <span class="verevon-chat-replay__speaker">{speakerOf(message().role, i18n.tr)}</span>
                  <time datetime={message().created_at}>{stampOf(message().created_at, locale())}</time>
                </header>
                <Show when={message().content.trim() !== ''}>
                  <p class="verevon-chat-replay__text">{message().content}</p>
                </Show>
                <Show when={artifactLabelsOf(message()).length > 0}>
                  <ul class="verevon-chat-replay__artifacts" aria-label={i18n.tr('Dokumenter i meldingen', 'Documents in this message')}>
                    <For each={artifactLabelsOf(message())}>
                      {(label) => <li class="verevon-chat-replay__chip">{label}</li>}
                    </For>
                  </ul>
                </Show>
              </article>
            )}
          </Show>

          <nav class="verevon-chat-replay__steps" aria-label={i18n.tr('Bla i meldingene', 'Step through the messages')}>
            <button
              type="button"
              aria-label={i18n.tr('Forrige melding', 'Previous message')}
              disabled={index() <= 0}
              onClick={() => step(-1)}
            >
              <ChevronLeft size={16} />
            </button>
            <span class="verevon-chat-replay__position" aria-live="polite">
              {count() === 0 ? '' : i18n.tr(`Melding ${index() + 1} av ${count()}`, `Message ${index() + 1} of ${count()}`)}
            </span>
            <button
              type="button"
              aria-label={i18n.tr('Neste melding', 'Next message')}
              disabled={index() >= count() - 1}
              onClick={() => step(1)}
            >
              <ChevronRight size={16} />
            </button>
          </nav>

          <aside class="verevon-chat-replay__notes" aria-label={i18n.tr('Ikke med i eksporten', 'Not in the export')}>
            <p>
              {i18n.tr(
                'Binære filer, vedleggsinnhold og genererte filer er aldri med.',
                'Binary files, attachment contents and generated files are never included.',
              )}
            </p>
            <p>
              {i18n.tr(
                'Bare den aktive grenen av samtalen er med.',
                'Only the active branch of the conversation is included.',
              )}
            </p>
            <Show when={props.conversation.profile === 'redacted'}>
              <p>
                {i18n.tr(
                  'Verktøyargumenter og -resultater, koblingsdata, siteringsutdrag, URL-spørringer og vedleggsnavn er utelatt.',
                  'Tool arguments and results, connector data, citation snippets, URL query strings and attachment names are left out.',
                )}
              </p>
            </Show>
            <Show when={props.conversation.truncated}>
              <p role="status">
                {i18n.tr(
                  'Eksporten stopper ved størrelsesgrensen, så de siste meldingene mangler.',
                  'The export stops at its size limit, so the last messages are missing.',
                )}
              </p>
            </Show>
          </aside>
        </section>
      </div>
    </Portal>
  )
}
