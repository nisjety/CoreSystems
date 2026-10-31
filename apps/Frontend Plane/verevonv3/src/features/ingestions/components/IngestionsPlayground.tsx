import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import {
  Bot,
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
  Plus,
  ScanSearch,
  Search,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  type LucideProps,
} from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import {
  codeSnippet,
  endpointGroups,
  extractTemplateSchema,
  isAiFormat,
  MAX_AGE_MINUTES,
  newAction,
  quarryRequestFor,
  type ActionType,
  type ExtractTemplate,
  type PlaygroundAction,
  type PlaygroundEndpoint,
  type PlaygroundState,
  type ScrapeFormat,
  type ScrollMode,
  type SnippetLanguage,
} from '@/features/ingestions/lib/playground'
import { CopyButton, kindLabel } from '@/features/ingestions/components/ingestions-ui'

/**
 * The Firecrawl-style playground: a grouped endpoint picker, one input card whose
 * fields follow the endpoint, an options panel, a format gallery, a browser-step
 * builder for scrape, and a "get code" panel built from the request the page sends.
 * Submitting is the page's job; this component only edits and renders the state.
 */

const endpointIcons: Record<PlaygroundEndpoint, Component<LucideProps>> = {
  search: Search,
  map: MapIcon,
  scrape: ScanSearch,
  extract: Sparkles,
  crawl: Network,
  batch: Layers,
  agent: Bot,
}

type Panel = 'options' | 'formats' | 'actions' | 'code'

const formatOptions: Array<{ id: ScrapeFormat; icon: Component<LucideProps> }> = [
  { id: 'markdown', icon: FileText },
  { id: 'html', icon: Code2 },
  { id: 'rawHtml', icon: Code2 },
  { id: 'links', icon: Link },
  { id: 'screenshot', icon: Image },
  { id: 'pdf', icon: FileText },
  { id: 'json', icon: Sparkles },
  { id: 'summary', icon: FileText },
  { id: 'question', icon: Search },
  { id: 'highlights', icon: ScanSearch },
]

const actionTypes: ActionType[] = ['wait', 'click', 'write', 'press', 'scroll', 'select']

type Tr = (no: string, en: string) => string

function groupLabel(id: 'discover' | 'extract' | 'crawl' | 'autonomous', tr: Tr) {
  switch (id) {
    case 'discover':
      return tr('Oppdag', 'Discover')
    case 'extract':
      return tr('Hent ut', 'Extract')
    case 'crawl':
      return 'Crawl'
    case 'autonomous':
      return tr('Autonom', 'Autonomous')
  }
}

function formatLabel(id: ScrapeFormat, tr: Tr) {
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
    case 'pdf':
      return 'PDF'
    case 'json':
      return tr('Strukturert svar (JSON)', 'Structured answer (JSON)')
    case 'summary':
      return tr('Sammendrag', 'Summary')
    case 'question':
      return tr('Spørsmål til siden', 'Question about the page')
    case 'highlights':
      return tr('Høydepunkter', 'Highlights')
  }
}

function actionLabel(id: ActionType, tr: Tr) {
  switch (id) {
    case 'wait':
      return tr('Vent', 'Wait')
    case 'click':
      return tr('Klikk', 'Click')
    case 'write':
      return tr('Skriv', 'Write')
    case 'press':
      return tr('Trykk tast', 'Press key')
    case 'scroll':
      return tr('Rull', 'Scroll')
    case 'select':
      return tr('Velg', 'Select')
  }
}

export function submitLabel(endpoint: PlaygroundEndpoint, tr: Tr) {
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
    case 'agent':
      return tr('Start agent', 'Start agent')
  }
}

