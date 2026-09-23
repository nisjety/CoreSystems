// @vitest-environment jsdom

/**
 * F-12 (CHAT_PARITY_AUDIT_2026-09-15.md §3.5): an artifact created during a
 * stop-then-regenerate sequence sometimes never got a "Resultat" tab,
 * observed once and not reproduced on a plain regenerate. The audit's own
 * hypothesis was an id/version divergence in `onArtifact`'s target turn.
 *
 * Driving the real controller through that exact sequence (stop mid-stream,
 * then regenerate) found a different, concrete bug upstream of `onArtifact`:
 * `regenerateLatest` truncates `state.turns` and, in the SAME synchronous
 * tick, calls `sendContent`, which independently re-reads `state.turns` to
 * build its own next-turns array. That read is a plain untracked read (not
 * inside an effect/JSX), and a store write here is not guaranteed visible to
 * such a read until flushed — so without an explicit `flush()` between the
 * truncation and the read, the truncation had not landed yet: the stopped
 * turn survived and the new turn was appended AFTER it instead of replacing
 * it, leaving TWO assistant turns for one user turn. The fix is the `flush()`
 * now in `regenerateLatest` (and `editAndResubmit`'s identically-shaped
 * final-exchange path).
 *
 * This does not, on its own, explain a missing "Resultat" tab — `onArtifact`
 * still finds the new (second) assistant turn correctly by id, so
 * `artifactItems()` (`ChatPage.tsx`'s "Resultat" tab condition) still sees
 * the artifact even with the stray duplicate turn present — but a stray extra
 * assistant turn breaking the "one assistant turn per user turn" invariant
 * that `lastUserIndex`/`chat-versions.ts` rely on is a genuine, user-visible
 * defect in its own right (a ghost empty "stopped" bubble reappearing after
 * every stop-then-regenerate), directly downstream of the exact sequence
 * F-12 named, and the most concrete bug this investigation turned up in that
 * codepath. See the assertions below for what was reproduced and fixed.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockResumeStream } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((_request: unknown, handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ handlers, resolve })
    })
  })
  const mockResumeStream = vi.fn(async (_id: string, handlers: ChatStreamHandlers) => handlers.onError?.({ code: 'connection_error', message: 'Buffer expired' }))
  return { streams, mockStreamChat, mockResumeStream }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    resumeStream: mockResumeStream,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

function withController<T>(
  fn: (controller: ReturnType<typeof useChatController>) => Promise<T>,
): Promise<T> {
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

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockResumeStream.mockClear()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('Q08 connection recovery', () => {
  it('shows document review before a tool or artifact exists and permits stopping', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Draft from my attachment', tools: [] })
      await tick()
      const stream = streams[0]!
      stream.handlers.onConnected?.({ ok: true, requestId: 'req-review' })
      stream.handlers.onStep?.({ id: 'ordinary-step', title: 'Preparing', status: 'running' })
      await tick()
      expect(controller.activeTab()).toBe('chat')
      stream.handlers.onStep?.({ id: 'draft-1:source-check', title: 'Kontrollerer utkastet mot kildene', status: 'running' })
      await tick()
      expect(controller.activeTab()).toBe('steps')
      expect(controller.state.turns.at(-1)?.artifacts ?? []).toHaveLength(0)
      expect(controller.state.turns.at(-1)?.toolCalls ?? []).toHaveLength(0)
      expect(controller.state.taskSteps.some(step => step.id.endsWith('draft-1:source-check') && step.status === 'active')).toBe(true)
      controller.handleStop()
      await tick()
      expect(controller.state.turns.at(-1)?.status).toBe('stopped')
      expect(controller.state.taskSteps.filter(step => step.status === 'active')).toHaveLength(0)
      stream.resolve()
      await tick()
    })
  })

  it('does not let a late replay terminal event settle a replacement generation', async () => {
    let resumeHandlers: ChatStreamHandlers | undefined
    let finishResume: (() => void) | undefined
    mockResumeStream.mockImplementationOnce((_id, handlers) => new Promise<void>(resolve => { resumeHandlers = handlers; finishResume = resolve }))
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Read this', tools: [] })
      await tick()
      streams[0]!.handlers.onConnected?.({ ok: true, requestId: 'req-old-replay' })
      streams[0]!.handlers.onMessage?.({ content: 'Partial' })
      await tick()
      streams[0]!.resolve()
      await tick(8)
      expect(resumeHandlers).toBeDefined()
      controller.handleStop()
      await tick()
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(2)
      resumeHandlers!.onDone?.({})
      finishResume!()
      await tick(8)
      expect(controller.state.status).toBe('streaming')
      expect(controller.state.turns.at(-1)?.status).toBe('waiting')
      streams[1]!.handlers.onDone?.({})
      streams[1]!.resolve()
      await tick()
    })
  })

  it('keeps an incomplete answer stopped when its replay buffer is unavailable', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Read this', tools: [] })
      await tick()
      streams[0]!.handlers.onConnected?.({ ok: true, requestId: 'req-interrupted' })
      streams[0]!.handlers.onMessage?.({ content: 'A partial answer' })
      streams[0]!.handlers.onFrameId?.('12')
      await tick()
      streams[0]!.resolve()
      await tick(8)
      expect(mockResumeStream).toHaveBeenCalledWith('req-interrupted', expect.any(Object), expect.any(AbortSignal), '12')
      expect(mockStreamChat).toHaveBeenCalledTimes(1)
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'A partial answer' })
    })
  })

  it('preserves server-stopped status after a replay, instead of treating it as done', async () => {
    mockResumeStream.mockImplementationOnce(async (_id, handlers) => handlers.onStopped?.({ reason: 'cancelled' }))
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Read this', tools: [] })
      await tick()
      streams[0]!.handlers.onConnected?.({ ok: true, requestId: 'req-stopped' })
      streams[0]!.handlers.onMessage?.({ content: 'Partial' })
      await tick()
      streams[0]!.resolve()
      await tick(8)
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'Partial' })
    })
  })

  it('does not repeat an invocation after a tool failure', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Read this', tools: [], model: 'verevon-balance' })
      await tick()
      streams[0]!.handlers.onMessage?.({ content: 'Partial' })
      streams[0]!.handlers.onError?.({ code: 'tool_execution_failed', message: 'Unavailable' })
      streams[0]!.resolve()
      await tick(8)
      expect(mockStreamChat).toHaveBeenCalledTimes(1)
      expect(mockResumeStream).not.toHaveBeenCalled()
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', content: 'Partial' })
    })
  })
})

describe('F-12: artifact on a regenerate-after-stop turn', () => {
  it('stays reachable in artifactItems() (the "Resultat" tab condition) after stop → regenerate → artifact', async () => {
    await withController(async (controller) => {
      await tick()

      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv en lang tekst om norsk fiskeoppdrett',
        tools: [],
      })
      await tick()

      // Preconditions matching §3.3/§3.4: one stream opened, an assistant
      // turn appended, still waiting (no content yet — stop lands "in the
      // thinking phase").
      expect(streams).toHaveLength(1)
      expect(controller.state.turns).toHaveLength(2)
      const firstAssistantId = controller.state.turns[1]?.id
      expect(controller.state.turns[1]?.status).toBe('waiting')

      // Stop mid-answer.
      controller.handleStop()
      flush()
      expect(controller.state.turns[1]?.status).toBe('stopped')

      // The aborted fetch's read loop settles asynchronously and without an
      // error (readSseStream swallows AbortError) — simulate that by
      // resolving the first stream's promise only now, AFTER the stop.
      streams[0]?.resolve()
      await tick()

      // Regenerate the stopped turn.
      controller.regenerateLatest()
      flush()

      // The truncation must have landed by the time `sendContent` (called
      // synchronously, in the same tick, by `regenerateLatest`) rebuilds
      // `state.turns` — this is the exact race the bug above reproduced.
      // Checked immediately, with no further ticks, on purpose.
      expect(controller.state.turns).toHaveLength(2)

      await tick()
      expect(streams).toHaveLength(2)
      const second = streams[1]
      expect(second).toBeTruthy()

      // The stopped turn must be gone from the live turns (truncated by
      // regenerateLatest) and replaced by a fresh assistant turn with a
      // DIFFERENT id — this is the id-divergence the audit suspected.
      expect(controller.state.turns).toHaveLength(2)
      const secondAssistantId = controller.state.turns[1]?.id
      expect(secondAssistantId).toBeTruthy()
      expect(secondAssistantId).not.toBe(firstAssistantId)

      // The regenerate stream produces the document artifact, exactly as in
      // §3.4 ("Historien til norsk fiskeoppdrett").
      second!.handlers.onArtifact?.({
        id: 'art-fiskeoppdrett',
        kind: 'document',
        title: 'Historien til norsk fiskeoppdrett',
        content: '# Historien til norsk fiskeoppdrett\n\n...',
        version: 1,
      })
      flush()

      // Ground truth for whether ChatPage would offer a "Resultat" tab.
      const items = controller.artifactItems()
      expect(items).toHaveLength(1)
      expect(items[0]?.artifact.id).toBe('art-fiskeoppdrett')
      // And it must be attached to the CURRENT (second) assistant turn, not
      // to the stopped one that no longer exists in `state.turns`.
      expect(items[0]?.turn.id).toBe(secondAssistantId)

      second!.handlers.onDone?.({
        requestId: 'req-2',
        modelUsed: 'verevon-balance',
        outputTokens: 500,
        stopReason: 'stop',
      })
      second!.resolve()
      await tick()

      // Still reachable after the stream settles.
      expect(controller.artifactItems()).toHaveLength(1)
    })
  })
})

describe('F-12 continued: a manual stop must not be reverted by the late-resolving stream', () => {
  it('keeps status "stopped" (and the "Stoppet" chip it drives) after the aborted fetch settles', async () => {
    // Root cause (found via a live browser repro, not simulated): `sendContent`
    // awaits `streamChat(...)`, and `readSseStream` swallows `AbortError`
    // rather than rejecting — so aborting a stream makes that `await` resolve
    // NORMALLY, just later than `handleStop`'s own synchronous write. Before
    // the fix, the `if (!settled)` branch that runs on that normal resolution
    // called `stopStreaming(undefined)` UNCONDITIONALLY, silently reverting
    // the turn's status from 'stopped' back to `undefined` moments after the
    // user stopped it — which is exactly what a live Stop click showed: no
    // "Stoppet" chip (chat-nodes/derive.ts:52 reads `turn.status === 'stopped'`
    // for that chip, so a reverted status renders nothing).
    await withController(async (controller) => {
      await tick()

      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv en lang og detaljert tekst om norsk fiskeoppdrett',
        tools: [],
      })
      await tick()

      expect(streams).toHaveLength(1)
      const stream = streams[0]!

      // Some partial text arrived before the stop — this is the "has partial
      // content" precondition `continueGeneration`'s own test exercises too,
      // and it is what makes the reverted-status bug visible: an empty
      // pending turn is indistinguishable from one whose status silently
      // became `undefined`.
      stream.handlers.onMessage?.({ content: 'Norsk fiskeoppdrett startet på 1970-tallet', requestId: 'req-1' })
      flush()
      expect(controller.state.turns[1]?.status).toBe('waiting')
      expect(controller.state.turns[1]?.content).toBe('Norsk fiskeoppdrett startet på 1970-tallet')

      controller.handleStop()
      flush()
      expect(controller.state.turns[1]?.status).toBe('stopped')

      // The aborted fetch's read loop settles asynchronously, same race as
      // the sibling describe block above — resolve it only now, AFTER stop.
      stream.resolve()
      await tick()

      // Must still read 'stopped': this is the assertion that failed before
      // the fix (the turn's status came back `undefined` here).
      expect(controller.state.turns[1]?.status).toBe('stopped')
      // The partial text must survive too — nothing about settling the stale
      // stream may touch content.
      expect(controller.state.turns[1]?.content).toBe('Norsk fiskeoppdrett startet på 1970-tallet')
    })
  })
})
