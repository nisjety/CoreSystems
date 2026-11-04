// @vitest-environment jsdom

/**
 * WS18-S4: a thread whose session names a Space is reopened through Chat's own transcript
 * route while `VITE_CHAT_PROJECTS_ENABLED` is off, as it was before the project picker.
 * Only a switched-on build reads the room's transcript, and only for a thread it has
 * recorded as a project thread.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'

const THREAD = 'thread-room'
const OTHER = 'thread-chat'
const SPACE_REF = 'space-room'
const STAMP = '2026-10-26T10:00:00.000Z'

const mocks = vi.hoisted(() => ({
  getChatThread: vi.fn(),
  getChatThreadTranscript: vi.fn(async (_threadId: string) => null as unknown),
  getThreadMessages: vi.fn(async (_threadId: string) => [] as unknown[]),
  getSpaceThreadTranscript: vi.fn(async (_spaceRef: string, _threadId: string) => null as unknown),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: vi.fn(),
    resumeStream: vi.fn(),
    cancelInvocation: vi.fn(async () => undefined),
    getChatThread: mocks.getChatThread,
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    getThreadMessages: mocks.getThreadMessages,
    listChatThreads: vi.fn(async () => [] as unknown[]),
    listModels: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/spaces-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/spaces-client')>()
  return { ...actual, getSpaceThreadTranscript: mocks.getSpaceThreadTranscript }
})

import { useChatController } from './use-chat-controller'

/** Lets the load that an active-thread event starts run to its end. */
async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
  }
}

/** Opens a thread as the sidebar does: the active-thread event starts the load. */
async function open(threadId: string) {
  setActiveChatThreadId(threadId)
  await settle()
}

function mount(): () => void {
  let dispose: () => void = () => undefined
  createRoot((disposeRoot) => {
    dispose = disposeRoot
    useChatController()
    flush()
  })
  return () => dispose()
}

describe('WS18-S4 project routing for a thread that names a Space', () => {
  let unmount: () => void = () => undefined

  beforeEach(() => {
    __resetRetentionForTests()
    window.localStorage.clear()
    window.sessionStorage.clear()
    mocks.getChatThread.mockImplementation(async (threadId: string) => (
      threadId === THREAD
        ? { threadId, title: 'Room', preview: 'Room', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'space' }
        : null
    ))
    mocks.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'From the room' }] })
  })

  afterEach(() => {
    unmount()
    unmount = () => undefined
    vi.unstubAllEnvs()
    vi.clearAllMocks()
  })

  it('reopens the thread through Chat\'s transcript route while the switch is off', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'false')
    unmount = mount()
    await open(THREAD)
    await open(OTHER)
    mocks.getChatThreadTranscript.mockClear()
    await open(THREAD)
    expect(mocks.getChatThreadTranscript).toHaveBeenCalledWith(THREAD)
    expect(mocks.getSpaceThreadTranscript).not.toHaveBeenCalled()
  })

  it('reads the room transcript for a recorded project thread while the switch is on', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    unmount = mount()
    await open(THREAD)
    await open(OTHER)
    await open(THREAD)
    expect(mocks.getSpaceThreadTranscript).toHaveBeenCalledWith(SPACE_REF, THREAD)
  })
})
