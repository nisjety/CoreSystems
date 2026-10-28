import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import {
  Bot,
  ChevronDown,
  Code2,
  FileText,
  Globe,
  Image,
  Layers,
  Link,
  Loader,
  Map as MapIcon,
  Network,
  Play,
  ScanSearch,
  Search,
  SlidersHorizontal,
  Sparkles,
  type LucideProps,
} from '@/shared/icons'
import { useI18n, translateApiError } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import { ApiError } from '@/shared/api/http'
import {
  codeSnippet,
  endpointGroups,
  extractTemplatePrompt,
  quarryRequestFor,
  type ExtractTemplate,
  type PlaygroundEndpoint,
  type PlaygroundState,
  type ScrapeFormat,
  type SnippetLanguage,
} from '@/features/ingestions/lib/playground'
import { CopyButton, NeedsBackend, kindLabel } from '@/features/ingestions/components/ingestions-ui'

/**
 * The Firecrawl-style playground: grouped endpoint picker, one input card with an
 * options panel, a format gallery and a "get code" panel. Submitting is the page's
 * job (it owns the run list the async kinds reconcile against); this component only
 * edits the state and renders it.
 */

const endpointIcons: Record<PlaygroundEndpoint, Component<LucideProps>> = {
  search: Search,
  map: MapIcon,
  scrape: ScanSearch,
  extract: Sparkles,
  crawl: Network,
  batch: Layers,
}

const formatOptions: Array<{ id: ScrapeFormat; icon: Component<LucideProps> }> = [
  { id: 'markdown', icon: FileText },
  { id: 'html', icon: Code2 },
  { id: 'rawHtml', icon: Code2 },
  { id: 'links', icon: Link },
  { id: 'screenshot', icon: Image },
]

function groupLabel(id: 'discover' | 'extract' | 'crawl', tr: (no: string, en: string) => string) {
  switch (id) {
    case 'discover':
      return tr('Oppdag', 'Discover')
    case 'extract':
      return tr('Hent ut', 'Extract')
    case 'crawl':
      return 'Crawl'
  }
}

function formatLabel(id: ScrapeFormat, tr: (no: string, en: string) => string) {
  switch (id) {
    case 'markdown':
      return 'Markdown'
    case 'html':
      return 'HTML'
    case 'rawHtml':
      return tr('Rå HTML', 'Raw HTML')
    case 'links':
      return tr('Lenker', 'Links')
    case 'screenshot':
      return tr('Skjermbilde', 'Screenshot')
  }
}

export function submitLabel(endpoint: PlaygroundEndpoint, tr: (no: string, en: string) => string) {
  switch (endpoint) {
    case 'search':
      return tr('Søk på nettet', 'Search the web')
    case 'map':
      return tr('Kartlegg nettsted', 'Map site')
    case 'scrape':
      return tr('Start kjøring', 'Start run')
    case 'extract':
      return tr('Trekk ut data', 'Extract data')
    case 'crawl':
      return tr('Start crawl', 'Start crawl')
    case 'batch':
      return tr('Start batch', 'Start batch')
  }
}

function endpointHint(endpoint: PlaygroundEndpoint, tr: (no: string, en: string) => string) {
  switch (endpoint) {
    case 'search':
      return tr('Søk på nettet og scrape treffene du vil ha.', 'Search the web, then scrape the hits you want.')
    case 'map':
      return tr('Finn alle URL-er på et nettsted uten å hente innholdet.', 'List every URL on a site without fetching content.')
    case 'scrape':
      return tr('Hent én side som Markdown, HTML, lenker eller skjermbilde.', 'Fetch one page as Markdown, HTML, links or a screenshot.')
    case 'extract':
      return tr('Beskriv hva du vil ha, og få strukturert data tilbake.', 'Describe what you want and get structured data back.')
    case 'crawl':
      return tr('Følg lenker fra én URL og hent hele nettstedet som en varig jobb.', 'Follow links from one URL and fetch the site as a durable job.')
    case 'batch':
      return tr('Hent en liste med URL-er som én varig jobb.', 'Fetch a list of URLs as one durable job.')
  }
}

