import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  dispatchCrawlPreviewPacket,
  getShippingCarriers,
  recommendPlan,
  searchBrreg,
  startCheckout,
} from '@/features/onboarding/lib/api'

afterEach(() => vi.unstubAllGlobals())

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

  it('rejects structurally malformed SSE payloads instead of casting them into crawl state', () => {
    const onSnippet = vi.fn()
    const onWarning = vi.fn()

    dispatchCrawlPreviewPacket('event: snippet\ndata: {"id":"snippet_1","kind":"text"}', { onSnippet, onWarning })

    expect(onSnippet).not.toHaveBeenCalled()
    expect(onWarning).toHaveBeenCalledWith(expect.objectContaining({
      code: 'invalid_sse_payload',
    }))
  })

  it('rejects malformed Brreg search, plan recommendation, and checkout responses', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      const body = url.includes('brreg/search')
        ? { results: [{ organisasjonsnummer: '999', navn: 42 }] }
        : url.includes('recommend-plan')
          ? { recommendation: { planId: 'enterprise' } }
          : {}
      return new Response(JSON.stringify(body), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    await expect(searchBrreg('Aquatiq')).rejects.toThrow('Invalid response')
    await expect(recommendPlan({ organizationName: 'Aquatiq' })).rejects.toThrow('Invalid response')
    await expect(startCheckout({
      actor: { userId: 'user_1' },
      orgId: 'org_1',
      plan: 'standard',
      successUrl: 'https://verevon.test/success',
      cancelUrl: 'https://verevon.test/cancel',
    })).rejects.toThrow('Invalid response')
  })

  it('rejects a malformed shipping-carrier payload before it reaches onboarding state', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      carriers: [{ name: 'Posten', is_mock: 'false' }],
    }), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    })))

    await expect(getShippingCarriers()).rejects.toThrow('Invalid response from /api/v1/shipping/carriers')
  })
})
