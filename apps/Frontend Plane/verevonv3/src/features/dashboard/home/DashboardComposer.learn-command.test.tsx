// @vitest-environment jsdom

// W3-EP-11-S7-a: the composer's `/learn` command. A typed `/learn note` is never a chat turn: the note goes to
// the learning request. The note is bounded at LEARN_NOTE_MAX_CHARS (500) characters, counted as the server
// counts them; a longer note is refused in the composer before any request, and the draft is kept. The command
// exists only behind VITE_LEARN_COMMAND === "true", on a saved thread, never on a temporary or zero-retention
// chat, and only while the memory controls allow learning.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { requestLearnFromThread } = vi.hoisted(() => ({ requestLearnFromThread: vi.fn() }))

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
  return {
    ...actual,
    loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS),
    requestLearnFromThread,
  }
})

import type { LearnCommandContext } from '@/shared/api/chat-actions-client'
import { DashboardComposer } from './DashboardComposer'

const submitted: unknown[] = []

const savedThread: LearnCommandContext = {
  threadId: 'thread-1',
  temporaryChat: false,
  zeroRetention: false,
  learningAllowed: true,
}

function renderComposer(learnContext: LearnCommandContext | undefined = savedThread, temporaryChat = false) {
  function ComposerPage() {
    const [message, setMessage] = createSignal('')
    return (
      <DashboardComposer
        message={message()}
        onMessageChange={setMessage}
        learnContext={learnContext}
        temporaryChat={temporaryChat}
        onSubmit={(payload) => {
          submitted.push(payload)
        }}
      />
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function textbox() {
  return document.querySelector('textarea') as HTMLTextAreaElement
}

function typeText(text: string) {
  fireEvent.input(textbox(), { target: { value: text } })
  flush()
}

function pressEnter() {
  fireEvent.keyDown(textbox(), { key: 'Enter', code: 'Enter' })
  flush()
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  requestLearnFromThread.mockReset()
  requestLearnFromThread.mockResolvedValue({ state: 'queued' })
})

afterEach(() => {
  submitted.length = 0
  vi.unstubAllEnvs()
})

describe('/learn with VITE_LEARN_COMMAND on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_LEARN_COMMAND', 'true')
  })

  it('refuses a 501-character note before any request and keeps the draft', async () => {
    renderComposer()
    await settle()
    const draft = `/learn ${'a'.repeat(501)}`
    typeText(draft)
    pressEnter()
    await settle()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(/500/)
    expect(requestLearnFromThread).not.toHaveBeenCalled()
    expect(submitted).toHaveLength(0)
    expect(textbox().value).toBe(draft)
  })

  it('counts characters, not UTF-16 units: 500 emoji are within the bound', async () => {
    renderComposer()
    await settle()
    const note = '\u{1F600}'.repeat(500)
    typeText(`/learn ${note}`)
    pressEnter()

    await waitFor(() => expect(requestLearnFromThread).toHaveBeenCalledTimes(1))
    expect(requestLearnFromThread).toHaveBeenCalledWith('thread-1', note)
  })

  it('sends a 500-character note as the learn request, never as a chat turn, and clears the draft', async () => {
    renderComposer()
    await settle()
    const note = 'b'.repeat(500)
    typeText(`/learn ${note}`)
    pressEnter()

    await waitFor(() => expect(requestLearnFromThread).toHaveBeenCalledTimes(1))
    expect(requestLearnFromThread).toHaveBeenCalledWith('thread-1', note)
    expect(await screen.findByRole('status')).toBeTruthy()
    await waitFor(() => expect(textbox().value).toBe(''))
    expect(submitted).toHaveLength(0)
  })

  it('keeps the draft and shows the refusal when the server refuses', async () => {
    requestLearnFromThread.mockRejectedValue(new Error('learn_command_disabled'))
    renderComposer()
    await settle()
    typeText('/learn kort notat')
    pressEnter()

    await waitFor(() => expect(requestLearnFromThread).toHaveBeenCalledTimes(1))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(textbox().value).toBe('/learn kort notat')
    expect(submitted).toHaveLength(0)
  })

  it.each([
    ['a zero-retention chat', { ...savedThread, zeroRetention: true }],
    ['a temporary chat (page context)', { ...savedThread, temporaryChat: true }],
    ['an unsaved chat', { ...savedThread, threadId: null }],
    ['a chat whose memory controls do not allow learning', { ...savedThread, learningAllowed: false }],
  ])('never sends a learn request on %s', async (_label, context) => {
    renderComposer(context)
    await settle()
    typeText('/learn notat')
    pressEnter()
    await settle()
    expect(requestLearnFromThread).not.toHaveBeenCalled()
  })

  it('never sends a learn request on a temporary chat the composer itself knows about', async () => {
    renderComposer(savedThread, true)
    await settle()
    typeText('/learn notat')
    pressEnter()
    await settle()
    expect(requestLearnFromThread).not.toHaveBeenCalled()
  })
})

describe('/learn with VITE_LEARN_COMMAND off', () => {
  it.each([[undefined], ['1'], ['TRUE'], ['yes']])('VITE_LEARN_COMMAND=%s sends no learn request', async (value) => {
    if (value !== undefined) vi.stubEnv('VITE_LEARN_COMMAND', value)
    renderComposer()
    await settle()
    typeText('/learn notat')
    pressEnter()
    await settle()
    expect(requestLearnFromThread).not.toHaveBeenCalled()
  })
})
