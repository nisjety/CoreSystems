import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getAttention, markThreadSeen } from './attention-client'
import { requestJson } from './http'

vi.mock('./http', () => ({ requestJson: vi.fn() }))

const requestJsonMock = vi.mocked(requestJson)

describe('attention client', () => {
  beforeEach(() => {
    requestJsonMock.mockReset()
  })

  it('reads the listing as a camel-case snapshot with the closed state and the badge counts', async () => {
    requestJsonMock.mockResolvedValue({
      enabled: true,
      items: [
        { threadId: 'thread-1', state: 'needs_approval', reason: 'approval', since: '2026-10-30T09:00:00.000Z', unseen: true, runId: 'run-1' },
        { threadId: 'thread-2', state: 'working', unseen: false },
      ],
      counts: { needsYou: 1, working: 1, unseen: 0 },
      capabilities: { stream: false, push: false },
    })

    const snapshot = await getAttention()

    expect(requestJsonMock).toHaveBeenCalledWith('/api/v1/attention')
    expect(snapshot).toEqual({
      enabled: true,
      items: [
        { threadId: 'thread-1', state: 'needs_approval', reason: 'approval', since: '2026-10-30T09:00:00.000Z', unseen: true, runId: 'run-1' },
        { threadId: 'thread-2', state: 'working', unseen: false },
      ],
      counts: { needsYou: 1, working: 1, unseen: 0 },
      capabilities: { stream: false, push: false },
    })
  })

  it('drops a state outside the closed set and an item with no thread, and keeps the rest of the item', async () => {
    requestJsonMock.mockResolvedValue({
      enabled: true,
      items: [
        { threadId: 'thread-3', state: 'awaiting_approval', reason: 'nonsense', unseen: 'yes' },
        { state: 'failed', unseen: true },
        'not an item',
      ],
      counts: { needsYou: 2.7, working: -4, unseen: 'many' },
    })

    const snapshot = await getAttention()

    expect(snapshot.items).toEqual([{ threadId: 'thread-3', unseen: false }])
    expect(snapshot.counts).toEqual({ needsYou: 2, working: 0, unseen: 0 })
    expect(snapshot.capabilities).toEqual({ stream: false, push: false })
  })

  it('reads anything it cannot recognise as a disabled, empty listing rather than throwing', async () => {
    requestJsonMock.mockResolvedValue(['unexpected'])

    expect(await getAttention()).toEqual({
      enabled: false,
      items: [],
      counts: { needsYou: 0, working: 0, unseen: 0 },
      capabilities: { stream: false, push: false },
    })
  })

  it('marks one thread read by its id and sends nothing else', async () => {
    requestJsonMock.mockResolvedValue({ threadId: 'thread-1' })

    await markThreadSeen('thread-1')

    expect(requestJsonMock).toHaveBeenCalledWith('/api/v1/attention/seen', {
      method: 'POST',
      body: JSON.stringify({ threadId: 'thread-1' }),
    })
  })

  it('lets a refused mark-read reject, so the caller can keep the thread unseen', async () => {
    requestJsonMock.mockRejectedValue(new Error('refused'))

    await expect(markThreadSeen('thread-1')).rejects.toThrow('refused')
  })
})