export function IngestionsPlayground(props: {
  state: PlaygroundState
  onChange: (patch: Partial<PlaygroundState>) => void
  onSubmit: () => void
  submitting: boolean
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [panel, setPanel] = createSignal<'options' | 'formats' | 'code' | null>(null)
  const [language, setLanguage] = createSignal<SnippetLanguage>('curl')
  const endpoint = () => props.state.endpoint
  const request = createMemo(() => quarryRequestFor(props.state))
  const snippet = createMemo(() => codeSnippet(language(), request()))
  const optionCount = createMemo(() => {
    const s = props.state
    switch (s.endpoint) {
      case 'scrape':
        return [!s.onlyMainContent, s.includeTags.trim(), s.excludeTags.trim(), s.waitMs.trim(), s.timeoutMs.trim()].filter(Boolean).length
      case 'crawl':
        return [s.maxDepth.trim(), s.includePaths.trim(), s.excludePaths.trim(), s.allowSubdomains, s.ingest].filter(Boolean).length
      case 'map':
        return [s.mapSearch.trim(), s.allowSubdomains].filter(Boolean).length
      default:
        return 0
    }
  })
  function togglePanel(next: 'options' | 'formats' | 'code') {
    setPanel((current) => (current === next ? null : next))
  }

  function toggleFormat(format: ScrapeFormat) {
    const current = props.state.formats
    const next = current.includes(format) ? current.filter((entry) => entry !== format) : [...current, format]
    props.onChange({ formats: next.length ? next : ['markdown'] })
  }

  return (
    <div class="ingestions-pg">
      <div class="ingestions-pg__endpoints" role="group" aria-label={tr('Endepunkt', 'Endpoint')}>
        <For each={endpointGroups}>
          {(group) => (
            <div class="ingestions-pg__group">
              <span class="ingestions-pg__group-label">{groupLabel(group.id, tr)}</span>
              <VerevonSegmented class="ingestions-pg__segmented">
                <For each={group.endpoints}>
                  {(id) => {
                    const Icon = endpointIcons[id]
                    return (
                      <VerevonSegmentedButton
                        class="ingestions-pg__endpoint"
                        selected={endpoint() === id}
                        onClick={() => {
                          props.onChange({ endpoint: id })
                          if (id !== 'scrape' && panel() === 'formats') setPanel(null)
                        }}
                      >
                        <Icon class="size-4" strokeWidth={1.9} />
                        {kindLabel(id, tr)}
                      </VerevonSegmentedButton>
                    )
                  }}
                </For>
              </VerevonSegmented>
            </div>
          )}
        </For>
        <div class="ingestions-pg__group">
          <span class="ingestions-pg__group-label">{tr('Autonom', 'Autonomous')}</span>
          <span class="ingestions-pg__endpoint ingestions-pg__endpoint--stub" aria-disabled="true">
            <Bot class="size-4" strokeWidth={1.9} />
            Agent
            <NeedsBackend
              reason={tr(
                'Innhentingsruten sender ukjente typer til scrape i dag; agentkjøringer går via nettleser-API-et.',
                'The ingestion route sends unknown kinds to scrape today; agent runs go through the browser API.',
              )}
            />
          </span>
        </div>
      </div>

      <form
        class="verevon-panel ingestions-pg__card"
        onSubmit={(event) => {
          event.preventDefault()
          props.onSubmit()
        }}
      >
        <p class="ingestions-pg__hint">{endpointHint(endpoint(), tr)}</p>

        <Show when={endpoint() === 'search'}>
          <label class="ingestions-field">
            {tr('Søkeord', 'Search query')}
            <VerevonInput
              value={props.state.query}
              onInput={(event) => props.onChange({ query: event.currentTarget.value })}
              placeholder={tr('f.eks. priser for Firecrawl', 'e.g. Firecrawl pricing')}
            />
          </label>
        </Show>
        <Show when={endpoint() === 'batch'}>
          <label class="ingestions-field">
            {tr('URL-er', 'URLs')}
            <VerevonTextarea
              rows={5}
              value={props.state.urls}
              onInput={(event) => props.onChange({ urls: event.currentTarget.value })}
              placeholder={'https://example.com/pricing\nhttps://example.com/docs'}
            />
          </label>
        </Show>
        <Show when={endpoint() !== 'search' && endpoint() !== 'batch'}>
          <label class="ingestions-field">
            {tr('Mål-URL', 'Target URL')}
            <div class="ingestions-pg__url">
              <Globe class="size-4" strokeWidth={1.9} aria-hidden="true" />
              <VerevonInput
                value={props.state.url}
                onInput={(event) => props.onChange({ url: event.currentTarget.value })}
                placeholder="https://example.com"
              />
            </div>
          </label>
        </Show>
        <Show when={endpoint() === 'extract'}>
          <div class="ingestions-field">
            <span class="ingestions-field__row">
              <label for="ingestions-extract-prompt">{tr('Hva skal trekkes ut?', 'What should be extracted?')}</label>
              <span class="ingestions-pill-row ingestions-pill-row--inline">
                <For each={['general', 'product', 'article'] as ExtractTemplate[]}>
                  {(template) => (
                    <button
                      type="button"
                      class="ingestions-chip-button"
                      onClick={() => props.onChange({ prompt: extractTemplatePrompt(template, tr) })}
                    >
                      {template === 'general' ? tr('Generell', 'General') : template === 'product' ? tr('Produkt', 'Product') : tr('Artikkel', 'Article')}
                    </button>
                  )}
                </For>
              </span>
            </span>
            <VerevonTextarea
              id="ingestions-extract-prompt"
              rows={3}
              value={props.state.prompt}
              onInput={(event) => props.onChange({ prompt: event.currentTarget.value })}
              placeholder={extractTemplatePrompt('general', tr)}
            />
          </div>
        </Show>

        <div class="ingestions-pg__toolbar">
          <Show when={['scrape', 'crawl', 'map', 'search', 'extract'].includes(endpoint())}>
            <button
              type="button"
              class={cn('ingestions-tool-button', panel() === 'options' && 'ingestions-tool-button--active')}
              aria-expanded={panel() === 'options' ? 'true' : 'false'}
              onClick={() => togglePanel('options')}
            >
              <SlidersHorizontal class="size-4" strokeWidth={1.9} />
              {tr('Alternativer', 'Options')}
              <Show when={optionCount() > 0}>
                <span class="ingestions-count-pill">{optionCount()}</span>
              </Show>
            </button>
          </Show>
          <Show when={endpoint() === 'scrape'}>
            <button
              type="button"
              class={cn('ingestions-tool-button', panel() === 'formats' && 'ingestions-tool-button--active')}
              aria-expanded={panel() === 'formats' ? 'true' : 'false'}
              onClick={() => togglePanel('formats')}
            >
              <FileText class="size-4" strokeWidth={1.9} />
              {tr('Format', 'Format')}: {props.state.formats.map((format) => formatLabel(format, tr)).join(', ')}
              <ChevronDown class="size-3.5" strokeWidth={1.9} />
            </button>
          </Show>
          <span class="ingestions-pg__spacer" />
          <button
            type="button"
            class={cn('ingestions-tool-button', panel() === 'code' && 'ingestions-tool-button--active')}
            aria-expanded={panel() === 'code' ? 'true' : 'false'}
            onClick={() => togglePanel('code')}
          >
            <Code2 class="size-4" strokeWidth={1.9} />
            {tr('Hent kode', 'Get code')}
          </button>
          <Button variant="primary" type="submit" disabled={props.submitting} class="ingestions-pg__submit">
            <Show when={props.submitting} fallback={<Play class="size-4" strokeWidth={1.9} />}>
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
            </Show>
            {submitLabel(endpoint(), tr)}
          </Button>
        </div>

        <Show when={panel() === 'formats' && endpoint() === 'scrape'}>
          <div class="ingestions-pg__panel" role="group" aria-label={tr('Utdataformater', 'Output formats')}>
            <div class="ingestions-format-grid">
              <For each={formatOptions}>
                {(option) => {
                  const Icon = option.icon
                  const selected = () => props.state.formats.includes(option.id)
                  return (
                    <button
                      type="button"
                      class={cn('ingestions-format-card', selected() && 'ingestions-format-card--selected')}
                      aria-pressed={selected() ? 'true' : 'false'}
                      onClick={() => toggleFormat(option.id)}
                    >
                      <Icon class="size-4" strokeWidth={1.8} />
                      <span>{formatLabel(option.id, tr)}</span>
                    </button>
                  )
                }}
              </For>
            </div>
            <p class="ingestions-pg__note">
              {tr(
                'Markdown følger alltid med for forhåndsvisningen. Skjermbilde kan være slått av på installasjonen; da sier resultatet fra.',
                'Markdown always comes along for the preview. Screenshots may be switched off on this installation; the result says so.',
              )}
            </p>
            <div class="ingestions-pill-row">
              <span class="ingestions-muted-label">{tr('Ikke tilgjengelig ennå:', 'Not available yet:')}</span>
              <NeedsBackend label={tr('Sammendrag', 'Summary')} reason={tr('AI-formater er slått av i Quarry på denne installasjonen.', 'AI formats are switched off in Quarry on this installation.')} />
              <NeedsBackend label="JSON" reason={tr('Strukturert JSON per side krever at gatewayen videresender schema.', 'Per-page structured JSON needs the gateway to forward a schema.')} />
              <NeedsBackend label={tr('Handlinger', 'Actions')} reason={tr('Nettleserhandlinger (klikk, skriv, rull) er slått av (QUARRY_EDGE__SCRAPE_ACTIONS).', 'Browser actions (click, type, scroll) are switched off (QUARRY_EDGE__SCRAPE_ACTIONS).')} />
            </div>
          </div>
        </Show>

        <Show when={panel() === 'options'}>
          <div class="ingestions-pg__panel ingestions-options-grid" role="group" aria-label={tr('Alternativer', 'Options')}>
            <Show when={endpoint() === 'scrape'}>
              <label class="ingestions-toggle">
                <input
                  type="checkbox"
                  checked={props.state.onlyMainContent}
                  onChange={(event) => props.onChange({ onlyMainContent: event.currentTarget.checked })}
                />
                <span>
                  <strong>{tr('Kun hovedinnhold', 'Main content only')}</strong>
                  <small>{tr('Fjerner meny, bunntekst og annonser.', 'Drops navigation, footers and ads.')}</small>
                </span>
              </label>
              <label class="ingestions-field">
                {tr('Inkluder tagger', 'Include tags')}
                <VerevonInput value={props.state.includeTags} onInput={(event) => props.onChange({ includeTags: event.currentTarget.value })} placeholder="main, article" />
              </label>
              <label class="ingestions-field">
                {tr('Ekskluder tagger', 'Exclude tags')}
                <VerevonInput value={props.state.excludeTags} onInput={(event) => props.onChange({ excludeTags: event.currentTarget.value })} placeholder="nav, footer, .cookie" />
              </label>
              <label class="ingestions-field">
                {tr('Vent før henting (ms)', 'Wait before fetch (ms)')}
                <VerevonInput type="number" min="0" max="30000" value={props.state.waitMs} onInput={(event) => props.onChange({ waitMs: event.currentTarget.value })} placeholder="0" />
              </label>
              <label class="ingestions-field">
                {tr('Tidsavbrudd (ms)', 'Timeout (ms)')}
                <VerevonInput type="number" min="1000" max="120000" value={props.state.timeoutMs} onInput={(event) => props.onChange({ timeoutMs: event.currentTarget.value })} placeholder="55000" />
              </label>
            </Show>
            <Show when={endpoint() === 'crawl'}>
              <label class="ingestions-field">
                {tr('Maks sider', 'Max pages')}
                <VerevonInput type="number" min="1" max="50" value={props.state.maxPages} onInput={(event) => props.onChange({ maxPages: event.currentTarget.value })} />
              </label>
              <label class="ingestions-field">
                {tr('Maks dybde', 'Max depth')}
                <VerevonInput type="number" min="1" max="10" value={props.state.maxDepth} onInput={(event) => props.onChange({ maxDepth: event.currentTarget.value })} placeholder="—" />
              </label>
              <label class="ingestions-field">
                {tr('Inkluder stier', 'Include paths')}
                <VerevonInput value={props.state.includePaths} onInput={(event) => props.onChange({ includePaths: event.currentTarget.value })} placeholder="/docs/*" />
              </label>
              <label class="ingestions-field">
                {tr('Ekskluder stier', 'Exclude paths')}
                <VerevonInput value={props.state.excludePaths} onInput={(event) => props.onChange({ excludePaths: event.currentTarget.value })} placeholder="/blog/*, re:\\?page=" />
              </label>
              <label class="ingestions-toggle">
                <input type="checkbox" checked={props.state.allowSubdomains} onChange={(event) => props.onChange({ allowSubdomains: event.currentTarget.checked })} />
                <span>
                  <strong>{tr('Følg underdomener', 'Follow subdomains')}</strong>
                  <small>{tr('docs.eksempel.no når du starter på eksempel.no.', 'docs.example.com when you start at example.com.')}</small>
                </span>
              </label>
              <label class="ingestions-toggle">
                <input type="checkbox" checked={props.state.ingest} onChange={(event) => props.onChange({ ingest: event.currentTarget.checked })} />
                <span>
                  <strong>{tr('Lagre i Kunnskap', 'Save to Knowledge')}</strong>
                  <small>{tr('Av: sidene holdes som arbeidssett og indekseres ikke.', 'Off: pages stay a working set and are not indexed.')}</small>
                </span>
              </label>
            </Show>
            <Show when={endpoint() === 'map'}>
              <label class="ingestions-field">
                {tr('Filtrer på ord', 'Filter by keyword')}
                <VerevonInput value={props.state.mapSearch} onInput={(event) => props.onChange({ mapSearch: event.currentTarget.value })} placeholder="pricing" />
              </label>
              <label class="ingestions-field">
                {tr('Maks URL-er', 'Max URLs')}
                <VerevonInput type="number" min="1" max="1000" value={props.state.mapLimit} onInput={(event) => props.onChange({ mapLimit: event.currentTarget.value })} />
              </label>
              <label class="ingestions-toggle">
                <input type="checkbox" checked={props.state.allowSubdomains} onChange={(event) => props.onChange({ allowSubdomains: event.currentTarget.checked })} />
                <span>
                  <strong>{tr('Ta med underdomener', 'Include subdomains')}</strong>
                </span>
              </label>
            </Show>
            <Show when={endpoint() === 'search'}>
              <label class="ingestions-field">
                {tr('Antall treff', 'Results')}
                <VerevonInput type="number" min="1" max="20" value={props.state.searchLimit} onInput={(event) => props.onChange({ searchLimit: event.currentTarget.value })} />
              </label>
              <div class="ingestions-pill-row">
                <NeedsBackend label={tr('KI-svar', 'AI answer')} reason={tr('Svarsyntese er ikke konfigurert i Quarry på denne installasjonen.', 'Answer synthesis is not configured in Quarry on this installation.')} />
              </div>
            </Show>
            <Show when={endpoint() === 'extract'}>
              <div class="ingestions-field">
                {tr('JSON-schema', 'JSON schema')}
                <div class="ingestions-pill-row">
                  <NeedsBackend reason={tr('Quarry tar imot schema, men gatewayen videresender bare prompten for uttrekk i dag.', 'Quarry accepts a schema, but the gateway forwards only the prompt for extract today.')} />
                </div>
              </div>
            </Show>
          </div>
        </Show>

        <Show when={panel() === 'code'}>
          <div class="ingestions-pg__panel ingestions-code-panel">
            <div class="ingestions-code-panel__head">
              <VerevonSegmented aria-label={tr('Språk', 'Language')}>
                <For each={[['curl', 'cURL'], ['javascript', 'JavaScript'], ['python', 'Python']] as Array<[SnippetLanguage, string]>}>
                  {([id, label]) => (
                    <VerevonSegmentedButton selected={language() === id} onClick={() => setLanguage(id)}>
                      {label}
                    </VerevonSegmentedButton>
                  )}
                </For>
              </VerevonSegmented>
              <CopyButton text={snippet} label={tr('Kopier kode', 'Copy code')} />
            </div>
            <pre class="ingestions-code-panel__code"><code>{snippet()}</code></pre>
            <p class="ingestions-pg__note">
              {tr(
                `Samme felt som siden sender, mot Quarry v2 sitt ${request().path}. Sett QUARRY_URL og QUARRY_TOKEN selv.`,
                `The same fields this page sends, against Quarry v2's ${request().path}. Set QUARRY_URL and QUARRY_TOKEN yourself.`,
              )}
            </p>
          </div>
        </Show>
      </form>
    </div>
  )
}

/** A failed scrape that asked for a capture the installation switched off. */
export function isCaptureDisabled(error: unknown): boolean {
  if (!(error instanceof ApiError)) return false
  const fields = error.details?.fields
  return Array.isArray(fields) && fields.some((field) => typeof field === 'object' && field !== null && (field as Record<string, unknown>).code === 'feature_disabled')
}

export function playgroundErrorMessage(error: unknown, endpoint: PlaygroundEndpoint, tr: (no: string, en: string) => string): string {
  if (isCaptureDisabled(error)) {
    return tr(
      'Skjermbilde er slått av på denne installasjonen (QUARRY_EDGE__SCRAPE_CAPTURE). Fjern Skjermbilde-formatet og kjør igjen.',
      'Screenshots are switched off on this installation (QUARRY_EDGE__SCRAPE_CAPTURE). Remove the Screenshot format and run again.',
    )
  }
  return translateApiError(error, tr, {
    no: `${kindLabel(endpoint, tr)} kunne ikke fullføres.`,
    en: `${kindLabel(endpoint, tr)} could not be completed.`,
  })
}
