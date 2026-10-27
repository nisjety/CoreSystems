import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const createSpacePost = vi.fn()
const replyInSpaceThread = vi.fn()

vi.mock('@/shared/api/spaces-client', () => ({
  createSpacePost: (...args: unknown[]) => createSpacePost(...args),
  replyInSpaceThread: (...args: unknown[]) => replyInSpaceThread(...args),
  updateSpaceThreadPresentation: vi.fn(),
}))

import { spaceConnectivity, noteSendTransportOk } from './space-connectivity'
import { createSpacePostSender, type SpacePostSender, type SpacePostSenderOptions } from './space-post-sender'
import { createSpaceSendJournal, type SpaceSendJournal } from './space-send-journal'

const receipt = { threadId: 'thr_new', messageId: 'msg_1', sequence: 1, deduplicated: false }

function apiError(status: number, code: string | null): ApiError {
  return new ApiError(code ?? 'failed', status, code)
}

type Harness = {
  journal: SpaceSendJournal
  sender: SpacePostSender
  onAccepted: ReturnType<typeof vi.fn>
  onZdrRefused: ReturnType<typeof vi.fn>
  online: { value: boolean }
}

// Solid 2 batches signal writes until the next flush, and the sender reads the journal straight
// after writing it. The logic tests below flush after every journal write so they test the
// sender's decisions; the one test of the unflushed path (DEPTH-SPA-1) runs on the plain journal.
function flushingJournal(): SpaceSendJournal {
  const inner = createSpaceSendJournal()
  const settled = <A extends unknown[], R>(fn: (...args: A) => R) => (...args: A): R => {
    const result = fn(...args)
    flush()
    return result
  }
  return {
    ...inner,
    begin: settled(inner.begin),
    update: settled(inner.update),
    remove: settled(inner.remove),
    reconcile: settled(inner.reconcile),
  }
}

function harness(overrides: Partial<SpacePostSenderOptions> = {}): Harness {
  const journal = flushingJournal()
  const online = { value: true }
  const onAccepted = vi.fn()
  const onZdrRefused = vi.fn()
  const sender = createSpacePostSender({
    journal,
    spaceRef: () => 'room',
    describeError: (error) => (error instanceof ApiError ? `described:${error.code}` : 'described:network'),
    isOnline: () => online.value,
    onAccepted,
    onZdrRefused,
    ...overrides,
  })
  return { journal, sender, onAccepted, onZdrRefused, online }
}

/** Send and let Solid settle its batched writes, as the app's own event loop does. */
async function send(h: Harness, input: Parameters<SpacePostSender['send']>[0]) {
  await h.sender.send(input)
  flush()
}

