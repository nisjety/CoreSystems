import { createSignal, For, onCleanup, Show } from 'solid-js'
import { ApiError } from '@/shared/api/http'
import {
  documentKindOf,
  MAX_DOCUMENT_BYTES,
  parseDocument,
  type DocumentKind,
  type DocumentParse,
} from '@/shared/api/parse-client'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { FileText, Loader, RefreshCw } from '@/shared/icons'
import { documentErrorMessage, documentSizeLabel, documentWarningMessage } from '@/features/ingestions/lib/documents'
import { CopyButton, EmptyState } from '@/features/ingestions/components/ingestions-ui'

/** The file the person picked, with the kind its name claims. Quarry decides the kind from the bytes. */
interface PickedDocument {
  file: File
  kind: DocumentKind
}

/**
 * Reads one PDF or DOCX to Markdown through Quarry. The file is sent as its raw bytes and
 * nothing is kept: no artifact, no history, no redaction. Warnings sit above the text, so a
 * scan or a missing page is visible before anyone copies the result. A busy server answers
 * with a wait, and the button counts it down before it can be pressed again.
 */
export function DocumentReadPanel() {
  const i18n = useI18n()
  const tr = i18n.tr
  const [picked, setPicked] = createSignal<PickedDocument | null>(null)
  const [running, setRunning] = createSignal(false)
  const [result, setResult] = createSignal<DocumentParse | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [wait, setWait] = createSignal(0)
  let controller: AbortController | undefined
  let countdown: ReturnType<typeof setInterval> | undefined

  function stopCountdown() {
    if (countdown !== undefined) clearInterval(countdown)
    countdown = undefined
  }

  function startCountdown(seconds: number) {
    stopCountdown()
    let left = seconds
    setWait(left)
    countdown = setInterval(() => {
      left -= 1
      setWait(Math.max(0, left))
      if (left <= 0) stopCountdown()
    }, 1000)
  }

  onCleanup(() => {
    controller?.abort()
    stopCountdown()
  })

  function choose(event: Event) {
    const input = event.currentTarget as HTMLInputElement
    const file = input.files?.[0] ?? null
    setResult(null)
    setError(null)
    if (!file) {
      setPicked(null)
      return
    }
    const kind = documentKindOf(file.name)
    if (!kind) {
      setPicked(null)
      setError(tr('Velg en PDF- eller DOCX-fil.', 'Choose a PDF or DOCX file.'))
      return
    }
    if (file.size > MAX_DOCUMENT_BYTES) {
      setPicked(null)
      setError(tr('Filen er større enn 25 MiB. Velg en mindre fil.', 'The file is larger than 25 MiB. Choose a smaller file.'))
      return
    }
    setPicked({ file, kind })
  }

  async function read() {
    const chosen = picked()
    if (!chosen || running() || wait() > 0) return
    controller?.abort()
    const attempt = new AbortController()
    controller = attempt
    setRunning(true)
    setError(null)
    setResult(null)
    try {
      setResult(await parseDocument(chosen.file, chosen.kind, attempt.signal))
    } catch (caught) {
      if (attempt.signal.aborted) return
      setError(
        documentErrorMessage(caught, tr, {
          no: 'Dokumentet kunne ikke leses.',
          en: 'The document could not be read.',
        }),
      )
      if (caught instanceof ApiError && caught.status === 429) {
        startCountdown(caught.retryAfterSeconds ?? 5)
      }
    } finally {
      if (controller === attempt) setRunning(false)
    }
  }

  function readLabel() {
    if (wait() > 0) return tr(`Prøv igjen om ${wait()} s`, `Try again in ${wait()} s`)
    if (running()) return tr('Leser …', 'Reading …')
    return tr('Les dokument', 'Read document')
  }

  return (
    <section class="verevon-panel ingestions-card ingestions-documents">
      <div class="ingestions-card__header">
        <div>
          <h2>{tr('Les dokument', 'Read document')}</h2>
          <p>
            {tr(
              'Last opp en PDF eller DOCX og få teksten som Markdown. Quarry lagrer ingenting, og fjerner ikke personopplysninger: teksten vises bare for deg.',
              'Upload a PDF or DOCX and get its text as Markdown. Quarry stores nothing and removes no personal data: the text is shown only to you.',
            )}
          </p>
        </div>
      </div>

      <div class="ingestions-doc__pick">
        <label class="ingestions-field">
          {tr('PDF eller DOCX, høyst 25 MiB', 'PDF or DOCX, at most 25 MiB')}
          <input
            class="ingestions-doc__input"
            type="file"
            accept=".pdf,.docx,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            onChange={choose}
          />
        </label>
        <Show when={picked()}>
          {(chosen) => (
            <p class="ingestions-muted">
              {chosen().file.name} · {documentSizeLabel(chosen().file.size)}
            </p>
          )}
        </Show>
        <div class="ingestions-actions-row">
          <Button disabled={!picked() || running() || wait() > 0} onClick={() => void read()}>
            <Show when={running()} fallback={wait() > 0 ? <RefreshCw class="size-4" strokeWidth={1.9} /> : <FileText class="size-4" strokeWidth={1.9} />}>
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
            </Show>
            {readLabel()}
          </Button>
        </div>
      </div>

      <Show when={error()}>
        {(message) => (
          <div class="ingestions-alert" role="alert">
            {message()}
          </div>
        )}
      </Show>

      <Show when={result()}>
        {(doc) => (
          <article class="ingestions-doc__result" aria-label={tr('Lest dokument', 'Read document')}>
            <dl class="ingestions-doc__facts">
              <div>
                <dt>{tr('Type', 'Type')}</dt>
                <dd>{doc().kind === 'pdf' ? 'PDF' : 'DOCX'}</dd>
              </div>
              <div>
                <dt>{tr('Størrelse', 'Size')}</dt>
                <dd>{documentSizeLabel(doc().bytes)}</dd>
              </div>
              <Show when={doc().pages !== undefined}>
                <div>
                  <dt>{tr('Sider', 'Pages')}</dt>
                  <dd>{doc().pages}</dd>
                </div>
              </Show>
            </dl>
            <p class="ingestions-muted">
              {tr(
                'Teksten er flat: overskrifter, lister og tabeller er ikke merket opp.',
                'The text is flat: headings, lists and tables are not marked up.',
              )}
            </p>

            <Show when={doc().warnings.length > 0}>
              <ul class="ingestions-doc__warnings" role="list">
                <For each={doc().warnings}>{(warning) => <li>{documentWarningMessage(warning, tr)}</li>}</For>
              </ul>
            </Show>

            <Show
              when={doc().markdown.trim()}
              fallback={
                <EmptyState
                  icon={<FileText class="size-5" strokeWidth={1.8} />}
                  title={tr('Ingen tekst ble funnet.', 'No text was found.')}
                >
                  {tr('Se meldingene over for årsaken.', 'See the messages above for the reason.')}
                </EmptyState>
              }
            >
              {(markdown) => (
                <>
                  <div class="ingestions-doc__text-head">
                    <span class="ingestions-mono">Markdown</span>
                    <CopyButton text={() => doc().markdown} />
                  </div>
                  <pre class="ingestions-doc__text">{markdown()}</pre>
                </>
              )}
            </Show>
          </article>
        )}
      </Show>
    </section>
  )
}
