import { afterEach, describe, expect, it, vi } from 'vitest'
import { resumeStream, streamChat } from './chat-client'

// The gateway stamps where an answer was processed onto the `usage` event
// (`provider_used`, `residency`, `privacy_tier`). The client carries those
// values and nothing else: an older gateway sends none, and then the usage
// event must read exactly as it always did.

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })
}

const frame = (event: string, data: unknown) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

describe('usage event: server-stamped processing provenance', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('parses the stamped provider, residency and tier onto servedBy', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', {
        input_tokens: 10,
        output_tokens: 5,
        cost_usd: 0.002,
        cost_basis: 'metered',
        latency_ms: 900,
        provider_used: 'azure-openai',
        residency: 'eu',
        privacy_tier: 'zdr_contractual',
      }),
      frame('usage', {
        input_tokens: 1,
        cost_usd: 0,
        cost_basis: 'subscription',
        provider_used: 'openai-codex-subscription',
        residency: 'global',
        privacy_tier: 'global',
      }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    const onUnknownEvent = vi.fn()
    await streamChat({ content: 'hi' }, { onUsage, onUnknownEvent })
    expect(onUsage.mock.calls[0]?.[0]).toMatchObject({
      costBasis: 'metered',
      servedBy: { providerUsed: 'azure-openai', residency: 'eu', privacyTier: 'zdr_contractual' },
    })
    expect(onUsage.mock.calls[1]?.[0]).toMatchObject({
      costBasis: 'subscription',
      servedBy: { providerUsed: 'openai-codex-subscription', residency: 'global', privacyTier: 'global' },
    })
    expect(onUnknownEvent).not.toHaveBeenCalled()
  })

  it('leaves servedBy off entirely when an older gateway sends nothing', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', { input_tokens: 10, output_tokens: 5, cost_usd: 0.0031, latency_ms: 900 }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    await streamChat({ content: 'hi' }, { onUsage })
    const usage = onUsage.mock.calls[0]?.[0]
    expect(usage).toMatchObject({ costUsd: 0.0031, latencyMs: 900 })
    expect('servedBy' in usage).toBe(false)
  })

  it('drops stamped values it does not recognise rather than rendering a guess', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', { provider_used: '', residency: 'atlantis', privacy_tier: 'platinum' }),
      frame('usage', { provider_used: 'azure-openai', residency: 'eu', privacy_tier: 'unspecified' }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    await streamChat({ content: 'hi' }, { onUsage })
    expect('servedBy' in onUsage.mock.calls[0]?.[0]).toBe(false)
    // `unspecified` is "no constraint", never a claim; the usable rest survives.
    expect(onUsage.mock.calls[1]?.[0].servedBy).toEqual({ providerUsed: 'azure-openai', residency: 'eu' })
  })

  it('reads the same stamp when a dropped stream is resumed', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', { latency_ms: 700, provider_used: 'azure-openai', residency: 'eu', privacy_tier: 'eu_resident' }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    await resumeStream('req-1', { onUsage })
    expect(onUsage.mock.calls[0]?.[0].servedBy).toEqual({
      providerUsed: 'azure-openai',
      residency: 'eu',
      privacyTier: 'eu_resident',
    })
  })
})
