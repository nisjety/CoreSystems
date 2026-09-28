import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  chunkForSpeech,
  speakableText,
  synthesizeSpeech,
  ttsLanguageOf,
  ttsVoiceFor,
} from '@/shared/api/speech-client'

// Governed text-to-speech client: always Azure Speech, always mp3, always a
// Blob the player can hand to a `blob:` URL (the CSP blocks `data:` audio).

afterEach(() => {
  vi.unstubAllGlobals()
})

function stubFetch(payload: unknown, status = 200): ReturnType<typeof vi.fn> {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } }),
  )
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

describe('synthesizeSpeech', () => {
  it('asks Azure Speech for mp3 on the governed speech route and returns the audio as a Blob', async () => {
    const fetchMock = stubFetch({
      audio_base64: btoa('ID3 fake mp3 bytes'),
      format: 'mp3',
      provider_used: 'azure-speech',
    })
    const controller = new AbortController()

    const blob = await synthesizeSpeech({
      text: 'Hei, dette er svaret.',
      language: 'nb-NO',
      voice: 'nb-NO-PernilleNeural',
      signal: controller.signal,
    })

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toMatch(/\/api\/v1\/ai\/speech$/)
    expect(init.method).toBe('POST')
    expect(init.signal).toBe(controller.signal)
    expect(JSON.parse(String(init.body))).toEqual({
      operation: 'tts',
      text: 'Hei, dette er svaret.',
      language: 'nb-NO',
      voice: 'nb-NO-PernilleNeural',
      format: 'mp3',
      provider: 'azure-speech',
    })
    expect(blob.type).toBe('audio/mpeg')
    expect(await blob.text()).toBe('ID3 fake mp3 bytes')
  })

  it('decodes binary audio byte for byte', async () => {
    const bytes = [0xff, 0xfb, 0x90, 0x00, 0x7f, 0x80]
    stubFetch({ audio_base64: btoa(String.fromCharCode(...bytes)), format: 'mp3' })
    const blob = await synthesizeSpeech({ text: 'x', language: 'en-GB', voice: 'en-GB-SoniaNeural' })
    expect([...new Uint8Array(await blob.arrayBuffer())]).toEqual(bytes)
  })

  it('fails closed when the audio came from another provider, in another format, or not at all', async () => {
    const input = { text: 'x', language: 'nb-NO', voice: 'nb-NO-PernilleNeural' }
    stubFetch({ audio_base64: btoa('a'), format: 'mp3', provider_used: 'openai' })
    await expect(synthesizeSpeech(input)).rejects.toThrow(/unexpected provider/)
    stubFetch({ audio_base64: btoa('a'), format: 'wav', provider_used: 'azure-speech' })
    await expect(synthesizeSpeech(input)).rejects.toThrow(/unexpected format/)
    stubFetch({ format: 'mp3', provider_used: 'azure-speech' })
    await expect(synthesizeSpeech(input)).rejects.toThrow(/no audio/)
  })

  it('surfaces a refusal from the server as an error', async () => {
    stubFetch({ error: { code: 'zdr_unavailable', message: 'Speech is not available with ZDR' } }, 412)
    await expect(synthesizeSpeech({ text: 'x', language: 'nb-NO', voice: 'nb-NO-PernilleNeural' })).rejects.toMatchObject({
      status: 412,
    })
  })
})

describe('ttsVoiceFor', () => {
  it('uses Pernille for Norwegian and Sonia for English', () => {
    expect(ttsVoiceFor('nb')).toBe('nb-NO-PernilleNeural')
    expect(ttsVoiceFor('no')).toBe('nb-NO-PernilleNeural')
    expect(ttsVoiceFor('nb-NO')).toBe('nb-NO-PernilleNeural')
    expect(ttsVoiceFor('nn-NO')).toBe('nb-NO-PernilleNeural')
    expect(ttsVoiceFor('en')).toBe('en-GB-SoniaNeural')
    expect(ttsVoiceFor('en-GB')).toBe('en-GB-SoniaNeural')
  })

  it('follows the Voice mode language where Azure has a voice, and falls back to Norwegian', () => {
    expect(ttsVoiceFor('en-US')).toBe('en-US-JennyNeural')
    expect(ttsVoiceFor('de-DE')).toBe('de-DE-KatjaNeural')
    expect(ttsVoiceFor('sv-SE')).toBe('sv-SE-SofieNeural')
    expect(ttsVoiceFor('xx-YY')).toBe('nb-NO-PernilleNeural')
    expect(ttsLanguageOf('nb-NO-PernilleNeural')).toBe('nb-NO')
    expect(ttsLanguageOf('en-GB-SoniaNeural')).toBe('en-GB')
  })
})

