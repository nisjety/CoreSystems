import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamChat } from './chat-client'

// A zero-data-retention refusal reaches the chat in three shapes: a frame in the
// stream, an AG-UI run error, or the HTTP answer that refused the stream before
// it opened. The first two must keep the code and the feature the server named.
// The third must stay that refusal: a refusal is not a lost connection, and the
// controller answers a lost connection by reattaching, which would be refused
// the same way again.

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

function refusal(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

describe('a zero-retention refusal in the stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps the code and the feature of an error frame', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('error', {
        code: 'zdr_feature_not_supported',
        message: 'This is not supported with zero data retention yet: web search and browsing.',
        retryable: false,
        action: 'turn_off_feature',
        feature: 'web_search',
      }),
    ])))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0]?.[0]).toMatchObject({
      code: 'zdr_feature_not_supported',
      feature: 'web_search',
      retryable: false,
    })
  })

  it('reads the feature from the nested error envelope as well', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('error', { error: { code: 'zdr_feature_not_supported', message: 'x', feature: 'browser_run' } }),
    ])))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'zdr_feature_not_supported', feature: 'browser_run' })
  })

  it('leaves the feature out when the frame names none', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('error', { code: 'zdr_unavailable', message: 'no attested provider', retryable: false }),
    ])))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    const event = onError.mock.calls[0]?.[0]
    expect(event).toMatchObject({ code: 'zdr_unavailable' })
    expect(event).not.toHaveProperty('feature')
  })

  it('keeps the code and the feature of an AG-UI run error', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('RUN_ERROR', {
        code: 'zdr_feature_not_supported',
        message: 'x',
        retryable: false,
        feature: 'agentic',
      }),
    ])))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'zdr_feature_not_supported', feature: 'agentic' })
  })
})

describe('a zero-retention refusal as the HTTP answer to the stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reports the refusal itself, not a lost connection', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(412, {
      error: { code: 'zdr_unavailable', message: 'no model deployment is attested', retryable: false, action: 'choose_another_model' },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'zdr_unavailable' })
  })

  it('keeps a zero-retention 403 a refusal, not a sign-in problem', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(403, {
      error: { code: 'zdr_durable_mutation_forbidden', message: 'Zero Data Retention credentials cannot change durable state.' },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'zdr_durable_mutation_forbidden' })
  })

  it('carries the feature of the refusal', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(412, {
      error: { code: 'zdr_feature_not_supported', message: 'x', feature: 'browser_run' },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'zdr_feature_not_supported', feature: 'browser_run' })
  })

  it('still reports an ordinary failed request as a lost connection', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(500, {
      error: { code: 'upstream_error', message: 'upstream failed' },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'connection_error' })
  })
})
