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

  it('discards support-to-Chat thread bindings when the authenticated session is cleared', () => {
    const scope = { userId: 'user_1', orgId: 'org_a', conversationId: 'conversation_1' }
    bindSupportChatThread(scope, 'thread_support_1')

    clearSession()

    expect(readSupportChatThread(scope)).toBeNull()
  })
})
