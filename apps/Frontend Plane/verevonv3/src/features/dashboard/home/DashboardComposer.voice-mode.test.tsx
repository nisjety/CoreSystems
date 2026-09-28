// @vitest-environment jsdom

// Voice mode on Verevon's governed speech path. It used to be a dictation
// modal on the browser's Web Speech API (Google's, Microsoft's or Apple's
// online recogniser, outside every provider and audit control). It now
// records one take per tap, sends it to `/api/v1/ai/dictate`, and sends the
// reviewed transcript through the composer's own submit, so the model and
// route are exactly those of a typed message. Temporary chat blocks it until
// the speech path can attest zero data retention.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import type { VoiceReply } from '@/features/voice/VoiceModeDialog'
import { latestVoiceReply } from '@/features/voice/voice-reply'
import { speechPlayer } from '@/features/voice/speech-playback'

type DictationResult = { text: string; rawText: string; cleaned: boolean }

const { dictateAudioBlob, mockListModels, mockListSubscriptions } = vi.hoisted(() => ({
  dictateAudioBlob: vi.fn(async (_blob: Blob, _language: string, _context?: string, _signal?: AbortSignal, _zdr?: boolean): Promise<DictationResult> => (
    { text: 'Diktert tekst', rawText: 'diktert tekst', cleaned: true }
  )),
  mockListModels: vi.fn(),
  mockListSubscriptions: vi.fn(),
}))

vi.mock('@/shared/api/audio-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/audio-client')>()
  return { ...actual, dictateAudioBlob }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: mockListSubscriptions }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer, type DashboardComposerSubmitPayload } from './DashboardComposer'

/** The FakeRecorder pattern from the dictation-zdr suite, plus the options it was built with. */
class FakeRecorder {
  static instances: FakeRecorder[] = []
  static isTypeSupported(mimeType: string) {
    return mimeType === 'audio/webm;codecs=opus'
  }

  mimeType: string
  options: MediaRecorderOptions | undefined
  ondataavailable: ((event: { data: Blob }) => void) | null = null
  onstop: (() => void) | null = null
  onerror: (() => void) | null = null

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    this.options = options
    this.mimeType = options?.mimeType ?? 'audio/webm'
    FakeRecorder.instances.push(this)
  }

  start() {}
  stop() {
    this.ondataavailable?.({ data: new Blob(['audio']) })
    this.onstop?.()
  }
}

// The browser's speech APIs, as spies that throw: Voice mode must never reach them.
const webSpeech = {
  SpeechRecognition: vi.fn(function SpeechRecognition() {
    throw new Error('SpeechRecognition must not be used')
  }),
  webkitSpeechRecognition: vi.fn(function webkitSpeechRecognition() {
    throw new Error('webkitSpeechRecognition must not be used')
  }),
  SpeechSynthesisUtterance: vi.fn(function SpeechSynthesisUtterance() {
    throw new Error('SpeechSynthesisUtterance must not be used')
  }),
  speak: vi.fn(() => {
    throw new Error('speechSynthesis.speak must not be used')
  }),
}

const track = { stop: vi.fn() }
const getUserMedia = vi.fn(async (_constraints: MediaStreamConstraints) => ({ getTracks: () => [track] }) as unknown as MediaStream)

type RenderOptions = {
  initialMessage?: string
  /** Fixed, or a signal to switch Temporary chat on mid-test. */
  temporaryChat?: boolean | (() => boolean)
  /** A chat-like host: Voice mode waits for this reply after a send. */
  reply?: () => VoiceReply | null
  submitting?: () => boolean
  onStop?: () => void
  onSubmit?: (payload: DashboardComposerSubmitPayload) => Promise<void> | void
}

