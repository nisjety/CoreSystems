import { createEffect, createSignal, Match, onCleanup, Show, Switch, untrack } from 'solid-js'
import { AudioWaveform, Mic, X } from '@/shared/icons'
import { dictateAudioBlob } from '@/shared/api/audio-client'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { classifyMicError, createVoiceRecorder, VOICE_MAX_MS, type VoiceRecorder } from './voice-recorder'
import { createPlayingSpeechId, speechPlayer } from './speech-playback'
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
  type VoicePhase,
} from './voice-mode-state'

/** The latest assistant turn on a surface that shows replies. */
export type VoiceReply = { id: string; text: string; streaming: boolean }

export type VoiceSendResult = { ok: true } | { ok: false; message?: string }

type ReplyBaseline = { id: string | null; streaming: boolean }

/** Per-viewer conveniences only; the default (off) is what matters if storage is unavailable. */
const AUTO_SEND_STORAGE_KEY = 'verevon.voice-mode.auto-send.v1'
const READ_REPLIES_STORAGE_KEY = 'verevon.voice-mode.read-replies.v1'
const TIMER_WARNING_MS = 50_000

let dialogSequence = 0

function readFlag(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === 'true'
  } catch {
    return false
  }
}

function rememberFlag(key: string, value: boolean): void {
  try {
    window.localStorage.setItem(key, value ? 'true' : 'false')
  } catch {
    // Storage may be unavailable in private mode; the toggle still works for this visit.
  }
}

function formatElapsed(ms: number): string {
  const seconds = Math.floor(Math.max(0, ms) / 1000)
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
}

/**
 * Voice mode: tap to talk, one governed dictate call per utterance, review,
 * then send through the composer's own submit so the model, subscription
 * route, retention flag and source scope are exactly those of a typed
 * message. The microphone opens only on a tap and never re-opens on its own.
 *
 * "Les opp svar" (off by default) reads a finished reply aloud through
 * governed text-to-speech. The microphone stays shut while audio plays, and a
 * failure switches the toggle off: there is no browser-voice fallback.
 *
 * Temporary chat blocks the whole dialog: the speech path cannot yet attest
 * zero data retention, so no audio is captured, sent or synthesised there.
 */
