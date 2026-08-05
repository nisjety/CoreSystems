import { gatewayBaseUrl } from '@/shared/api/config'
import type { SseHandlers } from '@/features/onboarding/lib/api/contracts'
import { dispatchCrawlPreviewPacket } from './packets'

export async function streamCrawlPreview(
  input: { url: string; brief?: string; maxPages?: number; orgId?: string },
  handlers: SseHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const headers = new Headers({ 'Content-Type': 'application/json' })

  const response = await fetch(`${gatewayBaseUrl()}/api/v1/onboarding/crawl-preview`, {
    method: 'POST',
    credentials: 'include',
    headers,
    signal,
    body: JSON.stringify({
      url: input.url,
      brief: input.brief,
      maxPages: input.maxPages ?? 3,
      orgId: input.orgId,
    }),
  })

  if (!response.ok || !response.body) {
    throw new Error(`Crawl preview failed (${response.status})`)
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })

    let splitIndex = buffer.indexOf('\n\n')
    while (splitIndex >= 0) {
      const packet = buffer.slice(0, splitIndex)
      buffer = buffer.slice(splitIndex + 2)
      dispatchCrawlPreviewPacket(packet, handlers)
      splitIndex = buffer.indexOf('\n\n')
    }
  }
}
