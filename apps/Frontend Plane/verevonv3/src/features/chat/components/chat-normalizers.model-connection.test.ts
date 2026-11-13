import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatTurn } from './chat-types'

// A customer-gateway turn keeps its connection across a reload (W4-BYO-6). The local transcript is one way a reopened thread
// learns it is a connection thread, and the side question names its model from that (SideChatPanel). The server names the
// connection too, on the question a send carried (rel1): a reopened thread without its local transcript keeps the connection
// its regenerate, edit, continue and retry replay on. Both are kept with the customer-gateway switch (VITE_BYO_GATEWAY) or the
// side chat (VITE_CHAT_SIDE_CHAT) on (V3-spa-1-rel2). With both off, the stored transcript and the reopened turns are what they
// were before, and no regenerate or edit request changes. A subscription connection is kept with the side chat only.

function turn(overrides: Partial<ChatTurn> & Pick<ChatTurn, 'id' | 'role'>): ChatTurn {
  return {
    content: 'Svar',
    createdAt: '2026-11-13T10:00:00.000Z',
    streaming: false,
    tools: [],
    attachments: [],
    ...overrides,
  }
}

const GATEWAY_ANSWER = turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' })

// The server names the connection a send was written under on its question (Session Core keeps no connection on the answer).
// A reopened thread that has no local transcript learns its connection from the message, and keeps it.
const SERVER_QUESTION = { id: 'u1', role: 'user' as const, content: 'Hei', createdAt: '2026-11-13T10:00:00.000Z' }

/**
 * Loads the normalizer and the transcript store with both switches set. They are read once, at load, so the modules are
 * reloaded under each setting. The customer-gateway switch is off unless a test names it: `load('true')` is the side-chat
 * build, and `load('false', 'true')` the customer-gateway build.
 */
async function load(sideChat: 'true' | 'false', byo = 'false') {
  vi.resetModules()
  vi.stubEnv('VITE_CHAT_SIDE_CHAT', sideChat)
  vi.stubEnv('VITE_BYO_GATEWAY', byo)
  const normalizers = await import('./chat-normalizers')
  const history = await import('@/features/chat/lib/chat-thread-history')
  return { normalizers, history }
}

afterEach(() => {
  vi.unstubAllEnvs()
  try {
    window.localStorage.clear()
  } catch {
    // ignore: a blocked store only means nothing is left over to clear
  }
})

describe('a customer-gateway connection across a reload, with the side chat on', () => {
  it('is written into the transcript, and only when the turn names one', async () => {
    const { normalizers } = await load('true')
    const [kept, platform] = normalizers.turnsToTranscript([GATEWAY_ANSWER, turn({ id: 'a2', role: 'assistant', model: 'verevon-balance' })])
    expect(kept?.modelConnectionId).toBe('conn-gw-1')
    expect(platform?.modelConnectionId).toBeUndefined()
  })

  it('is kept in the stored transcript, which a reopened thread reads back', async () => {
    const { normalizers, history } = await load('true')
    history.upsertChatThreadTranscript({ threadId: 'thread-1', turns: normalizers.turnsToTranscript([GATEWAY_ANSWER]) })
    expect(history.readChatThreadTranscript('thread-1')?.turns[0]?.modelConnectionId).toBe('conn-gw-1')
  })

  it('is restored onto the turn when the transcript is reopened', async () => {
    const { normalizers, history } = await load('true')
    history.upsertChatThreadTranscript({ threadId: 'thread-1', turns: normalizers.turnsToTranscript([GATEWAY_ANSWER]) })
    const stored = history.readChatThreadTranscript('thread-1')
    const reopened = normalizers.transcriptTurnToChatTurn(
      stored?.turns[0] ?? { id: 'missing', role: 'assistant', content: '', createdAt: '' },
    )
    expect(reopened.modelConnectionId).toBe('conn-gw-1')
    expect(reopened.model).toBe('gateway-a')
  })

  it('is kept when the server copy of the turn is merged over the cached one', async () => {
    // The server's message has the model but no connection id, so the cached turn's connection is what survives the merge.
    const { normalizers } = await load('true')
    const serverCopy = turn({ id: 'a1', role: 'assistant', content: 'Svar', model: 'gateway-a' })
    const [merged] = normalizers.mergeServerTurnsWithCachedMetadata([serverCopy], [GATEWAY_ANSWER])
    expect(merged?.modelConnectionId).toBe('conn-gw-1')
    expect(merged?.model).toBe('gateway-a')
  })
})

describe('with both switches off, the default build', () => {
  it('writes no connection id into the stored transcript, so the stored bytes are unchanged', async () => {
    const { normalizers, history } = await load('false')
    expect(JSON.stringify(normalizers.turnsToTranscript([GATEWAY_ANSWER]))).not.toContain('modelConnectionId')
    history.upsertChatThreadTranscript({ threadId: 'thread-1', turns: normalizers.turnsToTranscript([GATEWAY_ANSWER]) })
    expect(window.localStorage.getItem(history.CHAT_THREAD_TRANSCRIPTS_KEY) ?? '').not.toContain('modelConnectionId')
  })

  it('restores no connection onto a reopened turn, nor over the cached one in a merge', async () => {
    const { normalizers } = await load('false')
    const reopened = normalizers.transcriptTurnToChatTurn({
      id: 'a1',
      role: 'assistant',
      content: 'Svar',
      createdAt: '2026-11-13T10:00:00.000Z',
      model: 'gateway-a',
      modelConnectionId: 'conn-gw-1',
    })
    expect(reopened.modelConnectionId).toBeUndefined()
    const serverCopy = turn({ id: 'a1', role: 'assistant', content: 'Svar', model: 'gateway-a' })
    const [merged] = normalizers.mergeServerTurnsWithCachedMetadata([serverCopy], [GATEWAY_ANSWER])
    expect(merged?.modelConnectionId).toBeUndefined()
  })

  it('takes no connection from a server message, so a reopened turn is what it was before the server named one', async () => {
    const { normalizers } = await load('false')
    const reopened = normalizers.messageToTurn({
      ...SERVER_QUESTION,
      modelConnectionId: 'conn-gw-1',
      subscriptionConnectionId: 'conn-sub-1',
    })
    expect('modelConnectionId' in reopened).toBe(false)
    expect('subscriptionConnectionId' in reopened).toBe(false)
    expect('provider' in reopened).toBe(false)
  })
})

