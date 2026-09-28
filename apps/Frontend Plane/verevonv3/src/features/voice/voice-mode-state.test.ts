import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/http'
import {
  canRecordAgain,
  canStartTake,
  classifyDictateError,
  initialVoiceModeState,
  voiceErrorFromMic,
  voiceErrorMessage,
  voiceModeReducer,
  voiceStatusText,
  type VoiceModeEvent,
  type VoiceModeState,
} from './voice-mode-state'

const tr = (no: string) => no

function run(events: VoiceModeEvent[], from: VoiceModeState = initialVoiceModeState(false)): VoiceModeState {
  return events.reduce(voiceModeReducer, from)
}

const toReview: VoiceModeEvent[] = [
  { type: 'tap' },
  { type: 'mic-ready' },
  { type: 'recorded' },
  { type: 'transcribed', text: '  Hei på deg  ', cleaned: true },
]

describe('voiceModeReducer', () => {
  it('walks a spoken turn from tap to review', () => {
    const states = toReview.reduce<VoiceModeState[]>(
      (all, event) => [...all, voiceModeReducer(all.at(-1)!, event)],
      [initialVoiceModeState(false)],
    )
    expect(states.map((state) => state.phase)).toEqual(['ready', 'requesting', 'recording', 'transcribing', 'review'])
    expect(states.at(-1)).toMatchObject({ transcript: 'Hei på deg', cleaned: true, error: null })
  })

  it('keeps the raw-transcript flag from the dictate route', () => {
    const state = run([...toReview.slice(0, 3), { type: 'transcribed', text: 'rå tekst', cleaned: false }])
    expect(state).toMatchObject({ phase: 'review', cleaned: false })
  })

  it('treats an empty transcript as no speech', () => {
    const state = run([...toReview.slice(0, 3), { type: 'transcribed', text: '   ', cleaned: true }])
    expect(state).toMatchObject({ phase: 'error', error: 'no-speech' })
    expect(canRecordAgain(state)).toBe(true)
  })

  it('sends, waits for the reply, and returns to ready without opening the microphone', () => {
    const awaiting = run([...toReview, { type: 'send' }, { type: 'sent', awaitReply: true }])
    expect(awaiting.phase).toBe('awaiting')
    expect(canStartTake(awaiting)).toBe(false)
    const done = voiceModeReducer(awaiting, { type: 'reply-finished' })
    expect(done).toMatchObject({ phase: 'ready', replyReceived: true, transcript: '' })
    expect(voiceStatusText(done, tr)).toBe('Svar mottatt')
    // Nothing moves on from ready except a tap.
    expect(voiceModeReducer(done, { type: 'mic-ready' })).toBe(done)
  })

  it('reads the finished reply aloud with the microphone shut, then returns to ready', () => {
    const awaiting = run([...toReview, { type: 'send' }, { type: 'sent', awaitReply: true }])
    const speaking = voiceModeReducer(awaiting, { type: 'reply-finished', speak: true })
    expect(speaking.phase).toBe('speaking')
    expect(voiceStatusText(speaking, tr)).toBe('Leser opp svaret …')
    // No take while audio plays: the microphone would record the reply.
    expect(canStartTake(speaking)).toBe(false)
    expect(voiceModeReducer(speaking, { type: 'tap' })).toBe(speaking)
    const done = voiceModeReducer(speaking, { type: 'speech-ended' })
    expect(done).toMatchObject({ phase: 'ready', error: null })
    expect(voiceStatusText(done, tr)).toBe('Trykk for å snakke')
  })

  it('fails closed when the reply cannot be read aloud', () => {
    const speaking = run([...toReview, { type: 'send' }, { type: 'sent', awaitReply: true }, { type: 'reply-finished', speak: true }])
    const failed = voiceModeReducer(speaking, { type: 'speech-failed' })
    expect(failed).toMatchObject({ phase: 'ready', replyReceived: true, error: 'speech-unavailable' })
    expect(voiceErrorMessage(failed.error!, tr)).toBe('Opplesing er utilgjengelig. Svaret vises i chatten.')
    expect(canStartTake(failed)).toBe(true)
    // Speech events mean nothing outside the speaking phase.
    expect(voiceModeReducer(failed, { type: 'speech-ended' })).toBe(failed)
    expect(voiceModeReducer(speaking, { type: 'blocked', blocked: true }).phase).toBe('blocked')
  })

  it('goes straight back to ready where the host does not show replies', () => {
    expect(run([...toReview, { type: 'send' }, { type: 'sent', awaitReply: false }]).phase).toBe('ready')
  })

  it('keeps the transcript under review when the send fails', () => {
    const state = run([...toReview, { type: 'send' }, { type: 'failed', error: 'send-failed', detail: 'Koble til abonnementet' }])
    expect(state).toMatchObject({ phase: 'review', transcript: 'Hei på deg', error: 'send-failed', errorDetail: 'Koble til abonnementet' })
    expect(voiceErrorMessage(state.error!, tr, state.errorDetail)).toBe('Koble til abonnementet')
  })

  it('does not send an emptied transcript', () => {
    const review = run([...toReview, { type: 'edit', text: '  ' }])
    expect(voiceModeReducer(review, { type: 'send' })).toBe(review)
  })

  it('records again from review, discarding the transcript', () => {
    const state = run([...toReview, { type: 'tap' }])
    expect(state).toMatchObject({ phase: 'requesting', transcript: '' })
  })

  it('blocks from any state in Temporary chat and ignores everything until unblocked', () => {
    const recording = run(toReview.slice(0, 2))
    const blocked = voiceModeReducer(recording, { type: 'blocked', blocked: true })
    expect(blocked.phase).toBe('blocked')
    expect(voiceModeReducer(blocked, { type: 'tap' })).toBe(blocked)
    expect(voiceStatusText(blocked, tr)).toMatch(/midlertidig chat/)
    expect(voiceModeReducer(blocked, { type: 'blocked', blocked: false }).phase).toBe('ready')
    expect(initialVoiceModeState(true).phase).toBe('blocked')
  })

  it('offers no retry for failures a new take cannot fix', () => {
    for (const error of ['mic-insecure', 'unsupported', 'zdr-unavailable'] as const) {
      const state = run([{ type: 'tap' }, { type: 'failed', error }])
      expect(state.phase).toBe('error')
      expect(canStartTake(state)).toBe(false)
    }
    expect(canStartTake(run([{ type: 'tap' }, { type: 'failed', error: 'mic-denied' }]))).toBe(true)
  })
})

