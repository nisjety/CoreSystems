import type { CrawlRequest, ScrapeRequest } from '@/shared/api/knowledge-client'

/**
 * The Innhenting playground (Firecrawl-style): one endpoint at a time, its options,
 * and the exact Quarry v2 request those options produce. The request builder is the
 * single source for both what the page sends through the gateway and the "copy as
 * code" snippet, so the snippet never shows a field the page does not send.
 */
export type PlaygroundEndpoint = 'search' | 'map' | 'scrape' | 'extract' | 'crawl' | 'batch'

export type ScrapeFormat = 'markdown' | 'html' | 'rawHtml' | 'links' | 'screenshot'

export type ExtractTemplate = 'general' | 'product' | 'article'

export type PlaygroundState = {
  endpoint: PlaygroundEndpoint
  url: string
  urls: string
  query: string
  prompt: string
  formats: ScrapeFormat[]
  onlyMainContent: boolean
  includeTags: string
  excludeTags: string
  waitMs: string
  timeoutMs: string
  maxPages: string
  maxDepth: string
  includePaths: string
  excludePaths: string
  allowSubdomains: boolean
  ingest: boolean
  mapSearch: string
  mapLimit: string
  searchLimit: string
}

export const defaultPlaygroundState: PlaygroundState = {
  endpoint: 'scrape',
  url: '',
  urls: '',
  query: '',
  prompt: '',
  formats: ['markdown'],
  onlyMainContent: true,
  includeTags: '',
  excludeTags: '',
  waitMs: '',
  timeoutMs: '',
  maxPages: '10',
  maxDepth: '',
  includePaths: '',
  excludePaths: '',
  allowSubdomains: false,
  ingest: false,
  mapSearch: '',
  mapLimit: '100',
  searchLimit: '5',
}

/** Endpoints grouped the way Firecrawl groups them: discover, extract, crawl. */
export const endpointGroups: Array<{ id: 'discover' | 'extract' | 'crawl'; endpoints: PlaygroundEndpoint[] }> = [
  { id: 'discover', endpoints: ['search', 'map'] },
  { id: 'extract', endpoints: ['scrape', 'extract'] },
  { id: 'crawl', endpoints: ['crawl', 'batch'] },
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

/** What is missing before the endpoint can run, or `null` when it is ready. */
export function missingInput(state: PlaygroundState): 'url' | 'urls' | 'query' | 'prompt' | null {
  switch (state.endpoint) {
    case 'search':
      return state.query.trim() ? null : 'query'
    case 'batch':
      return splitList(state.urls).length > 0 ? null : 'urls'
    case 'extract':
      if (!state.url.trim()) return 'url'
      return state.prompt.trim() ? null : 'prompt'
    default:
      return state.url.trim() ? null : 'url'
  }
}

const QUARRY_FORMAT: Record<Exclude<ScrapeFormat, 'screenshot'>, string> = {
  markdown: 'markdown',
  html: 'html',
  rawHtml: 'rawHtml',
  links: 'links',
}

/** The scrape body for `/api/v1/knowledge/scrape` (the gateway passes it through to `/v1/scrape`). */
export function scrapeRequestFromState(state: PlaygroundState, ingest = false): ScrapeRequest {
  const formats = state.formats.filter((format) => format !== 'screenshot').map((format) => QUARRY_FORMAT[format])
  const includeTags = splitList(state.includeTags)
  const excludeTags = splitList(state.excludeTags)
  const waitMs = boundedInt(state.waitMs, 0, 30_000)
  const timeoutMs = boundedInt(state.timeoutMs, 1_000, 120_000)
  return {
    url: state.url.trim(),
    // Markdown always rides along so the preview has text to show.
    formats: formats.includes('markdown') ? formats : ['markdown', ...formats],
    only_main_content: state.onlyMainContent,
    ...(includeTags.length ? { include_tags: includeTags } : {}),
    ...(excludeTags.length ? { exclude_tags: excludeTags } : {}),
    ...(waitMs ? { render: { waitForTimeoutMs: waitMs } } : {}),
    ...(timeoutMs ? { timeout_ms: timeoutMs } : {}),
    ...(state.formats.includes('screenshot') ? { capture: { screenshot: true } } : {}),
    ...(ingest ? { ingest: true } : {}),
  }
}

/** The crawl body for `startCrawl` (`/api/v1/knowledge/crawl` → `/v1/crawl`). */
export function crawlRequestFromState(state: PlaygroundState): CrawlRequest {
  const includePaths = splitList(state.includePaths)
  const excludePaths = splitList(state.excludePaths)
  return {
    url: state.url.trim(),
    maxPages: boundedInt(state.maxPages, 1, 50) ?? 10,
    ...(boundedInt(state.maxDepth, 1, 10) ? { maxDepth: boundedInt(state.maxDepth, 1, 10) } : {}),
    ...(includePaths.length ? { includePaths } : {}),
    ...(excludePaths.length ? { excludePaths } : {}),
    ...(state.allowSubdomains ? { allowSubdomains: true } : {}),
    ingest: state.ingest,
  }
}

export type QuarryRequest = {
  method: 'POST'
  path: string
  body: Record<string, unknown>
}

/** The Quarry v2 request the current playground state stands for, in Quarry's own field names. */
export function quarryRequestFor(state: PlaygroundState): QuarryRequest {
  switch (state.endpoint) {
    case 'search':
      return {
        method: 'POST',
        path: '/v1/search',
        body: { query: state.query.trim(), limit: boundedInt(state.searchLimit, 1, 20) ?? 5 },
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
    case 'extract':
      return {
        method: 'POST',
        path: '/v1/extract',
        body: { urls: [state.url.trim()], prompt: state.prompt.trim() },
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
        },
      }
    }
    case 'scrape': {
      const { capture, ...scrape } = scrapeRequestFromState(state)
      const formats = [...(scrape.formats ?? []), ...(capture?.screenshot ? ['screenshot'] : [])]
      return { method: 'POST', path: '/v1/scrape', body: { ...scrape, formats } }
    }
  }
}

