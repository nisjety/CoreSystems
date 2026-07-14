import { afterEach, describe, expect, it, vi } from 'vitest'
import { dictateAudioBlob } from '@/shared/api/audio-client'

// Velion Flow dictation client: the wire is snake_case (Model Plane), the
// result camelCase; the endpoint is the dedicated /api/v1/ai/dictate route
// (NOT /ai/speech — plain azure transcribe returns empty text for the
// browser's webm/opus recordings).

afterEach(() => {
  vi.unstubAllGlobals()
})

function stubFetch(payload: unknown): ReturnType<typeof vi.fn> {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response(JSON.stringify(payload), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    }),
  )
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

describe('dictateAudioBlob', () => {
  it('POSTs snake_case audio to /api/v1/ai/dictate and maps the result to camelCase', async () => {
    const fetchMock = stubFetch({
      text: 'Hello team, ship it tomorrow.',
      raw_text: 'Um, hello team, uh, ship it tomorrow.',
      cleaned: true,
      detected_language: 'en-US',
    })

    const blob = new Blob(['fake-audio'], { type: 'audio/webm;codecs=opus' })
    const result = await dictateAudioBlob(blob, 'en-US', 'chat message')

    expect(result).toEqual({
      text: 'Hello team, ship it tomorrow.',
      rawText: 'Um, hello team, uh, ship it tomorrow.',
      cleaned: true,
      detectedLanguage: 'en-US',
    })

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toContain('/api/v1/ai/dictate')
    const body = JSON.parse(String(init.body)) as Record<string, unknown>
    expect(body.format).toBe('webm')
    expect(body.language).toBe('en-US')
    expect(body.context).toBe('chat message')
    expect(typeof body.audio_base64).toBe('string')
    expect((body.audio_base64 as string).length).toBeGreaterThan(0)
  })

  it('omits context when not provided and tolerates a raw-fallback response', async () => {
    const fetchMock = stubFetch({ text: '', raw_text: 'raw words', cleaned: false })

    const blob = new Blob(['fake-audio'], { type: 'audio/wav' })
    const result = await dictateAudioBlob(blob, 'nb-NO')

    expect(result.text).toBe('')
    expect(result.rawText).toBe('raw words')
    expect(result.cleaned).toBe(false)

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    const body = JSON.parse(String(init.body)) as Record<string, unknown>
    expect(body.format).toBe('wav')
    expect('context' in body).toBe(false)
  })
})