function renderComposer(options: RenderOptions = {}) {
  const submitted: DashboardComposerSubmitPayload[] = []
  const temporaryChat = () =>
    typeof options.temporaryChat === 'function' ? options.temporaryChat() : options.temporaryChat
  function ComposerPage() {
    const [message, setMessage] = createSignal(options.initialMessage ?? '')
    return (
      <DashboardComposer
        allowMidRunSubmit
        message={message()}
        onMessageChange={setMessage}
        onStop={options.onStop}
        onSubmit={(payload) => {
          submitted.push(payload)
          return options.onSubmit?.(payload)
        }}
        submitting={options.submitting?.()}
        temporaryChat={temporaryChat()}
        voiceReply={options.reply ? options.reply() : undefined}
      />
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  const view = render(() => <TestRouter>{(routed) => <>{routed.children}</>}</TestRouter>)
  return { ...view, submitted }
}

const voiceModeButton = () => screen.getByRole('button', { name: /^(Stemmemodus|Voice mode)$/ })

async function openVoiceMode() {
  fireEvent.click(voiceModeButton())
  flush()
  return screen.findByRole('dialog', { name: /^(Stemmemodus|Voice mode)$/ })
}

const micButton = (dialog: HTMLElement) =>
  within(dialog).getByRole('button', { name: /^(Start opptak|Start recording|Stopp opptak|Stop recording)$/ })

async function startRecording(dialog: HTMLElement) {
  fireEvent.click(micButton(dialog))
  flush()
  await waitFor(() => expect(micButton(dialog).getAttribute('aria-pressed')).toBe('true'))
}

/** Tap, tap: one take, one dictate call. */
async function speak(dialog: HTMLElement) {
  const calls = dictateAudioBlob.mock.calls.length
  await startRecording(dialog)
  fireEvent.click(micButton(dialog))
  flush()
  await waitFor(() => expect(dictateAudioBlob).toHaveBeenCalledTimes(calls + 1))
}

async function speakToReview(dialog: HTMLElement) {
  await speak(dialog)
  return (await within(dialog).findByRole('textbox', { name: /^(Transkripsjon|Transcript)$/ })) as HTMLTextAreaElement
}

const draft = () => screen.getByRole('textbox', { name: /^(Meld Verevon|Message Verevon)$/ }) as HTMLTextAreaElement

let restoreDialog: (() => void) | undefined

beforeEach(() => {
  FakeRecorder.instances = []
  vi.stubGlobal('MediaRecorder', FakeRecorder)
  vi.stubGlobal('SpeechRecognition', webSpeech.SpeechRecognition)
  vi.stubGlobal('webkitSpeechRecognition', webSpeech.webkitSpeechRecognition)
  vi.stubGlobal('SpeechSynthesisUtterance', webSpeech.SpeechSynthesisUtterance)
  vi.stubGlobal('speechSynthesis', { speak: webSpeech.speak, cancel: webSpeech.speak, getVoices: webSpeech.speak })
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia } })
  mockListModels.mockResolvedValue([])
  mockListSubscriptions.mockResolvedValue([])
  // jsdom has no modal dialogs; give it just enough of one.
  const proto = HTMLDialogElement.prototype as HTMLDialogElement & { showModal?: () => void; close?: () => void }
  const original = { showModal: proto.showModal, close: proto.close }
  proto.showModal = function showModal(this: HTMLDialogElement) {
    this.setAttribute('open', '')
  }
  proto.close = function close(this: HTMLDialogElement) {
    this.removeAttribute('open')
  }
  restoreDialog = () => {
    proto.showModal = original.showModal!
    proto.close = original.close!
  }
})

afterEach(() => {
  // Whatever a test did, the browser's own speech services were never touched.
  expect(webSpeech.SpeechRecognition).not.toHaveBeenCalled()
  expect(webSpeech.webkitSpeechRecognition).not.toHaveBeenCalled()
  expect(webSpeech.SpeechSynthesisUtterance).not.toHaveBeenCalled()
  expect(webSpeech.speak).not.toHaveBeenCalled()
  restoreDialog?.()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
  dictateAudioBlob.mockImplementation(async () => ({ text: 'Diktert tekst', rawText: 'diktert tekst', cleaned: true }))
  getUserMedia.mockImplementation(async () => ({ getTracks: () => [track] }) as unknown as MediaStream)
  window.localStorage.clear()
})

