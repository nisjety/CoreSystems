/**
 * Microphone capture for Voice mode: one take per utterance, recorded on the
 * device and handed to the governed dictate route as a single Blob.
 *
 * Nothing here talks to a speech service. The level meter and the silence
 * auto-stop read the stream through a Web Audio `AnalyserNode`, which runs
 * locally; the browser's own speech recognition is never used, because every
 * vendor recogniser sends the audio to that vendor instead of through
 * Verevon's provider and audit controls.
 *
 * The caps are this client's own. The BFF and model-gateway both accept
 * 8 MiB on the audio routes (base64 adds a third), so the 1.4 MB cap is well
 * inside what the transport takes. At 32 kbps a 60-second take is roughly
 * 240 KB.
 */

export const VOICE_MAX_MS = 60_000
export const VOICE_MAX_BYTES = 1_400_000
export const VOICE_BITRATE = 32_000

/**
 * Preference order for the recording container. Chrome and Edge record
 * webm/opus, Firefox also offers ogg/opus, and Safari records AAC in mp4
 * (which the dictate client labels `m4a`). Always feature-detected.
 */
const RECORDER_MIME_PREFERENCE = [
  'audio/webm;codecs=opus',
  'audio/webm',
  'audio/ogg;codecs=opus',
  'audio/mp4;codecs=mp4a.40.2',
  'audio/mp4',
  'audio/aac',
] as const

/** One speech channel with the browser's voice processing on. */
export const VOICE_AUDIO_CONSTRAINTS: MediaTrackConstraints = {
  channelCount: 1,
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true,
}

export type MicErrorKind = 'denied' | 'no-device' | 'busy' | 'insecure' | 'unsupported' | 'unknown'

export class MicError extends Error {
  readonly kind: MicErrorKind

  constructor(kind: MicErrorKind, cause?: unknown) {
    super(`Microphone unavailable: ${kind}`, { cause })
    this.name = 'MicError'
    this.kind = kind
  }
}

type MediaRecorderCtor = {
  new (stream: MediaStream, options?: MediaRecorderOptions): MediaRecorder
  isTypeSupported?: (mimeType: string) => boolean
}

type AudioContextCtor = new () => AudioContext

/** The browser surface the recorder needs, injectable so tests can fake it. */
export type RecorderEnv = {
  AudioContext?: AudioContextCtor
  MediaRecorder?: MediaRecorderCtor
  getUserMedia?: (constraints: MediaStreamConstraints) => Promise<MediaStream>
  isSecureContext?: boolean
}

export function browserRecorderEnv(): RecorderEnv {
  const mediaDevices = typeof navigator === 'undefined' ? undefined : navigator.mediaDevices
  const audioGlobals = globalThis as unknown as {
    AudioContext?: AudioContextCtor
    webkitAudioContext?: AudioContextCtor
  }
  return {
    AudioContext: audioGlobals.AudioContext ?? audioGlobals.webkitAudioContext,
    MediaRecorder: typeof MediaRecorder === 'undefined' ? undefined : (MediaRecorder as MediaRecorderCtor),
    getUserMedia: typeof mediaDevices?.getUserMedia === 'function'
      ? (constraints) => mediaDevices.getUserMedia(constraints)
      : undefined,
    isSecureContext: typeof window === 'undefined' ? undefined : window.isSecureContext,
  }
}

/**
 * The first container this browser can record, or `undefined` to let the
 * browser choose (older engines without `isTypeSupported`).
 */
export function pickRecorderMime(isTypeSupported: ((mimeType: string) => boolean) | undefined): string | undefined {
  if (typeof isTypeSupported !== 'function') return undefined
  for (const mimeType of RECORDER_MIME_PREFERENCE) {
    try {
      if (isTypeSupported(mimeType)) return mimeType
    } catch {
      // A probe that throws is a "no" for that container.
    }
  }
  return undefined
}

