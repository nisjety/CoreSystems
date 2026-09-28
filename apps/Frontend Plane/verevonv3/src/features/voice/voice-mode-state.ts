import type { MicErrorKind } from './voice-recorder'

/**
 * Voice mode as a pure state machine: tap to talk, review, send through the
 * composer, wait for the reply, optionally hear it read aloud, and back to
 * ready. The microphone is only ever opened by a tap; no transition re-opens
 * it on its own, and it stays shut while a reply is being read aloud.
 */
export type VoicePhase =
  | 'blocked'
  | 'ready'
  | 'requesting'
  | 'recording'
  | 'transcribing'
  | 'review'
  | 'sending'
  | 'awaiting'
  | 'speaking'
  | 'error'

export type VoiceErrorKind =
  | 'mic-denied'
  | 'mic-no-device'
  | 'mic-busy'
  | 'mic-insecure'
  | 'mic-failed'
  | 'unsupported'
  | 'no-speech'
  | 'too-long'
  | 'timeout'
  | 'zdr-unavailable'
  | 'transcription-unavailable'
  | 'send-failed'
  | 'speech-unavailable'

export type VoiceModeState = {
  phase: VoicePhase
  /** The editable transcript under review. */
  transcript: string
  /** False when the dictate route skipped its clean-up pass and returned the raw transcript. */
  cleaned: boolean
  error: VoiceErrorKind | null
  /** The composer's own reason for a failed send, when it gave one. */
  errorDetail: string | null
  /** A reply finished in this dialog; cleared by the next take. Drives the one "Svar mottatt". */
  replyReceived: boolean
}

export type VoiceModeEvent =
  | { type: 'blocked'; blocked: boolean }
  | { type: 'tap' }
  | { type: 'mic-ready' }
  | { type: 'recorded' }
  | { type: 'transcribed'; text: string; cleaned: boolean }
  | { type: 'failed'; error: VoiceErrorKind; detail?: string }
  | { type: 'edit'; text: string }
  | { type: 'send' }
  | { type: 'sent'; awaitReply: boolean }
  /** `speak`: the "Les opp svar" toggle is on, so the finished reply is read aloud next. */
  | { type: 'reply-finished'; speak?: boolean }
  | { type: 'speech-ended' }
  | { type: 'speech-failed' }
  | { type: 'reset' }

const READY: VoiceModeState = {
  phase: 'ready',
  transcript: '',
  cleaned: true,
  error: null,
  errorDetail: null,
  replyReceived: false,
}

export function initialVoiceModeState(blocked: boolean): VoiceModeState {
  return blocked ? { ...READY, phase: 'blocked' } : READY
}

/** Errors a new take cannot fix: the page, the browser or the organisation rules it out. */
const TERMINAL_ERRORS: ReadonlySet<VoiceErrorKind> = new Set(['mic-insecure', 'unsupported', 'zdr-unavailable'])

export function canRecordAgain(state: VoiceModeState): boolean {
  if (state.phase === 'review') return true
  return state.phase === 'error' && state.error !== null && !TERMINAL_ERRORS.has(state.error)
}

/** Whether a tap on the microphone may open it now. */
export function canStartTake(state: VoiceModeState): boolean {
  return state.phase === 'ready' || canRecordAgain(state)
}

