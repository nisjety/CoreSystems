// @vitest-environment jsdom

/**
 * WS18-S4 (F-f06-16, owner D-f06-C): a project thread is append-only in the first release. Regenerate, continue, edit,
 * branch, rewind, rerun and feedback return early on it, so none of them reaches an origin=chat endpoint, and
 * `turnActionsAllowed` tells the answer row to hide them. An ordinary thread whose load has settled, and any thread with
 * the switch off, is unchanged; while a thread's Space ref is still unknown its actions are withheld too.
 */

import { createEffect, createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'

type CapturedStream = {
  request: { content: string; spaceRef?: string; regenerated?: boolean; editResubmit?: boolean }
  handlers: ChatStreamHandlers
  resolve: () => void
}

type Controller = ReturnType<typeof useChatController>

const THREAD = 'thread-project'
const SPACE_REF = 'space-room'
const STAMP = '2026-11-24T10:00:00.000Z'

const mocks = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  return {
    streams,
    streamChat: vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => (
      new Promise<void>((resolve) => {
        streams.push({ request, handlers, resolve })
      })
    )),
    submitFeedback: vi.fn(async () => undefined),
    rewindChatThread: vi.fn(async () => undefined),
    getChatThread: vi.fn(async (_threadId: string) => null as unknown),
    getChatThreadTranscript: vi.fn(async (_threadId: string) => null as unknown),
    getThreadMessages: vi.fn(async (_threadId: string) => [] as unknown[]),
    getSpaceThreadTranscript: vi.fn(async (_spaceRef: string, _threadId: string) => null as unknown),
    saveChatThreadSnapshot: vi.fn(async (_threadId: string, _snapshot: unknown) => null as unknown),
    getRun: vi.fn(async (_runId: string) => null as unknown),
    listRuns: vi.fn(async (_request: unknown) => ({ runs: [] as unknown[], hasMore: false })),
  }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mocks.streamChat,
    submitFeedback: mocks.submitFeedback,
    rewindChatThread: mocks.rewindChatThread,
    getChatThread: mocks.getChatThread,
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    getThreadMessages: mocks.getThreadMessages,
    listChatThreads: vi.fn().mockResolvedValue([]),
    listModels: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: mocks.saveChatThreadSnapshot,
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('@/shared/api/spaces-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/spaces-client')>()
  return { ...actual, getSpaceThreadTranscript: mocks.getSpaceThreadTranscript }
})

vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return { ...actual, getRun: mocks.getRun, listRuns: mocks.listRuns }
})

import { useChatController } from './use-chat-controller'

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/** Lets the load that an active-thread event starts run to its end (as in use-chat-controller.project-gate.test.ts). */
async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
  }
}

function withController<T>(fn: (controller: Controller) => Promise<T>): Promise<T> {
  return new Promise((resolvePromise, rejectPromise) => {
    createRoot((dispose) => {
      const controller = useChatController()
      fn(controller).then(
        (value) => {
          dispose()
          resolvePromise(value)
        },
        (error) => {
          dispose()
          rejectPromise(error)
        },
      )
    })
  })
}

/** Mounts the controller as the page does, for a test that opens a thread through the active-thread event. */
function mount() {
  const mounted: { controller?: Controller; dispose?: () => void } = {}
  createRoot((disposeRoot) => {
    mounted.dispose = disposeRoot
    mounted.controller = useChatController()
  })
  // Flushed outside the root, as a rendered page runs its effects: the controller's mount effects write signals.
  flush()
  const controller = mounted.controller
  if (!controller) throw new Error('the controller did not mount')
  return { controller, unmount: () => mounted.dispose?.() }
}

/** Sends one question, streams a partial answer and stops it: a stopped answer that carries a request id. */
async function askAndStop(controller: Controller, text: string): Promise<CapturedStream> {
  await controller.handleComposerSubmit({ actions: [], attachments: [], text, tools: [] })
  await tick()
  const stream = mocks.streams[mocks.streams.length - 1]
  if (!stream) throw new Error('no answer was requested')
  stream.handlers.onMessage?.({ content: 'Status er grønn', requestId: 'req-1' })
  flush()
  controller.handleStop()
  flush()
  stream.resolve()
  await tick()
  return stream
}

