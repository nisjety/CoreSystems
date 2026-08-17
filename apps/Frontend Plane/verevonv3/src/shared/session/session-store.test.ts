import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clearSession,
  getSession,
  isOnboardingComplete,
  loadSession,
  markSessionOnboardingComplete,
  setSessionUser,
} from './session-store'
import { bindSupportChatThread, readSupportChatThread } from '../chat/support-chat-thread'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })
}

describe('session store organization transitions', () => {
  afterEach(() => {
    clearSession()
    vi.unstubAllGlobals()
  })

  it('does not carry completed onboarding from the previous organization', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'user@example.com',
      name: 'User',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org_a', name: 'Alpha', role: 'owner' })

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/session') {
        return jsonResponse({ user: { id: 'user_1', email: 'user@example.com' } })
      }
      return jsonResponse({
        user: { id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true },
        org: { id: 'org_b', name: 'Beta', role: 'member' },
        permissions: [],
        onboardingStatus: 'CREATED',
        status: 'authenticated',
      })
    }))

    await loadSession()

    expect(getSession().activeOrg?.id).toBe('org_b')
    expect(getSession().onboardingStatus).toBe('CREATED')
  })

  it('preserves a local completion only for the same user and organization', async () => {
    setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
    markSessionOnboardingComplete({ id: 'org_a', name: 'Alpha', role: 'owner' })
    expect(isOnboardingComplete()).toBe(true)

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/session') {
        return jsonResponse({ user: { id: 'user_1' } })
      }
      return jsonResponse({
        user: { id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true },
        org: { id: 'org_a', name: 'Alpha', role: 'owner' },
        permissions: [],
        onboardingStatus: 'PROFILE_READY',
        status: 'authenticated',
      })
    }))

    await loadSession()
    expect(getSession().onboardingStatus).toBe('COMPLETED')
  })

  it('clears tenant state for missing auth or a failed authoritative snapshot', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(null)))
    await loadSession()
    expect(getSession().status).toBe('unauthenticated')

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/session') return jsonResponse({ user: { id: 'user_1' } })
      throw new Error('snapshot unavailable')
    }))
    await loadSession()
    expect(getSession()).toMatchObject({
      status: 'unauthenticated',
      activeOrg: null,
      onboardingStatus: null,
    })
  })

  it('keeps a first load unauthenticated when the snapshot never answers', async () => {
    // Nothing established yet, so there is no session to preserve: falling back
    // to the sign-in screen is the only honest option.
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/session') return jsonResponse({ user: { id: 'user_1' } })
      throw new Error('snapshot unavailable')
    }))
    await loadSession()
    expect(getSession().status).toBe('unauthenticated')
  })

  it('discards support-to-Chat thread bindings when the authenticated session is cleared', () => {
    const scope = { userId: 'user_1', orgId: 'org_a', conversationId: 'conversation_1' }
    bindSupportChatThread(scope, 'thread_support_1')

    clearSession()

    expect(readSupportChatThread(scope)).toBeNull()
  })
})

/**
 * The random-logout regression. An established session must survive a backend
 * that cannot answer: only a server actually saying "no session" may end it.
 * Every authenticated request re-validates against auth-core, so treating an
 * unanswered check as a sign-out used to bounce a signed-in user back to the
 * login screen every few minutes while the stored session stayed valid for days.
 */
describe('an established session survives a backend that cannot answer', () => {
  afterEach(() => {
    clearSession()
    vi.unstubAllGlobals()
  })

  function errorResponse(status: number, code: string): Response {
    return new Response(JSON.stringify({ error: { code, message: 'x' } }), {
      status,
      headers: { 'Content-Type': 'application/json' },
    })
  }

  async function signIn(): Promise<void> {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/session') {
        return jsonResponse({ user: { id: 'user_1', email: 'user@example.com' } })
      }
      return jsonResponse({
        user: { id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true },
        org: { id: 'org_a', name: 'Alpha', role: 'owner' },
        permissions: [],
        onboardingStatus: 'COMPLETED',
        status: 'authenticated',
      })
    }))
    await loadSession()
    expect(getSession().status).toBe('authenticated')
  }

  it('keeps the session when the gateway reports verification unavailable', async () => {
    await signIn()
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(503, 'session_verification_unavailable')))

    await loadSession()

    expect(getSession().status).toBe('authenticated')
    expect(getSession().user?.id).toBe('user_1')
    expect(getSession().activeOrg?.id).toBe('org_a')
  })

  it('keeps the session when the network fails outright', async () => {
    await signIn()
    vi.stubGlobal('fetch', vi.fn(async () => {
      throw new TypeError('Failed to fetch')
    }))

    await loadSession()

    expect(getSession().status).toBe('authenticated')
  })

  it('keeps the session when the backend returns a server error', async () => {
    await signIn()
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(502, 'bad_gateway')))

    await loadSession()

    expect(getSession().status).toBe('authenticated')
  })

  it('still signs the user out when the server rejects the session outright', async () => {
    await signIn()
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(401, 'unauthorized')))

    await loadSession()

    expect(getSession()).toMatchObject({ status: 'unauthenticated', user: null, activeOrg: null })
  })

  it('still signs the user out when the server reports no session', async () => {
    await signIn()
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(null)))

    await loadSession()

    expect(getSession().status).toBe('unauthenticated')
  })
})
