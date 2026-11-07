import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CONNECT_INTENT_TTL_MS,
  clearIntent,
  continueSavedConnectIntent,
  findNewConnection,
  readIntent,
  runConnectIntent,
  saveIntent,
  takeIntent,
  waitForNewConnection,
  type ConnectIntent,
  type ConnectionRef,
} from './connect-intent'

const T0 = 1_700_000_000_000
const KEY = 'verevon-connect-intent:thread-a'

function intent(overrides: Partial<ConnectIntent> = {}): ConnectIntent {
  return {
    threadId: 'thread-a',
    suggestionId: 'suggestion-1',
    provider: 'google',
    prompt: 'What did Kari send me yesterday?',
    createdAt: T0,
    ...overrides,
  }
}

function connection(id: string, overrides: Partial<ConnectionRef> = {}): ConnectionRef {
  return { id, providerKey: 'google', status: 'active', ...overrides }
}

/** A clock the tests move by hand: sleeping advances time instead of waiting. */
function fakeClock() {
  let at = T0
  return {
    now: () => at,
    sleep: async (ms: number) => {
      at += ms
    },
  }
}

// The one resume the tests use: it records the call and answers with `result`.
function resumeReturning(result: boolean) {
  return vi.fn((_intent: ConnectIntent, _connectionId: string) => result)
}

beforeEach(() => {
  window.sessionStorage.clear()
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('connect intent storage', () => {
  it('keeps one intent per thread and replaces it on a new connect', () => {
    saveIntent(intent())
    saveIntent(intent({ threadId: 'thread-b', prompt: 'A different question' }))
    expect(readIntent('thread-a', T0)).toEqual(intent())
    expect(readIntent('thread-b', T0)?.prompt).toBe('A different question')

    saveIntent(intent({ prompt: 'Replaced question' }))
    expect(readIntent('thread-a', T0)?.prompt).toBe('Replaced question')
  })

  it('expires at the 15 minute TTL and removes the record', () => {
    saveIntent(intent())
    expect(readIntent('thread-a', T0 + CONNECT_INTENT_TTL_MS - 1)).not.toBeNull()
    expect(readIntent('thread-a', T0 + CONNECT_INTENT_TTL_MS)).toBeNull()
    expect(window.sessionStorage.getItem(KEY)).toBeNull()
  })

  it('drops a record dated after now, and a malformed one', () => {
    saveIntent(intent({ createdAt: T0 + 5_000 }))
    expect(readIntent('thread-a', T0)).toBeNull()
    expect(window.sessionStorage.getItem(KEY)).toBeNull()

    window.sessionStorage.setItem(KEY, '{"threadId":')
    expect(readIntent('thread-a', T0)).toBeNull()
    expect(window.sessionStorage.getItem(KEY)).toBeNull()
  })

  it('ignores a record whose thread id does not match its key', () => {
    window.sessionStorage.setItem(KEY, JSON.stringify(intent({ threadId: 'thread-z' })))
    expect(readIntent('thread-a', T0)).toBeNull()
  })

  it('never throws when storage is blocked or full', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError')
    })
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('SecurityError')
    })
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new Error('SecurityError')
    })
    expect(() => saveIntent(intent())).not.toThrow()
    expect(readIntent('thread-a', T0)).toBeNull()
    expect(takeIntent('thread-a', T0)).toBeNull()
    expect(() => clearIntent('thread-a')).not.toThrow()
  })
})

describe('takeIntent', () => {
  it('hands the intent to the first caller only', () => {
    saveIntent(intent())
    expect(takeIntent('thread-a', T0)).toEqual(intent())
    expect(takeIntent('thread-a', T0)).toBeNull()
  })
})

describe('findNewConnection', () => {
  it('matches an active connection of the provider that was not already known', () => {
    const connections = [
      connection('known'),
      connection('paused', { status: 'revoked' }),
      connection('outlook', { providerKey: 'microsoft' }),
      connection('fresh'),
    ]
    expect(findNewConnection(connections, 'google', new Set<string>(['known']))).toBe('fresh')
    expect(findNewConnection(connections, 'google', new Set<string>(['known', 'fresh']))).toBeNull()
    expect(findNewConnection(connections, 'microsoft', new Set<string>())).toBe('outlook')
  })
})

describe('waitForNewConnection', () => {
  it('keeps polling through a failed read until the connection appears', async () => {
    const list = vi.fn()
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce([connection('fresh')])
    const id = await waitForNewConnection({
      provider: 'google',
      knownIds: new Set<string>(),
      listConnections: list,
      ...fakeClock(),
    })
    expect(id).toBe('fresh')
    expect(list).toHaveBeenCalledTimes(3)
  })

  it('stops at the deadline and reports that no connection appeared', async () => {
    const list = vi.fn().mockResolvedValue([])
    const id = await waitForNewConnection({
      provider: 'google',
      knownIds: new Set<string>(),
      listConnections: list,
      timeoutMs: 5_000,
      intervalMs: 2_000,
      ...fakeClock(),
    })
    expect(id).toBeNull()
    // Reads at 0 s, 2 s and 4 s, then one last read after the final second before the 5 s deadline.
    expect(list).toHaveBeenCalledTimes(4)
  })
})

