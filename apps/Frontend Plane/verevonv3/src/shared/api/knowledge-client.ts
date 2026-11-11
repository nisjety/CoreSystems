import { ApiError, requestForm, requestJson } from './http'
import { readSseStream } from './sse'

export interface Source {
  id: string
  name: string
  type: string
  url?: string
  status?: string
  health?: string
  lastSync?: string
  coverage?: string
}

export interface Document {
  id: string
  title: string
  sourceId: string
  kind?: string
  url?: string
  createdAt?: string
  updatedAt?: string
}

export interface WikiPage {
  id: string
  title: string
  path: string
  orgId: string
  excerpt?: string
  updatedAt?: string
}

export interface SearchRequest {
  query: string
  limit?: number
  kinds?: string[]
}

export interface SearchHit {
  id: string
  title: string
  excerpt: string
  score: number
  kind: string
  sourceId?: string
  path?: string
}

export interface SearchResult {
  results: SearchHit[]
  total: number
}

export interface ImportJob {
  id: string
  status: 'pending' | 'running' | 'completed' | 'failed'
  orgId?: string
  sourceType?: string
  progress?: number
  error?: string
  createdAt?: string
}

export type ConnectorSourceType =
  | 'notion'
  | 'crm'
  | 'erp'
  | 'cms'
  | 'pim'
  | 'hubspot'
  | 'salesforce'
  | 'odoo'

/** imports-core connector import. The source-job endpoint is connector-only — it
 * pulls documents from a connected SaaS source (Notion/HubSpot/…), not arbitrary
 * URLs (those go through quarry scrape/crawl). */
export interface ImportSourceRequest {
  sourceType: ConnectorSourceType
  connection?: Record<string, unknown>
  options?: Record<string, unknown>
}

export interface CrawlJob {
  acceptedAt?: string
  eventStream?: string
  id: string
  status: string
  sourceId?: string
  orgId?: string
  createdAt?: string
  /** WS14.S6/S7: the scope the crawl runs with, when the handoff echoed one (`effective`). */
  effective?: CrawlEffectiveScope
}

export interface ScrapeRequest {
  url: string
  render?: Record<string, unknown>
  signals?: Record<string, unknown>
  /** Output formats quarry-edge should return. The gateway adds `['markdown']` when absent so the
   * text comes back inline in `data.markdown` instead of as an artifact reference. */
  /** Plain format names, or objects: `{type:'json',schema}`, `{type:'summary'}`, `{type:'question',question}`, `{type:'highlights',query}` (INNH-1). */
  formats?: Array<string | Record<string, unknown>>
  only_main_content?: boolean
  include_tags?: string[]
  exclude_tags?: string[]
  /** Upper bound for the whole fetch; quarry-edge applies its own default when absent. */
  timeout_ms?: number
  /** Content extractor: main content (default), the whole body without scripts, or the raw document. */
  extractor?: 'readability' | 'clean_full' | 'raw'
  /** Drop inline base64 images from the markdown. */
  remove_base64_images?: boolean
  /** Browser steps before the capture (`wait`, `click`, `write`, `press`, `scroll`, `select`). Needs the edge's scrape-actions switch. */
  actions?: Array<Record<string, unknown>>
  /** What a failed step does: stop (`abort`, the edge's default) or go on (`continue`). */
  actions_on_error?: 'abort' | 'continue'
  /** When true, quarry-edge persists the fetched page into the knowledge base
   * (not just returns it). Omit/false for preview-only scrapes. */
  ingest?: boolean
  /** WS13.7: screenshot and PDF captures. Sent as `formats` entries on `/scrape` only; the cached preview never sends them. */
  capture?: ScrapeCaptureRequest
  /** Freshness in milliseconds: a stored result this old or newer is served without a fetch. 0 never reads the cache.
   * Quarry answers 400 `feature_disabled` while `QUARRY_EDGE__SCRAPE_MAX_AGE` is off. */
  max_age?: number
}

export interface CrawlRequest {
  url: string
  maxPages?: number
  /** Phase 6 selective ingest: true = persist+embed; omitted = working-set only (default never). */
  ingest?: boolean
  /** WS14.S6: link depth from the seed (the gateway caps it at 10). */
  maxDepth?: number
  /** WS14.S6: patterns a link must match (glob, or `re:` regular expression). */
  includePaths?: string[]
  /** WS14.S6: patterns that deny a link; they beat `includePaths`. */
  excludePaths?: string[]
  /** WS14.S3: follow links to subdomains of the seed host. Omitted = false. */
  allowSubdomains?: boolean
  /** WS14.S3: follow links to unrelated hosts. UI only; the gateway drops it for a Model proposal. */
  allowExternalLinks?: boolean
  /** WS14.S7: ignore the site's robots.txt. Sent only as `true`; UI only (the gateway forwards it on the
   * UI surface behind VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE and drops it for a Model proposal), and the
   * edge allows it only for a person whose org's effective policy permits the override. */
  ignoreRobotsTxt?: boolean
  /** Sitemap discovery: `skip` (default), `include` (sitemap URLs join the links) or `only` (sitemap URLs alone). */
  sitemap?: 'skip' | 'include' | 'only'
  /** Cleaning for every page of the crawl, as on `/scrape`. The inner keys are snake_case. */
  scrapeOptions?: CrawlPageScrapeOptions
}

/** The per-page subset of `/scrape` options a crawl accepts (INNH-6). */
export interface CrawlPageScrapeOptions {
  formats?: Array<'markdown' | 'html' | 'rawHtml' | 'links' | 'metadata'>
  only_main_content?: boolean
  include_tags?: string[]
  exclude_tags?: string[]
  remove_base64_images?: boolean
  extractor?: 'readability' | 'clean_full' | 'raw'
  timeout_ms?: number
}

/** WS14.S6: the scope a crawl runs with, echoed by the handoff (`effective`). */
export interface CrawlEffectiveScope {
  scopeMode: string
  robots: string
  maxPages: number
  maxDepth: number
  legacy: boolean
  warnings: string[]
  requestWarnings: { code: string; field: string }[]
  /** WS14.S7: the authority that let this crawl ignore robots.txt (`effective.robots_override.authority`,
   * e.g. `web.robots_override`); absent when robots.txt is followed. */
  robotsOverride?: string
}

/** A single page surfaced by `/crawl/discover` (quarry `/v1/map`). */
export interface DiscoveredPage {
  url: string
  title?: string
  score?: number
}

export interface CrawlDiscovery {
  url: string
  pages: DiscoveredPage[]
  count: number
}

export interface DiscoverPagesRequest {
  url: string
  search?: string
  limit?: number
  includeSubdomains?: boolean
}

export interface ScrapeMetadata {
  title?: string
  description?: string
  url?: string
  sourceURL?: string
  source_url?: string
}

/** quarry-edge `/v1/scrape` payload. Fields may arrive top-level or under `data`
 * depending on the upstream envelope — callers should read both defensively. */
