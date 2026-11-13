import { afterEach, describe, expect, it, vi } from 'vitest'
import { getThreadMessages } from './chat-client'

// The thread's messages as the server names them (rel1). A turn carries the connection it was written under by name: Session
// Core keeps it on the question a send carried, and the gateway names it on the owner's own read while the customer-gateway
// switch is on. A turn that names none keeps the shape it always had, so a reopened platform thread reads exactly as before.

function reply(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200, headers: { 'Content-Type': 'application/json' } })
}

describe('getThreadMessages reads the connection each turn was written under', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads the customer-gateway and the subscription connection by their own keys', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        reply({
          thread_id: 'thread-1',
          messages: [
            { message_id: 'm1', role: 'user', content: 'Hei', model_connection_id: 'conn-gw-1' },
            { message_id: 'm2', role: 'assistant', content: 'Svar', model: 'gateway-a' },
            { message_id: 'm3', role: 'user', content: 'Hva ble sagt?', subscription_connection_id: 'conn-sub-1' },
          ],
        }),
      ),
    )
    const messages = await getThreadMessages('thread-1')
    expect(messages).toMatchObject([
      { id: 'm1', role: 'user', modelConnectionId: 'conn-gw-1' },
      { id: 'm2', role: 'assistant', model: 'gateway-a' },
      { id: 'm3', role: 'user', subscriptionConnectionId: 'conn-sub-1' },
    ])
    expect(messages[0]).not.toHaveProperty('subscriptionConnectionId')
    expect(messages[2]).not.toHaveProperty('modelConnectionId')
  })

  it('keeps no connection key for a platform turn, a blank id, or an id that is not a string', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        reply({
          thread_id: 'thread-1',
          messages: [
            { message_id: 'm1', role: 'user', content: 'Hei' },
            { message_id: 'm2', role: 'user', content: 'Hei', model_connection_id: '   ', subscription_connection_id: null },
          ],
        }),
      ),
    )
    const [platform, blank] = await getThreadMessages('thread-1')
    for (const message of [platform, blank]) {
      expect(message).not.toHaveProperty('modelConnectionId')
      expect(message).not.toHaveProperty('subscriptionConnectionId')
    }
  })
})