describe('classifyDictateError', () => {
  it('maps by HTTP status first', () => {
    expect(classifyDictateError(new ApiError('no speech detected in the audio', 422, null))).toBe('no-speech')
    expect(classifyDictateError(new ApiError('Request failed (413)', 413, null))).toBe('too-long')
    expect(classifyDictateError(new ApiError('bad audio', 400, 'invalid_input'))).toBe('too-long')
    expect(classifyDictateError(new ApiError('timeout', 504, 'model_request_timeout'))).toBe('timeout')
    expect(classifyDictateError(new ApiError('zdr', 412, null))).toBe('zdr-unavailable')
    expect(classifyDictateError(new ApiError('down', 502, null))).toBe('transcription-unavailable')
    expect(classifyDictateError(new ApiError('down', 503, 'no_speech_detected'))).toBe('transcription-unavailable')
  })

  it('falls back to the code, then to unavailable, and treats an abort as a cancel', () => {
    expect(classifyDictateError(new ApiError('odd', 409, 'no_speech_detected'))).toBe('no-speech')
    expect(classifyDictateError(new TypeError('Failed to fetch'))).toBe('transcription-unavailable')
    expect(classifyDictateError(new DOMException('aborted', 'AbortError'))).toBe('aborted')
  })

  it('has Norwegian copy for every case', () => {
    expect(voiceErrorMessage('no-speech', tr)).toBe('Hørte ingen tale. Prøv igjen.')
    expect(voiceErrorMessage('too-long', tr)).toBe('Opptaket var for langt. Hold hver melding under ett minutt.')
    expect(voiceErrorMessage('timeout', tr)).toBe('Transkriberingen tok for lang tid. Prøv igjen.')
    expect(voiceErrorMessage('zdr-unavailable', tr)).toBe('Tale er ikke tilgjengelig med nullagring for denne organisasjonen.')
    expect(voiceErrorMessage('transcription-unavailable', tr)).toBe('Transkribering er utilgjengelig akkurat nå. Ingenting ble sendt.')
    expect(voiceErrorMessage(voiceErrorFromMic('denied'), tr)).toMatch(/^Mikrofontilgang er blokkert\./)
    expect(voiceErrorMessage(voiceErrorFromMic('no-device'), tr)).toBe('Fant ingen mikrofon.')
    expect(voiceErrorMessage(voiceErrorFromMic('busy'), tr)).toBe('Mikrofonen brukes av et annet program.')
    expect(voiceErrorMessage(voiceErrorFromMic('insecure'), tr)).toBe('Mikrofonen krever en sikker tilkobling (HTTPS).')
    expect(voiceErrorMessage(voiceErrorFromMic('unsupported'), tr)).toMatch(/^Stemmeopptak støttes ikke/)
  })
})