export interface ScrapeResult {
  content?: string
  formats?: Record<string, unknown>
  html?: string
  markdown?: string
  metadata?: ScrapeMetadata
  raw_html?: string
  text?: string
  url?: string | { final?: string; final_url?: string; requested?: string }
  /** WS13.7: inline screenshot (ZDR only), or absent. */
  screenshot?: unknown
  /** WS13.7: inline PDF (ZDR only), or absent. */
  pdf?: unknown
  /** WS13.7: what the captures measured, counts only. */
  captures?: unknown
  data?: {
    content?: string
    formats?: Record<string, unknown>
    html?: string
    markdown?: string
    metadata?: ScrapeMetadata
    raw_html?: string
    text?: string
    url?: string | { final?: string; final_url?: string; requested?: string }
  }
}

export interface ScrapePreviewResult {
  description: string
  markdown: string
  quarry?: {
    fingerprint?: string
    htmlArtifactId?: string
    markdownArtifactId?: string
    runId?: string
    status?: number
  }
  source: 'artifact' | 'empty' | 'extract' | 'inline'
  title: string
  url: string
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-verevon-org-id': orgId }
}

type DataPlaneDocument = {
  created_at?: string
  document_id?: string
  id?: string
  source?: string
  title?: string
  type?: string
  updated_at?: string
}

function toDocument(document: DataPlaneDocument): Document {
  return {
    id: document.document_id ?? document.id ?? '',
    title: document.title ?? 'Untitled',
    sourceId: document.source ?? '',
    kind: document.type,
    createdAt: document.created_at,
    updatedAt: document.updated_at,
  }
}

type DataPlaneWikiPage = {
  org_id?: string
  page_id?: string
  path?: string
  title?: string
  updated_at?: string
}

type DataPlaneWikiVersion = {
  content?: string
}

function toWikiPage(page: DataPlaneWikiPage, version?: DataPlaneWikiVersion | null): WikiPage {
  return {
    id: page.page_id ?? '',
    title: page.title ?? 'Untitled',
    path: page.path ?? '',
    orgId: page.org_id ?? '',
    excerpt: version?.content,
    updatedAt: page.updated_at,
  }
}

// ── Documents ─────────────────────────────────────────────────────────────────

export async function listDocuments(orgId: string, signal?: AbortSignal): Promise<Document[]> {
  const payload = await requestJson<{ documents?: DataPlaneDocument[] }>('/api/v1/knowledge/documents', {
    headers: orgHeaders(orgId),
    signal,
  })
  return (payload.documents ?? []).map(toDocument)
}

