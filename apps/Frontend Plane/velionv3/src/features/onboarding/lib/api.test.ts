import { describe, expect, it, vi } from 'vitest'
import { dispatchCrawlPreviewPacket } from '@/features/onboarding/lib/api'

describe('dispatchCrawlPreviewPacket', () => {
  it('routes malformed SSE payloads to warning handlers instead of throwing', () => {
    const onWarning = vi.fn()

    dispatchCrawlPreviewPacket('event: snippet\ndata: {bad json}', { onWarning })

    expect(onWarning).toHaveBeenCalledWith({
      code: 'invalid_sse_packet',
      message: 'Ignored malformed crawl preview packet.',
    })
  })

  it('dispatches valid done packets to the matching handler', () => {
    const onDone = vi.fn()

    dispatchCrawlPreviewPacket('event: done\ndata: {"pages":2,"elements":9,"status":"completed"}', { onDone })

    expect(onDone).toHaveBeenCalledWith({ pages: 2, elements: 9, status: 'completed' })
  })
})
