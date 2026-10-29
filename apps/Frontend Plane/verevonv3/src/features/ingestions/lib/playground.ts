import type { CrawlRequest, ScrapeRequest } from '@/shared/api/knowledge-client'
import type { WebSearchFilters } from '@/shared/api/search-client'

/**
 * The Innhenting playground (Firecrawl-style): one endpoint at a time, its
 * options, and the exact request those options produce. Each endpoint maps to a
 * real backend call: `/v1/scrape`, `/v1/map`, `/v1/search`, `/v1/extract`,
 * `/v1/crawl`, `/v1/batch` through the gateway, and the browser agent through the
 * `ingestions.start_agent_run` action. The request builders here are the single
 * source for both what the page sends and the "get code" snippet.
 */
export type PlaygroundEndpoint = 'search' | 'map' | 'scrape' | 'extract' | 'crawl' | 'batch' | 'agent'

export type ScrapeFormat = 'markdown' | 'html' | 'rawHtml' | 'links' | 'screenshot' | 'pdf'

export type ExtractTemplate = 'general' | 'product' | 'article'

export type ActionType = 'wait' | 'click' | 'write' | 'press' | 'scroll' | 'select'

/** Quarry's content extractors: main content (default), the whole body without scripts, or the raw document. */
export type Extractor = 'readability' | 'clean_full' | 'raw'

export type ScrollMode = 'top' | 'bottom' | 'down' | 'up'

/** One browser step of a scrape, as the form holds it. Only the fields its type reads matter. */
export type PlaygroundAction = {
  id: string
  type: ActionType
  selector: string
  text: string
  ms: string
  key: string
  scroll: ScrollMode
  pixels: string
  value: string
}

export type PlaygroundState = {
  endpoint: PlaygroundEndpoint
  url: string
  urls: string
  query: string
  goal: string
  formats: ScrapeFormat[]
  onlyMainContent: boolean
  extractor: Extractor
  removeBase64Images: boolean
  includeTags: string
  excludeTags: string
  waitMs: string
  timeoutMs: string
  actions: PlaygroundAction[]
  actionsOnError: 'abort' | 'continue'
  schema: string
  maxPages: string
  maxDepth: string
  includePaths: string
  excludePaths: string
  allowSubdomains: boolean
  ingest: boolean
  mapSearch: string
  mapLimit: string
  searchLimit: string
  searchTimeRange: '' | 'day' | 'week' | 'month' | 'year'
  searchTopic: '' | 'general' | 'news' | 'finance'
  searchIncludeDomains: string
  searchExcludeDomains: string
  searchExact: boolean
  searchIncludeAnswer: boolean
  searchCountry: string
  searchLanguage: string
  maxSteps: string
  allowedDomains: string
}

export const defaultPlaygroundState: PlaygroundState = {
  endpoint: 'scrape',
  url: '',
  urls: '',
  query: '',
  goal: '',
  formats: ['markdown'],
  onlyMainContent: true,
  extractor: 'readability',
  removeBase64Images: false,
  includeTags: '',
  excludeTags: '',
  waitMs: '',
  timeoutMs: '',
  actions: [],
  actionsOnError: 'abort',
  schema: '',
  maxPages: '10',
  maxDepth: '',
  includePaths: '',
  excludePaths: '',
  allowSubdomains: false,
  ingest: false,
  mapSearch: '',
  mapLimit: '100',
  searchLimit: '5',
  searchTimeRange: '',
  searchTopic: '',
  searchIncludeDomains: '',
  searchExcludeDomains: '',
  searchExact: false,
  searchIncludeAnswer: false,
  searchCountry: '',
  searchLanguage: '',
  maxSteps: '',
  allowedDomains: '',
}

