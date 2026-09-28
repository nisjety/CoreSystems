// @vitest-environment jsdom

// "Les opp svar" in Voice mode: off by default, reads a finished reply aloud
// through governed text-to-speech (the BFF speech route, pinned to Azure
// Speech), keeps the microphone shut while it plays, can be stopped, and on
// failure switches itself off with an alert. It never falls back to the
// browser's own speech APIs.

import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { speechPlayer } from './speech-playback'
import { VoiceModeDialog, type VoiceReply, type VoiceSendResult } from './VoiceModeDialog'

const { dictateAudioBlob } = vi.hoisted(() => ({
  dictateAudioBlob: vi.fn(async () => ({ text: 'Hva er status?', rawText: 'hva er status', cleaned: true })),
}))

vi.mock('@/shared/api/audio-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/audio-client')>()
  return { ...actual, dictateAudioBlob }
})

class FakeRecorder {
  static isTypeSupported(mimeType: string) {
    return mimeType === 'audio/webm;codecs=opus'
  }

  mimeType: string
  ondataavailable: ((event: { data: Blob }) => void) | null = null
  onstop: (() => void) | null = null
  onerror: (() => void) | null = null

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    this.mimeType = options?.mimeType ?? 'audio/webm'
  }

  start() {}
  stop() {
    this.ondataavailable?.({ data: new Blob(['audio']) })
    this.onstop?.()
  }
}

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
const getUserMedia = vi.fn(async () => ({ getTracks: () => [track] }) as unknown as MediaStream)

type Played = { element: HTMLMediaElement; src: string }
let played: Played[] = []
let objectUrls = 0
const originalUrl = { create: URL.createObjectURL, revoke: URL.revokeObjectURL }
let restoreDialog: (() => void) | undefined

const speechOk = () =>
  new Response(JSON.stringify({ audio_base64: btoa('fake mp3'), format: 'mp3', provider_used: 'azure-speech' }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => speechOk())

beforeEach(() => {
  played = []
  vi.stubGlobal('MediaRecorder', FakeRecorder)
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('SpeechRecognition', webSpeech.SpeechRecognition)
  vi.stubGlobal('webkitSpeechRecognition', webSpeech.webkitSpeechRecognition)
  vi.stubGlobal('SpeechSynthesisUtterance', webSpeech.SpeechSynthesisUtterance)
  vi.stubGlobal('speechSynthesis', { speak: webSpeech.speak, cancel: webSpeech.speak, getVoices: webSpeech.speak })
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia } })
  URL.createObjectURL = vi.fn(() => `blob:verevon/${++objectUrls}`)
  URL.revokeObjectURL = vi.fn()
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(function play(this: HTMLMediaElement) {
    played.push({ element: this, src: this.src })
    return Promise.resolve()
  })
  vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => undefined)
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
  speechPlayer().stop()
  expect(webSpeech.SpeechRecognition).not.toHaveBeenCalled()
  expect(webSpeech.webkitSpeechRecognition).not.toHaveBeenCalled()
  expect(webSpeech.SpeechSynthesisUtterance).not.toHaveBeenCalled()
  expect(webSpeech.speak).not.toHaveBeenCalled()
  restoreDialog?.()
  URL.createObjectURL = originalUrl.create
  URL.revokeObjectURL = originalUrl.revoke
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  vi.clearAllMocks()
  fetchMock.mockImplementation(async () => speechOk())
  window.localStorage.clear()
})

/** A chat-like host: the reply streams in after the send and then settles. */
async function renderDialog() {
  const [reply, setReply] = createSignal<VoiceReply | null>({ id: 'asst-0', text: 'Forrige svar', streaming: false })
  const [submitting, setSubmitting] = createSignal(false)
  const onSend = vi.fn(async (_text: string): Promise<VoiceSendResult> => {
    setSubmitting(true)
    setReply({ id: 'asst-1', text: '', streaming: true })
    return { ok: true }
  })
  render(() => (
    <VoiceModeDialog
      awaitReply
      language="nb-NO"
      model="Terra"
      onClose={vi.fn()}
      onInsert={vi.fn()}
      onSend={onSend}
      onStop={vi.fn()}
      reply={reply()}
      submitting={submitting()}
      temporaryChat={false}
    />
  ))
  const finishReply = (text: string) => {
    setReply({ id: 'asst-1', text, streaming: false })
    setSubmitting(false)
    flush()
  }
  await screen.findByRole('dialog', { name: 'Stemmemodus' })
  return { onSend, finishReply }
}

const dialog = () => screen.getByRole('dialog', { name: 'Stemmemodus' })
const mic = () => within(dialog()).getByRole('button', { name: /^(Start opptak|Stopp opptak)$/ })
const status = () => within(dialog()).getByRole('status').textContent
const readRepliesToggle = () => within(dialog()).getByRole('checkbox', { name: 'Les opp svar' }) as HTMLInputElement