describe('Voice mode records on the governed dictate route', () => {
  it('opens as a dialog with the microphone focused, and dictates one take per tap', async () => {
    renderComposer()
    expect(voiceModeButton().getAttribute('aria-haspopup')).toBe('dialog')
    const dialog = await openVoiceMode()
    await waitFor(() => expect(document.activeElement).toBe(micButton(dialog)))
    expect(within(dialog).getByRole('status').textContent).toMatch(/Trykk for å snakke|Tap to speak/)
    // Tap-to-talk: nothing is recorded until the tap.
    expect(getUserMedia).not.toHaveBeenCalled()

    await speak(dialog)

    expect(dictateAudioBlob).toHaveBeenCalledWith(expect.any(Blob), 'nb-NO', 'chat message', expect.any(AbortSignal), false)
    expect(getUserMedia).toHaveBeenCalledWith({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    })
    expect(FakeRecorder.instances[0]?.options).toEqual({ audioBitsPerSecond: 32_000, mimeType: 'audio/webm;codecs=opus' })
    expect(track.stop).toHaveBeenCalled()
    // Only the "chat message" context; never the route's client-controllable model override.
    expect(dictateAudioBlob.mock.calls[0]).toHaveLength(5)
  })

  it('shows the transcript for review and notes when the clean-up pass was skipped', async () => {
    dictateAudioBlob.mockResolvedValueOnce({ text: '', rawText: 'eh hei der', cleaned: false })
    renderComposer()
    const dialog = await openVoiceMode()
    const transcript = await speakToReview(dialog)
    expect(transcript.value).toBe('eh hei der')
    await waitFor(() => expect(document.activeElement).toBe(transcript))
    expect(within(dialog).getByText(/Rå transkripsjon \(opprydding var utilgjengelig\)|Raw transcript/)).toBeTruthy()
    expect(within(dialog).getByRole('status').textContent).toMatch(/Se over og send|Review and send/)
  })
})

