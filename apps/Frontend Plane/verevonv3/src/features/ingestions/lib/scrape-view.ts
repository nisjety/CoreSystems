import { readScrapeCaptures, type ScrapeCaptureView, type ScrapeResult } from '@/shared/api/knowledge-client'

/**
 * A scrape response flattened for the playground preview. quarry-edge returns
 * markdown/html inline when they fit and otherwise as artifact references under
 * `formats`; links always come inline under `formats.links`. Every field is read
 * defensively because the envelope differs slightly between edge versions.
 */
export type ScrapeLink = { href: string; text?: string }

/** One exact span of the page text the highlights format found (INNH-1). Offsets are char positions. */
export type ScrapeHighlight = { text: string; start: number; end: number }

/** What one AI format did, as the edge reports it (INNH-1). A failure is a status, never an HTTP error. */
export type AiFormatEntry = {
  format: 'json' | 'summary' | 'question' | 'highlights'
  status: 'ok' | 'error' | 'skipped'
  code: string | null
  reason: string | null
  model: string | null
  retryAfterSeconds: number | null
  untrustedSource: boolean
  truncatedInput: boolean
  schemaValid: boolean | null
  schemaIssues: Array<{ path: string; code: string }>
  repair: string | null
  found: boolean | null
  grounded: boolean | null
  dropped: number | null
}

export type AiReportView = {
  entries: AiFormatEntry[]
  total: { calls: number; inputTokens: number; outputTokens: number }
}

export type ScrapeView = {
  requestedUrl: string
  finalUrl: string
  statusCode: number | null
  title: string
  description: string
  language: string
  wordCount: number | null
  driver: string
  durationMs: number | null
  fingerprint: string
  fetchedAt: string
  runId: string
  change: string
  markdown: string | null
  html: string | null
  rawHtml: string | null
  links: ScrapeLink[]
  artifacts: Array<{ format: string; artifactId: string; bytes: number | null }>
  warnings: string[]
  captures: ScrapeCaptureView | undefined
  metadata: Record<string, unknown>
  /** The `json` AI format's result, schema-valid when `ai.json.status` is ok. */
  json: unknown
  /** The `summary` AI format's result. */
  summary: string | null
  /** The `question` AI format's answer, when the page answers it. */
  answer: string | null
  /** The `highlights` AI format's spans, in page order. */
  highlights: ScrapeHighlight[]
  /** Per-format status and the call total; `null` when no AI format was asked for. */
  ai: AiReportView | null
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : {}
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function num(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

export function toScrapeView(result: ScrapeResult | unknown, requestedUrl: string): ScrapeView {
  const outer = record(result)
  const data = Object.keys(record(outer.data)).length ? record(outer.data) : outer
  const url = data.url
  const urlRecord = record(url)
  const formats = record(data.formats)
  const metadata = record(data.metadata)
  const extraction = record(data.extraction)
  const driver = record(data.driver)
  const links = Array.isArray(formats.links)
    ? formats.links
        .map((link) => (typeof link === 'string' ? { href: link } : { href: text(record(link).href), text: text(record(link).text) || undefined }))
        .filter((link) => link.href.length > 0)
    : []
  const artifacts = Object.entries(formats)
    .map(([format, value]) => ({ format, artifactId: text(record(value).artifact_id), bytes: num(record(value).bytes) }))
    .filter((entry) => entry.artifactId.length > 0)
  const warnings = Array.isArray(data.warnings)
    ? data.warnings.map((warning) => (typeof warning === 'string' ? warning : text(record(warning).message) || text(record(warning).code))).filter(Boolean)
    : []

  return {
    requestedUrl,
    finalUrl: text(urlRecord.final_url) || text(urlRecord.final) || text(url) || text(extraction.url) || requestedUrl,
    statusCode: num(data.status),
    title: text(metadata.title) || text(extraction.title),
    description: text(metadata.description) || text(extraction.excerpt),
    language: text(metadata.lang) || text(extraction.lang),
    wordCount: num(extraction.word_count),
    driver: text(driver.kind) || text(extraction.driver),
    durationMs: num(driver.duration_ms),
    fingerprint: text(data.fingerprint),
    fetchedAt: text(data.fetched_at),
    runId: text(data.run_id),
    change: text(record(data.change).status),
    markdown: typeof data.markdown === 'string' ? data.markdown : null,
    html: typeof data.html === 'string' ? data.html : null,
    rawHtml: typeof data.raw_html === 'string' ? data.raw_html : null,
    links,
    artifacts,
    warnings,
    captures: readScrapeCaptures(result),
    metadata,
    json: data.json ?? null,
    summary: text(data.summary) || null,
    answer: text(data.answer) || null,
    highlights: readHighlights(data.highlights),
    ai: readAiReport(data.ai),
  }
}

function readHighlights(value: unknown): ScrapeHighlight[] {
  if (!Array.isArray(value)) return []
  return value
    .map((entry) => record(entry))
    .filter((entry) => typeof entry.text === 'string' && num(entry.start) !== null && num(entry.end) !== null)
    .map((entry) => ({ text: text(entry.text), start: num(entry.start) ?? 0, end: num(entry.end) ?? 0 }))
}

const AI_FORMAT_NAMES: AiFormatEntry['format'][] = ['json', 'summary', 'question', 'highlights']

/** Reads `data.ai`. Every field is optional on the wire, so each one is read defensively. */
function readAiReport(value: unknown): AiReportView | null {
  const report = record(value)
  const entries: AiFormatEntry[] = []
  for (const format of AI_FORMAT_NAMES) {
    const entry = record(report[format])
    if (Object.keys(entry).length === 0) continue
    const status = entry.status === 'ok' || entry.status === 'error' || entry.status === 'skipped' ? entry.status : 'error'
    entries.push({
      format,
      status,
      code: typeof entry.code === 'string' ? entry.code : null,
      reason: typeof entry.reason === 'string' ? entry.reason : null,
      model: typeof entry.model === 'string' ? entry.model : null,
      retryAfterSeconds: num(entry.retry_after_s),
      untrustedSource: entry.untrusted_source === true,
      truncatedInput: entry.truncated_input === true,
      schemaValid: typeof entry.schema_valid === 'boolean' ? entry.schema_valid : null,
      schemaIssues: Array.isArray(entry.schema_issues)
        ? entry.schema_issues
            .map((issue) => record(issue))
            .filter((issue) => typeof issue.path === 'string')
            .map((issue) => ({ path: text(issue.path), code: text(issue.code) }))
        : [],
      repair: typeof entry.repair === 'string' ? entry.repair : null,
      found: typeof entry.found === 'boolean' ? entry.found : null,
      grounded: typeof entry.grounded === 'boolean' ? entry.grounded : null,
      dropped: num(entry.dropped),
    })
  }
  if (entries.length === 0) return null
  const total = record(report.total)
  return {
    entries,
    total: {
      calls: num(total.calls) ?? 0,
      inputTokens: num(total.input_tokens) ?? 0,
      outputTokens: num(total.output_tokens) ?? 0,
    },
  }
}