describe('a customer-gateway connection across a reload, with the customer-gateway switch on and the side chat off', () => {
  it('is written into the transcript and restored onto the reopened turn, so its regenerate, edit and continue keep it', async () => {
    const { normalizers, history } = await load('false', 'true')
    history.upsertChatThreadTranscript({ threadId: 'thread-1', turns: normalizers.turnsToTranscript([GATEWAY_ANSWER]) })
    const stored = history.readChatThreadTranscript('thread-1')?.turns[0]
    expect(stored?.modelConnectionId).toBe('conn-gw-1')
    const reopened = normalizers.transcriptTurnToChatTurn(stored ?? { id: 'missing', role: 'assistant', content: '', createdAt: '' })
    expect(reopened).toMatchObject({ model: 'gateway-a', modelConnectionId: 'conn-gw-1' })
  })

  it('is kept when the server copy of the turn is merged over the cached one', async () => {
    const { normalizers } = await load('false', 'true')
    const serverCopy = turn({ id: 'a1', role: 'assistant', content: 'Svar', model: 'gateway-a' })
    const [merged] = normalizers.mergeServerTurnsWithCachedMetadata([serverCopy], [GATEWAY_ANSWER])
    expect(merged?.modelConnectionId).toBe('conn-gw-1')
  })

  it('is restored from the server message the send carried, and no subscription comes with it while the side chat is off', async () => {
    const { normalizers } = await load('false', 'true')
    const reopened = normalizers.messageToTurn({ ...SERVER_QUESTION, modelConnectionId: 'conn-gw-1', subscriptionConnectionId: 'conn-sub-1' })
    expect(reopened.modelConnectionId).toBe('conn-gw-1')
    expect('subscriptionConnectionId' in reopened).toBe(false)
    expect('provider' in reopened).toBe(false)
  })
})

describe('the customer-gateway switch is read as the literal true, and nothing else turns it on', () => {
  it.each(['', 'false', '1', 'TRUE'])('keeps no connection with VITE_BYO_GATEWAY %j and the side chat off', async (byo) => {
    const { normalizers } = await load('false', byo)
    expect(JSON.stringify(normalizers.turnsToTranscript([GATEWAY_ANSWER]))).not.toContain('modelConnectionId')
    const reopened = normalizers.transcriptTurnToChatTurn({
      id: 'a1',
      role: 'assistant',
      content: 'Svar',
      createdAt: '2026-11-13T10:00:00.000Z',
      model: 'gateway-a',
      modelConnectionId: 'conn-gw-1',
    })
    expect(reopened.modelConnectionId).toBeUndefined()
    const fromServer = normalizers.messageToTurn({ ...SERVER_QUESTION, modelConnectionId: 'conn-gw-1' })
    expect('modelConnectionId' in fromServer).toBe(false)
  })
})

describe('a connection the server names on a message, with the side chat on', () => {
  it('is restored onto the reopened turn, so its regenerate, edit and retry replay on that connection', async () => {
    const { normalizers } = await load('true')
    const reopened = normalizers.messageToTurn({ ...SERVER_QUESTION, modelConnectionId: 'conn-gw-1' })
    expect(reopened.modelConnectionId).toBe('conn-gw-1')
    expect('subscriptionConnectionId' in reopened).toBe(false)
  })

  it('restores a ChatGPT subscription connection with the provider that connection serves', async () => {
    const { normalizers } = await load('true')
    const reopened = normalizers.messageToTurn({ ...SERVER_QUESTION, subscriptionConnectionId: 'conn-sub-1' })
    expect(reopened).toMatchObject({ subscriptionConnectionId: 'conn-sub-1', provider: 'openai-codex-subscription' })
    expect('modelConnectionId' in reopened).toBe(false)
  })

  it('restores no connection onto a platform message, which keeps the keys it had', async () => {
    const { normalizers } = await load('true')
    const platform = normalizers.messageToTurn(SERVER_QUESTION)
    expect('modelConnectionId' in platform).toBe(false)
    expect('subscriptionConnectionId' in platform).toBe(false)
    expect('provider' in platform).toBe(false)
  })

  it('keeps the server copy of the connection when it is merged over a cached copy that has none', async () => {
    const { normalizers } = await load('true')
    const serverQuestion = normalizers.messageToTurn({ ...SERVER_QUESTION, modelConnectionId: 'conn-gw-1' })
    const cachedQuestion = turn({ id: 'u1', role: 'user', content: 'Hei' })
    const [merged] = normalizers.mergeServerTurnsWithCachedMetadata([serverQuestion], [cachedQuestion])
    expect(merged?.modelConnectionId).toBe('conn-gw-1')
  })
})