export async function getDocument(orgId: string, id: string, signal?: AbortSignal): Promise<Document> {
  const document = await requestJson<DataPlaneDocument>(`/api/v1/knowledge/documents/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  return toDocument(document)
}

/** Flat document-summary list for pickers/typeaheads. The rich workspace payload
 * lives at `/api/v1/knowledge/sources` (see knowledge-live-client); this hits the
 * documents-api passthrough at `/source-list`, which returns `{ sources, total }`. */
export async function listSources(orgId: string, signal?: AbortSignal): Promise<Source[]> {
  const payload = await requestJson<{
    documents?: Array<{
      document_id?: string
      id?: string
      title?: string
      name?: string
      source?: string
      type?: string
      status?: string
    }>
  }>('/api/v1/knowledge/source-list', {
    headers: orgHeaders(orgId),
    signal,
  })
  return (payload.documents ?? []).map((entry) => ({
    id: entry.document_id ?? entry.id ?? '',
    name: entry.title ?? entry.name ?? '',
    type: entry.type ?? 'document',
    status: entry.status,
  }))
}

// ── Retrieval ─────────────────────────────────────────────────────────────────

export async function searchKnowledge(
  orgId: string,
  request: SearchRequest,
  signal?: AbortSignal,
): Promise<SearchResult> {
  return requestJson<SearchResult>('/api/v1/knowledge/search', {
    method: 'POST',
    body: JSON.stringify(request),
    headers: orgHeaders(orgId),
    signal,
  })
}

// ── Wiki ──────────────────────────────────────────────────────────────────────

export async function listWikiPages(orgId: string, signal?: AbortSignal): Promise<WikiPage[]> {
  const payload = await requestJson<{ pages?: DataPlaneWikiPage[] }>('/api/v1/knowledge/wiki/pages', {
    headers: orgHeaders(orgId),
    signal,
  })
  return (payload.pages ?? []).map((page) => toWikiPage(page))
}

export async function getWikiPageByPath(
  orgId: string,
  path: string,
  signal?: AbortSignal,
): Promise<WikiPage> {
  const qs = new URLSearchParams({ path })
  const payload = await requestJson<{ page: DataPlaneWikiPage; version?: DataPlaneWikiVersion | null }>(`/api/v1/knowledge/wiki/pages/by-path?${qs}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  return toWikiPage(payload.page, payload.version)
}

export async function getWikiPage(orgId: string, id: string, signal?: AbortSignal): Promise<WikiPage> {
  const payload = await requestJson<{ page: DataPlaneWikiPage; version?: DataPlaneWikiVersion | null }>(`/api/v1/knowledge/wiki/pages/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  return toWikiPage(payload.page, payload.version)
}

// ── Imports ───────────────────────────────────────────────────────────────────

export async function importSource(
  orgId: string,
  body: ImportSourceRequest,
  signal?: AbortSignal,
): Promise<ImportJob> {
  // imports-core validates a snake_case SourceImportRequest carrying org_id +
  // source_type in the body; the gateway also forwards org_id as x-org-id (auth).
  return requestJson<ImportJob>('/api/v1/knowledge/imports/source', {
    method: 'POST',
    body: JSON.stringify({
      org_id: orgId,
      source_type: body.sourceType,
      connection: body.connection ?? {},
      options: body.options ?? {},
    }),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function getImportJob(orgId: string, id: string, signal?: AbortSignal): Promise<ImportJob> {
  return requestJson<ImportJob>(`/api/v1/knowledge/imports/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function importUpload(
  orgId: string,
  files: File[],
  options?: { signal?: AbortSignal; zdr?: boolean },
): Promise<ImportJob> {
  const form = new FormData()
  for (const file of files) {
    form.append('files', file, file.name)
  }
  // Imports create durable Data Plane documents. Sending the explicit ZDR
  // field makes a temporary-chat attempt fail closed at imports-core instead
  // of relying on a client-only guard.
  if (options?.zdr !== undefined) form.append('zdr', String(options.zdr))
  return requestForm<ImportJob>('/api/v1/knowledge/imports/upload', form, {
    headers: orgHeaders(orgId),
    signal: options?.signal,
  })
}

// ── Quarry ────────────────────────────────────────────────────────────────────

/** WS13.7: a screenshot request. `true` asks for the defaults. */
export interface ScreenshotOptions {
  fullPage?: boolean
  quality?: number
}

/** WS13.7: captures to take with a scrape. */
export interface ScrapeCaptureRequest {
  screenshot?: boolean | ScreenshotOptions
  pdf?: boolean
}

/** WS13.7: a scrape's captures as the SPA reads them. `screenshotUrl` and `pdfUrl` are
 * gateway artifact routes; `screenshotInline` is ZDR only (the bytes come back in the
 * response and nothing is stored). */
export interface ScrapeCaptureView {
  pdfUrl?: string
  screenshotInline?: { contentType: string; dataBase64: string }
  screenshotUrl?: string
}

/** WS13.7: the gateway route that serves one scrape artifact (screenshot or PDF). */
export function scrapeArtifactUrl(artifactId: string): string {
  return `/api/v1/knowledge/scrape-artifacts/${encodeURIComponent(artifactId)}`
}

/** The `formats` entries for a capture request; empty when nothing is asked. */
export function captureFormats(
  capture: ScrapeCaptureRequest | undefined,
): Array<string | Record<string, unknown>> {
  const formats: Array<string | Record<string, unknown>> = []
  if (capture?.screenshot) {
    formats.push(
      typeof capture.screenshot === 'object' ? { type: 'screenshot', ...capture.screenshot } : 'screenshot',
    )
  }
  if (capture?.pdf) formats.push('pdf')
  return formats
}

/** The wire body for `/v1/knowledge/scrape`: `capture` becomes `formats` entries. A
 * capture request keeps `markdown` so the text still comes back inline. */
export function scrapeRequestBody(body: ScrapeRequest): Record<string, unknown> {
  const { capture, ...rest } = body
  const extra = captureFormats(capture)
  if (extra.length === 0) return rest as Record<string, unknown>
  return { ...rest, formats: [...(rest.formats ?? ['markdown']), ...extra] }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Read the captures of a scrape result. The fields may sit at the top level or under
 * `data`, so both are read; a value that is not the expected shape is ignored.
 * `undefined` when the response carries no capture. */
export function readScrapeCaptures(result: unknown): ScrapeCaptureView | undefined {
  if (!isRecord(result)) return undefined
  const data = isRecord(result.data) ? result.data : {}
  const pick = (key: string): unknown => result[key] ?? data[key]
  const view: ScrapeCaptureView = {}
  const inline = pick('screenshot')
  if (isRecord(inline) && typeof inline.data_base64 === 'string' && typeof inline.content_type === 'string') {
    view.screenshotInline = { contentType: inline.content_type, dataBase64: inline.data_base64 }
  }
  const formatsValue = pick('formats')
  const formats = isRecord(formatsValue) ? formatsValue : {}
  const artifactRef = (key: string): string | undefined => {
    const value = formats[key]
    return typeof value === 'string' && /^art_[A-Za-z0-9_-]+$/.test(value) ? value : undefined
  }
  const screenshotRef = artifactRef('screenshot')
  if (screenshotRef) view.screenshotUrl = scrapeArtifactUrl(screenshotRef)
  const pdfRef = artifactRef('pdf')
  if (pdfRef) view.pdfUrl = scrapeArtifactUrl(pdfRef)
  return view.screenshotInline || view.screenshotUrl || view.pdfUrl ? view : undefined
}

export async function scrapeUrl(
  orgId: string,
  body: ScrapeRequest,
  signal?: AbortSignal,
): Promise<ScrapeResult> {
  return requestJson<ScrapeResult>('/api/v1/knowledge/scrape', {
    method: 'POST',
    body: JSON.stringify(scrapeRequestBody(body)),
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * A failed preview carries a typed `error.code` (`invalid_url`, `site_blocked` with a
 * `blockReason`, `scrape_refused`, `scrape_rate_limited` with `retryAfterSeconds`,
 * `scrape_timeout`, `scrape_failed`). The caller shows it through `browserErrorMessage`,
 * which looks the code up in `shared/i18n/errors.ts`; a test reads the gateway to pin
 * the set, so there is no list of the codes here to keep in step.
 */
export async function scrapePreview(
  orgId: string,
  body: ScrapeRequest,
  signal?: AbortSignal,
): Promise<ScrapePreviewResult> {
  return requestJson<ScrapePreviewResult>('/api/v1/knowledge/scrape-preview', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

export interface CreateDocumentRequest {
  title: string
  content: string
  sourceUrl?: string
  type?: string
}

export interface CreateDocumentResult {
  document_id?: string
  status?: string
}

/** Ingest a curated document (a user-selected subset of a scraped page) directly
 * into Data Plane v2. The gateway fills source/zdr/org scoping server-side. */
export async function createDocument(
  orgId: string,
  body: CreateDocumentRequest,
  signal?: AbortSignal,
): Promise<CreateDocumentResult> {
  return requestJson<CreateDocumentResult>('/api/v1/knowledge/documents', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * Remove one document from the knowledge base.
 *
 * The knowledge base was append-only from the product's side until this
 * existed: a chat attachment is ingested as a durable, org-wide document, and
 * nothing in the UI could take it back out again — a demo source pack uploaded
 * once kept surfacing in unrelated conversations' knowledge search.
 */
export async function deleteDocument(orgId: string, id: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(`/api/v1/knowledge/documents/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    headers: orgHeaders(orgId),
    signal,
  })
}

/** W2-U65-51: the edge caps the crawl scope options; the action schema and these helpers agree. */
export const CRAWL_PATTERN_MAX_LENGTH = 512
export const CRAWL_PATTERN_MAX_COUNT = 100
export const CRAWL_MAX_DEPTH = 10

/** W2-U65-51: build flag for the crawl scope options and the scrape captures in the composer.
 * Off unless `VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED=true`; off, the composer sends what it sent before. */
export function knowledgeScrapeOptionsEnabled(): boolean {
  return (import.meta.env as Record<string, string | undefined>).VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED === 'true'
}

/** Split a pattern box into patterns: one per line, or comma separated (a comma inside
 * `{a,b}` braces stays part of its glob). Blank, over-long and repeated entries are dropped
 * and the list is capped, the same bounds the edge enforces. */
export function parseCrawlPatterns(text: string): string[] {
  const parts: string[] = []
  for (const line of text.split(/\r?\n/)) {
    let depth = 0
    let current = ''
    for (const char of line) {
      if (char === '{') depth += 1
      else if (char === '}') depth = Math.max(0, depth - 1)
      if (char === ',' && depth === 0) {
        parts.push(current)
        current = ''
      } else {
        current += char
      }
    }
    parts.push(current)
  }
  const seen = new Set<string>()
  for (const part of parts) {
    const pattern = part.trim()
    if (pattern.length === 0 || pattern.length > CRAWL_PATTERN_MAX_LENGTH) continue
    seen.add(pattern)
    if (seen.size >= CRAWL_PATTERN_MAX_COUNT) break
  }
  return [...seen]
}

/** The crawl scope options the user chose. A field the user left alone is absent, so the
 * edge applies its own default; `allowSubdomains` and `allowExternalLinks` are sent only when ticked. */
export type CrawlScopeOptions = Pick<
  CrawlRequest,
  'maxDepth' | 'includePaths' | 'excludePaths' | 'allowSubdomains' | 'allowExternalLinks'
>

export function buildCrawlScopeOptions(input: {
  maxDepth?: number
  includeText?: string
  excludeText?: string
  allowSubdomains?: boolean
  allowExternalLinks?: boolean
}): CrawlScopeOptions {
  const include = parseCrawlPatterns(input.includeText ?? '')
  const exclude = parseCrawlPatterns(input.excludeText ?? '')
  const depth = input.maxDepth
  return {
    ...(depth !== undefined && Number.isFinite(depth) && depth >= 1
      ? { maxDepth: Math.min(CRAWL_MAX_DEPTH, Math.round(depth)) }
      : {}),
    ...(include.length > 0 ? { includePaths: include } : {}),
    ...(exclude.length > 0 ? { excludePaths: exclude } : {}),
    ...(input.allowSubdomains === true ? { allowSubdomains: true } : {}),
    ...(input.allowExternalLinks === true ? { allowExternalLinks: true } : {}),
  }
}

/** The captures the user ticked. `undefined` when none: a capture is never sent as `false`. */
export function buildCaptureRequest(input: { screenshot?: boolean; pdf?: boolean }): ScrapeCaptureRequest | undefined {
  const capture: ScrapeCaptureRequest = {
    ...(input.screenshot === true ? { screenshot: true } : {}),
    ...(input.pdf === true ? { pdf: true } : {}),
  }
  return capture.screenshot || capture.pdf ? capture : undefined
}

export async function startCrawl(
  orgId: string,
  body: CrawlRequest,
  signal?: AbortSignal,
): Promise<CrawlJob> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/crawl', {
    method: 'POST',
    body: JSON.stringify({
      url: body.url,
      ...(body.maxPages ? { maxPages: body.maxPages } : {}),
      // Phase 6 selective ingest: forward the resolved decision so the gateway
      // gates persistence (omitted → quarry default NEVER = working-set only).
      ...(body.ingest === undefined ? {} : { ingest: body.ingest }),
      // WS14.S6: the scope options, sent only when the caller set them.
      ...(body.maxDepth ? { maxDepth: body.maxDepth } : {}),
      ...(body.includePaths?.length ? { includePaths: body.includePaths } : {}),
      ...(body.excludePaths?.length ? { excludePaths: body.excludePaths } : {}),
      ...(body.allowSubdomains === undefined ? {} : { allowSubdomains: body.allowSubdomains }),
      ...(body.allowExternalLinks === undefined ? {} : { allowExternalLinks: body.allowExternalLinks }),
      // WS14.S7: never sent as `false`; an untouched crawl follows robots.txt.
      ...(body.ignoreRobotsTxt === true ? { ignoreRobotsTxt: true } : {}),
      ...(body.sitemap ? { sitemap: body.sitemap } : {}),
      ...(body.scrapeOptions && Object.keys(body.scrapeOptions).length ? { scrapeOptions: body.scrapeOptions } : {}),
    }),
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlJob(raw)
}

/** Discover the pages of a site so the user can pick a subset to crawl. Proxies
 * quarry `/v1/map` (read-only) via the gateway; no ingest happens here. */
export async function discoverCrawlPages(
  orgId: string,
  body: DiscoverPagesRequest,
  signal?: AbortSignal,
): Promise<CrawlDiscovery> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/crawl/discover', {
    method: 'POST',
    body: JSON.stringify({
      url: body.url,
      ...(body.search ? { search: body.search } : {}),
      ...(body.limit ? { limit: body.limit } : {}),
      ...(body.includeSubdomains ? { includeSubdomains: true } : {}),
    }),
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlDiscovery(raw)
}

/** Crawl exactly the pages the user selected. The gateway hands the list to
 * quarry's durable `/v1/batch`, which ingests only those URLs and emits the same
 * run-event stream as a whole-site crawl — so live progress is scoped to the
 * selection. Returns the run, ready for {@link streamCrawlRunEvents}.
 *
 * `ingest` mirrors {@link startCrawl}'s flag: the resolved crawl_ingest_mode
 * decision (auto→true / never→false / prompt→user's answer). Previously this
 * was never forwarded at all, so the "Velg sider" picker could never persist
 * to the knowledge base regardless of the user's preference — quarry's
 * `/v1/batch` defaults `ingest` to NEVER when the field is absent. */
export async function crawlSelectedPages(
  orgId: string,
  urls: string[],
  ingest?: boolean,
  signal?: AbortSignal,
): Promise<CrawlJob> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/crawl', {
    method: 'POST',
    body: JSON.stringify({ urls, ...(ingest === undefined ? {} : { ingest }) }),
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlJob(raw)
}

/** A product extracted from a listing page by the Model Plane. */
export interface Product {
  name: string
  price?: string
  currency?: string
  image?: string
  url?: string
  specs?: string[]
  description?: string
}

export interface ProductExtraction {
  url: string
  source: string
  count: number
  products: Product[]
}

/** Render a listing page (basic → stealth-proxy escalation) and extract its
 * products as structured data. Works on cooperative sites; protected retailers
 * need a configured proxy key (gateway `enhanced_fetch`). */
export async function extractProducts(
  orgId: string,
  body: { url: string; prompt?: string },
  signal?: AbortSignal,
): Promise<ProductExtraction> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/scrape/products', {
    method: 'POST',
    body: JSON.stringify({ url: body.url, ...(body.prompt ? { prompt: body.prompt } : {}) }),
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeProductExtraction(raw)
}

/** Summarize a chosen subset of products into a markdown brief via the Model Plane. */
export async function summarizeProducts(
  orgId: string,
  products: Product[],
  prompt?: string,
  signal?: AbortSignal,
): Promise<string> {
  const raw = await requestJson<{ summary?: string }>('/api/v1/knowledge/scrape/products/summary', {
    method: 'POST',
    body: JSON.stringify({ products, ...(prompt ? { prompt } : {}) }),
    headers: orgHeaders(orgId),
    signal,
  })
  const record = asRecord(raw)
  return (record && typeof record.summary === 'string' ? record.summary : '').trim()
}

function normalizeProductExtraction(raw: unknown): ProductExtraction {
  const root = asRecord(raw) ?? {}
  const source = asRecord(root.data) ?? root
  const list = Array.isArray(source.products) ? source.products : []
  const products = list
    .map(normalizeProduct)
    .filter((product): product is Product => product !== null)
  return {
    url: stringField(source, 'url') ?? '',
    source: stringField(source, 'source') ?? 'basic',
    count: numberField(source, 'count') ?? products.length,
    products,
  }
}

function normalizeProduct(raw: unknown): Product | null {
  const record = asRecord(raw)
  if (!record) return null
  const name = stringField(record, 'name')
  if (!name) return null
  const specs = Array.isArray(record.specs)
    ? record.specs.filter((s): s is string => typeof s === 'string' && s.trim().length > 0)
    : undefined
  return {
    name,
    ...(stringField(record, 'price') ? { price: stringField(record, 'price') } : {}),
    ...(stringField(record, 'currency') ? { currency: stringField(record, 'currency') } : {}),
    ...(stringField(record, 'image') ? { image: stringField(record, 'image') } : {}),
    ...(stringField(record, 'url') ? { url: stringField(record, 'url') } : {}),
    ...(specs && specs.length > 0 ? { specs } : {}),
    ...(stringField(record, 'description') ? { description: stringField(record, 'description') } : {}),
  }
}

export async function listCrawlJobs(orgId: string, signal?: AbortSignal): Promise<CrawlJob[]> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/crawl/jobs', {
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlJobs(raw)
}

export type CrawlWorkflowEvent = {
  detail: string
  event?: string
  progress?: number
  status?: string
  /** Pages successfully fetched so far / at completion. Read from a
   * `run_completed` / `run_*` event's `pages_visited` (or the canonical
   * JobHistoryEvent `completed`). Drives the "N pages" UI count. */
  pagesVisited?: number
  /** Pages that failed during the crawl, when reported. */
  pagesFailed?: number
  /** Best-estimate total work, when the producer can compute it. */
  total?: number
  /** WS14.S3: pages the edge refused (robots or scope), from `run_*` payloads. */
  pagesBlocked?: number
  /** WS14.S4: the part of `pagesBlocked` the edge refused under robots.txt (`pages_blocked_robots`). */
  pagesBlockedByRobots?: number
  /** WS14.S3: links the crawl scope refused, from `run_*` payloads. */
  linksDenied?: number
  /** WS14.S3: denial counts by code (`denials.counts`), from `run_*` payloads. */
  denialCounts?: Record<string, number>
}

export type CrawlWorkflowHandlers = {
  onDone?: () => void
  onError?: (message: string) => void
  onEvent?: (event: CrawlWorkflowEvent) => void
}

/** WS15.L5: a crawl's event stream asks for the live tail (`follow=true`). A flagged edge then keeps the
 * stream open until the job is over; an edge without the flag drains what the job has and ends. */
function withFollow(path: string): string {
  if (/[?&]follow=/.test(path)) return path
  return `${path}${path.includes('?') ? '&' : '?'}follow=true`
}

/** How one connection of {@link streamCrawlRunEvents} ended. */
interface CrawlStreamAttempt {
  /** The stream reached its end: the `done` marker, or a terminal status. */
  finished: boolean
  /** The edge closed the stream with `reconnect` (its lifetime ran out): resume from the cursor. */
  reconnect: boolean
  /** The connection failed, or an `error` frame ended it. */
  failed: boolean
  /** The connection delivered at least one event (control frames do not count). */
  progressed: boolean
  error?: unknown
}

/** The `seq` of a frame's JSON envelope: the cursor of a frame that carries no SSE `id`. */
function frameSeq(data: string): string | undefined {
  try {
    const record = asRecord(JSON.parse(data) as unknown)
    const seq = record ? numberField(record, 'seq') : undefined
    return seq === undefined ? undefined : String(seq)
  } catch {
    return undefined
  }
}

/** The edge answers 403 to `follow=true` from a zero-retention caller: its live events name page URLs. */
function isLiveTailRefusal(error: unknown): boolean {
  return error instanceof ApiError && error.status === 403
}

export async function streamCrawlRunEvents(
  orgId: string,
  runId: string,
  handlers: CrawlWorkflowHandlers,
  signal?: AbortSignal,
  // Server-provided event-stream path (from the crawl handoff's
  // `eventStream`). Honor it verbatim — the gateway now points it at the
  // job-id-keyed `/jobs/{id}/events` SSE route (the run-id is not assigned
  // synchronously at handoff). Falls back to the same job-events path built
  // from the id we were given, NOT the old `/runs/{id}/events` JSON route
  // that the SSE reader could not parse (the 0-pages bug).
  streamPath?: string,
): Promise<void> {
  const base = streamPath && streamPath.trim()
    ? streamPath
    : `/api/v1/knowledge/jobs/${encodeURIComponent(runId)}/events`
  // The cursor: the last frame's id (or envelope seq), sent as Last-Event-ID on a reconnect. `liveTail`
  // drops to false when the edge refuses the live tail, and the stream then reads the drain instead.
  const cursor: { lastEventId?: string; liveTail: boolean; retriesLeft: number } = { liveTail: true, retriesLeft: 1 }
  try {
    for (;;) {
      const attempt: CrawlStreamAttempt = { finished: false, reconnect: false, failed: false, progressed: false }
      await readSseStream(
        cursor.liveTail ? withFollow(base) : base,
        { headers: orgHeaders(orgId), signal, lastEventId: cursor.lastEventId },
        (event) => {
          if (event.id !== undefined) cursor.lastEventId = event.id || undefined
          else cursor.lastEventId = frameSeq(event.data) ?? cursor.lastEventId
          // Control frames, not progress: the snapshot that opens a live tail and the audit of a control request.
          if (event.event === 'snapshot' || event.event === 'control_requested') return
          if (event.event === 'reconnect') {
            attempt.reconnect = true
            return
          }
          if (event.event === 'error') {
            attempt.failed = true
            return
          }
          // The end marker (`data: done`). A frame named `done` carries the same marker.
          if (event.data.trim() === 'done' && (event.event === undefined || event.event === 'done')) {
            attempt.finished = true
            return
          }
          const parsed = normalizeCrawlEvent(event.event, event.data)
          attempt.progressed = true
          handlers.onEvent?.(parsed)
          if (parsed.status && isTerminalCrawlStatus(parsed.status)) attempt.finished = true
        },
        (err) => {
          attempt.failed = true
          attempt.error = err
        },
      )
      if (signal?.aborted || attempt.finished) return
      // The edge ended its stream on purpose (its lifetime ran out): resume from the cursor, which is no failure.
      if (attempt.reconnect) continue
      if (attempt.failed && cursor.liveTail && isLiveTailRefusal(attempt.error)) {
        cursor.liveTail = false
        continue
      }
      // A dropped stream gets one reconnect from the cursor before it counts as a failure. A connection that
      // delivered events has made progress, so its drop earns a fresh reconnect; one that delivered none does not.
      if (attempt.progressed) cursor.retriesLeft = 1
      if (cursor.retriesLeft > 0) {
        cursor.retriesLeft -= 1
        continue
      }
      handlers.onError?.(
        attempt.failed
          ? (attempt.error instanceof Error ? attempt.error.message : 'Crawl event stream failed.')
          : 'Crawl event stream ended before the job finished.',
      )
      return
    }
  } finally {
    handlers.onDone?.()
  }
}

/** WS15.L5: one crawl's status document. A plain read: no `wait` or `resume` query. */
export async function getCrawlStatus(orgId: string, id: string, signal?: AbortSignal): Promise<CrawlJobStatus> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/crawl/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlStatus(raw)
}

/** WS15.L5: the failed and blocked pages of a crawl, one page at a time. `afterSeq` is the `nextSeq` of the
 * page before; `limit` caps the page (the edge's default is 100, its ceiling 500). A zero-retention caller
 * is refused, because the rows name page URLs. */
export async function getCrawlErrors(
  orgId: string,
  id: string,
  options: { limit?: number; afterSeq?: number } = {},
  signal?: AbortSignal,
): Promise<CrawlErrorsPage> {
  const query = new URLSearchParams()
  if (options.afterSeq !== undefined) query.set('after_seq', String(options.afterSeq))
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  const suffix = query.toString()
  const raw = await requestJson<unknown>(
    `/api/v1/knowledge/crawl/${encodeURIComponent(id)}/errors${suffix ? `?${suffix}` : ''}`,
    { headers: orgHeaders(orgId), signal },
  )
  return normalizeCrawlErrors(raw)
}

/** WS15.L5: pause a running crawl. Answers `202` while the orchestrator has yet to apply it, `200` once it has;
 * `409` when the job is not running. The gateway answers `501` while the edge's job control is off. */
export async function pauseCrawl(orgId: string, id: string, signal?: AbortSignal): Promise<CrawlControlResult> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/crawl/${encodeURIComponent(id)}/pause`, {
    method: 'POST',
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlControl(raw)
}

/** WS15.L5: resume a paused crawl (`409` when the job is not paused). Same answers as {@link pauseCrawl}. */
export async function resumeCrawl(orgId: string, id: string, signal?: AbortSignal): Promise<CrawlControlResult> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/crawl/${encodeURIComponent(id)}/resume`, {
    method: 'POST',
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlControl(raw)
}

/** WS15.L5: stop a crawl for good. Irreversible: the caller confirms first. A cancelled crawl cannot be resumed;
 * the pages already stored are kept. Same answers as {@link pauseCrawl}. */
export async function cancelCrawl(orgId: string, id: string, signal?: AbortSignal): Promise<CrawlControlResult> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/crawl/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeCrawlControl(raw)
}

// ── Crawl job control (WS15.L5) ───────────────────────────────────────────────
// The status, errors and control verbs of one crawl. The gateway proxies them to the edge's
// `/v1/crawl/{id}` family; the control verbs answer 501 while the edge's job control is off.

/** The counts of one crawl, read from its event log. */
export interface CrawlJobCounts {
  completed: number
  failed: number
  blocked: number
  retried: number
}

/** A non-fatal note about a crawl. `code` comes from a closed vocabulary on the edge. */
export interface CrawlJobWarning {
  code: string
  message: string
}

/** What a crawl has been asked to do next, and whether the orchestrator has yet to carry it out. */
export interface CrawlControlState {
  /** `pause`, `resume` or `cancel`: the state the person asked for. */
  desired?: string
  /** True until the orchestrator applies the request. */
  pending: boolean
}

/** One crawl's status document (`GET /api/v1/knowledge/crawl/{id}`). */
export interface CrawlJobStatus {
  jobId: string
  kind: string
  /** `accepted`, `running` or `paused` while the job is live; `completed`, `failed`, `cancelled`, `timed_out` or `skipped` once it is over. */
  status: string
  /** What a person reads: `queued`, `running`, `pausing`, `paused`, `resuming`, `cancelling`, then the terminal name. */
  phase: string
  /** True once the status is final; the job takes no further transition. */
  terminal: boolean
  counts: CrawlJobCounts
  /** The page total: the exact URL count of a batch, or the page ceiling of a crawl (see `totalKind`). */
  total?: number
  totalKind?: string
  warnings: CrawlJobWarning[]
  control: CrawlControlState
  runId?: string
  createdAt?: string
  finishedAt?: string
}

/** The answer to a pause, resume or cancel request: `202` while the orchestrator has yet to apply it, `200` once it has. */
export interface CrawlControlResult {
  jobId: string
  status: string
  phase: string
  control: CrawlControlState
}

/** One failed page of a crawl (a final attempt only). */
export interface CrawlErrorEntry {
  id: string
  ts: string
  url?: string
  code?: string
  category?: string
  retryable: boolean
  /** The HTTP status of the failed fetch, when there was one. */
  status?: number
}

/** One page a crawl did not visit because a rule blocked it. */
export interface CrawlBlockedEntry {
  url?: string
  ts: string
  reason?: string
}

/** One page of a crawl's failed and blocked pages. `nextSeq` is set when the page was full: pass it back as `afterSeq`. */
export interface CrawlErrorsPage {
  errors: CrawlErrorEntry[]
  blocked: CrawlBlockedEntry[]
  robotsBlocked: CrawlBlockedEntry[]
  nextSeq?: number
}

function normalizeCrawlStatus(raw: unknown): CrawlJobStatus {
  const root = asRecord(raw) ?? {}
  const source = asRecord(root.data) ?? root
  const status = stringField(source, 'status') ?? ''
  const counts = asRecord(source.counts) ?? {}
  const total = numberField(source, 'total')
  const totalKind = stringField(source, 'total_kind')
  const runId = stringField(source, 'run_id')
  const createdAt = stringField(source, 'created_at')
  const finishedAt = stringField(source, 'finished_at')
  return {
    jobId: stringField(source, 'job_id') ?? '',
    kind: stringField(source, 'kind') ?? '',
    status,
    phase: stringField(source, 'phase') ?? status,
    terminal: source.terminal === true || isTerminalCrawlStatus(status),
    counts: {
      completed: numberField(counts, 'completed') ?? 0,
      failed: numberField(counts, 'failed') ?? 0,
      blocked: numberField(counts, 'blocked') ?? 0,
      retried: numberField(counts, 'retried') ?? 0,
    },
    ...(total !== undefined ? { total } : {}),
    ...(totalKind ? { totalKind } : {}),
    warnings: crawlJobWarnings(source.warnings),
    control: crawlControlState(source.control),
    ...(runId ? { runId } : {}),
    ...(createdAt ? { createdAt } : {}),
    ...(finishedAt ? { finishedAt } : {}),
  }
}

function normalizeCrawlControl(raw: unknown): CrawlControlResult {
  const root = asRecord(raw) ?? {}
  const source = asRecord(root.data) ?? root
  return {
    jobId: stringField(source, 'job_id') ?? '',
    status: stringField(source, 'status') ?? '',
    phase: stringField(source, 'phase') ?? '',
    control: crawlControlState(source.control),
  }
}

function normalizeCrawlErrors(raw: unknown): CrawlErrorsPage {
  const root = asRecord(raw) ?? {}
  const source = asRecord(root.data) ?? root
  const nextSeq = numberField(source, 'next_seq')
  const errors = source.errors
  return {
    errors: Array.isArray(errors) ? errors.flatMap(normalizeCrawlErrorEntry) : [],
    blocked: crawlBlockedEntries(source.blocked),
    robotsBlocked: crawlBlockedEntries(source.robots_blocked),
    ...(nextSeq !== undefined ? { nextSeq } : {}),
  }
}

function crawlJobWarnings(raw: unknown): CrawlJobWarning[] {
  if (!Array.isArray(raw)) return []
  return raw.flatMap((item) => {
    const entry = asRecord(item)
    const code = entry ? stringField(entry, 'code') : undefined
    const message = entry ? stringField(entry, 'message') : undefined
    return code && message ? [{ code, message }] : []
  })
}

function crawlControlState(raw: unknown): CrawlControlState {
  const record = asRecord(raw) ?? {}
  const desired = stringField(record, 'desired')
  return { ...(desired ? { desired } : {}), pending: record.pending === true }
}

function normalizeCrawlErrorEntry(raw: unknown): CrawlErrorEntry[] {
  const record = asRecord(raw)
  const id = record ? stringField(record, 'id') : undefined
  if (!record || !id) return []
  const url = stringField(record, 'url')
  const code = stringField(record, 'code')
  const category = stringField(record, 'category')
  const status = numberField(record, 'status')
  return [{
    id,
    ts: stringField(record, 'ts') ?? '',
    retryable: record.retryable === true,
    ...(url ? { url } : {}),
    ...(code ? { code } : {}),
    ...(category ? { category } : {}),
    ...(status !== undefined ? { status } : {}),
  }]
}

function crawlBlockedEntries(raw: unknown): CrawlBlockedEntry[] {
  if (!Array.isArray(raw)) return []
  return raw.flatMap((item) => {
    const record = asRecord(item)
    if (!record) return []
    const url = stringField(record, 'url')
    const reason = stringField(record, 'reason')
    return [{
      ts: stringField(record, 'ts') ?? '',
      ...(url ? { url } : {}),
      ...(reason ? { reason } : {}),
    }]
  })
}

function normalizeCrawlJobs(raw: unknown): CrawlJob[] {
  const root = asRecord(raw)
  const source = Array.isArray(raw)
    ? raw
    : Array.isArray(root?.jobs)
      ? root.jobs
      : Array.isArray(root?.items)
        ? root.items
        : Array.isArray(root?.results)
          ? root.results
          : []
  return source.map(normalizeCrawlJob).filter((job) => job.id.length > 0)
}

function normalizeCrawlJob(raw: unknown): CrawlJob {
  const root = asRecord(raw) ?? {}
  const data = asRecord(root.data)
  const source = data ?? root
  const id = stringField(source, 'id')
    ?? stringField(source, 'job_id')
    ?? stringField(source, 'jobId')
    ?? stringField(source, 'run_id')
    ?? stringField(source, 'runId')
    ?? ''
  const effective = normalizeCrawlEffective(source.effective)
  return {
    acceptedAt: stringField(source, 'acceptedAt') ?? stringField(source, 'accepted_at'),
    createdAt: stringField(source, 'createdAt') ?? stringField(source, 'created_at'),
    eventStream: stringField(source, 'eventStream') ?? (id ? `/api/v1/knowledge/runs/${encodeURIComponent(id)}/events` : undefined),
    id,
    orgId: stringField(source, 'orgId') ?? stringField(source, 'org_id'),
    sourceId: stringField(source, 'sourceId') ?? stringField(source, 'source_id'),
    status: stringField(source, 'status') ?? stringField(source, 'state') ?? 'queued',
    ...(effective ? { effective } : {}),
  }
}

/** WS14.S6/S7: the handoff's `effective` echo (snake_case from quarry-edge); undefined when absent.
 * `robots_override` is `{ authority, rev }` only when the org's policy let this crawl ignore robots.txt. */
function normalizeCrawlEffective(raw: unknown): CrawlEffectiveScope | undefined {
  const record = asRecord(raw)
  if (!record) return undefined
  const override = asRecord(record.robots_override)
  const robotsOverride = override ? stringField(override, 'authority') : undefined
  const warnings = Array.isArray(record.warnings)
    ? record.warnings.filter((item): item is string => typeof item === 'string')
    : []
  const requestWarnings = Array.isArray(record.request_warnings)
    ? record.request_warnings.flatMap((item) => {
        const entry = asRecord(item)
        const code = entry ? stringField(entry, 'code') : undefined
        const field = entry ? stringField(entry, 'field') : undefined
        return code && field ? [{ code, field }] : []
      })
    : []
  return {
    scopeMode: stringField(record, 'scope_mode') ?? stringField(record, 'scopeMode') ?? '',
    robots: stringField(record, 'robots') ?? '',
    maxPages: numberField(record, 'max_pages') ?? numberField(record, 'maxPages') ?? 0,
    maxDepth: numberField(record, 'max_depth') ?? numberField(record, 'maxDepth') ?? 0,
    legacy: record.legacy === true,
    warnings,
    requestWarnings,
    ...(robotsOverride ? { robotsOverride } : {}),
  }
}

function normalizeCrawlDiscovery(raw: unknown): CrawlDiscovery {
  const root = asRecord(raw) ?? {}
  const source = asRecord(root.data) ?? root
  const list = Array.isArray(source.pages)
    ? source.pages
    : Array.isArray(source.links)
      ? source.links
      : []
  const pages = list
    .map(normalizeDiscoveredPage)
    .filter((page): page is DiscoveredPage => page !== null)
  const count = numberField(source, 'count')
  return {
    url: stringField(source, 'url') ?? '',
    count: count ?? pages.length,
    pages,
  }
}

function normalizeDiscoveredPage(raw: unknown): DiscoveredPage | null {
  const record = asRecord(raw)
  if (!record) return null
  const url = stringField(record, 'url')
  if (!url) return null
  const title = stringField(record, 'title')
  const score = numberField(record, 'score')
  return {
    url,
    ...(title ? { title } : {}),
    ...(score !== undefined ? { score } : {}),
  }
}

/** WS14.S3: `payload.denials.counts` as code → count; undefined when absent or malformed. */
function denialCountsField(payload: Record<string, unknown>): Record<string, number> | undefined {
  const denials = asRecord(payload.denials)
  const counts = denials ? asRecord(denials.counts) : undefined
  if (!counts) return undefined
  const out: Record<string, number> = {}
  for (const [code, value] of Object.entries(counts)) {
    if (typeof value === 'number' && Number.isFinite(value)) out[code] = value
  }
  return Object.keys(out).length > 0 ? out : undefined
}

function normalizeCrawlEvent(event: string | undefined, data: string): CrawlWorkflowEvent {
  if (!data) return { detail: event ?? 'Crawl updated.', event }
  try {
    const parsed = JSON.parse(data) as unknown
    const record = asRecord(parsed)
    if (!record) return { detail: data, event }

    // Quarry/control events carry their stage-specific fields under
    // `payload` (e.g. run_completed → { pages_visited, pages_failed });
    // the canonical JobHistoryEvent puts counts at the top level
    // (completed/total). Read from both so the UI sees the real count
    // regardless of which transport produced the frame.
    const payload = asRecord(record.payload) ?? {}

    // The SSE `event:` name is the event `type` (e.g. "run_completed").
    // A run_* terminal event maps to a terminal crawl status so onDone
    // fires and the job renders complete; `state`/`status`/`phase` cover
    // the JobHistoryEvent-style frames.
    const type = event ?? stringField(record, 'type')
    const status = mapEventTypeToStatus(type)
      ?? stringField(record, 'status')
      ?? stringField(record, 'state')
      ?? stringField(record, 'phase')

    const pagesVisited = numberField(payload, 'pages_visited')
      ?? numberField(record, 'pages_visited')
      ?? numberField(record, 'completed')
    const pagesFailed = numberField(payload, 'pages_failed')
      ?? numberField(record, 'pages_failed')
    const total = numberField(payload, 'total')
      ?? numberField(record, 'total')
    // WS14.S3: the scope summary of a terminal event. Absent for a legacy crawl.
    const pagesBlocked = numberField(payload, 'pages_blocked')
    const pagesBlockedByRobots = numberField(payload, 'pages_blocked_robots')
    const linksDenied = numberField(payload, 'links_denied')
    const denialCounts = denialCountsField(payload)

    const detail = stringField(payload, 'url')
      ?? stringField(record, 'message')
      ?? stringField(record, 'detail')
      ?? stringField(record, 'url')
      ?? summarizeCount(type, pagesVisited, pagesFailed)
      ?? status
      ?? event
      ?? 'Crawl updated.'
    const progress = numberField(record, 'progress')
      ?? numberField(record, 'progressPct')
      ?? numberField(record, 'progress_pct')
      ?? numberField(payload, 'progress')

    return {
      detail,
      event,
      ...(progress !== undefined ? { progress } : {}),
      ...(status ? { status } : {}),
      ...(pagesVisited !== undefined ? { pagesVisited } : {}),
      ...(pagesFailed !== undefined ? { pagesFailed } : {}),
      ...(total !== undefined ? { total } : {}),
      ...(pagesBlocked !== undefined ? { pagesBlocked } : {}),
      ...(pagesBlockedByRobots !== undefined ? { pagesBlockedByRobots } : {}),
      ...(linksDenied !== undefined ? { linksDenied } : {}),
      ...(denialCounts ? { denialCounts } : {}),
    }
  } catch {
    return { detail: data, event }
  }
}

/** Map a quarry event `type` to a crawl status so terminal run events
 * (`run_completed` / `run_failed` / `run_cancelled` / `run_timed_out`) drive onDone and the
 * completed UI state, and the pause and resume events move the row between running and paused
 * (WS15.L5). Other types return undefined so the frame's own status/phase wins. */
function mapEventTypeToStatus(type: string | undefined): string | undefined {
  switch (type) {
    case 'run_completed':
      return 'completed'
    case 'run_failed':
      return 'failed'
    case 'run_cancelled':
      return 'cancelled'
    case 'run_timed_out':
      return 'timed_out'
    case 'run_paused':
      return 'paused'
    case 'run_resumed':
      return 'running'
    case 'schedule_skipped':
      return 'skipped'
    default:
      return undefined
  }
}

/** Human-readable detail for a terminal run event carrying page counts,
 * e.g. "Crawled 5 pages" / "Crawled 5 pages (1 failed)". */
function summarizeCount(
  type: string | undefined,
  pagesVisited: number | undefined,
  pagesFailed: number | undefined,
): string | undefined {
  if (type !== 'run_completed' && type !== 'run_failed' && type !== 'run_cancelled') return undefined
  if (pagesVisited === undefined) return undefined
  const base = `Crawled ${pagesVisited} ${pagesVisited === 1 ? 'page' : 'pages'}`
  return pagesFailed && pagesFailed > 0 ? `${base} (${pagesFailed} failed)` : base
}

function isTerminalCrawlStatus(status: string): boolean {
  return ['completed', 'complete', 'failed', 'error', 'succeeded', 'cancelled', 'timed_out', 'skipped'].includes(status.trim().toLowerCase())
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null
}

function stringField(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key]
  return typeof value === 'string' && value.trim() ? value : undefined
}

function numberField(record: Record<string, unknown>, key: string): number | undefined {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

// ── SharePoint browse (pick a site, then a document library) ────────────────
// Backs the Add-source modal's SharePoint picker so users choose from their
// connected Microsoft 365 sites/libraries instead of hand-entering raw Graph
// ids. Both proxy to finspo's Graph browser via the gateway and require the
// org's Microsoft integration to be connected (otherwise the gateway returns a
// bounded error that the caller surfaces).

export interface SharePointSite {
  id: string
  name: string
  display_name?: string
  web_url?: string
  description?: string
}

export interface SharePointDrive {
  id: string
  name: string
  drive_type?: string
  web_url?: string
}

export async function listSharePointSites(orgId: string): Promise<SharePointSite[]> {
  // requestJson already unwraps finspo's `{ data: {...}, success }` envelope,
  // so the payload here is the INNER object. Reading `payload.data?.sites`
  // would double-unwrap and always yield [] — which the modal renders as
  // "connect Microsoft 365 first" even when 65 real sites came back.
  const payload = await requestJson<{ count?: number; sites?: SharePointSite[] }>(
    '/api/v1/knowledge/sharepoint/sites',
    { headers: orgHeaders(orgId) },
  )
  return payload.sites ?? []
}

export async function listSharePointDrives(orgId: string, siteId: string): Promise<SharePointDrive[]> {
  const payload = await requestJson<{ count?: number; drives?: SharePointDrive[] }>(
    `/api/v1/knowledge/sharepoint/sites/${encodeURIComponent(siteId)}/drives`,
    { headers: orgHeaders(orgId) },
  )
  return payload.drives ?? []
}

export interface SharePointFolder {
  id: string
  name: string
  path: string
  child_count?: number
  web_url?: string
}

export interface SharePointSourceRegistration {
  /** `drive` (default) registers a document library; `site_pages` a site's pages. */
  kind?: 'drive' | 'site_pages'
  siteId: string
  siteWebUrl?: string
  driveId?: string
  driveName?: string
  driveType?: string
  tenantId?: string
  folderId?: string
  folderPath?: string
}

export interface SharePointSourceRegistered {
  id: string
  syncStarted: boolean
}

/**
 * Registers a SharePoint/OneDrive library as a finspo-core source and starts
 * its first sync (gateway `POST /api/v1/knowledge/sharepoint` →
 * finspo-core `POST /api/v1/sources` + `/sources/{id}/sync`). This is the
 * step that makes a Microsoft connection's documents lane actually have
 * something to sync; integration-core answers `409 no_sources_registered`
 * to a generic Microsoft sync until it has happened.
 */
export async function registerSharePointSource(
  orgId: string,
  input: SharePointSourceRegistration,
  signal?: AbortSignal,
): Promise<SharePointSourceRegistered> {
  const payload = await requestJson<{ id?: string; syncStarted?: boolean }>('/api/v1/knowledge/sharepoint', {
    method: 'POST',
    body: JSON.stringify({
      kind: input.kind ?? 'drive',
      siteId: input.siteId,
      siteWebUrl: input.siteWebUrl ?? '',
      driveId: input.driveId ?? '',
      driveName: input.driveName ?? '',
      driveType: input.driveType ?? '',
      tenantId: input.tenantId ?? '',
      folderId: input.folderId ?? '',
      folderPath: input.folderPath ?? '',
    }),
    headers: orgHeaders(orgId),
    signal,
  })
  if (!payload?.id) throw new Error('SharePoint source registration returned no source id.')
  return { id: payload.id, syncStarted: payload.syncStarted === true }
}

/**
 * Lists the folders directly under one drive item (`itemId` omitted = the
 * library root) so the Add-source modal can drill into a library and register
 * a folder-scoped source instead of the whole drive.
 */
export async function listSharePointFolders(orgId: string, driveId: string, itemId?: string): Promise<SharePointFolder[]> {
  const query = itemId ? `?item_id=${encodeURIComponent(itemId)}` : ''
  const payload = await requestJson<{ count?: number; folders?: SharePointFolder[] }>(
    `/api/v1/knowledge/sharepoint/drives/${encodeURIComponent(driveId)}/children${query}`,
    { headers: orgHeaders(orgId) },
  )
  return payload.folders ?? []
}
