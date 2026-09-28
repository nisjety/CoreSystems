import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { SynthesizeSpeechInput } from '@/shared/api/speech-client'
import { createPlayingSpeechId, createSpeechPlayer, type SpeechPlayerDeps } from './speech-playback'

/** An audio element that plays whatever it is given until the test says it ended. */
class FakeAudio extends EventTarget {
  src = ''
  preload = ''
  sources: string[] = []
  refuse = false
  play = vi.fn(() => {
    this.sources.push(this.src)
    return this.refuse ? Promise.reject(new DOMException('No gesture', 'NotAllowedError')) : Promise.resolve()
  })
  pause = vi.fn()
  removeAttribute = vi.fn((name: string) => {
    if (name === 'src') this.src = ''
  })
  end() {
    this.dispatchEvent(new Event('ended'))
  }
}

type Deferred = { input: SynthesizeSpeechInput; resolve: (blob: Blob) => void; reject: (error: unknown) => void }

function harness(overrides: Partial<SpeechPlayerDeps> = {}) {
  const audio = new FakeAudio()
  const requests: Deferred[] = []
  const synthesize = vi.fn(
    (input: SynthesizeSpeechInput) =>
      new Promise<Blob>((resolve, reject) => {
        requests.push({ input, resolve, reject })
        input.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))
      }),
  )
  let urls = 0
  const created: string[] = []
  const revoked: string[] = []
  const player = createSpeechPlayer({
    synthesize,
    createAudio: () => audio as unknown as HTMLAudioElement,
    createObjectURL: () => {
      const url = `blob:verevon/${++urls}`
      created.push(url)
      return url
    },
    revokeObjectURL: (url) => {
      revoked.push(url)
    },
    maxChars: 30,
    ...overrides,
  })
  return { audio, requests, synthesize, created, revoked, player }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0))
