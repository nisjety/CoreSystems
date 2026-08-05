import type { BrandingSignals } from './branding'

export type CrawlSnippet = {
  id: string
  kind: 'text' | 'image' | 'file' | 'link'
  title: string
  excerpt?: string
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