beforeEach(() => {
  createSpacePost.mockReset()
  replyInSpaceThread.mockReset()
  noteSendTransportOk()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('space post sender (WS01-S6)', () => {
  // DEPTH-SPA-1 (fixed): the sender hands `attempt` the entry `begin` returned instead of reading
  // the journal back, which Solid 2 would answer with the not-yet-flushed list.
  it('DEPTH-SPA-1: a send reaches the transport without an explicit flush', async () => {
    createSpacePost.mockResolvedValue(receipt)
    const journal = createSpaceSendJournal()
    const sender = createSpacePostSender({
      journal,
      spaceRef: () => 'room',
      describeError: () => 'x',
      isOnline: () => true,
    })
    await sender.send({ content: 'hello' })
    expect(createSpacePost).toHaveBeenCalledTimes(1)
  })

  describe('send to accepted', () => {
    it('creates a new post with clientSendId = the journal sendId and calls onAccepted', async () => {
      createSpacePost.mockResolvedValue(receipt)
      const h = harness()
      await send(h, { content: '  hello  ' })
      expect(createSpacePost).toHaveBeenCalledTimes(1)
      expect(replyInSpaceThread).not.toHaveBeenCalled()
      const [spaceRef, input] = createSpacePost.mock.calls[0] as [string, { content: string; clientSendId: string }]
      expect(spaceRef).toBe('room')
      expect(input.content).toBe('hello')
      const [entry] = h.journal.entries()
      expect(entry?.sendId).toBe(input.clientSendId)
      expect(entry).toMatchObject({ kind: 'post_new', stage: 'accepted', threadId: 'thr_new' })
      expect(h.onAccepted).toHaveBeenCalledTimes(1)
      expect(h.onAccepted.mock.calls[0]?.[1]).toBe(receipt)
      // An accepted post is no longer this composer's unsettled post.
      expect(h.sender.current()).toBeUndefined()
      expect(h.sender.busy()).toBe(false)
    })

    it('replies in a thread through replyInSpaceThread and records the baseline', async () => {
      replyInSpaceThread.mockResolvedValue({ ...receipt, threadId: 'thr_9' })
      const h = harness({ baselineUpdatedAt: (threadId) => (threadId === 'thr_9' ? '2026-10-27T10:00:00Z' : undefined) })
      await send(h, { content: 'reply', replyThreadId: 'thr_9' })
      expect(createSpacePost).not.toHaveBeenCalled()
      expect(replyInSpaceThread).toHaveBeenCalledTimes(1)
      const [spaceRef, threadId, input] = replyInSpaceThread.mock.calls[0] as [string, string, { clientSendId: string }]
      expect([spaceRef, threadId]).toEqual(['room', 'thr_9'])
      expect(h.journal.entries()[0]).toMatchObject({
        kind: 'post_reply',
        threadId: 'thr_9',
        baselineUpdatedAt: '2026-10-27T10:00:00Z',
        sendId: input.clientSendId,
      })
    })

    it('ignores a blank post', async () => {
      const h = harness()
      await send(h, { content: '   ' })
      expect(createSpacePost).not.toHaveBeenCalled()
      expect(h.journal.entries()).toEqual([])
    })

    it('drops the accepted entry after its time-to-live if the room never shows it', async () => {
      vi.useFakeTimers()
      createSpacePost.mockResolvedValue(receipt)
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.journal.entries()).toHaveLength(1)
      vi.advanceTimersByTime(31_000)
      flush()
      expect(h.journal.entries()).toEqual([])
    })
  })

  describe('failures', () => {
    it('marks a 503 retryable', async () => {
      createSpacePost.mockRejectedValue(apiError(503, 'service_unavailable'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.sender.current()).toMatchObject({
        stage: 'failed',
        failure: { code: 'service_unavailable', retryable: true, message: 'described:service_unavailable' },
      })
      expect(h.onAccepted).not.toHaveBeenCalled()
    })

    it('marks session_unavailable retryable even on a 4xx', async () => {
      createSpacePost.mockRejectedValue(apiError(401, 'session_unavailable'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.sender.current()?.failure).toMatchObject({ code: 'session_unavailable', retryable: true })
    })

    it('marks 403 space_post_not_authorized not retryable', async () => {
      createSpacePost.mockRejectedValue(apiError(403, 'space_post_not_authorized'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.sender.current()?.failure).toMatchObject({ code: 'space_post_not_authorized', retryable: false })
      // Retry on a non-retryable failure sends nothing.
      await h.sender.retry()
      expect(createSpacePost).toHaveBeenCalledTimes(1)
    })

    it('names an error with no code by its status', async () => {
      createSpacePost.mockRejectedValue(apiError(404, null))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.sender.current()?.failure).toMatchObject({ code: 'http_404', retryable: false })
    })

    it('treats a network error as retryable, code network, and marks the room unreachable', async () => {
      createSpacePost.mockRejectedValue(new TypeError('Failed to fetch'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.sender.current()?.failure).toMatchObject({ code: 'network', retryable: true })
      expect(spaceConnectivity()).toBe('unreachable')
    })

    it('calls onZdrRefused for the typed zero-retention refusal', async () => {
      createSpacePost.mockRejectedValue(new ApiError('refused', 412, 'zdr_durable_mutation_forbidden'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.onZdrRefused).toHaveBeenCalledTimes(1)
      expect(h.sender.current()?.failure?.code).toBe('zdr_durable_mutation_forbidden')
    })

    it('does not call onZdrRefused for any other refusal', async () => {
      createSpacePost.mockRejectedValue(apiError(403, 'space_post_not_authorized'))
      const h = harness()
      await send(h, { content: 'hello' })
      expect(h.onZdrRefused).not.toHaveBeenCalled()
    })
  })

  describe('retry and the send id', () => {
    it('retry reuses the SAME sendId', async () => {
      createSpacePost.mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValueOnce(receipt)
      const h = harness()
      await send(h, { content: 'hello' })
      const firstId = h.sender.current()?.sendId
      expect(firstId).toBeTruthy()
      await h.sender.retry()
      flush()
      expect(createSpacePost).toHaveBeenCalledTimes(2)
      expect((createSpacePost.mock.calls[1]?.[1] as { clientSendId: string }).clientSendId).toBe(firstId)
      expect(h.journal.entries()).toHaveLength(1)
      expect(h.journal.entries()[0]).toMatchObject({ sendId: firstId, stage: 'accepted' })
    })

    it('the same text after a failure reuses the id', async () => {
      createSpacePost.mockRejectedValueOnce(apiError(503, 'x')).mockResolvedValueOnce(receipt)
      const h = harness()
      await send(h, { content: 'hello' })
      const firstId = h.sender.current()?.sendId
      await send(h, { content: 'hello' })
      expect((createSpacePost.mock.calls[1]?.[1] as { clientSendId: string }).clientSendId).toBe(firstId)
    })

    it('different text removes the stale entry and mints a new id', async () => {
      createSpacePost.mockRejectedValueOnce(apiError(503, 'x')).mockResolvedValueOnce(receipt)
      const h = harness()
      await send(h, { content: 'hello' })
      const firstId = h.sender.current()?.sendId
      await send(h, { content: 'something else' })
      const secondId = (createSpacePost.mock.calls[1]?.[1] as { clientSendId: string }).clientSendId
      expect(secondId).not.toBe(firstId)
      expect(h.journal.get(firstId as string)).toBeUndefined()
    })

    it('the same text to a different thread is a different send', async () => {
      replyInSpaceThread.mockRejectedValueOnce(apiError(503, 'x')).mockResolvedValueOnce(receipt)
      const h = harness()
      await send(h, { content: 'hello', replyThreadId: 'thr_1' })
      const firstId = h.sender.current()?.sendId
      await send(h, { content: 'hello', replyThreadId: 'thr_2' })
      expect((replyInSpaceThread.mock.calls[1]?.[2] as { clientSendId: string }).clientSendId).not.toBe(firstId)
    })

    it('discard drops the unsettled post and sends nothing', async () => {
      createSpacePost.mockRejectedValue(apiError(503, 'x'))
      const h = harness()
      await send(h, { content: 'hello' })
      h.sender.discard()
      flush()
      expect(h.sender.current()).toBeUndefined()
      expect(h.journal.entries()).toEqual([])
      expect(createSpacePost).toHaveBeenCalledTimes(1)
    })
  })

  describe('offline and busy', () => {
    it('queues while offline without a request, then sends once on the online event', async () => {
      createSpacePost.mockResolvedValue(receipt)
      const h = harness()
      h.online.value = false
      await send(h, { content: 'hello' })
      expect(createSpacePost).not.toHaveBeenCalled()
      expect(h.sender.current()?.stage).toBe('queued')
      expect(h.sender.busy()).toBe(true)

      h.online.value = true
      window.dispatchEvent(new Event('online'))
      await vi.waitFor(() => expect(createSpacePost).toHaveBeenCalledTimes(1))
      flush()
      // A second online event finds nothing queued and sends nothing more.
      window.dispatchEvent(new Event('online'))
      await Promise.resolve()
      expect(createSpacePost).toHaveBeenCalledTimes(1)
      expect(h.journal.entries()[0]?.stage).toBe('accepted')
      expect(h.onAccepted).toHaveBeenCalledTimes(1)
    })

    it('blocks a second send while one is queued or sending', async () => {
      let resolveFirst: (value: typeof receipt) => void = () => undefined
      createSpacePost.mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve }))
      const h = harness()
      const first = h.sender.send({ content: 'one' })
      flush()
      expect(h.sender.busy()).toBe(true)
      await h.sender.send({ content: 'two' })
      expect(createSpacePost).toHaveBeenCalledTimes(1)
      resolveFirst(receipt)
      await first
      flush()
      expect(h.sender.busy()).toBe(false)
    })

    it('stops listening for the online event after dispose', async () => {
      createSpacePost.mockResolvedValue(receipt)
      const h = harness()
      h.online.value = false
      await send(h, { content: 'hello' })
      h.sender.dispose()
      h.online.value = true
      window.dispatchEvent(new Event('online'))
      await Promise.resolve()
      expect(createSpacePost).not.toHaveBeenCalled()
    })
  })
})