/** Map a `getUserMedia` / `MediaRecorder` failure to what the person can do about it. */
export function classifyMicError(error: unknown): MicErrorKind {
  if (error instanceof MicError) return error.kind
  const name = error && typeof error === 'object' && 'name' in error ? String((error as { name: unknown }).name) : ''
  switch (name) {
    case 'NotAllowedError':
    case 'PermissionDeniedError':
      return 'denied'
    case 'NotFoundError':
    case 'DevicesNotFoundError':
    case 'OverconstrainedError':
      return 'no-device'
    case 'NotReadableError':
    case 'TrackStartError':
    case 'AbortError':
      return 'busy'
    case 'SecurityError':
      return 'insecure'
    case 'NotSupportedError':
      return 'unsupported'
    default:
      return 'unknown'
  }
}

/** `getUserMedia` needs a secure context; without one the API is simply absent. */
export function detectMicSupport(env: RecorderEnv = browserRecorderEnv()): 'ok' | 'insecure' | 'unsupported' {
  if (env.isSecureContext === false) return 'insecure'
  if (!env.getUserMedia || !env.MediaRecorder) return 'unsupported'
  return 'ok'
}

export type SilenceTrackerOptions = {
  /** Level (dBFS) at or above which a sample counts as speech. */
  thresholdDb?: number
  /** Quiet time after speech that ends the take. */
  silenceMs?: number
  /** Speech needed before silence may end the take, so a pause before speaking does not. */
  minSpeechMs?: number
}

export type SilenceTracker = {
  /** Feed one level sample; true once the speaker has talked and then gone quiet. */
  sample: (levelDb: number, atMs: number) => boolean
  readonly speechMs: number
}

export function createSilenceTracker(options: SilenceTrackerOptions = {}): SilenceTracker {
  const thresholdDb = options.thresholdDb ?? -45
  const silenceMs = options.silenceMs ?? 1500
  const minSpeechMs = options.minSpeechMs ?? 300
  let speechMs = 0
  let lastSampleAt: number | null = null
  let lastSpeechAt: number | null = null

  return {
    sample(levelDb, atMs) {
      const elapsed = lastSampleAt === null ? 0 : Math.max(0, atMs - lastSampleAt)
      lastSampleAt = atMs
      if (levelDb >= thresholdDb) {
        speechMs += elapsed
        lastSpeechAt = atMs
        return false
      }
      return speechMs >= minSpeechMs && lastSpeechAt !== null && atMs - lastSpeechAt >= silenceMs
    },
    get speechMs() {
      return speechMs
    },
  }
}

/** dBFS mapped onto 0..1 for the meter: -60 dB and below is empty, 0 dB is full. */
export function meterLevelFromDb(levelDb: number): number {
  if (!Number.isFinite(levelDb)) return 0
  return Math.min(1, Math.max(0, (levelDb + 60) / 60))
}

type LevelMeter = { readDb: () => number; dispose: () => void }

function createLevelMeter(context: AudioContext, stream: MediaStream): LevelMeter {
  const source = context.createMediaStreamSource(stream)
  const analyser = context.createAnalyser()
  analyser.fftSize = 1024
  source.connect(analyser)
  const samples = new Float32Array(analyser.fftSize)
  return {
    readDb() {
      analyser.getFloatTimeDomainData(samples)
      let sum = 0
      for (const sample of samples) sum += sample * sample
      const rms = Math.sqrt(sum / samples.length)
      return rms > 0 ? 20 * Math.log10(rms) : Number.NEGATIVE_INFINITY
    },
    dispose() {
      try {
        source.disconnect()
      } catch {
        // Already disconnected.
      }
    },
  }
}

export type VoiceStopReason = 'manual' | 'silence' | 'max-duration' | 'max-bytes'

export type VoiceRecording = {
  blob: Blob
  durationMs: number
  reason: VoiceStopReason
}

export type VoiceRecorderOptions = {
  maxMs?: number
  maxBytes?: number
  audioBitsPerSecond?: number
  /** `false` turns the silence auto-stop off; a second tap still stops the take. */
  silence?: SilenceTrackerOptions | false
  /** How often the level is sampled for the meter and the silence check. */
  sampleIntervalMs?: number
  /** Chunk interval, so the byte cap can be enforced while recording. */
  timesliceMs?: number
  /** Local input level, 0..1, for the meter. */
  onLevel?: (level: number) => void
  env?: RecorderEnv
}

