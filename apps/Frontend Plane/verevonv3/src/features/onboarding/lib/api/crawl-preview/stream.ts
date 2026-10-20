import { gatewayBaseUrl } from '@/shared/api/config'
import { apiErrorFromResponse, signalSessionExpiry } from '@/shared/api/http'
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

  if (!response.ok) {
    // The typed failure (code, Retry-After) rather than a bare Error, so the page can
    // say why through the shared copy, and an expired session leads back to sign-in.
    const failure = apiErrorFromResponse(await response.json().catch(() => null), response)
    signalSessionExpiry(failure, '/api/v1/onboarding/crawl-preview')
    throw failure
  }
  if (!response.body) {
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