describe('Voice mode sends like a typed message', () => {
  it('sends the transcript with the same model, subscription route and retention flag as typing', async () => {
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({
      model: 'gpt-5.6-terra', label: 'Terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn',
    }))
    mockListModels.mockResolvedValue([{ id: 'gpt-5.6-terra', name: 'Terra', provider: 'openai-codex-subscription' }])
    mockListSubscriptions.mockResolvedValue([{ id: 'conn', providerKey: 'openai-codex-subscription', status: 'active' }])
    const { submitted } = renderComposer()

    // A typed message first, for the baseline.
    const input = await screen.findByRole('textbox', { name: /^(Meld Verevon|Message Verevon)$/ })
    fireEvent.input(input, { target: { value: 'Skrevet melding' } })
    flush()
    const submit = input.closest('form')!.querySelector('button[type="submit"]') as HTMLButtonElement
    await waitFor(() => expect(submit.disabled).toBe(false))
    fireEvent.click(submit)
    await waitFor(() => expect(submitted).toHaveLength(1))

    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))
    await waitFor(() => expect(submitted).toHaveLength(2))

    const [typed, spoken] = submitted
    expect(spoken).toMatchObject({ text: 'Diktert tekst', model: 'gpt-5.6-terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn' })
    const { text: _typedText, ...typedRoute } = typed!
    const { text: _spokenText, ...spokenRoute } = spoken!
    expect(spokenRoute).toEqual(typedRoute)
    // An ordinary chat: the payload carries no retention flag at all, as when typing.
    expect(spoken?.zdr).toBeFalsy()

    // Where the host shows no replies, Voice mode closes and gives focus back.
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(document.activeElement).toBe(voiceModeButton())
    expect(draft().value).toBe('')
  })

  it('sends on Enter in the transcript, and Shift+Enter is a new line', async () => {
    const { submitted } = renderComposer()
    const dialog = await openVoiceMode()
    const transcript = await speakToReview(dialog)
    fireEvent.keyDown(transcript, { key: 'Enter', shiftKey: true })
    flush()
    expect(submitted).toHaveLength(0)
    fireEvent.input(transcript, { target: { value: 'Rettet tekst' } })
    flush()
    fireEvent.keyDown(transcript, { key: 'Enter' })
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('Rettet tekst')
  })

  it('holds a spoken turn sent before the model catalogue loads, then carries the model’s tier', async () => {
    // The typed path's privacy-tier wait, reached through Voice mode's send.
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({
      model: 'm-eu', label: 'EU Model', provider: 'eu-host',
    }))
    let resolveCatalogue!: (models: Array<{ id: string; name: string; provider: string; privacyTier: string }>) => void
    mockListModels.mockReturnValue(new Promise((resolve) => { resolveCatalogue = resolve }))
    const { submitted } = renderComposer()

    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))
    flush()
    await Promise.resolve()
    expect(submitted).toHaveLength(0)

    resolveCatalogue([{ id: 'm-eu', name: 'EU Model', provider: 'eu-host', privacyTier: 'eu_resident' }])
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ text: 'Diktert tekst', model: 'm-eu', minPrivacyTier: 'eu_resident' })
  })

  it('shows a failed send in the dialog and keeps the transcript and the typed draft', async () => {
    const { submitted } = renderComposer({
      initialMessage: 'Skrevet',
      onSubmit: () => {
        throw new Error('Kunne ikke nå Verevon.')
      },
    })
    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))
    await waitFor(() => expect(within(dialog).getByRole('alert').textContent).toBe('Kunne ikke nå Verevon.'))
    expect(submitted[0]?.text).toBe('Skrevet Diktert tekst')
    expect((within(dialog).getByRole('textbox', { name: /^(Transkripsjon|Transcript)$/ }) as HTMLTextAreaElement).value).toBe('Diktert tekst')
    expect(draft().value).toBe('Skrevet')
  })

  it('inserts into the draft instead of sending', async () => {
    const { submitted } = renderComposer({ initialMessage: 'Hei' })
    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^(Sett inn i utkast|Insert into draft)$/ }))
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(draft().value).toBe('Hei Diktert tekst')
    expect(submitted).toHaveLength(0)
  })

  it('waits for the reply in chat, can stop it, and returns to ready without reopening the microphone', async () => {
    const [reply, setReply] = createSignal<VoiceReply | null>({ id: 'asst-1', text: 'Forrige svar', streaming: false })
    const [submitting, setSubmitting] = createSignal(false)
    const onStop = vi.fn()
    renderComposer({
      reply,
      submitting,
      onStop,
      onSubmit: () => {
        setSubmitting(true)
        setReply({ id: 'asst-2', text: 'Verevon skri', streaming: true })
      },
    })
    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))

    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/Verevon svarer|Verevon is replying/))
    expect(within(dialog).getByText('Verevon skri')).toBeTruthy()
    expect(micButton(dialog).getAttribute('aria-disabled')).toBe('true')
    // The review controls are gone; focus goes to the one control that matters now.
    const stopReply = within(dialog).getByRole('button', { name: /^(Stopp svaret|Stop the reply)$/ })
    await waitFor(() => expect(document.activeElement).toBe(stopReply))
    fireEvent.click(stopReply)
    expect(onStop).toHaveBeenCalledTimes(1)

    setReply({ id: 'asst-2', text: 'Verevon skriver ferdig.', streaming: false })
    setSubmitting(false)
    flush()

    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/^(Svar mottatt|Reply received)$/))
    expect(within(dialog).getByText('Verevon skriver ferdig.')).toBeTruthy()
    expect(micButton(dialog).getAttribute('aria-pressed')).toBe('false')
    expect(micButton(dialog).getAttribute('aria-disabled')).toBeNull()
    await waitFor(() => expect(document.activeElement).toBe(micButton(dialog)))
    expect(getUserMedia).toHaveBeenCalledTimes(1)
  })

  it('keeps focus on Send while it sends, then hands it to "Stopp svaret"', async () => {
    let accept!: () => void
    const [reply, setReply] = createSignal<VoiceReply | null>(null)
    renderComposer({
      reply,
      onStop: vi.fn(),
      onSubmit: () =>
        new Promise<void>((resolve) => {
          accept = () => {
            setReply({ id: 'asst-1', text: '', streaming: true })
            resolve()
          }
        }),
    })
    const dialog = await openVoiceMode()
    const transcript = await speakToReview(dialog)
    const sendButton = within(dialog).getByRole('button', { name: /^Send$/ }) as HTMLButtonElement
    sendButton.focus()
    fireEvent.click(sendButton)
    flush()
    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/^(Sender …|Sending …)$/))
    // Off, but still focusable: a disabled control would drop focus to the page behind the modal.
    expect(sendButton.getAttribute('aria-disabled')).toBe('true')
    expect(sendButton.disabled).toBe(false)
    expect(document.activeElement).toBe(sendButton)
    expect(transcript.readOnly).toBe(true)
    expect(transcript.disabled).toBe(false)
    expect(transcript.getAttribute('aria-disabled')).toBe('true')
    // A second press while sending sends nothing more.
    fireEvent.click(sendButton)
    flush()

    accept()
    await waitFor(() =>
      expect(document.activeElement).toBe(within(dialog).getByRole('button', { name: /^(Stopp svaret|Stop the reply)$/ })),
    )
  })

  it('moves focus to the way out when an error cannot be fixed by another take', async () => {
    renderComposer()
    const dialog = await openVoiceMode()
    await waitFor(() => expect(document.activeElement).toBe(micButton(dialog)))
    // No recorder in this browser: nothing a new take can change.
    vi.stubGlobal('MediaRecorder', undefined)
    fireEvent.click(micButton(dialog))
    flush()
    const alert = await within(dialog).findByRole('alert')
    expect(alert.textContent).toMatch(/Stemmeopptak støttes ikke i denne nettleseren|Voice recording is not supported/)
    expect((micButton(dialog) as HTMLButtonElement).disabled).toBe(true)
    expect(within(dialog).queryByRole('button', { name: /^(Ta opp på nytt|Record again)$/ })).toBeNull()
    const closeAction = within(dialog).getByRole('button', { name: /^(Lukk|Close)$/ })
    await waitFor(() => expect(document.activeElement).toBe(closeAction))
    fireEvent.click(closeAction)
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(getUserMedia).not.toHaveBeenCalled()
  })

  it('waits for the turn that answers a spoken message the running reply could not take', async () => {
    // The chat page's mapping: a deferred mid-run message keeps the reply "streaming".
    type Turn = { id: string; role: 'assistant' | 'user'; content: string; streaming: boolean }
    const [turns, setTurns] = createSignal<Turn[]>([{ id: 'asst-1', role: 'assistant', content: 'Første svar', streaming: true }])
    const [followUpPending, setFollowUpPending] = createSignal(false)
    const [submitting, setSubmitting] = createSignal(true)
    renderComposer({
      reply: () => latestVoiceReply(turns(), followUpPending()),
      submitting,
      // Mid-run with no request id yet: the chat defers the message to the next turn.
      onSubmit: () => {
        setFollowUpPending(true)
      },
    })
    const dialog = await openVoiceMode()
    await speakToReview(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))
    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/Verevon svarer|Verevon is replying/))

    // The reply that was streaming finishes. It does not answer the spoken message.
    setTurns([{ id: 'asst-1', role: 'assistant', content: 'Første svar.', streaming: false }])
    setSubmitting(false)
    flush()
    await Promise.resolve()
    flush()
    expect(within(dialog).getByRole('status').textContent).toMatch(/Verevon svarer|Verevon is replying/)

    // The chat sends it as the next turn, in the same pass that clears the deferral.
    setFollowUpPending(false)
    setTurns((current) => [
      ...current,
      { id: 'user-2', role: 'user', content: 'Diktert tekst', streaming: false },
      { id: 'asst-2', role: 'assistant', content: 'Eva', streaming: true },
    ])
    setSubmitting(true)
    flush()
    await Promise.resolve()
    flush()
    expect(within(dialog).getByRole('status').textContent).toMatch(/Verevon svarer|Verevon is replying/)

    setTurns((current) => current.map((turn) => (turn.id === 'asst-2' ? { ...turn, content: 'Eva eier planen.', streaming: false } : turn)))
    setSubmitting(false)
    flush()
    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/^(Svar mottatt|Reply received)$/))
    expect(within(dialog).getByText('Eva eier planen.')).toBeTruthy()
    expect(within(dialog).queryByText('Første svar.')).toBeNull()
  })

  it('auto-sends only when the person turned it on, and remembers the choice', async () => {
    const { submitted } = renderComposer()
    const dialog = await openVoiceMode()
    const toggle = within(dialog).getByRole('checkbox', { name: /^(Send automatisk|Send automatically)$/ }) as HTMLInputElement
    expect(toggle.checked).toBe(false)
    fireEvent.click(toggle)
    flush()
    expect(window.localStorage.getItem('verevon.voice-mode.auto-send.v1')).toBe('true')

    await speak(dialog)
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('Diktert tekst')
  })
})