export function voiceModeReducer(state: VoiceModeState, event: VoiceModeEvent): VoiceModeState {
  if (event.type === 'blocked') {
    if (event.blocked) return state.phase === 'blocked' ? state : { ...READY, phase: 'blocked' }
    return state.phase === 'blocked' ? READY : state
  }
  if (state.phase === 'blocked') return state

  switch (event.type) {
    case 'tap':
      return canStartTake(state) ? { ...READY, phase: 'requesting' } : state
    case 'mic-ready':
      return state.phase === 'requesting' ? { ...state, phase: 'recording' } : state
    case 'recorded':
      return state.phase === 'recording' ? { ...state, phase: 'transcribing' } : state
    case 'transcribed': {
      if (state.phase !== 'transcribing') return state
      const text = event.text.trim()
      if (!text) return { ...state, phase: 'error', error: 'no-speech', errorDetail: null }
      return { ...state, phase: 'review', transcript: text, cleaned: event.cleaned, error: null, errorDetail: null }
    }
    case 'failed':
      // A failed send keeps the transcript under review so nothing is lost.
      if (state.phase === 'sending') {
        return { ...state, phase: 'review', error: event.error, errorDetail: event.detail ?? null }
      }
      if (state.phase === 'requesting' || state.phase === 'recording' || state.phase === 'transcribing') {
        return { ...state, phase: 'error', transcript: '', error: event.error, errorDetail: event.detail ?? null }
      }
      return state
    case 'edit':
      return state.phase === 'review' ? { ...state, transcript: event.text, error: null, errorDetail: null } : state
    case 'send':
      return state.phase === 'review' && state.transcript.trim()
        ? { ...state, phase: 'sending', error: null, errorDetail: null }
        : state
    case 'sent':
      if (state.phase !== 'sending') return state
      return event.awaitReply ? { ...READY, phase: 'awaiting' } : READY
    case 'reply-finished':
      if (state.phase !== 'awaiting') return state
      return { ...READY, phase: event.speak ? 'speaking' : 'ready', replyReceived: true }
    case 'speech-ended':
      // The reply was heard; the status goes back to the prompt to speak.
      return state.phase === 'speaking' ? READY : state
    case 'speech-failed':
      // Fail closed: no fallback voice, the reply stays in the chat.
      return state.phase === 'speaking' ? { ...READY, replyReceived: true, error: 'speech-unavailable' } : state
    case 'reset':
      return READY
  }
}

export function voiceErrorFromMic(kind: MicErrorKind): VoiceErrorKind {
  switch (kind) {
    case 'denied':
      return 'mic-denied'
    case 'no-device':
      return 'mic-no-device'
    case 'busy':
      return 'mic-busy'
    case 'insecure':
      return 'mic-insecure'
    case 'unsupported':
      return 'unsupported'
    case 'unknown':
      return 'mic-failed'
  }
}

function isAbortError(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && (error as { name?: unknown }).name === 'AbortError')
}

/**
 * Map a dictate failure to copy. Status first: model-gateway's 422 carries its
 * `no_speech_detected` code at the top level, and a 413 from axum has a plain
 * text body with no code at all. `aborted` is a cancel, not an error.
 */
export function classifyDictateError(error: unknown): VoiceErrorKind | 'aborted' {
  if (isAbortError(error)) return 'aborted'
  const record = error && typeof error === 'object' ? (error as { status?: unknown; code?: unknown }) : {}
  const status = typeof record.status === 'number' ? record.status : null
  if (status === 422) return 'no-speech'
  if (status === 413 || status === 400) return 'too-long'
  if (status === 504) return 'timeout'
  if (status === 412) return 'zdr-unavailable'
  if (status !== null && status >= 500) return 'transcription-unavailable'
  if (record.code === 'no_speech_detected') return 'no-speech'
  if (record.code === 'model_request_timeout') return 'timeout'
  return 'transcription-unavailable'
}

type Translate = (noText: string, enText: string) => string

