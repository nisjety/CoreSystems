// @vitest-environment jsdom

/**
 * Voice mode survives the first message of a new chat.
 *
 * ChatPage used to render one composer inside the empty state and another in
 * the thread dock. The first send of a new chat pushes its turns before the
 * stream opens, `hasMessages()` flips, and the empty-state composer unmounted
 * with its Voice mode dialog still open: no "Verevon svarer …", no
 * "Svar mottatt", no reading aloud, and focus dropped to the page body. The
 * unit suites render a standalone composer and the e2e spec typed its first
 * turn, so the spoken first turn was never exercised.
 *
 * The page now creates ONE composer and moves it between the two places. The
 * full page has no cheap unit harness (see work-content-identity.test.ts), so
 * this pins the page's source to that pattern and proves the pattern itself
 * keeps the dialog alive, with the real composer and the real empty state.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createMemo, createSignal, flush, Match, Show, Switch } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { latestVoiceReply } from '@/features/voice/voice-reply'

const { dictateAudioBlob } = vi.hoisted(() => ({
  dictateAudioBlob: vi.fn(async () => ({ text: 'Hvem eier planen?', rawText: 'hvem eier planen', cleaned: true })),
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

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer, type DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import { EmptyChatState } from './ChatMessages'

async function source(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

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

type Turn = { id: string; role: 'user' | 'assistant'; content: string; streaming: boolean }

/**
 * ChatPage's layout, reduced to what decides where the composer lives: the
 * empty state before the first message, the thread dock after it.
 */
function renderChatLayout() {
  const submitted: DashboardComposerSubmitPayload[] = []
  const [turns, setTurns] = createSignal<Turn[]>([])
  function Page() {
    const [input, setInput] = createSignal('')
    const hasMessages = () => turns().length > 0
    const isStreaming = () => turns().some((turn) => turn.streaming)
    const voiceReply = createMemo(() => latestVoiceReply(turns(), false))
    // As in ChatPage: the chat pushes both turns before the stream opens.
    const composerView = (
      <DashboardComposer
        allowMidRunSubmit
        appearance="chat"
        message={input()}
        onMessageChange={setInput}
        onStop={() => undefined}
        onSubmit={(payload) => {
          submitted.push(payload)
          setTurns([
            { id: 'user-1', role: 'user', content: payload.text, streaming: false },
            { id: 'asst-1', role: 'assistant', content: '', streaming: true },
          ])
        }}
        submitting={isStreaming()}
        voiceReply={voiceReply()}
      />
    )
    return (
      <section>
        <Switch>
          <Match when={!hasMessages()}>
            <EmptyChatState onSelectPrompt={setInput} onResumeThread={() => undefined}>
              {composerView}
            </EmptyChatState>
          </Match>
          <Match when={hasMessages()}>
            <div class="verevon-chat-message-list" />
          </Match>
        </Switch>
        <Show when={hasMessages()}>
          <div class="verevon-chat-composer-dock">{composerView}</div>
        </Show>
      </section>
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: Page }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  render(() => <TestRouter>{(routed) => <>{routed.children}</>}</TestRouter>)
  return { submitted, setTurns }
}

const micButton = (dialog: HTMLElement) =>
  within(dialog).getByRole('button', { name: /^(Start opptak|Start recording|Stopp opptak|Stop recording)$/ })

let restoreDialog: (() => void) | undefined

beforeEach(() => {
  vi.stubGlobal('MediaRecorder', FakeRecorder)
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: { getUserMedia: vi.fn(async () => ({ getTracks: () => [{ stop: () => undefined }] })) },
  })
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
  restoreDialog?.()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
  window.localStorage.clear()
})

describe('the chat page keeps one composer across its first message', () => {
  it('creates the composer once and places that same one in both layouts', async () => {
    const page = await source('src/features/chat/components/ChatPage.tsx')
    expect(page).toContain('const composerView = composer()')
    expect(page.match(/\{composerView\}/g)).toHaveLength(2)
    expect(page, 'a composer per layout unmounts the one that sent the first message').not.toContain('{composer()}')
  })

  it('keeps Voice mode open through a spoken first message, to "Svar mottatt"', async () => {
    const { submitted, setTurns } = renderChatLayout()
    fireEvent.click(screen.getByRole('button', { name: /^(Stemmemodus|Voice mode)$/ }))
    flush()
    const dialog = await screen.findByRole('dialog', { name: /^(Stemmemodus|Voice mode)$/ })

    fireEvent.click(micButton(dialog))
    flush()
    await waitFor(() => expect(micButton(dialog).getAttribute('aria-pressed')).toBe('true'))
    fireEvent.click(micButton(dialog))
    flush()
    await within(dialog).findByRole('textbox', { name: /^(Transkripsjon|Transcript)$/ })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Send$/ }))

    // The first message moved the page from the empty state to the thread.
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('Hvem eier planen?')
    await waitFor(() => expect(document.querySelector('.verevon-chat-message-list')).not.toBeNull())
    expect(document.querySelector('.verevon-chat-empty')).toBeNull()
    // One composer, now in the dock, and the same dialog still open.
    expect(screen.getAllByRole('textbox', { name: /^(Meld Verevon|Message Verevon)$/ })).toHaveLength(1)
    expect(document.querySelector('.verevon-chat-composer-dock #verevon-dashboard-input')).not.toBeNull()
    expect(dialog.isConnected).toBe(true)
    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/Verevon svarer|Verevon is replying/))
    const stopReply = within(dialog).getByRole('button', { name: /^(Stopp svaret|Stop the reply)$/ })
    await waitFor(() => expect(document.activeElement).toBe(stopReply))

    setTurns((current) => current.map((turn) => (turn.id === 'asst-1' ? { ...turn, content: 'Eva eier planen.', streaming: false } : turn)))
    flush()
    await waitFor(() => expect(within(dialog).getByRole('status').textContent).toMatch(/^(Svar mottatt|Reply received)$/))
    expect(within(dialog).getByText('Eva eier planen.')).toBeTruthy()
    await waitFor(() => expect(document.activeElement).toBe(micButton(dialog)))
  })
})