/** Tap, tap, review, Send: one spoken turn through the host's own send. */
async function sendSpokenTurn(onSend: Awaited<ReturnType<typeof renderDialog>>['onSend']) {
  fireEvent.click(mic())
  flush()
  await waitFor(() => expect(mic().getAttribute('aria-pressed')).toBe('true'))
  fireEvent.click(mic())
  flush()
  await within(dialog()).findByRole('textbox', { name: 'Transkripsjon' })
  fireEvent.click(within(dialog()).getByRole('button', { name: 'Send' }))
  flush()
  await waitFor(() => expect(onSend).toHaveBeenCalledWith('Hva er status?'))
  await waitFor(() => expect(status()).toBe('Verevon svarer …'))
}

describe('Voice mode "Les opp svar"', () => {
  it('is off by default, so a finished reply is only shown', async () => {
    const { onSend, finishReply } = await renderDialog()
    expect(readRepliesToggle().checked).toBe(false)
    await sendSpokenTurn(onSend)
    finishReply('Status er grønn.')
    await waitFor(() => expect(status()).toBe('Svar mottatt'))
    expect(within(dialog()).getByText('Status er grønn.')).toBeTruthy()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('reads the reply aloud through governed speech with the microphone shut, and can be stopped', async () => {
    const { onSend, finishReply } = await renderDialog()
    fireEvent.click(readRepliesToggle())
    flush()
    expect(window.localStorage.getItem('verevon.voice-mode.read-replies.v1')).toBe('true')
    await sendSpokenTurn(onSend)

    finishReply('Status er **grønn** [1].')
    await waitFor(() => expect(status()).toBe('Leser opp svaret …'))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toMatch(/\/api\/v1\/ai\/speech$/)
    // Voice mode speaks in its own voice language.
    expect(JSON.parse(String(init?.body))).toEqual({
      operation: 'tts',
      text: 'Status er grønn.',
      language: 'nb-NO',
      voice: 'nb-NO-PernilleNeural',
      format: 'mp3',
      provider: 'azure-speech',
    })
    await waitFor(() => expect(played.at(-1)?.src).toMatch(/^blob:/))
    // The reply stays on screen while it is read.
    expect(dialog().textContent).toContain('Status er **grønn** [1].')

    // The microphone must not record the reply.
    expect(mic().getAttribute('aria-disabled')).toBe('true')
    fireEvent.click(mic())
    flush()
    expect(getUserMedia).toHaveBeenCalledTimes(1)

    fireEvent.click(within(dialog()).getByRole('button', { name: 'Stopp opplesing' }))
    flush()
    await waitFor(() => expect(status()).toBe('Trykk for å snakke'))
    expect(mic().getAttribute('aria-disabled')).toBeNull()
    expect(speechPlayer().playingId()).toBeNull()
    // Reading stays on for the next reply.
    expect(readRepliesToggle().checked).toBe(true)
  })

  it('returns to ready by itself when the reply has been read', async () => {
    const { onSend, finishReply } = await renderDialog()
    fireEvent.click(readRepliesToggle())
    flush()
    await sendSpokenTurn(onSend)
    finishReply('Kort svar.')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(played.at(-1)?.src).toMatch(/^blob:/))
    const before = played.length
    played.at(-1)!.element.dispatchEvent(new Event('ended'))
    await waitFor(() => expect(status()).toBe('Trykk for å snakke'))
    expect(played).toHaveLength(before)
    await waitFor(() => expect(document.activeElement).toBe(mic()))
  })

  it('switches itself off with an alert when reading aloud fails, and leaves the reply', async () => {
    fetchMock.mockImplementation(async () =>
      new Response(JSON.stringify({ error: { code: 'zdr_unavailable', message: 'no' } }), {
        status: 412,
        headers: { 'content-type': 'application/json' },
      }),
    )
    const { onSend, finishReply } = await renderDialog()
    fireEvent.click(readRepliesToggle())
    flush()
    await sendSpokenTurn(onSend)
    finishReply('Status er grønn.')

    const alert = await within(dialog()).findByRole('alert')
    expect(alert.textContent).toBe('Opplesing er utilgjengelig. Svaret vises i chatten.')
    expect(readRepliesToggle().checked).toBe(false)
    expect(window.localStorage.getItem('verevon.voice-mode.read-replies.v1')).toBe('false')
    expect(within(dialog()).getByText('Status er grønn.')).toBeTruthy()
    expect(status()).toBe('Svar mottatt')
    // Fail closed: the microphone is available again, no audio played.
    expect(mic().getAttribute('aria-disabled')).toBeNull()
    expect(played.every((entry) => entry.src.startsWith('blob:'))).toBe(true)
  })

  it('does not read a reply the person stopped', async () => {
    const { onSend, finishReply } = await renderDialog()
    fireEvent.click(readRepliesToggle())
    flush()
    await sendSpokenTurn(onSend)
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Stopp svaret' }))
    finishReply('Delvis')
    await waitFor(() => expect(status()).toBe('Svar mottatt'))
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