describe('Voice mode fails closed and explains itself', () => {
  it('opens blocked in Temporary chat, says why, and never touches the microphone or the dictate route', async () => {
    renderComposer({ temporaryChat: true })
    const button = voiceModeButton() as HTMLButtonElement
    // Reachable by keyboard, touch and screen reader, with the reason attached.
    expect(button.disabled).toBe(false)
    expect(button.title).toMatch(/ikke tilgjengelig i midlertidig chat|not available in Temporary chat/)
    const description = document.getElementById(button.getAttribute('aria-describedby') ?? '')
    expect(description?.textContent).toMatch(/^Stemmemodus er ikke tilgjengelig i midlertidig chat ennå\./)

    const dialog = await openVoiceMode()
    expect(within(dialog).getByRole('status').textContent).toMatch(/^Stemmemodus er ikke tilgjengelig i midlertidig chat ennå\./)
    // The status is the dialog's description, read as focus enters.
    expect(document.getElementById(dialog.getAttribute('aria-describedby') ?? '')).toBe(within(dialog).getByRole('status'))
    expect((micButton(dialog) as HTMLButtonElement).disabled).toBe(true)
    expect(within(dialog).queryByRole('checkbox')).toBeNull()
    const closeAction = within(dialog).getByRole('button', { name: /^(Lukk|Close)$/ })
    await waitFor(() => expect(document.activeElement).toBe(closeAction))

    fireEvent.click(micButton(dialog))
    flush()
    await Promise.resolve()
    expect(getUserMedia).not.toHaveBeenCalled()
    expect(dictateAudioBlob).not.toHaveBeenCalled()

    fireEvent.click(closeAction)
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(document.activeElement).toBe(voiceModeButton())
  })

  it('shows a blocked microphone as an alert', async () => {
    getUserMedia.mockRejectedValueOnce(new DOMException('Permission denied', 'NotAllowedError'))
    renderComposer()
    const dialog = await openVoiceMode()
    fireEvent.click(micButton(dialog))
    flush()
    const alert = await within(dialog).findByRole('alert')
    expect(alert.textContent).toMatch(/Mikrofontilgang er blokkert|Microphone access is blocked/)
    expect(dictateAudioBlob).not.toHaveBeenCalled()
  })

  it('offers to record again when no speech was heard', async () => {
    dictateAudioBlob.mockRejectedValueOnce(new ApiError('no speech detected in the audio', 422, 'no_speech_detected'))
    renderComposer()
    const dialog = await openVoiceMode()
    await speak(dialog)
    expect((await within(dialog).findByRole('alert')).textContent).toMatch(/Hørte ingen tale\. Prøv igjen\.|No speech was detected/)

    fireEvent.click(within(dialog).getByRole('button', { name: /^(Ta opp på nytt|Record again)$/ }))
    flush()
    await waitFor(() => expect(micButton(dialog).getAttribute('aria-pressed')).toBe('true'))
    expect(getUserMedia).toHaveBeenCalledTimes(2)
    expect(within(dialog).queryByRole('alert')).toBeNull()
  })

  it('closing aborts the dictate call, inserts nothing and returns focus', async () => {
    let signal: AbortSignal | undefined
    dictateAudioBlob.mockImplementationOnce((_blob, _language, _context, abort) => {
      signal = abort
      return new Promise<DictationResult>(() => undefined)
    })
    renderComposer()
    const dialog = await openVoiceMode()
    await speak(dialog)
    expect(within(dialog).getByRole('status').textContent).toMatch(/Transkriberer|Transcribing/)
    expect(track.stop).toHaveBeenCalled()

    fireEvent.click(within(dialog).getByRole('button', { name: /^(Lukk stemmemodus|Close voice mode)$/ }))
    flush()
    expect(signal?.aborted).toBe(true)
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(draft().value).toBe('')
    expect(document.activeElement).toBe(voiceModeButton())
  })

  it('releases the microphone when the composer goes away mid-take', async () => {
    const { unmount } = renderComposer()
    const dialog = await openVoiceMode()
    await startRecording(dialog)
    expect(track.stop).not.toHaveBeenCalled()
    unmount()
    expect(track.stop).toHaveBeenCalled()
    expect(dictateAudioBlob).not.toHaveBeenCalled()
  })

  it('announces Voice input errors to screen readers', async () => {
    getUserMedia.mockRejectedValueOnce(new DOMException('Permission denied', 'NotAllowedError'))
    const { container } = renderComposer()
    fireEvent.click(screen.getByRole('button', { name: /^(Stemmeinndata|Voice input)$/ }))
    flush()
    await waitFor(() => {
      const status = [...container.querySelectorAll('.sr-only[role="status"]')].find((node) => node.textContent)
      expect(status?.textContent).toMatch(/Mikrofontilgang er blokkert|Microphone access is blocked/)
    })
  })
})

