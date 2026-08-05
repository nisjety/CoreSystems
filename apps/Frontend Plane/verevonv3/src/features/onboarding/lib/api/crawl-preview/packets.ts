import type {
  BrandingSignals,
  CrawlProgress,
  CrawlSnippet,
  SseHandlers,
} from '@/features/onboarding/lib/api/contracts'
import { z } from 'zod'

const startedPacket = z.object({
  jobId: z.string().trim().min(1).optional(),
  url: z.string().trim().min(1).optional(),
  target: z.number().int().nonnegative().optional(),
})

const snippetPacket = z.object({
  id: z.string().trim().min(1),
  kind: z.enum(['text', 'image', 'file', 'link']),
  title: z.string().trim().min(1),
  excerpt: z.string().optional(),
  url: z.string().trim().min(1),
  contentType: z.string().optional(),
  source: z.enum(['seed', 'live']).optional(),
  elementCount: z.number().int().nonnegative().optional(),
})

const progressPacket = z.object({
  status: z.enum(['starting', 'running', 'completed', 'failed', 'cancelled']),
  pages: z.number().int().nonnegative(),
  elements: z.number().int().nonnegative(),
  target: z.number().int().nonnegative().optional(),
  jobId: z.string().trim().min(1).optional(),
  latestUrl: z.string().optional(),
  latestTitle: z.string().optional(),
})

const brandingPacket = z.object({
  siteName: z.string().optional(),
  favicon: z.string().optional(),
  themeColor: z.string().optional(),
  logoCandidate: z.string().optional(),
  palette: z.array(z.string()).optional(),
})

const warningPacket = z.object({
  code: z.string().optional(),
  message: z.string().optional(),
})

const donePacket = z.object({
  count: z.number().int().nonnegative().optional(),
  pages: z.number().int().nonnegative().optional(),
  elements: z.number().int().nonnegative().optional(),
  status: z.string().optional(),
})

function invalidPayload(handlers: SseHandlers, event: string) {
  handlers.onWarning?.({
    code: 'invalid_sse_payload',
    message: `Ignored invalid ${event} crawl preview payload.`,
  })
}

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
    case 'started': {
      const result = startedPacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onStarted?.(result.data)
      break
    }
    case 'snippet': {
      const result = snippetPacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onSnippet?.(result.data satisfies CrawlSnippet)
      break
    }
    case 'progress': {
      const result = progressPacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onProgress?.(result.data satisfies CrawlProgress)
      break
    }
    case 'branding': {
      const result = brandingPacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onBranding?.(result.data satisfies BrandingSignals)
      break
    }
    case 'warning': {
      const result = warningPacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onWarning?.(result.data)
      break
    }
    case 'done': {
      const result = donePacket.safeParse(payload)
      if (!result.success) return invalidPayload(handlers, event)
      handlers.onDone?.(result.data)
      break
    }
  }
}