const mp3 = () => new Blob(['mp3'], { type: 'audio/mpeg' })
const THREE_SENTENCES = 'Første setning er her. Andre setning er her. Tredje setning er her.'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('createSpeechPlayer', () => {
  it('reads the reply chunk by chunk, fetching the next one while the current one plays', async () => {
    const { audio, requests, synthesize, created, revoked, player } = harness()
    const seen: Array<string | null> = []
    player.subscribe((id) => seen.push(id))

    const done = player.speak({ id: 'msg-1', text: `**${THREE_SENTENCES}**`, language: 'nb' })
    expect(player.playingId()).toBe('msg-1')
    expect(synthesize).toHaveBeenCalledTimes(1)
    expect(requests[0]?.input).toMatchObject({
      text: 'Første setning er her.',
      language: 'nb-NO',
      voice: 'nb-NO-PernilleNeural',
    })

    requests[0]!.resolve(mp3())
    await settle()
    // Chunk 1 plays from a blob: URL while chunk 2 is already being synthesised.
    expect(audio.sources).toEqual(['blob:verevon/1'])
    expect(synthesize).toHaveBeenCalledTimes(2)
    expect(requests[1]?.input.text).toBe('Andre setning er her.')

    requests[1]!.resolve(mp3())
    audio.end()
    await settle()
    expect(revoked).toContain('blob:verevon/1')
    expect(audio.sources).toEqual(['blob:verevon/1', 'blob:verevon/2'])
    expect(synthesize).toHaveBeenCalledTimes(3)

    requests[2]!.resolve(mp3())
    audio.end()
    await settle()
    audio.end()

    await expect(done).resolves.toBe('finished')
    expect(player.playingId()).toBeNull()
    expect(seen).toEqual(['msg-1', null])
    // Every URL it made, it revoked; it never played anything but blob: audio.
    expect([...revoked].sort()).toEqual([...created].sort())
    expect(audio.sources.every((src) => src.startsWith('blob:'))).toBe(true)
  })

  it('stops on request: the synthesis call is aborted and the audio paused', async () => {
    const { audio, requests, player } = harness()
    const done = player.speak({ id: 'msg-1', text: THREE_SENTENCES, language: 'en' })
    expect(requests[0]?.input.voice).toBe('en-GB-SoniaNeural')
    requests[0]!.resolve(mp3())
    await settle()

    player.stop()
    await expect(done).resolves.toBe('stopped')
    expect(requests[1]?.input.signal?.aborted).toBe(true)
    expect(audio.pause).toHaveBeenCalled()
    expect(player.playingId()).toBeNull()
  })

  it('fails closed when synthesis fails, and does not go on to the next chunk', async () => {
    const { requests, synthesize, player } = harness()
    const done = player.speak({ id: 'msg-1', text: THREE_SENTENCES, language: 'nb' })
    requests[0]!.reject(new Error('412'))
    await expect(done).resolves.toBe('failed')
    expect(synthesize).toHaveBeenCalledTimes(1)
    expect(player.playingId()).toBeNull()
  })

  it('fails closed when the browser refuses to play', async () => {
    const { audio, requests, revoked, player } = harness()
    audio.refuse = true
    const done = player.speak({ id: 'msg-1', text: 'Kort svar.', language: 'nb' })
    requests[0]!.resolve(mp3())
    await expect(done).resolves.toBe('failed')
    expect(revoked).toEqual(['blob:verevon/1'])
  })

  it('reads one thing at a time: a new request stops the previous one', async () => {
    const { requests, player } = harness()
    const first = player.speak({ id: 'msg-1', text: 'Første svar.', language: 'nb' })
    const second = player.speak({ id: 'msg-2', text: 'Andre svar.', language: 'nb' })
    await expect(first).resolves.toBe('stopped')
    expect(requests[0]?.input.signal?.aborted).toBe(true)
    expect(player.playingId()).toBe('msg-2')
    player.stop()
    await expect(second).resolves.toBe('stopped')
  })

  it('has nothing to say for a reply that is only code or links', async () => {
    const { synthesize, player } = harness()
    await expect(player.speak({ id: 'msg-1', text: '```\nx = 1\n```\nhttps://example.com', language: 'nb' })).resolves.toBe(
      'finished',
    )
    expect(synthesize).not.toHaveBeenCalled()
    expect(player.playingId()).toBeNull()
  })

  it('unlocks the element once, during the tap, with a silent blob: clip', async () => {
    const { audio, revoked, player } = harness()
    player.unlock()
    player.unlock()
    expect(audio.play).toHaveBeenCalledTimes(1)
    expect(audio.sources[0]).toMatch(/^blob:/)
    await settle()
    expect(audio.pause).toHaveBeenCalled()
    expect(revoked).toEqual([audio.sources[0]])
  })

  it('never swaps the source of a reply that is playing to unlock', async () => {
    const { audio, requests, player } = harness()
    void player.speak({ id: 'msg-1', text: 'Kort svar.', language: 'nb' })
    requests[0]!.resolve(mp3())
    await settle()
    player.unlock()
    expect(audio.play).toHaveBeenCalledTimes(1)
    player.stop()
  })
})

describe('createPlayingSpeechId', () => {
  it('follows the player and lets go with its owner', async () => {
    const { requests, player } = harness()
    let playing!: () => string | null
    const dispose = createRoot((disposeRoot) => {
      playing = createPlayingSpeechId(player)
      return disposeRoot
    })
    expect(playing()).toBeNull()
    const done = player.speak({ id: 'msg-1', text: 'Kort svar.', language: 'nb' })
    flush()
    expect(playing()).toBe('msg-1')
    requests[0]!.reject(new Error('down'))
    await done
    flush()
    expect(playing()).toBeNull()

    dispose()
    const later = player.speak({ id: 'msg-2', text: 'Et annet svar.', language: 'nb' })
    flush()
    expect(playing()).toBeNull()
    player.stop()
    await later
  })
})
