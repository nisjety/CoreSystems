import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  classifyMicError,
  createSilenceTracker,
  createVoiceRecorder,
  detectMicSupport,
  MicError,
  meterLevelFromDb,
  pickRecorderMime,
  VOICE_BITRATE,
  VOICE_MAX_BYTES,
  VOICE_MAX_MS,
  type RecorderEnv,
} from './voice-recorder'

// Voice mode's recorder: one local take per utterance. These pin the choices
// the transport forces (container, 32 kbps, 60 s / 1.4 MB caps), the local
// silence auto-stop, and that the microphone is always handed back.

class FakeTrack {
  stop = vi.fn()
}

class FakeRecorder {
  static instances: FakeRecorder[] = []
  static supported = new Set(['audio/webm;codecs=opus', 'audio/webm'])
  static isTypeSupported(mimeType: string) {
    return FakeRecorder.supported.has(mimeType)
  }

  mimeType: string
  options: MediaRecorderOptions | undefined
  state: 'inactive' | 'recording' = 'inactive'
  timeslice: number | undefined
  ondataavailable: ((event: { data: Blob }) => void) | null = null
  onstop: (() => void) | null = null
  onerror: ((event: unknown) => void) | null = null

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    this.options = options
    this.mimeType = options?.mimeType ?? ''
    FakeRecorder.instances.push(this)
  }

  start(timeslice?: number) {
    this.timeslice = timeslice
    this.state = 'recording'
  }

  emit(bytes: number) {
    this.ondataavailable?.({ data: new Blob([new Uint8Array(bytes)]) })
  }

  stop() {
    if (this.state === 'inactive') return
    this.state = 'inactive'
    this.emit(5)
    this.onstop?.()
  }
}

/** An AudioContext whose analyser reports a settable amplitude. */
class FakeAudioContext {
  static instances: FakeAudioContext[] = []
  amplitude = 0
  state: 'running' | 'closed' = 'running'
  resume = vi.fn(async () => undefined)
  close = vi.fn(async () => {
    this.state = 'closed'
  })

  constructor() {
    FakeAudioContext.instances.push(this)
  }

  createMediaStreamSource() {
    return { connect: vi.fn(), disconnect: vi.fn() }
  }

  createAnalyser() {
    return {
      fftSize: 0,
      getFloatTimeDomainData: (samples: Float32Array) => {
        samples.fill(this.amplitude)
      },
    }
  }
}

const SPEECH = 0.1 // -20 dBFS
const QUIET = 0.001 // -60 dBFS

function fakeEnv(overrides: Partial<RecorderEnv> = {}) {
  const tracks = [new FakeTrack()]
  const getUserMedia = vi.fn(async (_constraints: MediaStreamConstraints) => ({ getTracks: () => tracks }) as unknown as MediaStream)
  const env: RecorderEnv = {
    AudioContext: FakeAudioContext as unknown as RecorderEnv['AudioContext'],
    MediaRecorder: FakeRecorder as unknown as RecorderEnv['MediaRecorder'],
    getUserMedia,
    isSecureContext: true,
    ...overrides,
  }
  return { env, getUserMedia, tracks }
}

afterEach(() => {
  vi.useRealTimers()
  FakeRecorder.instances = []
  FakeRecorder.supported = new Set(['audio/webm;codecs=opus', 'audio/webm'])
  FakeAudioContext.instances = []
})

describe('pickRecorderMime', () => {
  it('prefers webm/opus where Chrome and Edge record it', () => {
    expect(pickRecorderMime((mime) => mime.startsWith('audio/webm') || mime.startsWith('audio/ogg'))).toBe('audio/webm;codecs=opus')
  })

  it('falls back to ogg/opus, then Safari’s mp4', () => {
    expect(pickRecorderMime((mime) => mime.startsWith('audio/ogg'))).toBe('audio/ogg;codecs=opus')
    expect(pickRecorderMime((mime) => mime === 'audio/mp4')).toBe('audio/mp4')
  })

  it('lets the browser choose when it cannot be asked or supports none of them', () => {
    expect(pickRecorderMime(undefined)).toBeUndefined()
    expect(pickRecorderMime(() => false)).toBeUndefined()
    expect(pickRecorderMime(() => {
      throw new Error('probe failed')
    })).toBeUndefined()
  })
})

