import { requestForm, requestJson } from './http'
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
}

export interface ScrapeRequest {
  url: string
  render?: Record<string, unknown>
  renderHints?: Record<string, unknown>
  signals?: Record<string, unknown>
  cachePolicy?: string
  /** When true, quarry-edge persists the fetched page into the knowledge base
   * (not just returns it). Omit/false for preview-only scrapes. */
  ingest?: boolean
}

export interface CrawlRequest {
  url: string
  maxPages?: number
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
  rawHtml?: string
  text?: string
  url?: string | { final?: string; final_url?: string; requested?: string }
  data?: {
    content?: string
    formats?: Record<string, unknown>
    html?: string
    markdown?: string
    metadata?: ScrapeMetadata
    rawHtml?: string
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
  return { 'x-velion-org-id': orgId }
}

// ── Documents ─────────────────────────────────────────────────────────────────

export async function listDocuments(orgId: string, signal?: AbortSignal): Promise<Document[]> {
  return requestJson<Document[]>('/api/v1/knowledge/documents', {
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function getDocument(orgId: string, id: string, signal?: AbortSignal): Promise<Document> {
  return requestJson<Document>(`/api/v1/knowledge/documents/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
}

/** Flat document-summary list for pickers/typeaheads. The rich workspace payload
 * lives at `/api/v1/knowledge/sources` (see knowledge-live-client); this hits the
 * documents-api passthrough at `/source-list`, which returns `{ sources, total }`. */
export async function listSources(orgId: string, signal?: AbortSignal): Promise<Source[]> {
  const payload = await requestJson<{
    sources?: Array<{
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
  return (payload.sources ?? []).map((entry) => ({
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
  return requestJson<WikiPage[]>('/api/v1/knowledge/wiki/pages', {
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function getWikiPageByPath(
  orgId: string,
  path: string,
  signal?: AbortSignal,
): Promise<WikiPage> {
  const qs = new URLSearchParams({ path })
  return requestJson<WikiPage>(`/api/v1/knowledge/wiki/pages/by-path?${qs}`, {
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function getWikiPage(orgId: string, id: string, signal?: AbortSignal): Promise<WikiPage> {
  return requestJson<WikiPage>(`/api/v1/knowledge/wiki/pages/${encodeURIComponent(id)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
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

export async function importUpload(orgId: string, files: File[], signal?: AbortSignal): Promise<ImportJob> {
  const form = new FormData()
  for (const file of files) {
    form.append('files', file, file.name)
  }
  return requestForm<ImportJob>('/api/v1/knowledge/imports/upload', form, {
    headers: orgHeaders(orgId),
    signal,
  })
}

// ── Quarry ────────────────────────────────────────────────────────────────────

export async function scrapeUrl(
  orgId: string,
  body: ScrapeRequest,
  signal?: AbortSignal,
): Promise<ScrapeResult> {
  return requestJson<ScrapeResult>('/api/v1/knowledge/scrape', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

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
 * selection. Returns the run, ready for {@link streamCrawlRunEvents}. */
export async function crawlSelectedPages(
  orgId: string,
  urls: string[],
  signal?: AbortSignal,
): Promise<CrawlJob> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/crawl', {
    method: 'POST',
    body: JSON.stringify({ urls }),
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
}

export type CrawlWorkflowHandlers = {
  onDone?: () => void
  onError?: (message: string) => void
  onEvent?: (event: CrawlWorkflowEvent) => void
}

export async function streamCrawlRunEvents(
  orgId: string,
  runId: string,
  handlers: CrawlWorkflowHandlers,
  signal?: AbortSignal,
): Promise<void> {
  await readSseStream(
    `/api/v1/knowledge/runs/${encodeURIComponent(runId)}/events`,
    { headers: orgHeaders(orgId), signal },
    (event) => {
      const parsed = normalizeCrawlEvent(event.event, event.data)
      handlers.onEvent?.(parsed)
      if (parsed.status && isTerminalCrawlStatus(parsed.status)) handlers.onDone?.()
    },
    (err) => handlers.onError?.(err instanceof Error ? err.message : 'Crawl event stream failed.'),
    () => handlers.onDone?.(),
  )
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
  return {
    acceptedAt: stringField(source, 'acceptedAt') ?? stringField(source, 'accepted_at'),
    createdAt: stringField(source, 'createdAt') ?? stringField(source, 'created_at'),
    eventStream: stringField(source, 'eventStream') ?? (id ? `/api/v1/knowledge/runs/${encodeURIComponent(id)}/events` : undefined),
    id,
    orgId: stringField(source, 'orgId') ?? stringField(source, 'org_id'),
    sourceId: stringField(source, 'sourceId') ?? stringField(source, 'source_id'),
    status: stringField(source, 'status') ?? stringField(source, 'state') ?? 'queued',
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

function normalizeCrawlEvent(event: string | undefined, data: string): CrawlWorkflowEvent {
  if (!data) return { detail: event ?? 'Crawl updated.', event }
  try {
    const parsed = JSON.parse(data) as unknown
    const record = asRecord(parsed)
    if (!record) return { detail: data, event }
    const status = stringField(record, 'status') ?? stringField(record, 'state') ?? stringField(record, 'phase')
    const detail = stringField(record, 'message')
      ?? stringField(record, 'detail')
      ?? stringField(record, 'url')
      ?? status
      ?? event
      ?? 'Crawl updated.'
    const progress = numberField(record, 'progress')
      ?? numberField(record, 'progressPct')
      ?? numberField(record, 'progress_pct')
    return { detail, event, progress, status }
  } catch {
    return { detail: data, event }
  }
}

function isTerminalCrawlStatus(status: string): boolean {
  return ['completed', 'complete', 'failed', 'error', 'succeeded', 'cancelled'].includes(status.trim().toLowerCase())
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