/** Endpoints grouped the way Firecrawl groups them. */
export const endpointGroups: Array<{ id: 'discover' | 'extract' | 'crawl' | 'autonomous'; endpoints: PlaygroundEndpoint[] }> = [
  { id: 'discover', endpoints: ['search', 'map'] },
  { id: 'extract', endpoints: ['scrape', 'extract'] },
  { id: 'crawl', endpoints: ['crawl', 'batch'] },
  { id: 'autonomous', endpoints: ['agent'] },
]

/** Endpoints that hand off a durable job instead of answering inline. */
export function isAsyncEndpoint(endpoint: PlaygroundEndpoint): boolean {
  return endpoint === 'crawl' || endpoint === 'batch'
}

/** Split a comma- or newline-separated field into trimmed, non-empty entries. */
export function splitList(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter(Boolean)
}

/** A positive integer from a text field, clamped to `[min, max]`; `undefined` when blank or invalid. */
export function boundedInt(value: string, min: number, max: number): number | undefined {
  const parsed = Number.parseInt(value.trim(), 10)
  if (!Number.isFinite(parsed)) return undefined
  return Math.min(max, Math.max(min, parsed))
}

export type MissingInput = 'url' | 'urls' | 'query' | 'goal'

/** What is missing before the endpoint can run, or `null` when it is ready. */
export function missingInput(state: PlaygroundState): MissingInput | null {
  switch (state.endpoint) {
    case 'search':
      return state.query.trim() ? null : 'query'
    case 'batch':
      return splitList(state.urls).length > 0 ? null : 'urls'
    case 'agent':
      if (!state.url.trim()) return 'url'
      return state.goal.trim() ? null : 'goal'
    default:
      return state.url.trim() ? null : 'url'
  }
}

export type SchemaParse =
  | { ok: true; schema: Record<string, unknown> | null }
  | { ok: false; reason: 'invalid_json' | 'not_object' }

/** The extraction schema the text holds: `null` when empty (markdown per source), else a JSON object. */
export function parseSchema(text: string): SchemaParse {
  const trimmed = text.trim()
  if (!trimmed) return { ok: true, schema: null }
  try {
    const value: unknown = JSON.parse(trimmed)
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return { ok: false, reason: 'not_object' }
    }
    return { ok: true, schema: value as Record<string, unknown> }
  } catch {
    return { ok: false, reason: 'invalid_json' }
  }
}

export type ActionBuild =
  | { ok: true; actions: Array<Record<string, unknown>> }
  | { ok: false; index: number; field: 'value' | 'selector' | 'text' | 'key' | 'ms' | 'pixels' }

/**
 * The Quarry `actions` list for a scrape, built from the form. Each step is
 * checked the way quarry-edge checks it, so a bad step is named here and never
 * reaches the edge half-formed.
 */
export function buildActions(actions: PlaygroundAction[]): ActionBuild {
  const built: Array<Record<string, unknown>> = []
  for (const [index, action] of actions.entries()) {
    const selector = action.selector.trim()
    switch (action.type) {
      case 'wait': {
        const ms = boundedInt(action.ms, 1, 30_000)
        if (ms === undefined) return { ok: false, index, field: 'ms' }
        built.push({ type: 'wait', milliseconds: ms })
        break
      }
      case 'click':
        if (!selector) return { ok: false, index, field: 'selector' }
        built.push({ type: 'click', selector })
        break
      case 'write':
        if (!selector) return { ok: false, index, field: 'selector' }
        if (!action.text) return { ok: false, index, field: 'text' }
        built.push({ type: 'write', selector, text: action.text })
        break
      case 'press':
        if (!action.key.trim()) return { ok: false, index, field: 'key' }
        built.push({ type: 'press', key: action.key.trim() })
        break
      case 'scroll':
        if (action.scroll === 'top' || action.scroll === 'bottom') {
          built.push({ type: 'scroll', to: action.scroll })
        } else {
          const pixels = boundedInt(action.pixels, 1, 10_000)
          if (pixels === undefined) return { ok: false, index, field: 'pixels' }
          built.push({ type: 'scroll', direction: action.scroll, pixels })
        }
        break
      case 'select':
        if (!selector) return { ok: false, index, field: 'selector' }
        if (!action.value.trim()) return { ok: false, index, field: 'value' }
        built.push({ type: 'select', selector, value: action.value })
        break
    }
  }
  return { ok: true, actions: built }
}