export type SnippetLanguage = 'curl' | 'javascript' | 'python'

const QUARRY_BASE_PLACEHOLDER = '$QUARRY_URL'

/** A copy-paste snippet for `request`, authenticated with a bearer token from the environment. */
export function codeSnippet(language: SnippetLanguage, request: QuarryRequest): string {
  const json = JSON.stringify(request.body, null, 2)
  switch (language) {
    case 'curl':
      return [
        `curl -X ${request.method} "${QUARRY_BASE_PLACEHOLDER}${request.path}" \\`,
        '  -H "Authorization: Bearer $QUARRY_TOKEN" \\',
        '  -H "Content-Type: application/json" \\',
        `  -d '${json.replace(/'/g, "'\\''")}'`,
      ].join('\n')
    case 'javascript':
      return [
        `const response = await fetch(\`\${process.env.QUARRY_URL}${request.path}\`, {`,
        `  method: '${request.method}',`,
        '  headers: {',
        '    Authorization: `Bearer ${process.env.QUARRY_TOKEN}`,',
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
        `    f"{os.environ['QUARRY_URL']}${request.path}",`,
        "    headers={\"Authorization\": f\"Bearer {os.environ['QUARRY_TOKEN']}\"},",
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

/** Prompt starters for extract; the prompt is the field the gateway forwards. */
export function extractTemplatePrompt(template: ExtractTemplate, tr: (no: string, en: string) => string): string {
  switch (template) {
    case 'product':
      return tr(
        'Trekk ut produktnavn, pris, valuta, tilgjengelighet, bilde-URL og vurdering for hvert produkt på siden.',
        'Extract product name, price, currency, availability, image URL and rating for each product on the page.',
      )
    case 'article':
      return tr(
        'Trekk ut tittel, forfatter, publiseringsdato, ingress og nøkkelord.',
        'Extract the title, author, publish date, lead paragraph and keywords.',
      )
    case 'general':
      return tr(
        'Oppsummer siden og trekk ut sentrale temaer, kontaktkanaler og prissignaler.',
        'Summarize the page and extract key topics, contact channels and pricing signals.',
      )
  }
}
