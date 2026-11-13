import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamChat } from './chat-client'

// The organisation's agent policy refusals (C-14-mg-rel1, F-t3-083) reach the chat as an error frame in the stream, or
// as the HTTP answer that refused the stream before it opened. `org_policy_unavailable` is refused before the run
// started, so it keeps its code and is not a lost connection: the controller answers a lost connection by reattaching,
// which would be refused the same way again. The server's English text is never the message.

const SERVER_TEXT =
  "The organisation's agent policy cannot be read right now, so this agent run cannot start. Try again shortly."

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

describe('an org policy refusal as the HTTP answer to the stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps the code of a 503 refusal and carries the words of the code, not the server text', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(503, {
      error: { code: 'org_policy_unavailable', message: SERVER_TEXT },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenCalledTimes(1)
    const event = onError.mock.calls[0]?.[0]
    expect(event).toMatchObject({ code: 'org_policy_unavailable', retryable: true })
    expect(event.message).toBe(
      'Organisasjonens agentpolicy kan ikke leses akkurat nå, så agenten startet ikke. Ingenting ble kjørt.',
    )
  })

  it('keeps a 403 for a paused organisation a refusal, not a sign-in problem', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(403, {
      error: { code: 'org_agents_paused', message: 'Agent runs are paused for this organization.' },
    })))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'org_agents_paused', retryable: false })
  })
})

describe('an org policy refusal in the stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps the code of an error frame', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('error', { code: 'org_policy_unavailable', message: SERVER_TEXT, retryable: true }),
    ])))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code: 'org_policy_unavailable', retryable: true })
  })
})
