import { readScrapeCaptures, type ScrapeCaptureView, type ScrapeResult } from '@/shared/api/knowledge-client'

/**
 * A scrape response flattened for the playground preview. quarry-edge returns
 * markdown/html inline when they fit and otherwise as artifact references under
 * `formats`; links always come inline under `formats.links`. Every field is read
 * defensively because the envelope differs slightly between edge versions.
 */
export type ScrapeLink = { href: string; text?: string }

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
  }
}
