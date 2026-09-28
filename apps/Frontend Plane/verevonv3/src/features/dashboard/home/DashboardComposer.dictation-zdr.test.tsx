// @vitest-environment jsdom

// Dictation ignored Temporary chat: the "Voice input" mic sent the recording
// through the governed `/api/v1/ai/dictate` route as an ordinary, retained
// request. Passing the route's `zdr` flag was not enough either: it reaches
// only the text clean-up pass, and the audio goes to speech-to-text without
// zero retention. So Voice input fails closed in Temporary chat, like Voice
// mode and "Les høyt": no microphone, no dictate call, and it says why. The
// separate "Voice mode" is covered by DashboardComposer.voice-mode.test.tsx.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { dictateAudioBlob } = vi.hoisted(() => ({
  dictateAudioBlob: vi.fn(async (_blob: Blob, _language: string, _context?: string, _signal?: AbortSignal, _zdr?: boolean) => (
    { text: 'Diktert tekst', rawText: 'diktert tekst', cleaned: true }
  )),
}))

vi.mock('@/shared/api/audio-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/audio-client')>()
  return { ...actual, dictateAudioBlob }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

import { DashboardComposer } from './DashboardComposer'

/** Just enough MediaRecorder for one take: stop() delivers a chunk, then ends. */
class FakeRecorder {
  mimeType = 'audio/webm'
  ondataavailable: ((event: { data: Blob }) => void) | null = null
  onstop: (() => void) | null = null
  onerror: (() => void) | null = null
  start() {}
  stop() {
    this.ondataavailable?.({ data: new Blob(['audio']) })
    this.onstop?.()
  }
}

function renderComposer(props: { temporaryChat?: boolean; temporaryChatLocked?: boolean }) {
  function ComposerPage() {
    const [message, setMessage] = createSignal('')
    return (
      <DashboardComposer
        message={message()}
        onMessageChange={setMessage}
        onSubmit={() => undefined}
        temporaryChat={props.temporaryChat}
        temporaryChatLocked={props.temporaryChatLocked}
      />
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routed) => <>{routed.children}</>}</TestRouter>)
}

const voiceInput = () => screen.getByRole('button', { name: /^(Stemmeinndata|Voice input)$/ })

async function dictateOnce() {
  const mic = voiceInput()
  fireEvent.click(mic)
  flush()
  await waitFor(() => expect(mic.getAttribute('aria-pressed')).toBe('true'))
  fireEvent.click(mic)
  flush()
  await waitFor(() => expect(dictateAudioBlob).toHaveBeenCalledTimes(1))
}

const getUserMedia = vi.fn(async (_constraints: MediaStreamConstraints) => ({ getTracks: () => [{ stop: () => undefined }] }))

beforeEach(() => {
  vi.stubGlobal('MediaRecorder', FakeRecorder)
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia } })
})

afterEach(() => {
  vi.unstubAllGlobals()
  dictateAudioBlob.mockClear()
  getUserMedia.mockClear()
})

/** Pressing Voice input in Temporary chat: nothing recorded, nothing sent, and the reason in view. */
async function expectBlockedPress() {
  const mic = voiceInput()
  // Off, but focusable and described, so the reason is announced.
  expect(mic.getAttribute('aria-disabled')).toBe('true')
  expect((mic as HTMLButtonElement).disabled).toBe(false)
  const description = document.getElementById(mic.getAttribute('aria-describedby') ?? '')
  expect(description?.textContent).toMatch(/Stemmeinndata er ikke tilgjengelig i midlertidig chat ennå/)
  expect(mic.title).toMatch(/ikke tilgjengelig i midlertidig chat/)

  fireEvent.click(mic)
  flush()
  await Promise.resolve()
  flush()
  expect(getUserMedia).not.toHaveBeenCalled()
  expect(dictateAudioBlob).not.toHaveBeenCalled()
  expect(mic.getAttribute('aria-pressed')).toBe('false')
  // Seen by someone who cannot hover for the tooltip, and announced once.
  const notice = document.querySelector('.dashboard-composer-attachments__notice')
  expect(notice?.textContent).toMatch(/^Stemmeinndata er ikke tilgjengelig i midlertidig chat ennå\./)
  const status = [...document.querySelectorAll('.sr-only[role="status"]')].find((node) => node.textContent)
  expect(status?.textContent).toBe(notice?.textContent)
}

describe('Voice input respects Temporary chat', () => {
  it('fails closed when the next message is temporary: no microphone and no dictate call', async () => {
    renderComposer({ temporaryChat: true })
    await expectBlockedPress()
  })

  it('fails closed inside a thread that is already temporary', async () => {
    renderComposer({ temporaryChatLocked: true })
    await expectBlockedPress()
  })

  it('leaves an ordinary dictation unchanged', async () => {
    renderComposer({})
    await dictateOnce()
    expect(dictateAudioBlob.mock.calls[0]?.[4]).toBe(false)
  })
})