describe('runConnectIntent', () => {
  function run(overrides: Partial<Parameters<typeof runConnectIntent>[0]> = {}) {
    return runConnectIntent({
      threadId: 'thread-a',
      suggestionId: 'suggestion-1',
      provider: 'google',
      prompt: 'What did Kari send me yesterday?',
      knownIds: new Set<string>(),
      openConnect: async () => undefined,
      listConnections: async () => [connection('fresh')],
      resume: resumeReturning(true),
      ...fakeClock(),
      ...overrides,
    })
  }

  it('saves the intent before the sign-in opens, then resumes once on the new connection', async () => {
    let savedWhileOpening: string | null = null
    const resume = resumeReturning(true)
    const outcome = await run({
      openConnect: async () => {
        savedWhileOpening = window.sessionStorage.getItem(KEY)
      },
      resume,
    })
    expect(savedWhileOpening).not.toBeNull()
    expect(resume).toHaveBeenCalledTimes(1)
    expect(resume).toHaveBeenCalledWith(intent(), 'fresh')
    expect(outcome).toEqual({ status: 'resumed', connectionId: 'fresh' })
    expect(window.sessionStorage.getItem(KEY)).toBeNull()
  })

  it('clears the intent when the sign-in fails, and reports the provider error', async () => {
    const failure = new Error('Access was denied by the provider.')
    const resume = resumeReturning(true)
    const outcome = await run({
      openConnect: async () => {
        throw failure
      },
      resume,
    })
    expect(outcome).toEqual({ status: 'failed', error: failure })
    expect(resume).not.toHaveBeenCalled()
    expect(window.sessionStorage.getItem(KEY)).toBeNull()
  })

  it('keeps the intent when the resend is refused, so it can be finished later', async () => {
    const outcome = await run({ resume: resumeReturning(false) })
    expect(outcome).toEqual({ status: 'pending' })
    expect(readIntent('thread-a', T0)).toEqual(intent())
  })

  it('keeps the intent and resumes nothing when no connection appears in time', async () => {
    const resume = resumeReturning(true)
    const outcome = await run({
      listConnections: async () => [],
      timeoutMs: 4_000,
      intervalMs: 2_000,
      resume,
    })
    expect(outcome).toEqual({ status: 'pending' })
    expect(resume).not.toHaveBeenCalled()
    expect(readIntent('thread-a', T0)).toEqual(intent())
  })

  it('does not resume on a connection the card already knew about', async () => {
    const resume = resumeReturning(true)
    const outcome = await run({
      knownIds: new Set<string>(['known']),
      listConnections: async () => [connection('known')],
      timeoutMs: 0,
      resume,
    })
    expect(outcome).toEqual({ status: 'pending' })
    expect(resume).not.toHaveBeenCalled()
  })
})

describe('continueSavedConnectIntent', () => {
  it('does nothing when no intent is saved', async () => {
    const listConnections = vi.fn(async () => [connection('fresh')])
    const outcome = await continueSavedConnectIntent({
      threadId: 'thread-a',
      listConnections,
      resume: resumeReturning(true),
      now: () => T0,
    })
    expect(outcome).toBe('none')
    expect(listConnections).not.toHaveBeenCalled()
  })

  it('reports not connected and keeps the intent until the connection exists', async () => {
    saveIntent(intent())
    const resume = resumeReturning(true)
    const outcome = await continueSavedConnectIntent({
      threadId: 'thread-a',
      listConnections: async () => [],
      resume,
      now: () => T0,
    })
    expect(outcome).toBe('not_connected')
    expect(resume).not.toHaveBeenCalled()
    expect(readIntent('thread-a', T0)).toEqual(intent())
  })

  it('resumes once after a reload, and a second click finds nothing to send', async () => {
    saveIntent(intent())
    const resume = resumeReturning(true)
    const listConnections = async () => [connection('fresh')]
    const first = await continueSavedConnectIntent({ threadId: 'thread-a', listConnections, resume, now: () => T0 })
    const second = await continueSavedConnectIntent({ threadId: 'thread-a', listConnections, resume, now: () => T0 })
    expect(first).toBe('resumed')
    expect(second).toBe('none')
    expect(resume).toHaveBeenCalledTimes(1)
    expect(resume).toHaveBeenCalledWith(intent(), 'fresh')
  })

  it('saves the intent again when the resend is refused', async () => {
    saveIntent(intent())
    const outcome = await continueSavedConnectIntent({
      threadId: 'thread-a',
      listConnections: async () => [connection('fresh')],
      resume: resumeReturning(false),
      now: () => T0,
    })
    expect(outcome).toBe('pending')
    expect(readIntent('thread-a', T0)).toEqual(intent())
  })

  it('treats an expired intent as absent and does not list connections', async () => {
    saveIntent(intent())
    const listConnections = vi.fn(async () => [connection('fresh')])
    const outcome = await continueSavedConnectIntent({
      threadId: 'thread-a',
      listConnections,
      resume: resumeReturning(true),
      now: () => T0 + CONNECT_INTENT_TTL_MS,
    })
    expect(outcome).toBe('none')
    expect(listConnections).not.toHaveBeenCalled()
  })
})
