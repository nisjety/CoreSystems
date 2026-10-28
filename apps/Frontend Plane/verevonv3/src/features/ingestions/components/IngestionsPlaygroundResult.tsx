import { createMemo, createSignal, For, Show } from 'solid-js'
import {
  AlertTriangle,
  ArrowUpRight,
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
import type { WebSearchResult } from '@/shared/api/search-client'
import type { ScrapeView } from '@/features/ingestions/lib/scrape-view'
import { shortFingerprint } from '@/features/ingestions/lib/monitoring'
import { formatDuration } from '@/features/ingestions/lib/runs'
import { CopyButton, DownloadButton, EmptyState, NeedsBackend, absoluteTime, kindLabel } from '@/features/ingestions/components/ingestions-ui'

/** What the last playground submission produced. */
export type PlaygroundResult =
  | { kind: 'scrape'; view: ScrapeView; raw: unknown }
  | { kind: 'extract'; url: string; result: unknown; raw: unknown }
  | { kind: 'map'; url: string; pages: DiscoveredPage[] }
  | { kind: 'search'; query: string; results: WebSearchResult[] }
  | { kind: 'job'; endpoint: 'crawl' | 'batch'; id: string; target: string; acceptedAt: string }

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
            <span>{tr('Henter …', 'Fetching…')}</span>
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
                  'Velg et endepunkt, lim inn en URL og kjør. Markdown, JSON, lenker og metadata dukker opp her, klare til å kopieres.',
                  'Pick an endpoint, paste a URL and run. Markdown, JSON, links and metadata show up here, ready to copy.',
                )}
              </EmptyState>
            </Show>
          }
        >
          {(result) => (
            <>
              <Show when={result().kind === 'scrape' ? (result() as Extract<PlaygroundResult, { kind: 'scrape' }>) : null}>
                {(scrape) => (
                  <ScrapeResultView
                    view={scrape().view}
                    raw={scrape().raw}
                    ingestState={props.ingestState}
                    onIngest={props.onIngest}
                  />
                )}
              </Show>
              <Show when={result().kind === 'extract' ? (result() as Extract<PlaygroundResult, { kind: 'extract' }>) : null}>
                {(extract) => <ExtractResultView url={extract().url} result={extract().result} raw={extract().raw} />}
              </Show>
              <Show when={result().kind === 'map' ? (result() as Extract<PlaygroundResult, { kind: 'map' }>) : null}>
                {(map) => (
                  <MapResultView url={map().url} pages={map().pages} onScrapeUrl={props.onScrapeUrl} onBatchUrls={props.onBatchUrls} />
                )}
              </Show>
              <Show when={result().kind === 'search' ? (result() as Extract<PlaygroundResult, { kind: 'search' }>) : null}>
                {(search) => <SearchResultView query={search().query} results={search().results} onScrapeUrl={props.onScrapeUrl} />}
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
                          'Jobben kjører varig i Quarry. Fremdrift, logg og utdata finner du under Kjøringer.',
                          'The job runs durably in Quarry. Progress, log and output are under Runs.',
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
                      <ArrowUpRight class="size-4" strokeWidth={1.9} />
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

type ScrapeTab = 'markdown' | 'json' | 'links' | 'metadata' | 'html' | 'screenshot'

function ScrapeResultView(props: { view: ScrapeView; raw: unknown; ingestState: IngestState; onIngest: () => void }) {
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
    if (html()) list.push({ id: 'html', label: 'HTML' })
    if (props.view.captures) list.push({ id: 'screenshot', label: tr('Skjermbilde', 'Screenshot') })
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
      default:
        return json()
    }
  }
  const ok = () => props.view.statusCode === null || (props.view.statusCode >= 200 && props.view.statusCode < 400)

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
          <span>{tr('Driver', 'Driver')}: <strong>{props.view.driver}</strong></span>
        </Show>
        <Show when={props.view.durationMs !== null}>
          <span><Clock class="size-3" strokeWidth={2} /> {formatDuration(props.view.durationMs, tr)}</span>
        </Show>
        <Show when={props.view.wordCount !== null}>
          <span>{props.view.wordCount} {tr('ord', 'words')}</span>
        </Show>
        <Show when={props.view.language}>
          <span>{props.view.language}</span>
        </Show>
        <Show when={props.view.change}>
          <span>{tr('Endring', 'Change')}: <strong>{props.view.change}</strong></span>
        </Show>
        <Show when={props.view.fingerprint}>
          <span class="ingestions-mono" title={props.view.fingerprint}>{shortFingerprint(props.view.fingerprint)}</span>
        </Show>
      </div>

      <Show when={props.ingestState === 'failed'}>
        <div class="ingestions-alert" role="alert">
          {tr('Siden kunne ikke legges i Kunnskap. Prøv igjen om litt.', 'The page could not be added to Knowledge. Try again shortly.')}
        </div>
      </Show>
      <Show when={props.view.warnings.length > 0}>
        <div class="ingestions-warning-box">
          <ul>
            <For each={props.view.warnings}>{(warning) => <li>{warning}</li>}</For>
          </ul>
        </div>
      </Show>

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
        <Show when={tab() !== 'screenshot'}>
          <CopyButton text={tabText} />
          <DownloadButton
            filename={tab() === 'markdown' ? 'scrape.md' : tab() === 'html' ? 'scrape.html' : tab() === 'links' ? 'links.txt' : 'scrape.json'}
            content={tabText}
            type={tab() === 'markdown' ? 'text/markdown' : tab() === 'html' ? 'text/html' : tab() === 'links' ? 'text/plain' : 'application/json'}
          />
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
        <Show when={tab() === 'json' || tab() === 'metadata'}>
          <pre class="ingestions-json-pre">{tab() === 'json' ? json() : JSON.stringify(props.view.metadata, null, 2)}</pre>
        </Show>
        <Show when={tab() === 'html'}>
          <pre class="ingestions-json-pre">{html()}</pre>
        </Show>
        <Show when={tab() === 'links'}>
          <ol class="ingestions-link-list">
            <For each={props.view.links} fallback={<li class="ingestions-muted">{tr('Ingen lenker funnet.', 'No links found.')}</li>}>
              {(link) => (
                <li>
                  <a href={link.href} target="_blank" rel="noreferrer" class="ingestions-truncate">{link.href}</a>
                  <Show when={link.text}>{(label) => <small>{label()}</small>}</Show>
                </li>
              )}
            </For>
          </ol>
        </Show>
        <Show when={tab() === 'screenshot' && props.view.captures}>
          {(captures) => (
            <Show
              when={captures().screenshotInline ? `data:${captures().screenshotInline!.contentType};base64,${captures().screenshotInline!.dataBase64}` : captures().screenshotUrl}
              fallback={<EmptyState title={tr('Ingen skjermbilde i svaret', 'No screenshot in the response')} />}
            >
              {(src) => <img class="ingestions-screenshot" src={src()} alt={tr('Skjermbilde av siden', 'Screenshot of the page')} />}
            </Show>
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

function ExtractResultView(props: { url: string; result: unknown; raw: unknown }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [tab, setTab] = createSignal<'data' | 'raw'>('data')
  const data = createMemo(() => {
    const record = typeof props.result === 'object' && props.result !== null ? (props.result as Record<string, unknown>) : {}
    return record.structured ?? record.data ?? props.result
  })
  const text = () => JSON.stringify(tab() === 'data' ? data() : props.raw, null, 2)
  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <Sparkles class="size-4" strokeWidth={1.9} />
            <h3>{tr('Uttrekk', 'Extract')}</h3>
          </div>
          <span class="ingestions-result__url">{props.url}</span>
        </div>
      </header>
      <div class="ingestions-result__tabs">
        <VerevonSegmented aria-label={tr('Visning', 'View')}>
          <VerevonSegmentedButton selected={tab() === 'data'} onClick={() => setTab('data')}>{tr('Data', 'Data')}</VerevonSegmentedButton>
          <VerevonSegmentedButton selected={tab() === 'raw'} onClick={() => setTab('raw')}>{tr('Rått svar', 'Raw response')}</VerevonSegmentedButton>
        </VerevonSegmented>
        <span class="ingestions-pg__spacer" />
        <CopyButton text={text} />
        <DownloadButton filename="extract.json" content={text} />
      </div>
      <div class="ingestions-result__viewport">
        <pre class="ingestions-json-pre">{text()}</pre>
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
                  Scrape
                </button>
              </li>
            )}
          </For>
        </ul>
      </div>
    </div>
  )
}

function SearchResultView(props: { query: string; results: WebSearchResult[]; onScrapeUrl: (url: string) => void }) {
  const i18n = useI18n()
  const tr = i18n.tr
  return (
    <div class="ingestions-result__body">
      <header class="ingestions-result__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <h3>{tr(`${props.results.length} treff`, `${props.results.length} results`)}</h3>
          </div>
          <span class="ingestions-result__url">“{props.query}”</span>
        </div>
        <NeedsBackend label={tr('KI-svar av', 'AI answer off')} reason={tr('Svarsyntese er ikke konfigurert på denne installasjonen.', 'Answer synthesis is not configured on this installation.')} />
      </header>
      <div class="ingestions-result__viewport">
        <ol class="ingestions-search-list">
          <For each={props.results} fallback={<li class="ingestions-muted">{tr('Ingen treff.', 'No results.')}</li>}>
            {(hit) => (
              <li>
                <div class="ingestions-min-width">
                  <a href={hit.url} target="_blank" rel="noreferrer"><strong>{hit.title || hit.url}</strong></a>
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