describe('Voice input and Voice mode never share the microphone', () => {
  const voiceInput = () => screen.getByRole('button', { name: /^(Stemmeinndata|Voice input)$/ })

  /** A permission prompt that stays open until the test answers it. */
  function holdPermissionPrompt() {
    let grant!: () => void
    getUserMedia.mockImplementationOnce(
      () =>
        new Promise<MediaStream>((resolve) => {
          grant = () => resolve({ getTracks: () => [track] } as unknown as MediaStream)
        }),
    )
    return { grant: () => grant() }
  }

  async function settle() {
    for (let i = 0; i < 3; i += 1) {
      await Promise.resolve()
      flush()
    }
  }

  it('keeps Voice mode shut while Voice input waits on the permission prompt', async () => {
    const prompt = holdPermissionPrompt()
    renderComposer()
    fireEvent.click(voiceInput())
    flush()
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(1))
    // The prompt is open and nothing records yet, but the microphone is spoken for.
    expect(voiceInput().getAttribute('aria-pressed')).toBe('false')
    expect((voiceModeButton() as HTMLButtonElement).disabled).toBe(true)

    prompt.grant()
    await waitFor(() => expect(voiceInput().getAttribute('aria-pressed')).toBe('true'))
    expect((voiceModeButton() as HTMLButtonElement).disabled).toBe(true)
    // Voice input records on Voice mode's capped recorder.
    expect(FakeRecorder.instances[0]?.options).toEqual({ audioBitsPerSecond: 32_000, mimeType: 'audio/webm;codecs=opus' })
  })

  it('hands back a stream granted after the composer went away', async () => {
    const prompt = holdPermissionPrompt()
    const { unmount } = renderComposer()
    fireEvent.click(voiceInput())
    flush()
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(1))
    unmount()
    prompt.grant()
    await settle()
    expect(track.stop).toHaveBeenCalled()
    expect(FakeRecorder.instances).toHaveLength(0)
    expect(dictateAudioBlob).not.toHaveBeenCalled()
  })

  it('cancels on a second press during the prompt and hands back the late stream', async () => {
    const prompt = holdPermissionPrompt()
    renderComposer()
    fireEvent.click(voiceInput())
    flush()
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(1))
    fireEvent.click(voiceInput())
    flush()
    expect((voiceModeButton() as HTMLButtonElement).disabled).toBe(false)
    prompt.grant()
    await settle()
    expect(track.stop).toHaveBeenCalled()
    expect(FakeRecorder.instances).toHaveLength(0)
    expect(voiceInput().getAttribute('aria-pressed')).toBe('false')
    expect(dictateAudioBlob).not.toHaveBeenCalled()
  })

  it('drops a take, unsent, when Temporary chat starts mid-recording', async () => {
    const [temporary, setTemporary] = createSignal(false)
    renderComposer({ temporaryChat: temporary })
    fireEvent.click(voiceInput())
    flush()
    await waitFor(() => expect(voiceInput().getAttribute('aria-pressed')).toBe('true'))
    setTemporary(true)
    flush()
    await settle()
    expect(track.stop).toHaveBeenCalled()
    expect(voiceInput().getAttribute('aria-pressed')).toBe('false')
    expect(voiceInput().getAttribute('aria-disabled')).toBe('true')
    expect(dictateAudioBlob).not.toHaveBeenCalled()
  })

  it('stops a reply being read aloud before it records, so the take cannot transcribe it', async () => {
    const player = speechPlayer()
    const playing = vi.spyOn(player, 'playingId').mockReturnValue('asst-1')
    const stop = vi.spyOn(player, 'stop').mockImplementation(() => undefined)
    try {
      renderComposer()
      fireEvent.click(voiceInput())
      flush()
      await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(1))
      expect(stop).toHaveBeenCalled()
      expect(stop.mock.invocationCallOrder[0]!).toBeLessThan(getUserMedia.mock.invocationCallOrder[0]!)
    } finally {
      playing.mockRestore()
      stop.mockRestore()
    }
  })
})
