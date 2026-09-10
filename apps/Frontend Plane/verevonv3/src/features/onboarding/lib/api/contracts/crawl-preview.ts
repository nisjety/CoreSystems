import type { BrandingSignals } from './branding'

export type CrawlTitleSource = 'html' | 'model' | 'host'

export type CrawlSnippet = {
  id: string
  kind: 'text' | 'image' | 'file' | 'link'
  title: string
  /** Where `title` came from: the page's own `<title>`, a Model Plane
   * proposal (HTML title was missing/generic), or the bare host label. */
  titleSource?: CrawlTitleSource
  /** ~300-char plain-text excerpt of the page (from quarry `page_extracted`). */
  excerpt?: string
  /** One-sentence model summary; only present when `titleSource === 'model'`. */
  summary?: string
  wordCount?: number
  lang?: string
  /** Quarry driver that served the fetch: `static` | `tls` | `browser`. */
  driver?: string
  url: string
  contentType?: string
  source?: 'seed' | 'live'
  elementCount?: number
}

export type CrawlProgress = {
  status: 'starting' | 'running' | 'completed' | 'failed' | 'cancelled'
  pages: number
  elements: number
  target?: number
  jobId?: string
  latestUrl?: string
  latestTitle?: string
}

export type SseHandlers = {
  onStarted?: (payload: { jobId?: string; url?: string; target?: number }) => void
  onSnippet?: (payload: CrawlSnippet) => void
  onProgress?: (payload: CrawlProgress) => void
  onBranding?: (payload: BrandingSignals) => void
  onWarning?: (payload: { code?: string; message?: string }) => void
  onDone?: (payload: { count?: number; pages?: number; elements?: number; status?: string }) => void
}