describe('speakableText', () => {
  it('reads the words, not the markdown', () => {
    const spoken = speakableText(
      [
        '## Oppsummering',
        '',
        'Dette er **viktig** og _litt_ `teknisk`.',
        '',
        '- Første punkt',
        '- Andre punkt',
        '',
        '```ts',
        'const secret = 1',
        '```',
        '',
        '> Et sitat',
      ].join('\n'),
    )
    expect(spoken).toBe('Oppsummering. Dette er viktig og litt teknisk. Første punkt. Andre punkt. Et sitat.')
  })

  it('drops citations, link targets, bare URLs and images', () => {
    const spoken = speakableText(
      'Laks vokser raskt [1] i kaldt vann [2, 3].[^4] Se [rapporten](https://example.com/rapport) eller https://example.com/x. ![graf](https://example.com/g.png)【5†kilde】\n\n[1]: https://example.com/kilde',
    )
    expect(spoken).toBe('Laks vokser raskt i kaldt vann. Se rapporten eller.')
    expect(spoken).not.toMatch(/https?:|\[|\]|【/)
    // A URL mid-paragraph takes its link away, not the sentence break after it.
    expect(speakableText('Les https://example.com/a. Så dette.')).toBe('Les. Så dette.')
  })

  it('reads a table row as a list of cells and skips the separator', () => {
    expect(speakableText('| Art | Vekt |\n| --- | --- |\n| Laks | 4 kg |')).toBe('Art, Vekt. Laks, 4 kg.')
  })
})

describe('chunkForSpeech', () => {
  it('keeps short text in one chunk and empty text in none', () => {
    expect(chunkForSpeech('Hei. Hvordan går det?')).toEqual(['Hei. Hvordan går det?'])
    expect(chunkForSpeech('   ')).toEqual([])
  })

  it('packs whole sentences up to the limit', () => {
    const sentence = 'Dette er en setning på omtrent seksti tegn som gjentas her.'
    const text = Array.from({ length: 40 }, () => sentence).join(' ')
    const chunks = chunkForSpeech(text, 800)
    expect(chunks.length).toBeGreaterThan(1)
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(800)
      expect(chunk.endsWith('her.')).toBe(true)
    }
    expect(chunks.join(' ')).toBe(text)
  })

  it('splits an over-long sentence at clause punctuation, then between words', () => {
    const clause = 'ett ord til og enda flere ord'
    const long = `${Array.from({ length: 12 }, () => clause).join(', ')} og slutt`
    const chunks = chunkForSpeech(long, 100)
    for (const chunk of chunks) expect(chunk.length).toBeLessThanOrEqual(100)
    expect(chunks[0]?.endsWith(',')).toBe(true)
    expect(chunks.join(' ')).toBe(long)

    const words = Array.from({ length: 60 }, (_, index) => `ord${index}`).join(' ')
    const wordChunks = chunkForSpeech(words, 50)
    for (const chunk of wordChunks) expect(chunk.length).toBeLessThanOrEqual(50)
    expect(wordChunks.join(' ')).toBe(words)
  })

  it('never exceeds the default 800 characters for a long reply', () => {
    const reply = Array.from({ length: 300 }, (_, index) => `Punkt ${index} har litt tekst, og litt mer`).join('. ')
    const chunks = chunkForSpeech(reply)
    expect(chunks.length).toBeGreaterThan(5)
    expect(Math.max(...chunks.map((chunk) => chunk.length))).toBeLessThanOrEqual(800)
  })
})