describe('classifyMicError', () => {
  it('maps each getUserMedia failure to what the person can do about it', () => {
    expect(classifyMicError(new DOMException('denied', 'NotAllowedError'))).toBe('denied')
    expect(classifyMicError(new DOMException('none', 'NotFoundError'))).toBe('no-device')
    expect(classifyMicError(new DOMException('busy', 'NotReadableError'))).toBe('busy')
    expect(classifyMicError(new DOMException('insecure', 'SecurityError'))).toBe('insecure')
    expect(classifyMicError(new DOMException('nope', 'NotSupportedError'))).toBe('unsupported')
    expect(classifyMicError(new MicError('insecure'))).toBe('insecure')
    expect(classifyMicError(new Error('something else'))).toBe('unknown')
    expect(classifyMicError(null)).toBe('unknown')
  })

  it('treats an insecure page and a missing API as their own cases', () => {
    expect(detectMicSupport({ ...fakeEnv().env, isSecureContext: false })).toBe('insecure')
    expect(detectMicSupport({ ...fakeEnv().env, getUserMedia: undefined })).toBe('unsupported')
    expect(detectMicSupport({ ...fakeEnv().env, MediaRecorder: undefined })).toBe('unsupported')
    expect(detectMicSupport(fakeEnv().env)).toBe('ok')
  })
})

describe('createSilenceTracker', () => {
  it('stops about 1.5 s after at least 300 ms of speech', () => {
    const tracker = createSilenceTracker({ thresholdDb: -45, silenceMs: 1500, minSpeechMs: 300 })
    let at = 0
    for (; at <= 400; at += 50) expect(tracker.sample(-20, at)).toBe(false)
    expect(tracker.speechMs).toBeGreaterThanOrEqual(300)
    let stoppedAt: number | null = null
    for (; at <= 3000 && stoppedAt === null; at += 50) {
      if (tracker.sample(-60, at)) stoppedAt = at
    }
    expect(stoppedAt).toBe(400 + 1500)
  })

  it('never stops on silence before anything was said', () => {
    const tracker = createSilenceTracker()
    for (let at = 0; at <= 10_000; at += 50) expect(tracker.sample(-70, at)).toBe(false)
  })

  it('does not count a blip shorter than the minimum as speech', () => {
    const tracker = createSilenceTracker({ minSpeechMs: 300 })
    tracker.sample(-20, 0)
    tracker.sample(-20, 100)
    for (let at = 150; at <= 5000; at += 50) expect(tracker.sample(-60, at)).toBe(false)
  })

  it('maps dBFS onto the meter', () => {
    expect(meterLevelFromDb(Number.NEGATIVE_INFINITY)).toBe(0)
    expect(meterLevelFromDb(-80)).toBe(0)
    expect(meterLevelFromDb(-30)).toBeCloseTo(0.5)
    expect(meterLevelFromDb(0)).toBe(1)
  })
})

