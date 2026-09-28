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
  /** Phase 6 selective ingest: true = persist+embed; omitted = working-set only (default never). */
  ingest?: boolean
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
  // Server-provided event-stream path (from the crawl handoff's
  // `eventStream`). Honor it verbatim — the gateway now points it at the
  // job-id-keyed `/jobs/{id}/events` SSE route (the run-id is not assigned
  // synchronously at handoff). Falls back to the same job-events path built
  // from the id we were given, NOT the old `/runs/{id}/events` JSON route
  // that the SSE reader could not parse (the 0-pages bug).
  streamPath?: string,
): Promise<void> {
  const path = streamPath && streamPath.trim()
    ? streamPath
    : `/api/v1/knowledge/jobs/${encodeURIComponent(runId)}/events`
  await readSseStream(
    path,
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
    }
  } catch {
    return { detail: data, event }
  }
}

/** Map a quarry event `type` to a crawl status so terminal run events
 * (`run_completed` / `run_failed` / `run_cancelled`) drive onDone and the
 * completed UI state. Non-terminal types return undefined so the frame's
 * own status/phase wins. */
function mapEventTypeToStatus(type: string | undefined): string | undefined {
  switch (type) {
    case 'run_completed':
      return 'completed'
    case 'run_failed':
      return 'failed'
    case 'run_cancelled':
      return 'cancelled'
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
