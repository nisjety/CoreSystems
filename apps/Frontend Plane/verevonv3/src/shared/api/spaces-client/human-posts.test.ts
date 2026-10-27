import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '../http'
import { createSpacePost, replyInSpaceThread, SPACE_POST_MAX_CHARS_DEFAULT } from './human-posts'

type FetchCall = { url: string; init: RequestInit }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url: String(url), init })
    return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
  }))
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('createSpacePost (WS01-S6)', () => {
  it('POSTs content and client_send_id to the room threads path', async () => {
    const calls = stubFetch(201, {
      data: { space_ref: 'room', thread_id: 'thr_1', message_id: 'msg_1', sequence: 3, deduplicated: false },
    })
    const receipt = await createSpacePost('room', { content: 'hello', clientSendId: 'a'.repeat(32) })
    expect(calls).toHaveLength(1)
    expect(calls[0].url.endsWith('/api/v1/spaces/room/threads')).toBe(true)
    expect(calls[0].init.method).toBe('POST')
    expect(JSON.parse(String(calls[0].init.body))).toEqual({ content: 'hello', client_send_id: 'a'.repeat(32) })
    expect(receipt).toEqual({ threadId: 'thr_1', messageId: 'msg_1', sequence: 3, deduplicated: false })
  })

  it('encodes the space ref in the path', async () => {
    const calls = stubFetch(201, { data: { thread_id: 'thr_1', message_id: 'm', sequence: 1 } })
    await createSpacePost('a b/c', { content: 'x', clientSendId: 'b'.repeat(32) })
    expect(calls[0].url.endsWith('/api/v1/spaces/a%20b%2Fc/threads')).toBe(true)
  })

  it('maps a 200 deduplicated:true receipt to deduplicated', async () => {
    stubFetch(200, { data: { thread_id: 'thr_1', message_id: 'msg_1', sequence: 3, deduplicated: true } })
    const receipt = await createSpacePost('room', { content: 'hello', clientSendId: 'c'.repeat(32) })
    expect(receipt.deduplicated).toBe(true)
  })

  it('throws when the receipt names no thread', async () => {
    stubFetch(201, { data: { message_id: 'msg_1', sequence: 1 } })
    await expect(createSpacePost('room', { content: 'hello', clientSendId: 'd'.repeat(32) })).rejects.toThrow(
      'post_receipt_missing_thread',
    )
  })

  it('tolerates a missing message id and sequence', async () => {
    stubFetch(201, { data: { thread_id: 'thr_1' } })
    await expect(createSpacePost('room', { content: 'hello', clientSendId: 'e'.repeat(32) })).resolves.toEqual({
      threadId: 'thr_1',
      messageId: '',
      sequence: 0,
      deduplicated: false,
    })
  })

  it('surfaces the typed error envelope as an ApiError with its details', async () => {
    stubFetch(403, {
      error: { code: 'space_post_not_authorized', message: 'no', details: { reason: 'role_cannot_post' } },
    })
    const error = await createSpacePost('room', { content: 'hello', clientSendId: 'f'.repeat(32) }).catch(
      (caught: unknown) => caught,
    )
    expect(error).toBeInstanceOf(ApiError)
    expect((error as ApiError).status).toBe(403)
    expect((error as ApiError).code).toBe('space_post_not_authorized')
    expect((error as ApiError).details).toEqual({ reason: 'role_cannot_post' })
  })
})

describe('replyInSpaceThread (WS01-S6)', () => {
  it('POSTs to the thread messages path with the same body shape', async () => {
    const calls = stubFetch(201, { data: { thread_id: 'thr_9', message_id: 'msg_2', sequence: 4 } })
    const receipt = await replyInSpaceThread('room', 'thr_9', { content: 'hi', clientSendId: 'g'.repeat(32) })
    expect(calls[0].url.endsWith('/api/v1/spaces/room/threads/thr_9/messages')).toBe(true)
    expect(JSON.parse(String(calls[0].init.body))).toEqual({ content: 'hi', client_send_id: 'g'.repeat(32) })
    expect(receipt.threadId).toBe('thr_9')
  })

  it('falls back to the given thread id when the receipt omits it', async () => {
    stubFetch(201, { data: { message_id: 'msg_2', sequence: 4 } })
    const receipt = await replyInSpaceThread('room', 'thr_9', { content: 'hi', clientSendId: 'h'.repeat(32) })
    expect(receipt.threadId).toBe('thr_9')
  })

  it('encodes the thread id in the path', async () => {
    const calls = stubFetch(201, { data: { message_id: 'm' } })
    await replyInSpaceThread('room', 'thr/9 x', { content: 'hi', clientSendId: 'i'.repeat(32) })
    expect(calls[0].url.endsWith('/threads/thr%2F9%20x/messages')).toBe(true)
  })
})

describe('SPACE_POST_MAX_CHARS_DEFAULT', () => {
  it('is the gateway default bound', () => {
    expect(SPACE_POST_MAX_CHARS_DEFAULT).toBe(8000)
  })
})