export type VoiceRecorder = {
  /**
   * Call from the tap handler. The AudioContext is created and resumed before
   * the first await, because iOS only lets audio start inside a user gesture.
   * Rejects with a {@link MicError}.
   */
  start: () => Promise<void>
  /** Finish the take; `result` resolves with the recording. */
  stop: () => void
  /** Discard the take; `result` resolves with `null`. Always releases the microphone. */
  cancel: () => void
  /**
   * Settles once: the recording, `null` when cancelled or never started, or a
   * rejection when the recorder itself failed mid-take.
   */
  readonly result: Promise<VoiceRecording | null>
}

/** A single-use recorder for one utterance. */
export function createVoiceRecorder(options: VoiceRecorderOptions = {}): VoiceRecorder {
  const env = options.env ?? browserRecorderEnv()
  const maxMs = options.maxMs ?? VOICE_MAX_MS
  const maxBytes = options.maxBytes ?? VOICE_MAX_BYTES
  const bitrate = options.audioBitsPerSecond ?? VOICE_BITRATE
  const sampleIntervalMs = options.sampleIntervalMs ?? 50
  const timesliceMs = options.timesliceMs ?? 1000
  const tracker = options.silence === false ? null : createSilenceTracker(options.silence)

  let phase: 'idle' | 'requesting' | 'recording' | 'stopping' | 'done' = 'idle'
  let cancelled = false
  let stream: MediaStream | undefined
  let recorder: MediaRecorder | undefined
  let audioContext: AudioContext | undefined
  let meter: LevelMeter | undefined
  let chunks: Blob[] = []
  let bytes = 0
  let startedAt = 0
  let stopReason: VoiceStopReason = 'manual'
  let maxTimer: ReturnType<typeof setTimeout> | undefined
  let sampleTimer: ReturnType<typeof setInterval> | undefined

  let settled = false
  let resolveResult!: (value: VoiceRecording | null) => void
  let rejectResult!: (reason: unknown) => void
  const result = new Promise<VoiceRecording | null>((resolve, reject) => {
    resolveResult = resolve
    rejectResult = reject
  })
  // Callers await `result` once a take has started; this keeps a rejection
  // from surfacing as unhandled before they do.
  result.catch(() => undefined)
  const settle = (value: VoiceRecording | null) => {
    if (settled) return
    settled = true
    resolveResult(value)
  }
  const fail = (reason: unknown) => {
    if (settled) return
    settled = true
    rejectResult(reason)
  }

  const clearTimers = () => {
    if (maxTimer !== undefined) clearTimeout(maxTimer)
    if (sampleTimer !== undefined) clearInterval(sampleTimer)
    maxTimer = undefined
    sampleTimer = undefined
  }

  const releaseDevices = () => {
    clearTimers()
    meter?.dispose()
    meter = undefined
    stream?.getTracks().forEach((track) => {
      try {
        track.stop()
      } catch {
        // A track that already stopped needs nothing.
      }
    })
    if (audioContext && audioContext.state !== 'closed') {
      void audioContext.close().catch(() => undefined)
    }
    audioContext = undefined
    options.onLevel?.(0)
  }

  const requestStop = (reason: VoiceStopReason) => {
    if (phase !== 'recording') return
    phase = 'stopping'
    stopReason = reason
    clearTimers()
    try {
      recorder?.stop()
    } catch (error) {
      phase = 'done'
      releaseDevices()
      fail(new MicError('unknown', error))
    }
  }

  const start = async () => {
    if (phase !== 'idle') throw new Error('A voice recorder records one take.')
    phase = 'requesting'
    const support = detectMicSupport(env)
    if (support !== 'ok') {
      phase = 'done'
      settle(null)
      throw new MicError(support)
    }
    if (env.AudioContext && (tracker || options.onLevel)) {
      try {
        audioContext = new env.AudioContext()
        void audioContext.resume?.()?.catch(() => undefined)
      } catch {
        // No meter and no silence auto-stop; a second tap still stops.
        audioContext = undefined
      }
    }

    let acquired: MediaStream
    try {
      acquired = await env.getUserMedia!({ audio: VOICE_AUDIO_CONSTRAINTS })
    } catch (error) {
      phase = 'done'
      releaseDevices()
      settle(null)
      throw new MicError(classifyMicError(error), error)
    }
    stream = acquired
    if (cancelled || phase !== 'requesting') {
      // Cancelled while the permission prompt was open: nobody wants this
      // stream any more, so hand the microphone straight back.
      releaseDevices()
      return
    }

    const Recorder = env.MediaRecorder!
    const mimeType = pickRecorderMime(Recorder.isTypeSupported?.bind(Recorder))
    let take: MediaRecorder
    try {
      take = createMediaRecorder(Recorder, acquired, mimeType, bitrate)
    } catch (error) {
      phase = 'done'
      releaseDevices()
      settle(null)
      throw new MicError('unsupported', error)
    }
    recorder = take
    chunks = []
    bytes = 0
    take.ondataavailable = (event) => {
      if (!event.data || event.data.size === 0) return
      chunks.push(event.data)
      bytes += event.data.size
      if (bytes >= maxBytes) requestStop('max-bytes')
    }
    take.onstop = () => {
      const durationMs = Math.max(0, Date.now() - startedAt)
      phase = 'done'
      releaseDevices()
      if (cancelled) {
        settle(null)
        return
      }
      const type = take.mimeType || mimeType || 'audio/webm'
      settle({ blob: new Blob(chunks, { type }), durationMs, reason: stopReason })
      chunks = []
    }
    take.onerror = (event) => {
      phase = 'done'
      releaseDevices()
      if (cancelled) settle(null)
      else fail(new MicError('unknown', event))
    }

    try {
      take.start(timesliceMs)
    } catch (error) {
      phase = 'done'
      releaseDevices()
      settle(null)
      throw new MicError(classifyMicError(error), error)
    }
    startedAt = Date.now()
    phase = 'recording'
    maxTimer = setTimeout(() => requestStop('max-duration'), maxMs)

    if (audioContext) {
      try {
        meter = createLevelMeter(audioContext, acquired)
      } catch {
        meter = undefined
      }
    }
    const activeMeter = meter
    if (activeMeter) {
      sampleTimer = setInterval(() => {
        if (phase !== 'recording') return
        const levelDb = activeMeter.readDb()
        options.onLevel?.(meterLevelFromDb(levelDb))
        if (tracker?.sample(levelDb, Date.now())) requestStop('silence')
      }, sampleIntervalMs)
    }
  }

  const cancel = () => {
    if (cancelled || phase === 'done') return
    cancelled = true
    const wasRecording = phase === 'recording' || phase === 'stopping'
    phase = 'done'
    if (wasRecording) {
      try {
        recorder?.stop()
      } catch {
        // Stopping a recorder that already stopped needs nothing.
      }
    }
    // Idle or waiting on the permission prompt: `start` releases a stream
    // that arrives after this point.
    releaseDevices()
    settle(null)
  }

  return {
    start,
    stop: () => {
      // Nothing has been recorded yet, so a stop is a cancel.
      if (phase === 'idle' || phase === 'requesting') cancel()
      else requestStop('manual')
    },
    cancel,
    result,
  }
}

function createMediaRecorder(
  Recorder: MediaRecorderCtor,
  stream: MediaStream,
  mimeType: string | undefined,
  audioBitsPerSecond: number,
): MediaRecorder {
  try {
    return new Recorder(stream, { audioBitsPerSecond, ...(mimeType ? { mimeType } : {}) })
  } catch (error) {
    // A container the probe claimed but the constructor refuses: let the
    // browser pick its own, keeping the bitrate.
    if (!mimeType) throw error
    return new Recorder(stream, { audioBitsPerSecond })
  }
}
