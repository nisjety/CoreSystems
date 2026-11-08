import { createMemo, createSignal, For, Show } from 'solid-js'
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  Clock,
  Database,
  ExternalLink,
  Layers,
  Loader,
  Network,
  ScanSearch,
  Sparkles,
} from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import type { DiscoveredPage } from '@/shared/api/knowledge-client'
import type { WebSearchCitation, WebSearchResult } from '@/shared/api/search-client'
import type { ExtractItem } from '@/shared/api/ingestions-client'
import type { AiFormatEntry, AiReportView, ScrapeCacheView, ScrapeView } from '@/features/ingestions/lib/scrape-view'
import { aiCodeMessage, answerStatusMessage } from '@/features/ingestions/lib/errors'
import { shortFingerprint } from '@/features/ingestions/lib/monitoring'
import { formatDuration } from '@/features/ingestions/lib/runs'
import {
  CopyButton,
  DownloadButton,
  EmptyState,
  absoluteTime,
  kindLabel,
} from '@/features/ingestions/components/ingestions-ui'

/** What the last playground submission produced. */
export type PlaygroundResult =
  | { kind: 'scrape'; view: ScrapeView; raw: unknown }
  | { kind: 'extract'; url: string; items: ExtractItem[]; schemaUsed: boolean; raw: unknown }
  | { kind: 'map'; url: string; pages: DiscoveredPage[] }
  | {
      kind: 'search'
      query: string
      results: WebSearchResult[]
      answer: string | null
      citations: WebSearchCitation[]
      answerRequested: boolean
      /** quarry's answer status (ok, no_sources, skipped_zdr, unavailable); absent on older answers. */
      answerStatus?: string | null
      answerReason?: string | null
    }
  | { kind: 'job'; endpoint: 'crawl' | 'batch'; id: string; target: string; acceptedAt: string }
  | { kind: 'agent'; url: string; goal: string; runId: string; threadId: string; sessionId: string }

export type IngestState = 'idle' | 'sending' | 'sent' | 'failed'

