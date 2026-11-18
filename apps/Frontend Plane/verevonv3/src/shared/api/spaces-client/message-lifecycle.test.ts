import { afterEach, describe, expect, it, vi } from 'vitest'

import { deleteSpaceMessage, editSpaceMessage } from './message-lifecycle'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init?.body)) as Record<string, unknown>

describe('editSpaceMessage (WS12-S7)', () => {
  it('PATCHes the encoded message path with the content and the idempotency key', async () => {
    const calls = stubFetch(200, {
      data: {
        message_id: 'msg/1',
        revision_state: 'edited',
        revision_no: 2,
        revised_at: '2026-11-02T10:00:00Z',
        deduplicated: false,
      },
    })
    const receipt = await editSpaceMessage('space 1', 'thr/1', 'msg/1', {
      content: 'fixed',
      idempotencyKey: 'edit-key-0000000001',
    })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/threads/thr%2F1/messages/msg%2F1')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PATCH')
    expect(sentBody(calls[0])).toEqual({ content: 'fixed', idempotency_key: 'edit-key-0000000001' })
    expect(receipt).toEqual({
      messageId: 'msg/1',
      revisionState: 'edited',
      revisionNo: 2,
      revisedAt: '2026-11-02T10:00:00Z',
      deduplicated: false,
    })
  })
})

describe('deleteSpaceMessage (WS12-S7)', () => {
  it('DELETEs the message path with the Idempotency-Key header and no body', async () => {
    const calls = stubFetch(200, {
      data: { message_id: 'msg/1', revision_state: 'deleted', revision_no: 3, deduplicated: true },
    })
    const receipt = await deleteSpaceMessage('space 1', 'thr/1', 'msg/1', { idempotencyKey: 'del-key-0000000001' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/threads/thr%2F1/messages/msg%2F1')).toBe(true)
    expect(calls[0]?.init?.method).toBe('DELETE')
    expect(new Headers(calls[0]?.init?.headers).get('Idempotency-Key')).toBe('del-key-0000000001')
    expect(calls[0]?.init?.body).toBeUndefined()
    expect(receipt).toEqual({ messageId: 'msg/1', revisionState: 'deleted', revisionNo: 3, deduplicated: true })
  })

  it('adds the moderation reason as a query parameter only when one is given', async () => {
    const calls = stubFetch(200, { data: { message_id: 'msg_1', revision_state: 'deleted', revision_no: 4 } })
    await deleteSpaceMessage('room', 'thr_1', 'msg_1', {
      idempotencyKey: 'del-key-0000000002',
      moderationReason: 'spam',
    })
    expect(calls[0]?.url.endsWith('/messages/msg_1?moderation_reason=spam')).toBe(true)
    await deleteSpaceMessage('room', 'thr_1', 'msg_1', { idempotencyKey: 'del-key-0000000003' })
    expect(calls[1]?.url.includes('?')).toBe(false)
  })
})

describe('message revision receipts (WS12-S7)', () => {
  it('throws when the receipt names no message or an unknown revision state', async () => {
    stubFetch(200, { data: { revision_state: 'edited', revision_no: 2 } })
    await expect(
      editSpaceMessage('room', 'thr_1', 'msg_1', { content: 'x', idempotencyKey: 'edit-key-0000000004' }),
    ).rejects.toThrow('message_revision_unreadable')
    stubFetch(200, { data: { message_id: 'msg_1', revision_state: 'hidden', revision_no: 2 } })
    await expect(
      deleteSpaceMessage('room', 'thr_1', 'msg_1', { idempotencyKey: 'del-key-0000000004' }),
    ).rejects.toThrow('message_revision_unreadable')
  })
})