export function voiceErrorMessage(kind: VoiceErrorKind, tr: Translate, detail?: string | null): string {
  switch (kind) {
    case 'mic-denied':
      return tr(
        'Mikrofontilgang er blokkert. Tillat mikrofonen for denne siden i nettleserinnstillingene og prøv igjen.',
        'Microphone access is blocked. Allow the microphone for this site in your browser settings and try again.',
      )
    case 'mic-no-device':
      return tr('Fant ingen mikrofon.', 'No microphone was found.')
    case 'mic-busy':
      return tr('Mikrofonen brukes av et annet program.', 'The microphone is in use by another app.')
    case 'mic-insecure':
      return tr('Mikrofonen krever en sikker tilkobling (HTTPS).', 'The microphone requires a secure connection (HTTPS).')
    case 'mic-failed':
      return tr('Mikrofontilgang var ikke tilgjengelig.', 'Microphone access was not available.')
    case 'unsupported':
      return tr(
        'Stemmeopptak støttes ikke i denne nettleseren. Skriv meldingen i stedet.',
        'Voice recording is not supported in this browser. Type your message instead.',
      )
    case 'no-speech':
      return tr('Hørte ingen tale. Prøv igjen.', 'No speech was detected. Try again.')
    case 'too-long':
      return tr(
        'Opptaket var for langt. Hold hver melding under ett minutt.',
        'The recording was too long. Keep each message under one minute.',
      )
    case 'timeout':
      return tr('Transkriberingen tok for lang tid. Prøv igjen.', 'Transcription took too long. Try again.')
    case 'zdr-unavailable':
      return tr(
        'Tale er ikke tilgjengelig med nullagring for denne organisasjonen.',
        'Speech is not available with zero data retention for this organisation.',
      )
    case 'transcription-unavailable':
      return tr(
        'Transkribering er utilgjengelig akkurat nå. Ingenting ble sendt.',
        'Transcription is unavailable right now. Nothing was sent.',
      )
    case 'send-failed':
      return detail?.trim() || tr('Kunne ikke sende. Prøv igjen.', 'Could not send. Please retry.')
    case 'speech-unavailable':
      return readAloudUnavailableMessage(tr)
  }
}

/** Governed text-to-speech failed; nothing falls back to the browser's voices. */
export function readAloudUnavailableMessage(tr: Translate): string {
  return tr('Opplesing er utilgjengelig. Svaret vises i chatten.', 'Reading aloud is unavailable. The reply is shown in the chat.')
}

/** "Les høyt" fails closed in Temporary chat for the same reason Voice mode does. */
export function readAloudBlockedMessage(tr: Translate): string {
  return tr(
    'Opplesing er ikke tilgjengelig i midlertidig chat ennå. Tale kan ikke behandles uten lagring.',
    'Reading aloud is not available in Temporary chat yet. Speech cannot be processed with zero data retention.',
  )
}

export function voiceModeBlockedMessage(tr: Translate): string {
  return tr(
    'Stemmemodus er ikke tilgjengelig i midlertidig chat ennå. Tale kan ikke behandles uten lagring.',
    'Voice mode is not available in Temporary chat yet. Speech cannot be processed with zero data retention.',
  )
}

/** The composer's "Voice input" mic sends audio to the same speech path, so it is off there too. */
export function voiceInputBlockedMessage(tr: Translate): string {
  return tr(
    'Stemmeinndata er ikke tilgjengelig i midlertidig chat ennå. Tale kan ikke behandles uten lagring.',
    'Voice input is not available in Temporary chat yet. Speech cannot be processed with zero data retention.',
  )
}

/** The one polite status line. Errors go to the alert instead; reply tokens never come here. */
export function voiceStatusText(state: VoiceModeState, tr: Translate): string {
  switch (state.phase) {
    case 'blocked':
      return voiceModeBlockedMessage(tr)
    case 'ready':
      return state.replyReceived ? tr('Svar mottatt', 'Reply received') : tr('Trykk for å snakke', 'Tap to speak')
    case 'requesting':
      return tr('Venter på tilgang til mikrofonen …', 'Waiting for microphone access …')
    case 'recording':
      return tr('Lytter … trykk for å stoppe', 'Listening … tap to stop')
    case 'transcribing':
      return tr('Transkriberer …', 'Transcribing …')
    case 'review':
      return tr('Se over og send', 'Review and send')
    case 'sending':
      return tr('Sender …', 'Sending …')
    case 'awaiting':
      return tr('Verevon svarer …', 'Verevon is replying …')
    case 'speaking':
      return tr('Leser opp svaret …', 'Reading the reply aloud …')
    case 'error':
      return ''
  }
}
