import type {
  BrandingSignals,
  CrawlProgress,
  CrawlSnippet,
  SseHandlers,
} from '@/features/onboarding/lib/api/contracts'

export function dispatchCrawlPreviewPacket(packet: string, handlers: SseHandlers) {
  let event = 'message'
  let data = ''

  for (const line of packet.split('\n')) {
    if (line.startsWith('event:')) event = line.slice(6).trim()
    if (line.startsWith('data:')) data += line.slice(5).trim()
  }

  if (!data) return
  let payload: unknown
  try {
    payload = JSON.parse(data) as unknown
  } catch {
    handlers.onWarning?.({
      code: 'invalid_sse_packet',
      message: 'Ignored malformed crawl preview packet.',
    })
    return
  }

  switch (event) {
    case 'started':
      handlers.onStarted?.(payload as { jobId?: string; url?: string; target?: number })
      break
    case 'snippet':
      handlers.onSnippet?.(payload as CrawlSnippet)
      break
    case 'progress':
      handlers.onProgress?.(payload as CrawlProgress)
      break
    case 'branding':
      handlers.onBranding?.(payload as BrandingSignals)
      break
    case 'warning':
      handlers.onWarning?.(payload as { code?: string; message?: string })
      break
    case 'done':
      handlers.onDone?.(payload as { count?: number; pages?: number; elements?: number; status?: string })
      break
  }
}