export function IngestionsPlaygroundResult(props: {
  result: PlaygroundResult | null
  running: boolean
  error: string | null
  ingestState: IngestState
  onIngest: () => void
  onScrapeUrl: (url: string) => void
  onBatchUrls: (urls: string[]) => void
  onOpenRun: (id: string) => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  return (
    <section class="verevon-panel ingestions-result" aria-live="polite" aria-label={tr('Resultat', 'Result')}>
      <Show
        when={!props.running}
        fallback={
          <div class="ingestions-result__loading">
            <Loader class="size-5 ingestions-spin" strokeWidth={1.9} />
            <span>{tr('Kjører …', 'Running…')}</span>
            <div class="ingestions-skeleton" />
            <div class="ingestions-skeleton ingestions-skeleton--short" />
            <div class="ingestions-skeleton" />
          </div>
        }
      >
        <Show when={props.error}>
          {(message) => (
            <div class="ingestions-alert" role="alert">
              <AlertTriangle class="size-4" strokeWidth={1.9} />
              {message()}
            </div>
          )}
        </Show>
        <Show
          when={props.result}
          fallback={
            <Show when={!props.error}>
              <EmptyState icon={<ScanSearch class="size-5" strokeWidth={1.8} />} title={tr('Resultatet vises her', 'The result shows here')}>
                {tr(
                  'Velg et endepunkt, fyll inn og kjør. Markdown, JSON, lenker og metadata dukker opp her, klare til å kopieres.',
                  'Pick an endpoint, fill in and run. Markdown, JSON, links and metadata show up here, ready to copy.',
                )}
              </EmptyState>
            </Show>
          }
        >
          {(result) => (
            <>
              <Show when={result().kind === 'scrape' ? (result() as Extract<PlaygroundResult, { kind: 'scrape' }>) : null}>
                {(scrape) => <ScrapeResultView view={scrape().view} raw={scrape().raw} ingestState={props.ingestState} onIngest={props.onIngest} onBatchUrls={props.onBatchUrls} />}
              </Show>
              <Show when={result().kind === 'extract' ? (result() as Extract<PlaygroundResult, { kind: 'extract' }>) : null}>
                {(extract) => <ExtractResultView url={extract().url} items={extract().items} schemaUsed={extract().schemaUsed} raw={extract().raw} />}
              </Show>
              <Show when={result().kind === 'map' ? (result() as Extract<PlaygroundResult, { kind: 'map' }>) : null}>
                {(map) => <MapResultView url={map().url} pages={map().pages} onScrapeUrl={props.onScrapeUrl} onBatchUrls={props.onBatchUrls} />}
              </Show>
              <Show when={result().kind === 'search' ? (result() as Extract<PlaygroundResult, { kind: 'search' }>) : null}>
                {(search) => <SearchResultView search={search()} onScrapeUrl={props.onScrapeUrl} onBatchUrls={props.onBatchUrls} />}
              </Show>
              <Show when={result().kind === 'agent' ? (result() as Extract<PlaygroundResult, { kind: 'agent' }>) : null}>
                {(agent) => <AgentReceipt run={agent()} />}
              </Show>
              <Show when={result().kind === 'job' ? (result() as Extract<PlaygroundResult, { kind: 'job' }>) : null}>
                {(job) => (
                  <div class="ingestions-job-receipt">
                    <div class="ingestions-job-receipt__icon">
                      {job().endpoint === 'crawl' ? <Network class="size-5" strokeWidth={1.8} /> : <Layers class="size-5" strokeWidth={1.8} />}
                    </div>
                    <div class="ingestions-job-receipt__copy">
                      <h3>{job().endpoint === 'crawl' ? tr('Crawl startet', 'Crawl started') : tr('Batch startet', 'Batch started')}</h3>
                      <p>
                        {tr(
                          'Jobben kjører varig i Quarry. Fremdrift, feil og logg finner du under Kjøringer, der du også kan sette jobben på pause.',
                          'The job runs durably in Quarry. Progress, errors and log are under Runs, where you can also pause the job.',
                        )}
                      </p>
                      <dl class="ingestions-kv">
                        <dt>{tr('Mål', 'Target')}</dt>
                        <dd class="ingestions-truncate">{job().target}</dd>
                        <Show when={job().id}>
                          <dt>{tr('Jobb-ID', 'Job ID')}</dt>
                          <dd class="ingestions-mono">{job().id}</dd>
                        </Show>
                        <Show when={job().acceptedAt}>
                          <dt>{tr('Mottatt', 'Accepted')}</dt>
                          <dd>{absoluteTime(job().acceptedAt, i18n)}</dd>
                        </Show>
                      </dl>
                    </div>
                    <Button variant="secondary" size="sm" onClick={() => props.onOpenRun(job().id)}>
                      {tr('Følg kjøringen', 'Follow the run')}
                      <ExternalLink class="size-4" strokeWidth={1.9} />
                    </Button>
                  </div>
                )}
              </Show>
            </>
          )}
        </Show>
      </Show>
    </section>
  )
}

function AgentReceipt(props: { run: Extract<PlaygroundResult, { kind: 'agent' }> }) {
  const i18n = useI18n()
  const tr = i18n.tr
  return (
    <div class="ingestions-job-receipt">
      <div class="ingestions-job-receipt__icon">
        <Bot class="size-5" strokeWidth={1.8} />
      </div>
      <div class="ingestions-job-receipt__copy">
        <h3>{tr('Agenten jobber', 'The agent is working')}</h3>
        <p>
          {tr(
            'Agenten kjører i en nettleserøkt og logger hvert steg i kjøringen. Følg fremdriften i kjøringskonsollen.',
            'The agent runs in a browser session and logs each step to the run. Follow its progress in the run console.',
          )}
        </p>
        <dl class="ingestions-kv">
          <dt>{tr('Mål', 'Target')}</dt>
          <dd class="ingestions-truncate">{props.run.url}</dd>
          <dt>{tr('Oppdrag', 'Goal')}</dt>
          <dd>{props.run.goal}</dd>
          <dt>{tr('Kjørings-ID', 'Run ID')}</dt>
          <dd class="ingestions-mono">{props.run.runId}</dd>
          <dt>{tr('Økt-ID', 'Session ID')}</dt>
          <dd class="ingestions-mono">{props.run.sessionId}</dd>
        </dl>
      </div>
      <a class="button button--secondary button--sm" href="/agents/runs" link>
        {tr('Åpne kjøringskonsollen', 'Open the run console')}
        <ExternalLink class="size-4" strokeWidth={1.9} />
      </a>
    </div>
  )
}

function extractStatusLabel(status: string, tr: (no: string, en: string) => string) {
  switch (status) {
    case 'ok':
      return tr('Strukturert', 'Structured')
    case 'fetched':
      return tr('Hentet som Markdown', 'Fetched as Markdown')
    case 'error':
      return tr('Feilet', 'Failed')
    default:
      return status
  }
}

type ScrapeTab =
  | 'markdown'
  | 'json'
  | 'links'
  | 'metadata'
  | 'html'
  | 'screenshot'
  | 'pdf'
  | 'structured'
  | 'summary'
  | 'answer'
  | 'highlights'

function ScrapeResultView(props: {
  view: ScrapeView
  raw: unknown
  ingestState: IngestState
  onIngest: () => void
  onBatchUrls: (urls: string[]) => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [tab, setTab] = createSignal<ScrapeTab>('markdown')
  const json = createMemo(() => JSON.stringify(props.raw, null, 2))
  const html = () => props.view.html ?? props.view.rawHtml
  const tabs = createMemo(() => {
    const list: Array<{ id: ScrapeTab; label: string }> = [
      { id: 'markdown', label: 'Markdown' },
      { id: 'json', label: 'JSON' },
      { id: 'links', label: `${tr('Lenker', 'Links')} (${props.view.links.length})` },
      { id: 'metadata', label: tr('Metadata', 'Metadata') },
    ]
    if (props.view.json !== null) list.push({ id: 'structured', label: tr('Strukturert svar', 'Structured answer') })
    if (props.view.summary) list.push({ id: 'summary', label: tr('Sammendrag', 'Summary') })
    if (props.view.answer) list.push({ id: 'answer', label: tr('Svar', 'Answer') })
    if (props.view.ai?.entries.some((entry) => entry.format === 'highlights')) {
      list.push({ id: 'highlights', label: tr(`Høydepunkter (${props.view.highlights.length})`, `Highlights (${props.view.highlights.length})`) })
    }
    if (html()) list.push({ id: 'html', label: 'HTML' })
    if (props.view.captures?.screenshotInline || props.view.captures?.screenshotUrl) {
      list.push({ id: 'screenshot', label: tr('Skjermbilde', 'Screenshot') })
    }
    if (props.view.captures?.pdfUrl) list.push({ id: 'pdf', label: 'PDF' })
    return list
  })
  const tabText = () => {
    switch (tab()) {
      case 'markdown':
        return props.view.markdown ?? ''
      case 'links':
        return props.view.links.map((link) => link.href).join('\n')
      case 'metadata':
        return JSON.stringify(props.view.metadata, null, 2)
      case 'html':
        return html() ?? ''
      case 'structured':
        return JSON.stringify(props.view.json, null, 2)
      case 'summary':
        return props.view.summary ?? ''
      case 'answer':
        return props.view.answer ?? ''
      case 'highlights':
        return props.view.highlights.map((span) => span.text).join('\n\n')
      default:
        return json()
    }
  }
  const ok = () => props.view.statusCode === null || (props.view.statusCode >= 200 && props.view.statusCode < 400)
  const questionEntry = () => props.view.ai?.entries.find((entry) => entry.format === 'question')

  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <span class={cn('ingestions-status', ok() ? 'ingestions-status--success' : 'ingestions-status--warning')}>
              {props.view.statusCode ?? '—'}
            </span>
            <h3 class="ingestions-truncate">{props.view.title || props.view.finalUrl}</h3>
          </div>
          <a class="ingestions-result__url" href={props.view.finalUrl} target="_blank" rel="noreferrer">
            {props.view.finalUrl}
            <ExternalLink class="size-3" strokeWidth={2} />
          </a>
        </div>
        <Button
          size="sm"
          variant={props.ingestState === 'sent' ? 'soft' : 'secondary'}
          disabled={props.ingestState === 'sending' || props.ingestState === 'sent'}
          onClick={() => props.onIngest()}
        >
          <Show
            when={props.ingestState === 'sending'}
            fallback={props.ingestState === 'sent' ? <CheckCircle2 class="size-4" strokeWidth={1.9} /> : <Database class="size-4" strokeWidth={1.9} />}
          >
            <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
          </Show>
          {props.ingestState === 'sent' ? tr('Lagt i Kunnskap', 'Added to Knowledge') : tr('Send til Kunnskap', 'Send to Knowledge')}
        </Button>
      </header>

      <div class="ingestions-result__facts">
        <Show when={props.view.driver}>
          <span>
            {tr('Driver', 'Driver')}: <strong>{props.view.driver}</strong>
          </span>
        </Show>
        <Show when={props.view.durationMs !== null}>
          <span>
            <Clock class="size-3" strokeWidth={2} /> {formatDuration(props.view.durationMs, tr)}
          </span>
        </Show>
        <Show when={props.view.wordCount !== null}>
          <span>
            {props.view.wordCount} {tr('ord', 'words')}
          </span>
        </Show>
        <Show when={props.view.language}>
          <span>{props.view.language}</span>
        </Show>
        <Show when={props.view.change}>
          <span>
            {tr('Endring', 'Change')}: <strong>{props.view.change}</strong>
          </span>
        </Show>
        <Show when={props.view.fingerprint}>
          <span class="ingestions-mono" title={props.view.fingerprint}>
            {shortFingerprint(props.view.fingerprint)}
          </span>
        </Show>
      </div>

      <Show when={props.ingestState === 'failed'}>
        <div class="ingestions-alert" role="alert">
          {tr('Siden kunne ikke legges i Kunnskap. Prøv igjen om litt.', 'The page could not be added to Knowledge. Try again shortly.')}
        </div>
      </Show>
      <Show when={cacheNote(props.view.cache, tr)}>
        {(note) => (
          <p class="ingestions-muted" role="status">
            {note()}
          </p>
        )}
      </Show>
      <Show when={props.view.warnings.length > 0}>
        <div class="ingestions-warning-box">
          <ul>
            <For each={props.view.warnings}>{(warning) => <li>{warning}</li>}</For>
          </ul>
        </div>
      </Show>
      <Show when={props.view.ai}>{(report) => <AiReportPanel report={report()} />}</Show>

      <div class="ingestions-result__tabs">
        <VerevonSegmented aria-label={tr('Visning', 'View')}>
          <For each={tabs()}>
            {(entry) => (
              <VerevonSegmentedButton selected={tab() === entry.id} onClick={() => setTab(entry.id)}>
                {entry.label}
              </VerevonSegmentedButton>
            )}
          </For>
        </VerevonSegmented>
        <span class="ingestions-pg__spacer" />
        <Show when={tab() !== 'screenshot' && tab() !== 'pdf'}>
          <CopyButton text={tabText} />
          <DownloadButton
            filename={tab() === 'markdown' ? 'scrape.md' : tab() === 'html' ? 'scrape.html' : tab() === 'links' ? 'links.txt' : tab() === 'summary' || tab() === 'answer' || tab() === 'highlights' ? 'scrape.txt' : 'scrape.json'}
            content={tabText}
            type={tab() === 'markdown' ? 'text/markdown' : tab() === 'html' ? 'text/html' : tab() === 'links' || tab() === 'summary' || tab() === 'answer' || tab() === 'highlights' ? 'text/plain' : 'application/json'}
          />
        </Show>
        <Show when={tab() === 'links' && props.view.links.length > 0}>
          <Button size="sm" variant="secondary" onClick={() => props.onBatchUrls(props.view.links.map((link) => link.href))}>
            <Layers class="size-4" strokeWidth={1.9} />
            {tr('Batch alle lenker', 'Batch all links')}
          </Button>
        </Show>
      </div>

      <div class="ingestions-result__viewport">
        <Show when={tab() === 'markdown'}>
          <Show
            when={props.view.markdown}
            fallback={
              <EmptyState title={tr('Ingen Markdown i svaret', 'No Markdown in the response')}>
                {tr('Siden kom tilbake som artefakt eller uten tekst. Se JSON-fanen.', 'The page came back as an artifact or without text. See the JSON tab.')}
              </EmptyState>
            }
          >
            {(markdown) => <pre class="ingestions-prose-pre">{markdown()}</pre>}
          </Show>
        </Show>
        <Show when={tab() === 'json'}>
          <pre class="ingestions-json-pre">{json()}</pre>
        </Show>
        <Show when={tab() === 'metadata'}>
          <pre class="ingestions-json-pre">{JSON.stringify(props.view.metadata, null, 2)}</pre>
        </Show>
        <Show when={tab() === 'html'}>
          <pre class="ingestions-json-pre">{html()}</pre>
        </Show>
        <Show when={tab() === 'structured'}>
          <div class="ingestions-ai-answer">
            <span class="ingestions-status ingestions-status--info">{tr('Ikke verifisert: generert fra sidetekst', 'Not verified: generated from page text')}</span>
            <pre class="ingestions-json-pre">{JSON.stringify(props.view.json, null, 2)}</pre>
          </div>
        </Show>
        <Show when={tab() === 'summary'}>
          <div class="ingestions-ai-answer">
            <span class="ingestions-status ingestions-status--info">{tr('Ikke verifisert: generert fra sidetekst', 'Not verified: generated from page text')}</span>
            <p>{props.view.summary}</p>
          </div>
        </Show>
        <Show when={tab() === 'answer'}>
          <div class="ingestions-ai-answer">
            <span class="ingestions-status ingestions-status--info">{tr('Ikke verifisert: generert fra sidetekst', 'Not verified: generated from page text')}</span>
            <p>{props.view.answer}</p>
            <Show when={questionEntry()?.grounded === false}>
              <small class="ingestions-muted">{tr('Sitatet modellen støtter svaret på er ikke ordrett på siden.', 'The quote the model rests the answer on is not verbatim on the page.')}</small>
            </Show>
          </div>
        </Show>
        <Show when={tab() === 'highlights'}>
          <ol class="ingestions-highlight-list">
            <For each={props.view.highlights} fallback={<li class="ingestions-muted">{tr('Ingen høydepunkter funnet på siden.', 'No highlights found on the page.')}</li>}>
              {(span) => (
                <li>
                  <q>{span.text}</q>
                  <small class="ingestions-mono">
                    {span.start}–{span.end}
                  </small>
                </li>
              )}
            </For>
          </ol>
        </Show>
        <Show when={tab() === 'links'}>
          <ol class="ingestions-link-list">
            <For each={props.view.links} fallback={<li class="ingestions-muted">{tr('Ingen lenker funnet.', 'No links found.')}</li>}>
              {(link) => (
                <li>
                  <a href={link.href} target="_blank" rel="noreferrer" class="ingestions-truncate">
                    {link.href}
                  </a>
                  <Show when={link.text}>{(label) => <small>{label()}</small>}</Show>
                </li>
              )}
            </For>
          </ol>
        </Show>
        <Show when={tab() === 'screenshot' && props.view.captures}>
          {(captures) => (
            <Show
              when={
                captures().screenshotInline
                  ? `data:${captures().screenshotInline?.contentType};base64,${captures().screenshotInline?.dataBase64}`
                  : captures().screenshotUrl
              }
              fallback={<EmptyState title={tr('Ingen skjermbilde i svaret', 'No screenshot in the response')} />}
            >
              {(src) => <img class="ingestions-screenshot" src={src()} alt={tr('Skjermbilde av siden', 'Screenshot of the page')} />}
            </Show>
          )}
        </Show>
        <Show when={tab() === 'pdf' && props.view.captures?.pdfUrl}>
          {(pdf) => (
            <div class="ingestions-pdf-link">
              <a href={pdf()} target="_blank" rel="noreferrer" class="button button--secondary button--sm">
                <ExternalLink class="size-4" strokeWidth={1.9} />
                {tr('Åpne PDF', 'Open PDF')}
              </a>
            </div>
          )}
        </Show>
      </div>

      <Show when={props.view.artifacts.length > 0}>
        <p class="ingestions-pg__note">
          {tr('Lagret som artefakter i Quarry:', 'Stored as artifacts in Quarry:')}{' '}
          <For each={props.view.artifacts}>
            {(artifact, index) => (
              <>
                {index() > 0 ? ', ' : ''}
                <span class="ingestions-mono">{artifact.format}</span>
                {artifact.bytes !== null ? ` (${artifact.bytes} B)` : ''}
              </>
            )}
          </For>
        </p>
      </Show>
    </div>
  )
}

const AI_FORMAT_LABELS: Record<AiFormatEntry['format'], (tr: (no: string, en: string) => string) => string> = {
  json: (tr) => tr('Strukturert svar', 'Structured answer'),
  summary: (tr) => tr('Sammendrag', 'Summary'),
  question: (tr) => tr('Spørsmål', 'Question'),
  highlights: (tr) => tr('Høydepunkter', 'Highlights'),
}

/** What each AI format did on this scrape: its status, the reason in product copy, and what the caller must check. */
/** The note under a scrape that asked for `max_age`: served from a stored result, or why the page was fetched again. */
function cacheNote(cache: ScrapeCacheView, tr: (no: string, en: string) => string): string | null {
  switch (cache.ignoredReason) {
    case 'max_age_ignored_zdr':
      return tr(
        'Lagrede resultater brukes ikke under dataskjerming (ZDR). Siden ble hentet på nytt.',
        'Stored results are not used under zero data retention (ZDR). The page was fetched again.',
      )
    case 'max_age_ignored':
      return tr(
        'Et lagret resultat kan ikke brukes for denne forespørselen, for eksempel med nettlesersteg, skjermbilder eller AI-formater. Siden ble hentet på nytt.',
        'A stored result cannot be used for this request, for example with browser steps, captures or AI formats. The page was fetched again.',
      )
    case 'cache_unavailable':
      return tr(
        'Hurtiglageret er ikke tilgjengelig på Quarry-edge, så siden ble hentet på nytt.',
        'The result cache is not available on Quarry-edge, so the page was fetched again.',
      )
    default:
      break
  }
  if (cache.state !== 'hit') return null
  if (cache.ageMs === null) return tr('Fra lagret resultat.', 'From a stored result.')
  const minutes = Math.max(0, Math.round(cache.ageMs / 60_000))
  return tr(`Fra lagret resultat, ${minutes} min gammelt.`, `From a stored result, ${minutes} min old.`)
}

function AiReportPanel(props: { report: AiReportView }) {
  const i18n = useI18n()
  const tr = i18n.tr
  return (
    <section class="ingestions-ai-report" aria-label={tr('AI-rapport', 'AI report')}>
      <h3>{tr('AI-rapport', 'AI report')}</h3>
      <ul>
        <For each={props.report.entries}>
          {(entry) => (
            <li>
              <div class="ingestions-title-row">
                <span
                  class={cn(
                    'ingestions-status',
                    entry.status === 'ok' ? 'ingestions-status--success' : entry.status === 'skipped' ? 'ingestions-status--neutral' : 'ingestions-status--warning',
                  )}
                >
                  {AI_FORMAT_LABELS[entry.format](tr)}: {entry.status === 'ok' ? tr('ok', 'ok') : entry.status === 'skipped' ? tr('hoppet over', 'skipped') : tr('feilet', 'failed')}
                </span>
                <Show when={entry.status === 'ok' && entry.untrustedSource}>
                  <span class="ingestions-muted">{tr('data fra sidetekst', 'data from page text')}</span>
                </Show>
              </div>
              <Show when={entry.status !== 'ok'}>
                <p>{aiCodeMessage(entry.code, tr, entry.retryAfterSeconds)}</p>
              </Show>
              <Show when={entry.truncatedInput}>
                <p>{tr('Siden var for lang: bare starten ble analysert.', 'The page was too long: only its start was analysed.')}</p>
              </Show>
              <Show when={entry.schemaValid === false}>
                <p>{tr('Svaret bryter skjemaet. Feltene under er der det feiler.', 'The answer breaks the schema. The fields below are where it fails.')}</p>
                <ul class="ingestions-issue-list">
                  <For each={entry.schemaIssues}>{(issue) => <li class="ingestions-mono">{`${issue.path || '/'}: ${issue.code}`}</li>}</For>
                </ul>
              </Show>
              <Show when={entry.format === 'question' && entry.found === false}>
                <p>{tr('Svaret finnes ikke på siden.', 'The page does not answer the question.')}</p>
              </Show>
              <Show when={entry.format === 'question' && entry.grounded === false && entry.found !== false}>
                <p>{tr('Sitatet svaret bygger på er ikke ordrett på siden.', 'The quote the answer rests on is not verbatim on the page.')}</p>
              </Show>
              <Show when={entry.format === 'highlights' && (entry.dropped ?? 0) > 0}>
                <p>{tr(`${entry.dropped} sitater ble fjernet fordi de ikke står på siden.`, `${entry.dropped} quotes were dropped because they are not on the page.`)}</p>
              </Show>
              <Show when={entry.repair && entry.repair !== 'none'}>
                <p class="ingestions-muted">{tr(`Svaret ble reparert (${entry.repair}) før det ble vist.`, `The answer was repaired (${entry.repair}) before it was shown.`)}</p>
              </Show>
            </li>
          )}
        </For>
      </ul>
      <p class="ingestions-muted">
        {tr(
          `Kall til modellen: ${props.report.total.calls} · tokens inn ${props.report.total.inputTokens}, ut ${props.report.total.outputTokens}. Ingen belastning for tokens i denne versjonen.`,
          `Model calls: ${props.report.total.calls} · tokens in ${props.report.total.inputTokens}, out ${props.report.total.outputTokens}. Tokens are not billed in this version.`,
        )}
      </p>
    </section>
  )
}

function ExtractResultView(props: { url: string; items: ExtractItem[]; schemaUsed: boolean; raw: unknown }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [tab, setTab] = createSignal<'data' | 'raw'>('data')
  const data = createMemo(() =>
    props.items.map((item) => ({
      url: item.url,
      status: item.status,
      data: item.data ?? item.structured ?? null,
      markdown: item.markdown ?? null,
      error: item.error ?? null,
    })),
  )
  const text = () => JSON.stringify(tab() === 'data' ? data() : props.raw, null, 2)
  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <Sparkles class="size-4" strokeWidth={1.9} />
            <h3>{props.schemaUsed ? tr('Strukturert uttrekk', 'Structured extraction') : tr('Uttrekk', 'Extraction')}</h3>
          </div>
          <span class="ingestions-result__url">{props.url}</span>
        </div>
      </header>
      <div class="ingestions-result__tabs">
        <VerevonSegmented aria-label={tr('Visning', 'View')}>
          <VerevonSegmentedButton selected={tab() === 'data'} onClick={() => setTab('data')}>
            {tr('Data', 'Data')}
          </VerevonSegmentedButton>
          <VerevonSegmentedButton selected={tab() === 'raw'} onClick={() => setTab('raw')}>
            {tr('Rått svar', 'Raw response')}
          </VerevonSegmentedButton>
        </VerevonSegmented>
        <span class="ingestions-pg__spacer" />
        <CopyButton text={text} />
        <DownloadButton filename="extract.json" content={text} />
      </div>
      <div class="ingestions-result__viewport">
        <Show when={tab() === 'data'} fallback={<pre class="ingestions-json-pre">{text()}</pre>}>
          <ul class="ingestions-extract-list">
            <For each={props.items} fallback={<li class="ingestions-muted">{tr('Quarry ga ingen resultater.', 'Quarry returned no results.')}</li>}>
              {(item) => (
                <li class="ingestions-extract-item">
                  <div class="ingestions-title-row ingestions-title-row--spread">
                    <a href={item.url} target="_blank" rel="noreferrer" class="ingestions-truncate">
                      {item.url}
                    </a>
                    <span class={cn('ingestions-status', item.status === 'error' ? 'ingestions-status--warning' : 'ingestions-status--success')}>
                      {extractStatusLabel(item.status, tr)}
                    </span>
                  </div>
                  <Show when={item.schema_valid === true}>
                    <span class="ingestions-status ingestions-status--success">{tr('Følger skjemaet', 'Follows the schema')}</span>
                  </Show>
                  <Show when={item.schema_valid === false}>
                    <div class="ingestions-alert">
                      {tr('Svaret bryter skjemaet. Feltene under er der det feiler.', 'The answer breaks the schema. The fields below are where it fails.')}
                      <ul class="ingestions-issue-list">
                        <For each={item.schema_issues ?? []}>{(issue) => <li class="ingestions-mono">{issue}</li>}</For>
                      </ul>
                    </div>
                  </Show>
                  <Show when={item.truncated_input}>
                    <p class="ingestions-muted">{tr('Siden var for lang: bare starten ble analysert.', 'The page was too long: only its start was analysed.')}</p>
                  </Show>
                  <Show when={item.error}>
                    {(message) => <div class="ingestions-alert">{/^[A-Z_]+$/.test(message()) ? aiCodeMessage(message(), tr) : message()}</div>}
                  </Show>
                  <Show when={item.data !== null && item.data !== undefined}>
                    <pre class="ingestions-json-pre">{JSON.stringify(item.data, null, 2)}</pre>
                  </Show>
                  <Show when={(item.data === null || item.data === undefined) && item.markdown}>
                    <pre class="ingestions-prose-pre">{item.markdown}</pre>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </div>
  )
}

function MapResultView(props: {
  url: string
  pages: DiscoveredPage[]
  onScrapeUrl: (url: string) => void
  onBatchUrls: (urls: string[]) => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [selected, setSelected] = createSignal<string[]>([])
  const toggle = (url: string) =>
    setSelected((current) => (current.includes(url) ? current.filter((entry) => entry !== url) : [...current, url]))
  const allUrls = () => props.pages.map((page) => page.url).join('\n')
  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <h3>{tr(`${props.pages.length} URL-er funnet`, `${props.pages.length} URLs found`)}</h3>
          </div>
          <span class="ingestions-result__url">{props.url}</span>
        </div>
        <div class="ingestions-actions-row">
          <CopyButton text={allUrls} label={tr('Kopier alle', 'Copy all')} />
          <Button size="sm" variant="primary" disabled={selected().length === 0} onClick={() => props.onBatchUrls(selected())}>
            <Layers class="size-4" strokeWidth={1.9} />
            {tr(`Batch valgte (${selected().length})`, `Batch selected (${selected().length})`)}
          </Button>
        </div>
      </header>
      <div class="ingestions-result__viewport">
        <ul class="ingestions-pick-list">
          <For each={props.pages} fallback={<li class="ingestions-muted">{tr('Ingen URL-er funnet.', 'No URLs found.')}</li>}>
            {(page) => (
              <li>
                <label class="ingestions-pick-list__label">
                  <input type="checkbox" checked={selected().includes(page.url)} onChange={() => toggle(page.url)} />
                  <span class="ingestions-min-width">
                    <strong class="ingestions-truncate">{page.title || page.url}</strong>
                    <small class="ingestions-truncate">{page.url}</small>
                  </span>
                </label>
                <button type="button" class="ingestions-icon-button" onClick={() => props.onScrapeUrl(page.url)}>
                  <ScanSearch class="size-3.5" strokeWidth={1.9} />
                  {kindLabel('scrape', tr)}
                </button>
              </li>
            )}
          </For>
        </ul>
      </div>
    </div>
  )
}

function SearchResultView(props: {
  search: Extract<PlaygroundResult, { kind: 'search' }>
  onScrapeUrl: (url: string) => void
  onBatchUrls: (urls: string[]) => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const urls = () => props.search.results.map((hit) => hit.url)
  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <h3>{tr(`${props.search.results.length} treff`, `${props.search.results.length} results`)}</h3>
          </div>
          <span class="ingestions-result__url">“{props.search.query}”</span>
        </div>
        <Button size="sm" variant="primary" disabled={urls().length === 0} onClick={() => props.onBatchUrls(urls())}>
          <Layers class="size-4" strokeWidth={1.9} />
          {tr('Batch alle treff', 'Batch all hits')}
        </Button>
      </header>
      <Show when={props.search.answerRequested}>
        <Show
          when={props.search.answer}
          fallback={
            <div class="ingestions-warning-box">
              {answerStatusMessage(props.search.answerStatus, props.search.answerReason, tr) ??
                tr(
                  'Quarry ga ikke noe svar. Svar krever en svarleverandør på Quarry-edge; treffene under er likevel gyldige.',
                  'Quarry returned no answer. An answer needs an answer provider on Quarry-edge; the hits below are still valid.',
                )}
            </div>
          }
        >
          {(answer) => (
            <div class="ingestions-answer">
              <p>{answer()}</p>
              <Show when={props.search.citations.length > 0}>
                <ol class="ingestions-citations">
                  <For each={props.search.citations}>
                    {(citation) => (
                      <li>
                        <a href={citation.url} target="_blank" rel="noreferrer">
                          {citation.title || citation.url}
                        </a>
                      </li>
                    )}
                  </For>
                </ol>
              </Show>
            </div>
          )}
        </Show>
      </Show>
      <div class="ingestions-result__viewport">
        <ol class="ingestions-search-list">
          <For each={props.search.results} fallback={<li class="ingestions-muted">{tr('Ingen treff.', 'No results.')}</li>}>
            {(hit) => (
              <li>
                <div class="ingestions-min-width">
                  <a href={hit.url} target="_blank" rel="noreferrer">
                    <strong>{hit.title || hit.url}</strong>
                  </a>
                  <small class="ingestions-truncate">{hit.url}</small>
                  <Show when={hit.snippet}>{(snippet) => <p>{snippet()}</p>}</Show>
                </div>
                <button type="button" class="ingestions-icon-button" onClick={() => props.onScrapeUrl(hit.url)}>
                  <ScanSearch class="size-3.5" strokeWidth={1.9} />
                  {kindLabel('scrape', tr)}
                </button>
              </li>
            )}
          </For>
        </ol>
      </div>
    </div>
  )
}
