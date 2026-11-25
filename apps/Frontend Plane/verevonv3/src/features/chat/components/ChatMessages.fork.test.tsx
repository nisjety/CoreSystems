// @vitest-environment jsdom

/**
 * "Fork from here" on an assistant turn, through the message that mounts the control (F-f05-21). The fork call and its
 * refusals are covered in `ForkAction.test.tsx`; this file covers that the message hands the page's `onForked` to the
 * control, so the page gets the copy's id and the server's session for it and can open the copy.
 */

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const { forkChatThread } = vi.hoisted(() => ({ forkChatThread: vi.fn() }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, forkChatThread }
})

/** A finished assistant turn, the shape the fork control is offered on. */
const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er ferdig.',
  createdAt: '2026-11-20T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

/** The required `AssistantMessage` props, with no-op stubs for the rest. */
const props = (message: ChatTurn, onForked?: (threadId: string, session?: unknown) => void) => ({
  copied: false,
  message,
  threadId: 'thread-source',
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
  onForked,
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  forkChatThread.mockReset()
})

describe('AssistantMessage: Fork from here hands the copy to the page', () => {
  it('passes onForked to the fork control, which hands the new thread and its session up', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    const session = { threadId: 'thread-fork', title: 'Kopi av samtalen', preview: 'Forhåndsvisning', updatedAt: '2026-11-20T09:00:00.000Z' }
    forkChatThread.mockResolvedValue(session)
    const onForked = vi.fn()
    const { getByRole } = render(() => <AssistantMessage {...props(turn(), onForked)} />)

    fireEvent.click(getByRole('button', { name: /Forgren herfra|Fork from here/ }))

    await waitFor(() => expect(onForked).toHaveBeenCalledWith('thread-fork', session))
    expect(forkChatThread).toHaveBeenCalledWith('thread-source', { fromMessageId: 'asst-1' })
  })

  it('offers no fork control while the build keeps forking off', () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'false')
    const { queryByRole } = render(() => <AssistantMessage {...props(turn(), vi.fn())} />)

    expect(queryByRole('button', { name: /Forgren herfra|Fork from here/ })).toBeNull()
  })
})