const QUARRY_FORMAT: Record<'markdown' | 'html' | 'rawHtml' | 'links', string> = {
  markdown: 'markdown',
  html: 'html',
  rawHtml: 'rawHtml',
  links: 'links',
}

/** The scrape body for `scrapeUrl` (`/api/v1/knowledge/scrape` → `/v1/scrape`). */
export function scrapeRequestFromState(state: PlaygroundState, ingest = false): ScrapeRequest {
  const formats = state.formats
    .filter((format): format is keyof typeof QUARRY_FORMAT => format in QUARRY_FORMAT)
    .map((format) => QUARRY_FORMAT[format])
  const includeTags = splitList(state.includeTags)
  const excludeTags = splitList(state.excludeTags)
  const waitMs = boundedInt(state.waitMs, 0, 30_000)
  const timeoutMs = boundedInt(state.timeoutMs, 1_000, 120_000)
  const built = buildActions(state.actions)
  return {
    url: state.url.trim(),
    // Markdown always rides along so the preview has text to show.
    formats: formats.includes('markdown') ? formats : ['markdown', ...formats],
    only_main_content: state.onlyMainContent,
    ...(state.extractor !== 'readability' ? { extractor: state.extractor } : {}),
    ...(state.removeBase64Images ? { remove_base64_images: true } : {}),
    ...(includeTags.length ? { include_tags: includeTags } : {}),
    ...(excludeTags.length ? { exclude_tags: excludeTags } : {}),
    ...(waitMs ? { render: { waitForTimeoutMs: waitMs } } : {}),
    ...(timeoutMs ? { timeout_ms: timeoutMs } : {}),
    ...(built.ok && built.actions.length ? { actions: built.actions, actions_on_error: state.actionsOnError } : {}),
    ...(state.formats.includes('screenshot') || state.formats.includes('pdf')
      ? {
          capture: {
            ...(state.formats.includes('screenshot') ? { screenshot: true } : {}),
            ...(state.formats.includes('pdf') ? { pdf: true } : {}),
          },
        }
      : {}),
    ...(ingest ? { ingest: true } : {}),
  }
}

/** The crawl body for `startCrawl` (`/api/v1/knowledge/crawl` → `/v1/crawl`). */
export function crawlRequestFromState(state: PlaygroundState): CrawlRequest {
  const includePaths = splitList(state.includePaths)
  const excludePaths = splitList(state.excludePaths)
  const maxDepth = boundedInt(state.maxDepth, 1, 10)
  return {
    url: state.url.trim(),
    maxPages: boundedInt(state.maxPages, 1, 50) ?? 10,
    ...(maxDepth ? { maxDepth } : {}),
    ...(includePaths.length ? { includePaths } : {}),
    ...(excludePaths.length ? { excludePaths } : {}),
    ...(state.allowSubdomains ? { allowSubdomains: true } : {}),
    ingest: state.ingest,
  }
}

/** The search filters for `runWebSearch`; an empty field is no filter. */
export function searchFiltersFromState(state: PlaygroundState): WebSearchFilters {
  return {
    timeRange: state.searchTimeRange || null,
    topic: state.searchTopic || null,
    exactMatch: state.searchExact,
    includeDomains: splitList(state.searchIncludeDomains),
    excludeDomains: splitList(state.searchExcludeDomains),
    country: state.searchCountry.trim().toUpperCase() || null,
    language: state.searchLanguage.trim().toLowerCase() || null,
  }
}