export function VoiceModeDialog(props: {
  /** BCP 47 tag sent to the dictate route (the composer's voice language). */
  language: string
  /** Human-readable name of `language` for the header. */
  languageLabel?: string
  model: string
  temporaryChat: boolean
  /** The composer's own send or stream is in flight. */
  submitting?: boolean
  reply?: VoiceReply | null
  /** The host shows replies (chat), so the dialog waits for one after a send; otherwise it closes. */
  awaitReply: boolean
  onClose: () => void
  onInsert: (text: string) => void
  onSend: (text: string) => Promise<VoiceSendResult>
  onStop?: () => void
}) {
  const i18n = useI18n()
  const ids = `verevon-voice-mode-${++dialogSequence}`
  let dialogRef: HTMLDialogElement | undefined
  let micRef: HTMLButtonElement | undefined
  let transcriptRef: HTMLTextAreaElement | undefined
  let stopReadingRef: HTMLButtonElement | undefined
  let stopReplyRef: HTMLButtonElement | undefined
  let closeActionRef: HTMLButtonElement | undefined
  let recorder: VoiceRecorder | undefined
  let dictation: AbortController | undefined
  let clock: ReturnType<typeof setInterval> | undefined
  let disposed = false

  // The reducer's latest state lives here as well as in the signal, so a
  // handler that dispatches and then reads sees its own write immediately.
  let current = initialVoiceModeState(untrack(() => props.temporaryChat))
  const [state, setState] = createSignal(current)
  const dispatch = (event: VoiceModeEvent) => {
    const next = voiceModeReducer(current, event)
    if (next === current) return
    current = next
    setState(next)
  }
  const [autoSend, setAutoSend] = createSignal(readFlag(AUTO_SEND_STORAGE_KEY))
  const [readReplies, setReadReplies] = createSignal(readFlag(READ_REPLIES_STORAGE_KEY))
  const player = speechPlayer()
  // Anything reading aloud, here or a "Les høyt" started before the dialog
  // opened: either way the microphone must not record over it.
  const playingId = createPlayingSpeechId(player)
  const audioPlaying = () => playingId() !== null
  /** The reply this dialog is reading aloud, if any. */
  let spokenId: string | null = null
  /** "Stopp svaret" was pressed: a cut-off reply is not read aloud. */
  let replyStopped = false
  const [elapsed, setElapsed] = createSignal(0)
  const [level, setLevel] = createSignal(0)
  const [replyBaseline, setReplyBaseline] = createSignal<ReplyBaseline | null>(null)
  const [lastReply, setLastReply] = createSignal('')

  const stopClock = () => {
    if (clock !== undefined) clearInterval(clock)
    clock = undefined
  }
  const startClock = () => {
    stopClock()
    const startedAt = Date.now()
    setElapsed(0)
    clock = setInterval(() => setElapsed(Date.now() - startedAt), 250)
  }

  /** Release the microphone and cancel any dictate call. Never writes signals. */
  const releaseWork = () => {
    const take = recorder
    recorder = undefined
    take?.cancel()
    const pending = dictation
    dictation = undefined
    pending?.abort()
    stopClock()
  }
  const abortWork = () => {
    releaseWork()
    setLevel(0)
  }

  /** Inside a tap, so iOS lets a reply that arrives later play. */
  const unlockPlayback = () => {
    if (readReplies()) player.unlock()
  }

  const stopOwnReading = () => {
    if (spokenId !== null && player.playingId() === spokenId) player.stop()
  }

  const readReplyAloud = async (reply: VoiceReply) => {
    spokenId = reply.id
    const outcome = await player.speak({ id: reply.id, text: reply.text, language: props.language })
    if (spokenId === reply.id) spokenId = null
    if (disposed) return
    if (outcome === 'failed') {
      // Fail closed: say so, switch reading off, and leave the reply on screen.
      setReadReplies(false)
      rememberFlag(READ_REPLIES_STORAGE_KEY, false)
      dispatch({ type: 'speech-failed' })
      return
    }
    dispatch({ type: 'speech-ended' })
  }

  const transcribe = async (blob: Blob) => {
    dispatch({ type: 'recorded' })
    if (blob.size === 0) {
      dispatch({ type: 'transcribed', text: '', cleaned: true })
      return
    }
    const controller = new AbortController()
    dictation = controller
    try {
      const dictated = await dictateAudioBlob(blob, props.language, 'chat message', controller.signal, props.temporaryChat)
      if (disposed || dictation !== controller) return
      dictation = undefined
      dispatch({ type: 'transcribed', text: dictated.text || dictated.rawText, cleaned: dictated.cleaned })
      if (autoSend() && current.phase === 'review') void send()
    } catch (error) {
      if (disposed || dictation !== controller) return
      dictation = undefined
      const kind = classifyDictateError(error)
      if (kind !== 'aborted') dispatch({ type: 'failed', error: kind })
    }
  }

  const startTake = () => {
    // Never record over audio that is playing: it would transcribe the reply.
    if (!canStartTake(current) || player.playingId() !== null) return
    abortWork()
    dispatch({ type: 'tap' })
    setLastReply('')
    const take = createVoiceRecorder({ onLevel: (value) => setLevel(value) })
    recorder = take
    // Called inside the tap handler: the recorder creates its AudioContext
    // before awaiting the permission prompt, which iOS requires.
    take.start().then(
      () => {
        if (recorder !== take) return
        dispatch({ type: 'mic-ready' })
        startClock()
      },
      (error: unknown) => {
        if (recorder !== take) return
        recorder = undefined
        dispatch({ type: 'failed', error: voiceErrorFromMic(classifyMicError(error)) })
      },
    )
    void finishTake(take)
  }

  /** Once the take ends (second tap, silence or a cap), transcribe it. */
  const finishTake = async (take: VoiceRecorder) => {
    let recording: Awaited<VoiceRecorder['result']>
    try {
      recording = await take.result
    } catch {
      if (recorder !== take) return
      recorder = undefined
      stopClock()
      dispatch({ type: 'failed', error: 'mic-failed' })
      return
    }
    // Null is a cancel, or a start that failed and already reported itself.
    if (recorder !== take || !recording) return
    recorder = undefined
    stopClock()
    await transcribe(recording.blob)
  }

  const onMicClick = () => {
    unlockPlayback()
    if (current.phase === 'recording') {
      recorder?.stop()
      return
    }
    startTake()
  }

  const recordAgain = () => {
    unlockPlayback()
    startTake()
  }

  async function send() {
    if (current.phase !== 'review') return
    const text = current.transcript.trim()
    if (!text) return
    const reply = props.reply
    setReplyBaseline({ id: reply?.id ?? null, streaming: Boolean(reply?.streaming) })
    replyStopped = false
    dispatch({ type: 'send' })
    let result: VoiceSendResult
    try {
      result = await props.onSend(text)
    } catch (error) {
      result = { ok: false, message: error instanceof Error ? error.message : undefined }
    }
    if (disposed) return
    if (!result.ok) {
      setReplyBaseline(null)
      dispatch({ type: 'failed', error: 'send-failed', detail: result.message })
      return
    }
    const awaitReply = props.awaitReply
    dispatch({ type: 'sent', awaitReply })
    if (!awaitReply) props.onClose()
  }

  const insert = () => {
    const text = current.transcript.trim()
    if (current.phase !== 'review' || !text) return
    abortWork()
    props.onInsert(text)
  }

  const discard = () => {
    abortWork()
    dispatch({ type: 'reset' })
  }

  const close = () => {
    abortWork()
    stopOwnReading()
    props.onClose()
  }

  // Temporary chat can start (or a thread can become temporary) while the
  // dialog is open: drop whatever was in flight and block.
  createEffect(
    () => Boolean(props.temporaryChat),
    (blocked) => {
      if (blocked) {
        abortWork()
        stopOwnReading()
      }
      dispatch({ type: 'blocked', blocked })
    },
  )

  // The reply is done once the stream has settled on a turn that was not the
  // latest finished one when this dialog sent. A send while a reply was
  // already streaming (a mid-run message) finishes with that same turn.
  createEffect(
    () => ({ baseline: replyBaseline(), phase: state().phase, reply: props.reply, submitting: Boolean(props.submitting) }),
    ({ baseline, phase, reply, submitting }) => {
      if (phase !== 'awaiting' || !baseline || submitting || !reply || reply.streaming) return
      if (reply.id === baseline.id && !baseline.streaming) return
      setReplyBaseline(null)
      setLastReply(reply.text)
      const speak = readReplies() && !replyStopped && reply.text.trim().length > 0
      dispatch({ type: 'reply-finished', speak })
      if (speak) void readReplyAloud(reply)
    },
  )

  /** The first of these still on the page; refs to a control that unmounted go stale. */
  const focusFirst = (...candidates: (HTMLElement | undefined)[]) => {
    candidates.find((element) => element?.isConnected)?.focus()
  }

  /**
   * Where focus belongs in each phase, so it never drops to the page behind
   * the modal when the focused control goes away or turns off.
   */
  const focusForPhase = (phase: VoicePhase) => {
    if (phase === 'review') focusFirst(transcriptRef)
    else if (phase === 'ready') focusFirst(micRef)
    // The button that was pressed to send is gone; put focus on the one control that matters.
    else if (phase === 'speaking') focusFirst(stopReadingRef, micRef)
    else if (phase === 'awaiting') focusFirst(stopReplyRef, micRef)
    // Nothing here can be retried and the microphone is off: offer the way out.
    else if (phase === 'blocked' || (phase === 'error' && !canRecordAgain(current))) focusFirst(closeActionRef)
    // 'sending' keeps focus where it is: Send and the transcript stay focusable (aria-disabled).
  }

  createEffect(
    () => state().phase,
    (phase) => {
      queueMicrotask(() => {
        if (!disposed && current.phase === phase) focusForPhase(phase)
      })
    },
  )

  onCleanup(() => {
    disposed = true
    releaseWork()
    // Closing Voice mode ends its reading. Deferred: stopping notifies other
    // components' signals, which must not be written during this cleanup.
    const reading = spokenId
    if (reading !== null) {
      queueMicrotask(() => {
        if (player.playingId() === reading) player.stop()
      })
    }
    // Leave the top layer cleanly on unmount.
    if (dialogRef?.open && typeof dialogRef.close === 'function') dialogRef.close()
  })

  const phase = () => state().phase
  const recording = () => phase() === 'recording'
  const sending = () => phase() === 'sending'
  const inReview = () => phase() === 'review' || phase() === 'sending'
  // Busy phases keep the button focusable (aria-disabled) so focus is not
  // lost while a take is being transcribed, a reply streams or is read aloud.
  const micBusy = () =>
    ['requesting', 'transcribing', 'sending', 'awaiting', 'speaking'].includes(phase()) || audioPlaying()
  const showStopReading = () =>
    phase() === 'speaking' || (audioPlaying() && (phase() === 'ready' || phase() === 'error'))
  const statusText = () =>
    phase() === 'ready' && audioPlaying()
      ? i18n.tr('Leser opp svaret …', 'Reading the reply aloud …')
      : voiceStatusText(state(), i18n.tr)
  const micUnavailable = () => phase() === 'blocked' || (phase() === 'error' && !canRecordAgain(state()))
  const errorText = () => {
    const snapshot = state()
    return snapshot.error ? voiceErrorMessage(snapshot.error, i18n.tr, snapshot.errorDetail) : ''
  }
  const replyText = () => {
    if (phase() === 'ready' || phase() === 'speaking') return lastReply()
    if (phase() !== 'awaiting') return ''
    const baseline = replyBaseline()
    const reply = props.reply
    if (!baseline || !reply || (reply.id === baseline.id && !baseline.streaming)) return ''
    return reply.text
  }
  const hasActions = () => {
    if (showStopReading()) return true
    const now = phase()
    if (now === 'ready') return false
    if (now === 'awaiting') return Boolean(props.onStop)
    // An error offers either another take or, when none can help, a way out.
    return true
  }

  return (
    <dialog
      // The native modal (showModal) gives focus trapping, Escape and an inert
      // backdrop; Escape's `cancel` goes through close() so the microphone and
      // any dictate call are released exactly as the X button does.
      ref={(element) => {
        dialogRef = element
        queueMicrotask(() => {
          if (disposed || element.open) return
          if (typeof element.showModal === 'function') element.showModal()
          else element.setAttribute('open', '')
          focusForPhase(current.phase)
        })
      }}
      class="realtime-voice-modal"
      aria-labelledby={`${ids}-title`}
      // Read when focus enters, so opening it blocked in Temporary chat says why.
      aria-describedby={`${ids}-status`}
      data-dashboard-modal="true"
      onCancel={(event: Event) => {
        event.preventDefault()
        close()
      }}
    >
      <button type="button" tabindex={-1} aria-hidden="true" class="realtime-voice-modal__scrim" onClick={close} />
      <div class="realtime-voice-modal__panel verevon-panel-in">
        <div class="realtime-voice-modal__header">
          <span aria-hidden="true">
            <AudioWaveform class="size-5" />
          </span>
          <div>
            <p id={`${ids}-title`}>{i18n.tr('Stemmemodus', 'Voice mode')}</p>
            <small>
              {props.model} · {props.languageLabel ?? props.language}
            </small>
          </div>
          <button type="button" onClick={close} aria-label={i18n.tr('Lukk stemmemodus', 'Close voice mode')}>
            <X class="size-4" />
          </button>
        </div>

        <div class="realtime-voice-modal__body">
          <button
            ref={(element) => {
              micRef = element
            }}
            type="button"
            class={cn(
              'realtime-voice-modal__mic',
              recording() ? 'realtime-voice-modal__mic--listening verevon-voice-pulse' : '',
            )}
            aria-pressed={recording() ? 'true' : 'false'}
            aria-label={recording() ? i18n.tr('Stopp opptak', 'Stop recording') : i18n.tr('Start opptak', 'Start recording')}
            aria-disabled={micBusy() ? 'true' : undefined}
            disabled={micUnavailable()}
            onClick={() => {
              if (!micBusy()) onMicClick()
            }}
          >
            <Mic class="size-6" />
          </button>

          <Show when={recording()}>
            <div class="realtime-voice-modal__meter" aria-hidden="true">
              <span style={{ transform: `scaleX(${level()})` }} />
            </div>
            <p
              class={cn(
                'realtime-voice-modal__timer',
                elapsed() >= TIMER_WARNING_MS ? 'realtime-voice-modal__timer--warning' : '',
              )}
              aria-hidden="true"
            >
              {formatElapsed(elapsed())} / {formatElapsed(VOICE_MAX_MS)}
            </p>
          </Show>

          <p id={`${ids}-status`} class="realtime-voice-modal__status" role="status" aria-live="polite">
            {statusText()}
          </p>
          <Show when={errorText()}>
            {(text) => (
              <p class="realtime-voice-modal__error" role="alert">
                {text()}
              </p>
            )}
          </Show>

          <Show when={inReview()}>
            <label class="sr-only" for={`${ids}-transcript`}>
              {i18n.tr('Transkripsjon', 'Transcript')}
            </label>
            <textarea
              ref={(element) => {
                transcriptRef = element
              }}
              id={`${ids}-transcript`}
              class="realtime-voice-modal__transcript realtime-voice-modal__transcript--editable"
              rows="3"
              value={state().transcript}
              // Read-only rather than disabled while sending: a disabled
              // control drops focus to the page behind the modal.
              readonly={sending()}
              aria-disabled={sending() ? 'true' : undefined}
              onInput={(event) => dispatch({ type: 'edit', text: event.currentTarget.value })}
              onKeyDown={(event) => {
                // Enter sends, Shift+Enter is a new line, and an IME composition keeps its Enter.
                if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
                event.preventDefault()
                unlockPlayback()
                void send()
              }}
            />
            <Show when={!state().cleaned}>
              <small class="realtime-voice-modal__note">
                {i18n.tr('Rå transkripsjon (opprydding var utilgjengelig)', 'Raw transcript (clean-up was unavailable)')}
              </small>
            </Show>
          </Show>

          <Show when={replyText()}>
            {(text) => <div class="realtime-voice-modal__reply">{text()}</div>}
          </Show>
        </div>

        <Show when={hasActions()}>
          <div class={cn('realtime-voice-modal__actions', inReview() ? 'realtime-voice-modal__actions--review' : '')}>
            <Switch>
              <Match when={showStopReading()}>
                <button
                  ref={(element) => {
                    stopReadingRef = element
                  }}
                  type="button"
                  onClick={() => player.stop()}
                >
                  {i18n.tr('Stopp opplesing', 'Stop reading')}
                </button>
              </Match>
              <Match when={inReview()}>
                <button
                  type="button"
                  class="realtime-voice-modal__primary"
                  // Focus stays here through the send (aria-disabled), then moves to "Stopp svaret".
                  aria-disabled={sending() ? 'true' : undefined}
                  disabled={!state().transcript.trim()}
                  onClick={() => {
                    if (sending()) return
                    unlockPlayback()
                    void send()
                  }}
                >
                  {i18n.tr('Send', 'Send')}
                </button>
                <button type="button" disabled={sending()} onClick={insert}>
                  {i18n.tr('Sett inn i utkast', 'Insert into draft')}
                </button>
                <button type="button" disabled={sending()} onClick={recordAgain}>
                  {i18n.tr('Ta opp på nytt', 'Record again')}
                </button>
                <button type="button" class="realtime-voice-modal__quiet" disabled={sending()} onClick={discard}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </button>
              </Match>
              <Match when={recording()}>
                <button type="button" onClick={() => recorder?.stop()}>
                  {i18n.tr('Stopp', 'Stop')}
                </button>
                <button type="button" onClick={discard}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </button>
              </Match>
              <Match when={phase() === 'requesting' || phase() === 'transcribing'}>
                <button type="button" onClick={discard}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </button>
              </Match>
              <Match when={phase() === 'awaiting'}>
                <button
                  ref={(element) => {
                    stopReplyRef = element
                  }}
                  type="button"
                  onClick={() => {
                    replyStopped = true
                    props.onStop?.()
                  }}
                >
                  {i18n.tr('Stopp svaret', 'Stop the reply')}
                </button>
              </Match>
              <Match when={phase() === 'error' && canRecordAgain(state())}>
                <button type="button" onClick={recordAgain}>
                  {i18n.tr('Ta opp på nytt', 'Record again')}
                </button>
              </Match>
              {/* Blocked, or an error no new take can fix (the page, the browser or the organisation). */}
              <Match when={phase() === 'blocked' || phase() === 'error'}>
                <button
                  ref={(element) => {
                    closeActionRef = element
                  }}
                  type="button"
                  onClick={close}
                >
                  {i18n.tr('Lukk', 'Close')}
                </button>
              </Match>
            </Switch>
          </div>
        </Show>

        <Show when={phase() !== 'blocked'}>
          <div class="realtime-voice-modal__toggles">
            <label class="realtime-voice-modal__toggle">
              <input
                type="checkbox"
                checked={autoSend()}
                onChange={(event) => {
                  const value = event.currentTarget.checked
                  setAutoSend(value)
                  rememberFlag(AUTO_SEND_STORAGE_KEY, value)
                }}
              />
              <span>{i18n.tr('Send automatisk', 'Send automatically')}</span>
            </label>
            <label class="realtime-voice-modal__toggle">
              <input
                type="checkbox"
                checked={readReplies()}
                onChange={(event) => {
                  const value = event.currentTarget.checked
                  setReadReplies(value)
                  rememberFlag(READ_REPLIES_STORAGE_KEY, value)
                  // The change comes from a tap: unlock now for the reply that comes later.
                  if (value) player.unlock()
                  else stopOwnReading()
                }}
              />
              <span>{i18n.tr('Les opp svar', 'Read replies aloud')}</span>
            </label>
          </div>
        </Show>
      </div>
    </dialog>
  )
}