describe('WS18-S4 turn actions on a project thread (F-f06-16)', () => {
  let unmount: () => void = () => undefined

  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    mocks.getChatThread.mockResolvedValue(null)
    mocks.getChatThreadTranscript.mockResolvedValue(null)
    mocks.getThreadMessages.mockResolvedValue([])
    mocks.getSpaceThreadTranscript.mockResolvedValue(null)
  })

  afterEach(() => {
    unmount()
    unmount = () => undefined
    mocks.streams.length = 0
    vi.clearAllMocks()
    vi.unstubAllEnvs()
    __resetRetentionForTests()
    try {
      window.localStorage.clear()
      window.sessionStorage.clear()
    } catch {
      // ignore
    }
  })

  it('withholds regenerate, continue, edit, branch, rewind and feedback on a project chat started here', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setProjectSpaceRef(SPACE_REF)
      const stream = await askAndStop(controller, 'Skriv statusen')
      expect(stream.request.spaceRef).toBe(SPACE_REF)

      const [question, answer] = controller.state.turns
      expect(question?.id).toBeTruthy()
      expect(answer?.id).toBeTruthy()
      expect(controller.turnActionsAllowed()).toBe(false)
      const threadId = controller.state.threadId

      controller.regenerateLatest()
      controller.continueGeneration(answer!.id)
      await controller.editAndResubmit(question!.id, 'Ny tekst')
      controller.branchAt(answer!.id)
      await controller.rewindTo(answer!.id)
      expect(await controller.submitTurnFeedback(answer!.id, 'positive')).toBe(false)
      flush()
      await tick()

      // No second answer was requested, so no chat endpoint was called for any of them.
      expect(mocks.streams).toHaveLength(1)
      expect(mocks.submitFeedback).not.toHaveBeenCalled()
      expect(mocks.rewindChatThread).not.toHaveBeenCalled()
      expect(controller.state.threadId).toBe(threadId)
      expect(controller.state.turns).toHaveLength(2)
      expect(controller.state.turns[1]?.content).toBe('Status er grønn')
    })
  })

  it('withholds the same actions on a project thread opened from its server session', async () => {
    mocks.getChatThread.mockImplementation(async (threadId: string) => (
      threadId === THREAD
        ? { threadId, title: 'Prosjekt', preview: 'Prosjekt', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'chat' }
        : null
    ))
    mocks.getSpaceThreadTranscript.mockResolvedValue({
      turns: [
        { role: 'user', content: 'Hva er status?' },
        { role: 'assistant', content: 'Status er grønn.' },
      ],
    })
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(THREAD)
    await settle()

    expect(controller.state.turns).toHaveLength(2)
    expect(controller.turnActionsAllowed()).toBe(false)
    const [question, answer] = controller.state.turns
    controller.regenerateLatest()
    controller.continueGeneration(answer!.id)
    await controller.editAndResubmit(question!.id, 'Ny tekst')
    controller.branchAt(answer!.id)
    await controller.rewindTo(answer!.id)
    expect(await controller.submitTurnFeedback(answer!.id, 'negative')).toBe(false)
    await settle()

    expect(mocks.streamChat).not.toHaveBeenCalled()
    expect(mocks.submitFeedback).not.toHaveBeenCalled()
    expect(mocks.rewindChatThread).not.toHaveBeenCalled()
    expect(controller.state.threadId).toBe(THREAD)
    expect(controller.state.turns).toHaveLength(2)
  })

  /*
   * Reactivity of the two accessors (F-f06-16). `scopedThreadRefs` is a plain Map, so a Show memo over an accessor re-runs
   * only when a signal it read changes. The thread id lands in the store before the thread's lookup answers; what changes
   * after that is the settled mark in `spaceRefLoads`. Each test below records every reading an effect makes, as the
   * answer row and the hint under the header do, and asserts the reading made after the settle.
   *
   * Mutation-tested by hand on 2026-11-24 (each read removed, the file run, the read put back):
   * - `projectThreadOpen` without its `spaceRefLoads()` read: the second test fails. Its effect reads only `state.threadId`
   *   and the plain Map, runs once while the lookup is pending (false) and never again, so `[false, true]` is not seen.
   * - The predicate (`projectThreadBlocksTurnActions`) reading the mark untracked, together with the accessor's own read
   *   removed: the first test fails. Its effect stops at the pending reading (false) and the ordinary thread never gets its
   *   controls back. The predicate untracked with the accessor's read kept: green, so that read alone is enough.
   * - The accessor's own read removed alone: both tests stay green, correctly. For a thread that is neither scoped nor
   *   temporary the predicate itself reads the mark, so the accessor is still reactive; the accessor's read only adds the
   *   dependency on the scoped branch, where the predicate returns before it reads, and that branch never flips back to
   *   allowed while the thread id stays the same.
   */
  it('re-reads turnActionsAllowed once an ordinary thread\'s lookup settles, giving its answer row the controls back', async () => {
    const ORDINARY = 'thread-ordinary'
    let answerLookup: (session: null) => void = () => undefined
    const lookup = new Promise<null>((resolve) => {
      answerLookup = resolve
    })
    mocks.getChatThread.mockImplementation((threadId: string) => (threadId === ORDINARY ? lookup : Promise.resolve(null)))
    mocks.getThreadMessages.mockResolvedValue([
      { id: 'q-1', role: 'user', content: 'Skriv om laks', createdAt: STAMP },
      { id: 'a-1', role: 'assistant', content: 'Laks er godt.', createdAt: STAMP },
    ])
    const allowed: boolean[] = []
    const mounted: { dispose?: () => void } = {}
    createRoot((disposeRoot) => {
      mounted.dispose = disposeRoot
      const controller = useChatController()
      createEffect(() => controller.turnActionsAllowed(), (value) => {
        allowed.push(value)
      })
    })
    flush()
    unmount = () => mounted.dispose?.()

    setActiveChatThreadId(ORDINARY)
    await settle()
    // The thread is open and its lookup has not answered: its actions are withheld.
    expect(allowed).toContain(false)
    expect(allowed[allowed.length - 1]).toBe(false)

    answerLookup(null)
    await settle()
    // The lookup answered that this is no project thread and the load settled its Space ref: the effect read again.
    expect(allowed[allowed.length - 1]).toBe(true)
  })

  it('re-reads projectThreadOpen once a project thread opened from its server session settles, and keeps its actions withheld', async () => {
    let answerLookup: (session: unknown) => void = () => undefined
    const lookup = new Promise<unknown>((resolve) => {
      answerLookup = resolve
    })
    mocks.getChatThread.mockImplementation((threadId: string) => (threadId === THREAD ? lookup : Promise.resolve(null)))
    mocks.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hva er status?' }] })
    const open: boolean[] = []
    const allowed: boolean[] = []
    const mounted: { dispose?: () => void } = {}
    createRoot((disposeRoot) => {
      mounted.dispose = disposeRoot
      const controller = useChatController()
      createEffect(() => controller.projectThreadOpen(), (value) => {
        open.push(value)
      })
      createEffect(() => controller.turnActionsAllowed(), (value) => {
        allowed.push(value)
      })
    })
    flush()
    unmount = () => mounted.dispose?.()

    setActiveChatThreadId(THREAD)
    await settle()
    // The thread id is on screen; whether it is a project thread is not known yet, so no project hint shows.
    expect(open[open.length - 1]).toBe(false)

    answerLookup({ threadId: THREAD, title: 'Prosjekt', preview: 'Prosjekt', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'chat' })
    await settle()
    // The load recorded the Space ref and settled it: the hint's effect read again and the hint shows.
    expect(open).toContain(false)
    expect(open[open.length - 1]).toBe(true)
    // And the turn actions stay withheld, now because it is a project thread.
    expect(allowed[allowed.length - 1]).toBe(false)
  })

  it('leaves an ordinary thread as it was: its answer can be rated', async () => {
    await withController(async (controller) => {
      await tick()
      await askAndStop(controller, 'Skriv om laks')
      const answer = controller.state.turns[1]
      expect(controller.turnActionsAllowed()).toBe(true)

      await expect(controller.submitTurnFeedback(answer!.id, 'positive')).resolves.toBe(true)
      expect(mocks.submitFeedback).toHaveBeenCalledWith('req-1', 'positive', expect.anything())
    })
  })

  it('leaves an ordinary thread as it was: regenerate opens a new answer', async () => {
    await withController(async (controller) => {
      await tick()
      await askAndStop(controller, 'Skriv om laks')
      expect(controller.turnActionsAllowed()).toBe(true)

      controller.regenerateLatest()
      flush()
      await tick()
      expect(mocks.streams).toHaveLength(2)
      expect(mocks.streams[1]?.request.regenerated).toBe(true)
    })
  })

  it('leaves an ordinary thread as it was: a stopped answer can be continued', async () => {
    await withController(async (controller) => {
      await tick()
      await askAndStop(controller, 'Skriv om laks')
      const answer = controller.state.turns[1]

      controller.continueGeneration(answer!.id)
      flush()
      await tick()
      expect(mocks.streams).toHaveLength(2)
      expect(mocks.streams[1]?.request.content).toContain('Status er grønn')
    })
  })

  it('leaves an ordinary thread as it was: an edited question is sent again', async () => {
    await withController(async (controller) => {
      await tick()
      await askAndStop(controller, 'Skriv om laks')
      const question = controller.state.turns[0]

      // Not awaited: the edit resolves only when the resent answer ends, and this answer is still open.
      void controller.editAndResubmit(question!.id, 'Skriv om torsk')
      await tick(6)
      expect(mocks.streams).toHaveLength(2)
      expect(mocks.streams[1]?.request.editResubmit).toBe(true)
    })
  })

  it('with the switch off, a thread scoped to a Space keeps its actions', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'false')
    await withController(async (controller) => {
      await tick()
      controller.setProjectSpaceRef(SPACE_REF)
      await askAndStop(controller, 'Skriv statusen')
      const answer = controller.state.turns[1]
      expect(controller.turnActionsAllowed()).toBe(true)

      await expect(controller.submitTurnFeedback(answer!.id, 'negative')).resolves.toBe(true)
      expect(mocks.submitFeedback).toHaveBeenCalledTimes(1)

      controller.regenerateLatest()
      flush()
      await tick()
      expect(mocks.streams).toHaveLength(2)
      expect(mocks.streams[1]?.request.spaceRef).toBe(SPACE_REF)
    })
  })
})