export type QuarryRequest = {
  method: 'POST'
  path: string
  body: Record<string, unknown>
  /** Set when the call is not a Quarry request, so the snippet says what it really is. */
  note?: string
}

/** The request the current playground state stands for, in Quarry's own field names. */
export function quarryRequestFor(state: PlaygroundState): QuarryRequest {
  switch (state.endpoint) {
    case 'search': {
      const filters = searchFiltersFromState(state)
      return {
        method: 'POST',
        path: '/v1/search',
        body: {
          query: state.query.trim(),
          limit: boundedInt(state.searchLimit, 1, 20) ?? 5,
          ...(state.searchIncludeAnswer ? { include_answer: true } : {}),
          ...(filters.timeRange ? { time_range: filters.timeRange } : {}),
          ...(filters.topic ? { topic: filters.topic } : {}),
          ...(filters.exactMatch ? { exact_match: true } : {}),
          ...(filters.includeDomains?.length ? { include_domains: filters.includeDomains } : {}),
          ...(filters.excludeDomains?.length ? { exclude_domains: filters.excludeDomains } : {}),
          ...(filters.country ? { country: filters.country } : {}),
          ...(filters.language ? { language: filters.language } : {}),
        },
      }
    }
    case 'map': {
      const search = state.mapSearch.trim()
      return {
        method: 'POST',
        path: '/v1/map',
        body: {
          url: state.url.trim(),
          ...(search ? { search } : {}),
          limit: boundedInt(state.mapLimit, 1, 1000) ?? 100,
          ...(state.allowSubdomains ? { include_subdomains: true } : {}),
        },
      }
    }
    case 'extract': {
      const parsed = parseSchema(state.schema)
      return {
        method: 'POST',
        path: '/v1/extract',
        body: {
          urls: [state.url.trim()],
          ...(parsed.ok && parsed.schema ? { schema: parsed.schema } : {}),
        },
      }
    }
    case 'batch':
      return { method: 'POST', path: '/v1/batch', body: { urls: splitList(state.urls) } }
    case 'crawl': {
      const crawl = crawlRequestFromState(state)
      return {
        method: 'POST',
        path: '/v1/crawl',
        body: {
          url: crawl.url,
          max_pages: crawl.maxPages,
          ...(crawl.maxDepth ? { max_depth: crawl.maxDepth } : {}),
          ...(crawl.includePaths ? { include_paths: crawl.includePaths } : {}),
          ...(crawl.excludePaths ? { exclude_paths: crawl.excludePaths } : {}),
          ...(crawl.allowSubdomains ? { allow_subdomains: true } : {}),
          ...(crawl.ingest ? { ingest: true } : {}),
        },
      }
    }
    case 'scrape': {
      const { capture, ...scrape } = scrapeRequestFromState(state)
      const formats = [...(scrape.formats ?? []), ...(capture?.screenshot ? ['screenshot'] : []), ...(capture?.pdf ? ['pdf'] : [])]
      return { method: 'POST', path: '/v1/scrape', body: { ...scrape, formats } }
    }
    case 'agent':
      return {
        method: 'POST',
        path: '/api/v1/actions/execute',
        note: 'Agentstart går gjennom Verevon-gatewayen, som utsteder nettleserøkten og tillatelsen Quarry krever. Ingen Quarry-token trengs.',
        body: {
          actionId: 'ingestions.start_agent_run',
          input: agentInput(state),
        },
      }
  }
}

/** The input of `ingestions.start_agent_run` for the current state. */
export function agentInput(state: PlaygroundState): Record<string, unknown> {
  const maxSteps = boundedInt(state.maxSteps, 1, 100)
  const allowedDomains = splitList(state.allowedDomains)
  return {
    url: state.url.trim(),
    goal: state.goal.trim(),
    ...(maxSteps ? { maxSteps } : {}),
    ...(allowedDomains.length ? { allowedDomains } : {}),
  }
}

export type SnippetLanguage = 'curl' | 'javascript' | 'python'

