import { createSignal, onCleanup, type Accessor } from 'solid-js'
import {
  chunkForSpeech,
  SPEECH_CHUNK_MAX_CHARS,
  speakableText,
  synthesizeSpeech,
  ttsLanguageOf,
  ttsVoiceFor,
  type SynthesizeSpeechInput,
} from '@/shared/api/speech-client'

/**
 * Governed read-aloud playback, shared by "Les høyt" and Voice mode so only
 * one thing ever speaks.
 *
 * - One `HTMLAudioElement`, unlocked during a user tap: iOS refuses `play()`
 *   for audio that starts after an async response unless that element already
 *   played inside a gesture.
 * - The reply is spoken in chunks (see `chunkForSpeech`); the next chunk is
 *   synthesised while the current one plays.
 * - Audio only ever plays from `blob:` URLs, each revoked once used. The CSP's
 *   `media-src` blocks `data:` audio.
 * - Nothing here falls back to the browser's own speech synthesis.
 */

export type SpeakOutcome = 'finished' | 'stopped' | 'failed'

export type SpeakRequest = {
  /** What is being read (a message id); `playingId()` reports it while it plays. */
  id: string
  /** The reply as markdown; it is made speakable here. */
  text: string
  /** A BCP 47 tag or UI locale; picks the Azure Speech voice. */
  language: string
}

export type SpeechPlayer = {
  /** Call inside a click or key handler, before anything async. Cheap and idempotent. */
  unlock: () => void
  /** Stops whatever was playing, then reads `request`. Resolves when it ends, is stopped, or fails. */
  speak: (request: SpeakRequest) => Promise<SpeakOutcome>
  stop: () => void
  playingId: () => string | null
  subscribe: (listener: (id: string | null) => void) => () => void
}

export type SpeechPlayerDeps = {
  synthesize?: (input: SynthesizeSpeechInput) => Promise<Blob>
  createAudio?: () => HTMLAudioElement
  createObjectURL?: (blob: Blob) => string
  revokeObjectURL?: (url: string) => void
  maxChars?: number
}

function abortError(): DOMException {
  return new DOMException('Reading aloud was stopped.', 'AbortError')
}