describe('createVoiceRecorder', () => {
  it('records mono with voice processing, an explicit container and 32 kbps', async () => {
    const { env, getUserMedia } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()

    expect(getUserMedia).toHaveBeenCalledWith({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    })
    const take = FakeRecorder.instances[0]!
    expect(take.options).toEqual({ audioBitsPerSecond: VOICE_BITRATE, mimeType: 'audio/webm;codecs=opus' })
    expect(VOICE_BITRATE).toBe(32_000)
    // Chunked, so the byte cap can be enforced while recording.
    expect(take.timeslice).toBe(1000)
    recorder.cancel()
  })

  it('creates and resumes the AudioContext inside the tap, before the permission prompt resolves', async () => {
    let grant!: (stream: MediaStream) => void
    const tracks = [new FakeTrack()]
    const { env } = fakeEnv({
      getUserMedia: vi.fn(() => new Promise<MediaStream>((resolve) => {
        grant = resolve
      })),
    })
    const recorder = createVoiceRecorder({ env })
    const started = recorder.start()

    expect(FakeAudioContext.instances).toHaveLength(1)
    expect(FakeAudioContext.instances[0]!.resume).toHaveBeenCalledTimes(1)

    grant({ getTracks: () => tracks } as unknown as MediaStream)
    await started
    recorder.stop()
    await expect(recorder.result).resolves.toMatchObject({ reason: 'manual' })
  })

  it('hands back one Blob labelled with the recorded container and releases the microphone', async () => {
    const { env, tracks } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    FakeRecorder.instances[0]!.emit(10)
    recorder.stop()

    const recording = await recorder.result
    expect(recording?.blob.type).toBe('audio/webm;codecs=opus')
    expect(recording?.blob.size).toBe(15)
    expect(recording?.reason).toBe('manual')
    expect(tracks[0]!.stop).toHaveBeenCalled()
    expect(FakeAudioContext.instances[0]!.close).toHaveBeenCalled()
  })

  it('stops by itself after speech followed by silence', async () => {
    vi.useFakeTimers()
    const { env } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    const context = FakeAudioContext.instances[0]!

    context.amplitude = SPEECH
    vi.advanceTimersByTime(500)
    context.amplitude = QUIET
    vi.advanceTimersByTime(1400)
    expect(FakeRecorder.instances[0]!.state).toBe('recording')
    vi.advanceTimersByTime(200)

    await expect(recorder.result).resolves.toMatchObject({ reason: 'silence' })
  })

  it('keeps listening through a quiet start', async () => {
    vi.useFakeTimers()
    const { env } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    FakeAudioContext.instances[0]!.amplitude = QUIET
    vi.advanceTimersByTime(10_000)
    expect(FakeRecorder.instances[0]!.state).toBe('recording')
    recorder.cancel()
  })

  it('reports the input level for the meter', async () => {
    vi.useFakeTimers()
    const { env } = fakeEnv()
    const levels: number[] = []
    const recorder = createVoiceRecorder({ env, onLevel: (level) => levels.push(level) })
    await recorder.start()
    FakeAudioContext.instances[0]!.amplitude = SPEECH
    vi.advanceTimersByTime(60)
    expect(levels.at(-1)).toBeCloseTo(meterLevelFromDb(-20))
    recorder.cancel()
    expect(levels.at(-1)).toBe(0)
  })

  it('caps a take at 60 seconds', async () => {
    vi.useFakeTimers()
    const { env, tracks } = fakeEnv()
    const recorder = createVoiceRecorder({ env, silence: false })
    await recorder.start()
    vi.advanceTimersByTime(VOICE_MAX_MS - 1)
    expect(FakeRecorder.instances[0]!.state).toBe('recording')
    vi.advanceTimersByTime(1)
    await expect(recorder.result).resolves.toMatchObject({ reason: 'max-duration' })
    expect(tracks[0]!.stop).toHaveBeenCalled()
  })

  it('caps a take at 1.4 MB, under what the 2 MiB body limit allows after base64', async () => {
    const { env } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    const take = FakeRecorder.instances[0]!
    take.emit(VOICE_MAX_BYTES - 1)
    expect(take.state).toBe('recording')
    take.emit(1)
    await expect(recorder.result).resolves.toMatchObject({ reason: 'max-bytes' })
    expect(Math.ceil((VOICE_MAX_BYTES * 4) / 3)).toBeLessThan(2 * 1024 * 1024)
  })

  it('hands the microphone back when cancelled while the permission prompt is open', async () => {
    let grant!: (stream: MediaStream) => void
    const tracks = [new FakeTrack()]
    const { env } = fakeEnv({
      getUserMedia: vi.fn(() => new Promise<MediaStream>((resolve) => {
        grant = resolve
      })),
    })
    const recorder = createVoiceRecorder({ env })
    const started = recorder.start()
    recorder.cancel()
    grant({ getTracks: () => tracks } as unknown as MediaStream)
    await started

    expect(tracks[0]!.stop).toHaveBeenCalled()
    expect(FakeRecorder.instances).toHaveLength(0)
    await expect(recorder.result).resolves.toBeNull()
  })

  it('discards a cancelled take and stops the tracks', async () => {
    const { env, tracks } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    recorder.cancel()
    await expect(recorder.result).resolves.toBeNull()
    expect(tracks[0]!.stop).toHaveBeenCalled()
  })

  it('rejects a denied microphone as a MicError and releases the audio context', async () => {
    const { env } = fakeEnv({
      getUserMedia: vi.fn(async () => {
        throw new DOMException('Permission denied', 'NotAllowedError')
      }),
    })
    const recorder = createVoiceRecorder({ env })
    await expect(recorder.start()).rejects.toMatchObject({ name: 'MicError', kind: 'denied' })
    await expect(recorder.result).resolves.toBeNull()
    expect(FakeAudioContext.instances[0]!.close).toHaveBeenCalled()
  })

  it('never asks for the microphone on an insecure page', async () => {
    const { env, getUserMedia } = fakeEnv({ isSecureContext: false })
    const recorder = createVoiceRecorder({ env })
    await expect(recorder.start()).rejects.toMatchObject({ kind: 'insecure' })
    expect(getUserMedia).not.toHaveBeenCalled()
    expect(FakeAudioContext.instances).toHaveLength(0)
  })

  it('stops the tracks when the recorder fails mid-take', async () => {
    const { env, tracks } = fakeEnv()
    const recorder = createVoiceRecorder({ env })
    await recorder.start()
    FakeRecorder.instances[0]!.onerror?.(new Event('error'))
    await expect(recorder.result).rejects.toMatchObject({ kind: 'unknown' })
    expect(tracks[0]!.stop).toHaveBeenCalled()
  })

  it('falls back to the browser’s own container when the preferred one is refused', async () => {
    const { env } = fakeEnv()
    const Refusing = class extends FakeRecorder {
      constructor(stream: MediaStream, options?: MediaRecorderOptions) {
        if (options?.mimeType) throw new DOMException('no', 'NotSupportedError')
        super(stream, options)
        this.mimeType = 'audio/mp4'
      }
    }
    const recorder = createVoiceRecorder({ env: { ...env, MediaRecorder: Refusing as unknown as RecorderEnv['MediaRecorder'] } })
    await recorder.start()
    expect(FakeRecorder.instances[0]!.options).toEqual({ audioBitsPerSecond: VOICE_BITRATE })
    recorder.stop()
    await expect(recorder.result).resolves.toMatchObject({ blob: expect.objectContaining({ type: 'audio/mp4' }) })
  })
})