function endpointHint(endpoint: PlaygroundEndpoint, tr: Tr) {
  switch (endpoint) {
    case 'search':
      return tr('Søk på nettet, og scrape treffene du vil ha.', 'Search the web, then scrape the hits you want.')
    case 'map':
      return tr('Finn alle URL-er på et nettsted uten å hente innholdet.', 'List every URL on a site without fetching content.')
    case 'scrape':
      return tr(
        'Hent én side som Markdown, HTML, lenker, skjermbilde eller PDF, etter egne nettleserhandlinger om du vil.',
        'Fetch one page as Markdown, HTML, links, a screenshot or a PDF, after browser steps if you add them.',
      )
    case 'extract':
      return tr('Gi et JSON-skjema, og få strukturert data tilbake per side.', 'Give a JSON schema and get structured data back per page.')
    case 'crawl':
      return tr('Følg lenker fra én URL og hent nettstedet som en varig jobb du kan sette på pause.', 'Follow links from one URL and fetch the site as a durable job you can pause.')
    case 'batch':
      return tr('Hent en liste med URL-er som én varig jobb.', 'Fetch a list of URLs as one durable job.')
    case 'agent':
      return tr(
        'En nettleseragent jobber mot målet i inntil valgt antall steg, og logger hvert steg.',
        'A browser agent works toward the goal for up to the chosen number of steps and logs each step.',
      )
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
  const [panel, setPanel] = createSignal<Panel | null>(null)
  const [language, setLanguage] = createSignal<SnippetLanguage>('curl')
  const endpoint = () => props.state.endpoint
  const request = createMemo(() => quarryRequestFor(props.state))
  const snippet = createMemo(() => codeSnippet(language(), request()))
  const optionCount = createMemo(() => {
    const s = props.state
    switch (s.endpoint) {
      case 'scrape':
        return [
          !s.onlyMainContent,
          s.extractor !== 'readability',
          s.removeBase64Images,
          s.includeTags.trim(),
          s.excludeTags.trim(),
          s.waitMs.trim(),
          s.timeoutMs.trim(),
          s.maxAgeMinutes.trim(),
        ].filter(Boolean).length
      case 'crawl':
        return [
          s.maxDepth.trim(),
          s.includePaths.trim(),
          s.excludePaths.trim(),
          s.allowSubdomains,
          s.ingest,
          s.sitemap !== 'skip',
          !s.onlyMainContent,
          s.extractor !== 'readability',
          s.removeBase64Images,
          s.includeTags.trim(),
          s.excludeTags.trim(),
        ].filter(Boolean).length
      case 'map':
        return [s.mapSearch.trim(), s.allowSubdomains].filter(Boolean).length
      case 'search':
        return [
          s.searchTimeRange,
          s.searchTopic,
          s.searchIncludeDomains.trim(),
          s.searchExcludeDomains.trim(),
          s.searchExact,
          s.searchCountry.trim(),
          s.searchLanguage.trim(),
        ].filter(Boolean).length
      default:
        return 0
    }
  })
  const togglePanel = (next: Panel) => setPanel((current) => (current === next ? null : next))

  function toggleFormat(format: ScrapeFormat) {
    const current = props.state.formats
    const next = current.includes(format) ? current.filter((entry) => entry !== format) : [...current, format]
    props.onChange({ formats: next.length ? next : ['markdown'] })
  }

  function patchAction(id: string, patch: Partial<PlaygroundAction>) {
    props.onChange({
      actions: props.state.actions.map((action) => (action.id === id ? { ...action, ...patch } : action)),
    })
  }

  function removeAction(id: string) {
    props.onChange({ actions: props.state.actions.filter((action) => action.id !== id) })
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
                          setPanel(null)
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

        <Show when={endpoint() === 'agent'}>
          <label class="ingestions-field">
            {tr('Mål for agenten', 'Goal for the agent')}
            <VerevonTextarea
              rows={3}
              value={props.state.goal}
              onInput={(event) => props.onChange({ goal: event.currentTarget.value })}
              placeholder={tr(
                'Finn prisen på den billigste sykkelen og noter lagerstatus.',
                'Find the price of the cheapest bike and note the stock status.',
              )}
            />
          </label>
          <div class="ingestions-pg__panel ingestions-options-grid" role="group" aria-label={tr('Agentgrenser', 'Agent limits')}>
            <label class="ingestions-field">
              {tr('Maks steg', 'Max steps')}
              <VerevonInput
                type="number"
                min="1"
                max="100"
                value={props.state.maxSteps}
                onInput={(event) => props.onChange({ maxSteps: event.currentTarget.value })}
                placeholder="25"
              />
            </label>
            <label class="ingestions-field">
              {tr('Tillatte domener', 'Allowed domains')}
              <VerevonInput
                value={props.state.allowedDomains}
                onInput={(event) => props.onChange({ allowedDomains: event.currentTarget.value })}
                placeholder="example.com, docs.example.com"
              />
            </label>
          </div>
        </Show>

        <Show when={endpoint() === 'extract'}>
          <div class="ingestions-field">
            <span class="ingestions-field__row">
              <label for="ingestions-extract-schema">{tr('JSON-skjema', 'JSON schema')}</label>
              <span class="ingestions-pill-row ingestions-pill-row--inline">
                <For each={['general', 'product', 'article'] as ExtractTemplate[]}>
                  {(template) => (
                    <button
                      type="button"
                      class="ingestions-chip-button"
                      onClick={() => props.onChange({ schema: extractTemplateSchema(template) })}
                    >
                      {template === 'general' ? tr('Generell', 'General') : template === 'product' ? tr('Produkt', 'Product') : tr('Artikkel', 'Article')}
                    </button>
                  )}
                </For>
              </span>
            </span>
            <VerevonTextarea
              id="ingestions-extract-schema"
              class="ingestions-mono"
              rows={6}
              value={props.state.schema}
              onInput={(event) => props.onChange({ schema: event.currentTarget.value })}
              placeholder={tr(
                'Tomt: siden returneres som Markdown. Et JSON-objekt gir strukturert data.',
                'Empty: the page comes back as Markdown. A JSON object gives structured data.',
              )}
            />
            <small class="ingestions-field__help">
              {tr(
                'Skjemaet er en JSON Schema-beskrivelse av feltene du vil ha ut. Quarry fyller dem fra siden.',
                'The schema is a JSON Schema describing the fields you want. Quarry fills them from the page.',
              )}
            </small>
          </div>
        </Show>

        <Show when={endpoint() === 'extract'}>
          <div class="ingestions-field">
            <label for="ingestions-extract-prompt">{tr('Instruks (valgfritt)', 'Instruction (optional)')}</label>
            <VerevonTextarea
              id="ingestions-extract-prompt"
              rows={3}
              value={props.state.prompt}
              onInput={(event) => props.onChange({ prompt: event.currentTarget.value })}
              placeholder={tr(
                'Uten skjema: ett fritt JSON-objekt. Med skjema: styrer uttrekket innenfor skjemaet.',
                'Without a schema: one free-form JSON object. With a schema: steers the extraction within it.',
              )}
            />
            <small class="ingestions-field__help">
              {tr(`${props.state.prompt.trim().length} av 10 000 tegn`, `${props.state.prompt.trim().length} of 10,000 characters`)}
            </small>
          </div>
        </Show>

        <div class="ingestions-pg__toolbar">
          <Show when={['scrape', 'crawl', 'map', 'search'].includes(endpoint())}>
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
            </button>
            <button
              type="button"
              class={cn('ingestions-tool-button', panel() === 'actions' && 'ingestions-tool-button--active')}
              aria-expanded={panel() === 'actions' ? 'true' : 'false'}
              onClick={() => togglePanel('actions')}
            >
              <Play class="size-4" strokeWidth={1.9} />
              {tr('Nettleserhandlinger', 'Browser steps')}
              <Show when={props.state.actions.length > 0}>
                <span class="ingestions-count-pill">{props.state.actions.length}</span>
              </Show>
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
                'Markdown følger alltid med, så forhåndsvisningen har tekst. Skjermbilde og PDF må være slått på på Quarry-edge; er de av, sier resultatet fra hvilken bryter som mangler.',
                'Markdown always comes along, so the preview has text. Screenshot and PDF must be switched on in Quarry-edge; when they are off, the result names the switch that is missing.',
              )}
            </p>
            <Show when={props.state.formats.some(isAiFormat)}>
              <div class="ingestions-ai-inputs">
                <p class="ingestions-pg__note">
                  {tr(
                    'AI-formatene lar en modell lese siden. Svaret er data fra siden, ikke verifisert fakta. Quarry må ha AI-formater slått på; er de av, sier resultatet hvilken bryter som mangler.',
                    'The AI formats let a model read the page. The answer is data from the page, not verified fact. Quarry must have AI formats switched on; when they are off, the result names the switch that is missing.',
                  )}
                </p>
                <Show when={props.state.formats.includes('json')}>
                  <div class="ingestions-field">
                    <span class="ingestions-field__row">
                      <label for="ingestions-ai-schema">{tr('JSON-skjema for svaret', 'JSON schema for the answer')}</label>
                      <span class="ingestions-pill-row ingestions-pill-row--inline">
                        <For each={['general', 'product', 'article'] as ExtractTemplate[]}>
                          {(template) => (
                            <button
                              type="button"
                              class="ingestions-chip-button"
                              onClick={() => props.onChange({ aiSchema: extractTemplateSchema(template) })}
                            >
                              {template === 'general' ? tr('Generell', 'General') : template === 'product' ? tr('Produkt', 'Product') : tr('Artikkel', 'Article')}
                            </button>
                          )}
                        </For>
                      </span>
                    </span>
                    <VerevonTextarea
                      id="ingestions-ai-schema"
                      class="ingestions-mono"
                      rows={6}
                      value={props.state.aiSchema}
                      onInput={(event) => props.onChange({ aiSchema: event.currentTarget.value })}
                      placeholder={tr('Et JSON-objekt som beskriver feltene du vil ha.', 'A JSON object describing the fields you want.')}
                    />
                    <small class="ingestions-field__help">
                      {tr(
                        'Quarry godtar et utvalg av JSON Schema-nøkkelord. Svaret valideres mot skjemaet før det vises.',
                        'Quarry accepts a subset of JSON Schema keywords. The answer is checked against the schema before it is shown.',
                      )}
                    </small>
                  </div>
                  <div class="ingestions-field">
                    <label for="ingestions-ai-json-prompt">{tr('Instruks til modellen (valgfritt)', 'Instruction to the model (optional)')}</label>
                    <VerevonTextarea
                      id="ingestions-ai-json-prompt"
                      rows={2}
                      value={props.state.aiPrompt}
                      onInput={(event) => props.onChange({ aiPrompt: event.currentTarget.value })}
                    />
                  </div>
                </Show>
                <Show when={props.state.formats.includes('summary')}>
                  <div class="ingestions-field">
                    <label for="ingestions-ai-summary-length">{tr('Lengde på sammendrag (tegn)', 'Summary length (characters)')}</label>
                    <VerevonInput
                      id="ingestions-ai-summary-length"
                      type="number"
                      min="200"
                      max="10000"
                      value={props.state.aiSummaryMaxChars}
                      onInput={(event) => props.onChange({ aiSummaryMaxChars: event.currentTarget.value })}
                      placeholder="2000"
                    />
                    <label for="ingestions-ai-summary-language">{tr('Språk (valgfritt)', 'Language (optional)')}</label>
                    <VerevonInput
                      id="ingestions-ai-summary-language"
                      value={props.state.aiSummaryLanguage}
                      onInput={(event) => props.onChange({ aiSummaryLanguage: event.currentTarget.value })}
                      placeholder={tr('Sidens eget språk', "The page's own language")}
                    />
                  </div>
                </Show>
                <Show when={props.state.formats.includes('question')}>
                  <div class="ingestions-field">
                    <label for="ingestions-ai-question">{tr('Spørsmålet til siden', 'The question about the page')}</label>
                    <VerevonTextarea
                      id="ingestions-ai-question"
                      rows={2}
                      value={props.state.aiQuestion}
                      onInput={(event) => props.onChange({ aiQuestion: event.currentTarget.value })}
                      placeholder={tr('Hva koster den billigste modellen, og er den på lager?', 'What does the cheapest model cost, and is it in stock?')}
                    />
                  </div>
                </Show>
                <Show when={props.state.formats.includes('highlights')}>
                  <div class="ingestions-field">
                    <label for="ingestions-ai-highlight-query">{tr('Hva skal finnes på siden?', 'What should be found on the page?')}</label>
                    <VerevonInput
                      id="ingestions-ai-highlight-query"
                      value={props.state.aiHighlightQuery}
                      onInput={(event) => props.onChange({ aiHighlightQuery: event.currentTarget.value })}
                      placeholder={tr('priser, lagerstatus, garanti', 'prices, stock status, warranty')}
                    />
                  </div>
                </Show>
              </div>
            </Show>
          </div>
        </Show>

        <Show when={panel() === 'actions' && endpoint() === 'scrape'}>
          <div class="ingestions-pg__panel ingestions-actions-builder" role="group" aria-label={tr('Nettleserhandlinger', 'Browser steps')}>
            <p class="ingestions-pg__note">
              {tr(
                'Handlingene kjøres i rekkefølge i nettleseren før siden hentes. Krever at nettleserhandlinger er slått på på Quarry-edge.',
                'Steps run in order in the browser before the page is captured. They need browser steps switched on in Quarry-edge.',
              )}
            </p>
            <For
              each={props.state.actions}
              fallback={<div class="ingestions-empty-box">{tr('Ingen handlinger ennå. Legg til et steg under.', 'No steps yet. Add one below.')}</div>}
            >
              {(action, index) => (
                <div class="ingestions-action-row">
                  <span class="ingestions-action-row__index">{index() + 1}</span>
                  <VerevonSelect
                    aria-label={tr('Type handling', 'Step type')}
                    value={action.type}
                    onChange={(event) => patchAction(action.id, { type: event.currentTarget.value as ActionType })}
                  >
                    <For each={actionTypes}>{(type) => <option value={type}>{actionLabel(type, tr)}</option>}</For>
                  </VerevonSelect>
                  <Show when={action.type === 'wait'}>
                    <VerevonInput
                      type="number"
                      min="1"
                      max="30000"
                      aria-label={tr('Millisekunder', 'Milliseconds')}
                      value={action.ms}
                      onInput={(event) => patchAction(action.id, { ms: event.currentTarget.value })}
                    />
                  </Show>
                  <Show when={action.type === 'click' || action.type === 'write' || action.type === 'select'}>
                    <VerevonInput
                      class="ingestions-mono"
                      aria-label={tr('CSS-selektor', 'CSS selector')}
                      value={action.selector}
                      onInput={(event) => patchAction(action.id, { selector: event.currentTarget.value })}
                      placeholder="#search, button[type=submit]"
                    />
                  </Show>
                  <Show when={action.type === 'write'}>
                    <VerevonInput
                      aria-label={tr('Tekst', 'Text')}
                      value={action.text}
                      onInput={(event) => patchAction(action.id, { text: event.currentTarget.value })}
                      placeholder={tr('Teksten som skal skrives', 'The text to type')}
                    />
                  </Show>
                  <Show when={action.type === 'select'}>
                    <VerevonInput
                      aria-label={tr('Verdi', 'Value')}
                      value={action.value}
                      onInput={(event) => patchAction(action.id, { value: event.currentTarget.value })}
                      placeholder={tr('Verdien som skal velges', 'The option value to pick')}
                    />
                  </Show>
                  <Show when={action.type === 'press'}>
                    <VerevonInput
                      aria-label={tr('Tast', 'Key')}
                      value={action.key}
                      onInput={(event) => patchAction(action.id, { key: event.currentTarget.value })}
                      placeholder="Enter, Tab, ArrowDown"
                    />
                  </Show>
                  <Show when={action.type === 'scroll'}>
                    <VerevonSelect
                      aria-label={tr('Rullretning', 'Scroll direction')}
                      value={action.scroll}
                      onChange={(event) => patchAction(action.id, { scroll: event.currentTarget.value as ScrollMode })}
                    >
                      <option value="bottom">{tr('Til bunnen', 'To bottom')}</option>
                      <option value="top">{tr('Til toppen', 'To top')}</option>
                      <option value="down">{tr('Ned med piksler', 'Down by pixels')}</option>
                      <option value="up">{tr('Opp med piksler', 'Up by pixels')}</option>
                    </VerevonSelect>
                    <Show when={action.scroll === 'down' || action.scroll === 'up'}>
                      <VerevonInput
                        type="number"
                        min="1"
                        max="10000"
                        aria-label={tr('Piksler', 'Pixels')}
                        value={action.pixels}
                        onInput={(event) => patchAction(action.id, { pixels: event.currentTarget.value })}
                      />
                    </Show>
                  </Show>
                  <button type="button" class="ingestions-icon-button" aria-label={tr('Fjern handling', 'Remove step')} onClick={() => removeAction(action.id)}>
                    <Trash2 class="size-3.5" strokeWidth={1.9} />
                  </button>
                </div>
              )}
            </For>
            <div class="ingestions-pill-row">
              <For each={actionTypes}>
                {(type) => (
                  <button
                    type="button"
                    class="ingestions-chip-button"
                    onClick={() => props.onChange({ actions: [...props.state.actions, newAction(type)] })}
                  >
                    <Plus class="size-3" strokeWidth={2.2} />
                    {actionLabel(type, tr)}
                  </button>
                )}
              </For>
            </div>
            <label class="ingestions-field">
              {tr('Ved feil i et steg', 'When a step fails')}
              <VerevonSelect
                value={props.state.actionsOnError}
                onChange={(event) => props.onChange({ actionsOnError: event.currentTarget.value as 'abort' | 'continue' })}
              >
                <option value="abort">{tr('Stopp og rapporter', 'Stop and report')}</option>
                <option value="continue">{tr('Gå videre til neste steg', 'Go on to the next step')}</option>
              </VerevonSelect>
            </label>
          </div>
        </Show>

        <Show when={panel() === 'options'}>
          <div class="ingestions-pg__panel ingestions-options-grid" role="group" aria-label={tr('Alternativer', 'Options')}>
            <Show when={endpoint() === 'scrape' || endpoint() === 'crawl'}>
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
                {tr('Utdrag', 'Extraction')}
                <VerevonSelect
                  value={props.state.extractor}
                  onChange={(event) => props.onChange({ extractor: event.currentTarget.value as PlaygroundState['extractor'] })}
                >
                  <option value="readability">{tr('Hovedinnhold (Readability)', 'Main content (Readability)')}</option>
                  <option value="clean_full">{tr('Hele siden, uten skript og kulisser', 'Whole page, without scripts and chrome')}</option>
                  <option value="raw">{tr('Råt dokument', 'Raw document')}</option>
                </VerevonSelect>
              </label>
              <label class="ingestions-toggle">
                <input
                  type="checkbox"
                  checked={props.state.removeBase64Images}
                  onChange={(event) => props.onChange({ removeBase64Images: event.currentTarget.checked })}
                />
                <span>
                  <strong>{tr('Fjern base64-bilder', 'Remove base64 images')}</strong>
                  <small>{tr('Fjerner innebygde bilder fra Markdown.', 'Drops inline images from the Markdown.')}</small>
                </span>
              </label>
              <label class="ingestions-field">
                {tr('Inkluder tagger', 'Include tags')}
                <VerevonInput value={props.state.includeTags} onInput={(event) => props.onChange({ includeTags: event.currentTarget.value })} placeholder="main, article" />
              </label>
              <label class="ingestions-field">
                {tr('Ekskluder tagger', 'Exclude tags')}
                <VerevonInput value={props.state.excludeTags} onInput={(event) => props.onChange({ excludeTags: event.currentTarget.value })} placeholder="nav, footer" />
              </label>
              <Show when={endpoint() === 'scrape'}>
                <label class="ingestions-field">
                  {tr('Vent før henting (ms)', 'Wait before fetch (ms)')}
                  <VerevonInput type="number" min="0" max="30000" value={props.state.waitMs} onInput={(event) => props.onChange({ waitMs: event.currentTarget.value })} placeholder="0" />
                </label>
                <label class="ingestions-field">
                  {tr('Tidsavbrudd (ms)', 'Timeout (ms)')}
                  <VerevonInput type="number" min="1000" max="120000" value={props.state.timeoutMs} onInput={(event) => props.onChange({ timeoutMs: event.currentTarget.value })} placeholder="55000" />
                </label>
                <label class="ingestions-field">
                  {tr('Bruk lagret resultat opptil (minutter)', 'Use a stored result up to (minutes)')}
                  <VerevonInput
                    id="ingestions-max-age"
                    type="number"
                    min="1"
                    max={MAX_AGE_MINUTES}
                    value={props.state.maxAgeMinutes}
                    onInput={(event) => props.onChange({ maxAgeMinutes: event.currentTarget.value })}
                    placeholder={tr('Hent alltid', 'Always fetch')}
                  />
                </label>
              </Show>
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
                {tr('Sitemap', 'Sitemap')}
                <VerevonSelect
                  value={props.state.sitemap}
                  onChange={(event) => props.onChange({ sitemap: event.currentTarget.value as PlaygroundState['sitemap'] })}
                >
                  <option value="skip">{tr('Hopp over sitemap', 'Skip the sitemap')}</option>
                  <option value="include">{tr('Sidene fra lenkene og sitemap', 'Pages from links and the sitemap')}</option>
                  <option value="only">{tr('Bare sidene i sitemap', 'Only the pages in the sitemap')}</option>
                </VerevonSelect>
              </label>
              <label class="ingestions-field">
                {tr('Inkluder stier', 'Include paths')}
                <VerevonInput value={props.state.includePaths} onInput={(event) => props.onChange({ includePaths: event.currentTarget.value })} placeholder="/docs/*" />
              </label>
              <label class="ingestions-field">
                {tr('Ekskluder stier', 'Exclude paths')}
                <VerevonInput value={props.state.excludePaths} onInput={(event) => props.onChange({ excludePaths: event.currentTarget.value })} placeholder="/blog/*" />
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
              <label class="ingestions-field">
                {tr('Tidsrom', 'Time range')}
                <VerevonSelect
                  value={props.state.searchTimeRange}
                  onChange={(event) => props.onChange({ searchTimeRange: event.currentTarget.value as PlaygroundState['searchTimeRange'] })}
                >
                  <option value="">{tr('Når som helst', 'Any time')}</option>
                  <option value="day">{tr('Siste døgn', 'Past day')}</option>
                  <option value="week">{tr('Siste uke', 'Past week')}</option>
                  <option value="month">{tr('Siste måned', 'Past month')}</option>
                  <option value="year">{tr('Siste år', 'Past year')}</option>
                </VerevonSelect>
              </label>
              <label class="ingestions-field">
                {tr('Emne', 'Topic')}
                <VerevonSelect
                  value={props.state.searchTopic}
                  onChange={(event) => props.onChange({ searchTopic: event.currentTarget.value as PlaygroundState['searchTopic'] })}
                >
                  <option value="">{tr('Alle emner', 'All topics')}</option>
                  <option value="general">{tr('Generelt', 'General')}</option>
                  <option value="news">{tr('Nyheter', 'News')}</option>
                  <option value="finance">{tr('Finans', 'Finance')}</option>
                </VerevonSelect>
              </label>
              <label class="ingestions-field">
                {tr('Kun disse domenene', 'Only these domains')}
                <VerevonInput value={props.state.searchIncludeDomains} onInput={(event) => props.onChange({ searchIncludeDomains: event.currentTarget.value })} placeholder="docs.example.com" />
              </label>
              <label class="ingestions-field">
                {tr('Ikke disse domenene', 'Exclude domains')}
                <VerevonInput value={props.state.searchExcludeDomains} onInput={(event) => props.onChange({ searchExcludeDomains: event.currentTarget.value })} placeholder="pinterest.com" />
              </label>
              <label class="ingestions-field">
                {tr('Marked (land)', 'Market (country)')}
                <VerevonInput value={props.state.searchCountry} onInput={(event) => props.onChange({ searchCountry: event.currentTarget.value.slice(0, 2) })} placeholder="NO" />
              </label>
              <label class="ingestions-field">
                {tr('Språk', 'Language')}
                <VerevonInput value={props.state.searchLanguage} onInput={(event) => props.onChange({ searchLanguage: event.currentTarget.value.slice(0, 3) })} placeholder="no" />
              </label>
              <label class="ingestions-toggle">
                <input type="checkbox" checked={props.state.searchExact} onChange={(event) => props.onChange({ searchExact: event.currentTarget.checked })} />
                <span>
                  <strong>{tr('Eksakt treff', 'Exact match')}</strong>
                  <small>{tr('Søk etter nøyaktig ordlyd.', 'Search for the exact wording.')}</small>
                </span>
              </label>
              <label class="ingestions-toggle">
                <input
                  type="checkbox"
                  checked={props.state.searchIncludeAnswer}
                  onChange={(event) => props.onChange({ searchIncludeAnswer: event.currentTarget.checked })}
                />
                <span>
                  <strong>{tr('Be om svar', 'Ask for an answer')}</strong>
                  <small>
                    {tr(
                      'Quarry skriver et kort svar fra treffene når en svarleverandør er satt opp.',
                      'Quarry writes a short answer from the hits when an answer provider is set up.',
                    )}
                  </small>
                </span>
              </label>
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
            <pre class="ingestions-code-panel__code">
              <code>{snippet()}</code>
            </pre>
            <p class="ingestions-pg__note">
              {request().note ??
                tr(
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