/** 10 ms of 8 kHz mono silence: enough for a gesture-time `play()` to unlock the element. */
function silentWav(): Blob {
  const samples = 80
  const buffer = new ArrayBuffer(44 + samples * 2)
  const view = new DataView(buffer)
  const ascii = (offset: number, text: string) => {
    for (let index = 0; index < text.length; index += 1) view.setUint8(offset + index, text.charCodeAt(index))
  }
  ascii(0, 'RIFF')
  view.setUint32(4, 36 + samples * 2, true)
  ascii(8, 'WAVE')
  ascii(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, 8000, true)
  view.setUint32(28, 16000, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  ascii(36, 'data')
  view.setUint32(40, samples * 2, true)
  return new Blob([buffer], { type: 'audio/wav' })
}

export function createSpeechPlayer(deps: SpeechPlayerDeps = {}): SpeechPlayer {
  const synthesize = deps.synthesize ?? synthesizeSpeech
  const createObjectURL = deps.createObjectURL ?? ((blob: Blob) => URL.createObjectURL(blob))
  const revokeObjectURL = deps.revokeObjectURL ?? ((url: string) => URL.revokeObjectURL(url))
  const maxChars = deps.maxChars ?? SPEECH_CHUNK_MAX_CHARS
  const listeners = new Set<(id: string | null) => void>()

  let audio: HTMLAudioElement | undefined
  let unlocked = false
  /** The blob URL the element was last given, so a late unlock cannot pause a reply. */
  let loadedUrl: string | null = null
  let current: { id: string; controller: AbortController } | null = null

  const element = (): HTMLAudioElement => {
    if (!audio) {
      audio = deps.createAudio ? deps.createAudio() : new Audio()
      audio.preload = 'auto'
    }
    return audio
  }

  const notify = () => {
    const id = current?.id ?? null
    for (const listener of [...listeners]) listener(id)
  }

  const unlock = () => {
    // A playing element is already unlocked, and swapping its source would cut the reply off.
    if (unlocked || current) return
    const player = element()
    let url: string
    try {
      url = createObjectURL(silentWav())
    } catch {
      return
    }
    unlocked = true
    loadedUrl = url
    player.src = url
    const settle = (played: boolean) => {
      if (loadedUrl === url) {
        // Refused outright (not superseded by a reply): let the next tap try again.
        if (!played) unlocked = false
        player.pause()
        player.removeAttribute('src')
        loadedUrl = null
      }
      revokeObjectURL(url)
    }
    let attempt: Promise<void> | undefined
    try {
      attempt = player.play()
    } catch {
      settle(false)
      return
    }
    Promise.resolve(attempt).then(
      () => settle(true),
      () => settle(false),
    )
  }

  const playThrough = (player: HTMLAudioElement, url: string, signal: AbortSignal) =>
    new Promise<void>((resolve, reject) => {
      let settled = false
      const finish = (error?: unknown) => {
        if (settled) return
        settled = true
        player.removeEventListener('ended', onEnded)
        player.removeEventListener('error', onError)
        signal.removeEventListener('abort', onAbort)
        if (error === undefined) resolve()
        else reject(error)
      }
      const onEnded = () => finish()
      const onError = () => finish(new Error('Audio playback failed.'))
      const onAbort = () => {
        player.pause()
        finish(abortError())
      }
      if (signal.aborted) {
        finish(abortError())
        return
      }
      player.addEventListener('ended', onEnded)
      player.addEventListener('error', onError)
      signal.addEventListener('abort', onAbort)
      loadedUrl = url
      player.src = url
      let attempt: Promise<void> | undefined
      try {
        attempt = player.play()
      } catch (error) {
        finish(error ?? new Error('Audio playback was refused.'))
        return
      }
      Promise.resolve(attempt).catch((error: unknown) => finish(error ?? new Error('Audio playback was refused.')))
    })

  const stop = () => {
    const session = current
    if (!session) return
    current = null
    session.controller.abort()
    audio?.pause()
    notify()
  }

  const speak = async (request: SpeakRequest): Promise<SpeakOutcome> => {
    stop()
    const chunks = chunkForSpeech(speakableText(request.text), maxChars)
    if (chunks.length === 0) return 'finished'

    const controller = new AbortController()
    const session = { id: request.id, controller }
    current = session
    notify()

    const voice = ttsVoiceFor(request.language)
    const language = ttsLanguageOf(voice)
    const player = element()
    const fetchChunk = (index: number) => {
      const pending = synthesize({ text: chunks[index] ?? '', language, voice, signal: controller.signal })
      // Awaited below; this keeps a prefetch that fails mid-playback from
      // surfacing as an unhandled rejection before its turn comes.
      pending.catch(() => undefined)
      return pending
    }

    let outcome: SpeakOutcome = 'finished'
    try {
      let next: Promise<Blob> | null = fetchChunk(0)
      for (let index = 0; next; index += 1) {
        const blob: Blob = await next
        if (controller.signal.aborted) throw abortError()
        next = index + 1 < chunks.length ? fetchChunk(index + 1) : null
        const url = createObjectURL(blob)
        try {
          await playThrough(player, url, controller.signal)
        } finally {
          if (loadedUrl === url) loadedUrl = null
          revokeObjectURL(url)
        }
      }
    } catch {
      outcome = controller.signal.aborted ? 'stopped' : 'failed'
    } finally {
      if (current === session) {
        current = null
        // Cancels a prefetch still in flight after a failure.
        controller.abort()
        player.pause()
        notify()
      }
    }
    return outcome
  }

  return {
    unlock,
    speak,
    stop,
    playingId: () => current?.id ?? null,
    subscribe: (listener) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

let sharedPlayer: SpeechPlayer | undefined

/** The one player every surface shares. */
export function speechPlayer(): SpeechPlayer {
  sharedPlayer ??= createSpeechPlayer()
  return sharedPlayer
}

/** What `player` is reading right now, as a signal; unsubscribes with its owner. */
export function createPlayingSpeechId(player: SpeechPlayer = speechPlayer()): Accessor<string | null> {
  const [playing, setPlaying] = createSignal<string | null>(player.playingId())
  const unsubscribe = player.subscribe((id) => setPlaying(id))
  onCleanup(unsubscribe)
  return playing
}