const QUARRY_BASE_PLACEHOLDER = '$QUARRY_URL'

/** A copy-paste snippet for `request`, authenticated with a token from the environment. */
export function codeSnippet(language: SnippetLanguage, request: QuarryRequest): string {
  const json = JSON.stringify(request.body, null, 2)
  const base = request.path.startsWith('/api/') ? '$VEREVON_URL' : QUARRY_BASE_PLACEHOLDER
  const token = request.path.startsWith('/api/') ? '$VEREVON_SESSION_COOKIE' : '$QUARRY_TOKEN'
  switch (language) {
    case 'curl':
      return [
        `curl -X ${request.method} "${base}${request.path}" \\`,
        `  -H "Authorization: Bearer ${token}" \\`,
        '  -H "Content-Type: application/json" \\',
        `  -d '${json.replace(/'/g, "'\\''")}'`,
      ].join('\n')
    case 'javascript':
      return [
        `const response = await fetch(\`\${process.env.${base.slice(1)}}${request.path}\`, {`,
        `  method: '${request.method}',`,
        '  headers: {',
        `    Authorization: \`Bearer \${process.env.${token.slice(1)}}\`,`,
        "    'Content-Type': 'application/json',",
        '  },',
        `  body: JSON.stringify(${indent(json, 2)}),`,
        '})',
        'const { data } = await response.json()',
      ].join('\n')
    case 'python':
      return [
        'import os',
        'import requests',
        '',
        'response = requests.post(',
        `    f"{os.environ['${base.slice(1)}']}${request.path}",`,
        `    headers={"Authorization": f"Bearer {os.environ['${token.slice(1)}']}"},`,
        `    json=${indent(pythonLiteral(json), 4)},`,
        ')',
        'data = response.json()["data"]',
      ].join('\n')
  }
}

function indent(text: string, spaces: number): string {
  const pad = ' '.repeat(spaces)
  return text.split('\n').map((line, index) => (index === 0 ? line : pad + line)).join('\n')
}

/** JSON → Python literal: only the three JSON keywords differ. */
function pythonLiteral(json: string): string {
  return json.replace(/("(?:[^"\\]|\\.)*")|\b(true|false|null)\b/g, (_match, quoted: string | undefined, keyword: string | undefined) => {
    if (quoted) return quoted
    if (keyword === 'true') return 'True'
    if (keyword === 'false') return 'False'
    return 'None'
  })
}

/** A starter JSON Schema for extraction, one per template. */
export function extractTemplateSchema(template: ExtractTemplate): string {
  switch (template) {
    case 'product':
      return JSON.stringify(
        {
          type: 'object',
          properties: {
            products: {
              type: 'array',
              items: {
                type: 'object',
                properties: {
                  name: { type: 'string' },
                  price: { type: 'string' },
                  currency: { type: 'string' },
                  availability: { type: 'string' },
                  image: { type: 'string' },
                  rating: { type: 'string' },
                },
              },
            },
          },
        },
        null,
        2,
      )
    case 'article':
      return JSON.stringify(
        {
          type: 'object',
          properties: {
            title: { type: 'string' },
            author: { type: 'string' },
            published_at: { type: 'string' },
            content: { type: 'string' },
            keywords: { type: 'array', items: { type: 'string' } },
          },
        },
        null,
        2,
      )
    case 'general':
      return JSON.stringify(
        {
          type: 'object',
          properties: {
            summary: { type: 'string' },
            topics: { type: 'array', items: { type: 'string' } },
          },
        },
        null,
        2,
      )
  }
}

let actionSeq = 0

/** A new, empty browser step of the given type. */
export function newAction(type: ActionType): PlaygroundAction {
  actionSeq += 1
  return { id: `a${actionSeq}`, type, selector: '', text: '', ms: '1000', key: 'Enter', scroll: 'bottom', pixels: '800', value: '' }
}