/**
 * The fail-open window (F-f06-16): `loadThread` moves the open thread before its Space ref is known, so the turns on
 * screen are still those of the thread before it. Until the lookup has answered, the turn actions are withheld, and they
 * stay withheld when the lookup failed. An ordinary thread whose load has settled is unchanged.
 */
describe('WS18-S4 turn actions while the Space ref of the open thread is not known (F-f06-16)', () => {
  const ORDINARY = 'thread-ordinary'
  const PENDING = 'thread-pending'
  const ORDINARY_MESSAGES = [
    { id: 'q-1', role: 'user', content: 'Skriv om laks', createdAt: STAMP },
    { id: 'a-1', role: 'assistant', content: 'Laks er godt.', createdAt: STAMP },
  ]
  let unmount: () => void = () => undefined

  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    mocks.getChatThread.mockResolvedValue(null)
    mocks.getChatThreadTranscript.mockResolvedValue(null)
    mocks.getThreadMessages.mockResolvedValue(ORDINARY_MESSAGES)
    mocks.getSpaceThreadTranscript.mockResolvedValue(null)
  })

  afterEach(() => {
    unmount()
    unmount = () => undefined
    mocks.streams.length = 0
    vi.clearAllMocks()
    vi.unstubAllEnvs()
    __resetRetentionForTests()
    try {
      window.localStorage.clear()
      window.sessionStorage.clear()
    } catch {
      // ignore
    }
  })

  /** Every turn action of the open thread's answer row, called as a stale button would call it. Nothing may happen. */
  async function callEveryTurnAction(controller: Controller) {
    const [question, answer] = controller.state.turns
    const threadId = controller.state.threadId
    const shown = controller.state.turns.map((turn) => turn.id)
    mocks.saveChatThreadSnapshot.mockClear()

    controller.regenerateLatest()
    controller.continueGeneration(answer!.id)
    await controller.editAndResubmit(question!.id, 'Ny tekst')
    controller.branchAt(answer!.id)
    await controller.rewindTo(answer!.id)
    expect(await controller.submitTurnFeedback(answer!.id, 'positive')).toBe(false)
    await settle()

    expect(mocks.streamChat).not.toHaveBeenCalled()
    expect(mocks.submitFeedback).not.toHaveBeenCalled()
    expect(mocks.rewindChatThread).not.toHaveBeenCalled()
    expect(mocks.saveChatThreadSnapshot).not.toHaveBeenCalled()
    expect(controller.state.threadId).toBe(threadId)
    expect(controller.state.turns.map((turn) => turn.id)).toEqual(shown)
  }

  it('withholds every turn action while the lookup of the open thread has not answered, and keeps the turns on screen', async () => {
    mocks.getChatThread.mockImplementation(async (threadId: string) => (
      threadId === PENDING ? new Promise<never>(() => undefined) : null
    ))
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()
    expect(controller.state.turns).toHaveLength(2)
    expect(controller.turnActionsAllowed()).toBe(true)

    // The lookup for PENDING never answers: the thread is open, and the answers of the thread before it are still on screen.
    setActiveChatThreadId(PENDING)
    await settle()
    expect(controller.state.threadId).toBe(PENDING)
    expect(controller.state.turns.map((turn) => turn.id)).toEqual(['q-1', 'a-1'])
    expect(controller.turnActionsAllowed()).toBe(false)
    expect(controller.projectThreadOpen()).toBe(false)

    await callEveryTurnAction(controller)
  })

  it('withholds the same actions once the lookup of the open thread has failed', async () => {
    mocks.getChatThread.mockImplementation(async (threadId: string) => {
      if (threadId === PENDING) throw new Error('offline')
      return null
    })
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()

    setActiveChatThreadId(PENDING)
    await settle()
    expect(controller.state.threadId).toBe(PENDING)
    expect(controller.state.turns.map((turn) => turn.id)).toEqual(['q-1', 'a-1'])
    expect(controller.turnActionsAllowed()).toBe(false)
    expect(controller.projectThreadOpen()).toBe(false)

    await callEveryTurnAction(controller)
  })

  it('sends no connector-suggestion resend while the lookup of the open thread has not answered', async () => {
    mocks.getChatThread.mockImplementation(async (threadId: string) => (
      threadId === PENDING ? new Promise<never>(() => undefined) : null
    ))
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()
    setActiveChatThreadId(PENDING)
    await settle()
    expect(controller.state.turns.map((turn) => turn.id)).toEqual(['q-1', 'a-1'])

    const suggestion = { suggestionId: 'sug_01', provider: 'google' }
    expect(controller.acceptConnectorSuggestion(suggestion, 'conn_1')).toBe(false)
    expect(controller.pendingConnectIntent()).toBeNull()
    await settle()
    expect(mocks.streamChat).not.toHaveBeenCalled()
  })

  it('leaves an ordinary thread as it was once its load has settled: a connector suggestion resends the question', async () => {
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()

    expect(controller.acceptConnectorSuggestion({ suggestionId: 'sug_01', provider: 'google' }, 'conn_1')).toBe(true)
    await tick()
    expect(mocks.streams).toHaveLength(1)
    expect(mocks.streams[0]?.request.content).toBe('Skriv om laks')
  })

  it('leaves an ordinary thread as it was once its load has settled: its answer can be regenerated', async () => {
    mocks.getChatThreadTranscript.mockResolvedValue({
      turns: [{ role: 'user', content: 'Skriv om laks' }, { role: 'assistant', content: 'Laks er godt.' }],
    })
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()
    expect(controller.state.threadId).toBe(ORDINARY)
    expect(controller.turnActionsAllowed()).toBe(true)
    expect(controller.projectThreadOpen()).toBe(false)

    controller.regenerateLatest()
    flush()
    await tick()
    expect(mocks.streams).toHaveLength(1)
    expect(mocks.streams[0]?.request.regenerated).toBe(true)
  })

  it('with the switch off, a thread whose lookup fails keeps its actions', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'false')
    mocks.getChatThread.mockRejectedValue(new Error('offline'))
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController
    setActiveChatThreadId(ORDINARY)
    await settle()
    expect(controller.state.turns).toHaveLength(2)
    expect(controller.turnActionsAllowed()).toBe(true)
    expect(controller.projectThreadOpen()).toBe(false)

    controller.regenerateLatest()
    flush()
    await tick()
    expect(mocks.streams).toHaveLength(1)
  })

  it('shows the project hint only for a known project thread, not for an ordinary one that is still loading', async () => {
    mocks.getChatThread.mockImplementation(async (threadId: string) => {
      if (threadId === THREAD) {
        return { threadId, title: 'Prosjekt', preview: 'Prosjekt', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'chat' }
      }
      return threadId === PENDING ? new Promise<never>(() => undefined) : null
    })
    mocks.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hva er status?' }] })
    const { controller, unmount: unmountController } = mount()
    unmount = unmountController

    setActiveChatThreadId(ORDINARY)
    await settle()
    expect(controller.projectThreadOpen()).toBe(false)

    setActiveChatThreadId(PENDING)
    await settle()
    expect(controller.projectThreadOpen()).toBe(false)
    expect(controller.turnActionsAllowed()).toBe(false)

    setActiveChatThreadId(THREAD)
    await settle()
    expect(controller.projectThreadOpen()).toBe(true)
    expect(controller.turnActionsAllowed()).toBe(false)
  })
})
