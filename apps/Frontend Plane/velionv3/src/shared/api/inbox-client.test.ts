import { afterEach, describe, expect, it, vi } from 'vitest'
import { sendReply } from './inbox-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('sendReply', () => {
  it('includes the caller idempotency key in the JSON body forwarded for signing', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      id: 'message-1',
      conversation_id: 'conversation/1',
      direction: 'outbound',
      sender_type: 'agent',
      body_text: 'The replacement is submitted.',
      internal: false,
      occurred_at: '2026-07-13T14:00:00Z',
      created_at: '2026-07-13T14:00:00Z',
    }))
    vi.stubGlobal('fetch', fetchMock)

    await sendReply(
      'org-acme',
      'conversation/1',
      'The replacement is submitted.',
      false,
      'manual-reply-1234567890',
    )

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/conversations/conversation%2F1/messages')
    expect(init.method).toBe('POST')
    expect(new Headers(init.headers).get('x-velion-org-id')).toBe('org-acme')
    expect(JSON.parse(String(init.body))).toEqual({
      body_text: 'The replacement is submitted.',
      idempotency_key: 'manual-reply-1234567890',
      internal: false,
    })
  })
})

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
