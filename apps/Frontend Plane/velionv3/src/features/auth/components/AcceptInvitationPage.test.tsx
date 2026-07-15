// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { switchActiveOrganization } from '@/shared/api/organization-client'
import { clearSession, loadSession, setSessionUser } from '@/shared/session/session-store'
import AcceptInvitationPage from './AcceptInvitationPage'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function renderInvitation(invitationId = 'inv_123') {
  window.history.pushState(null, '', `/accept-invitation/${invitationId}`)
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/accept-invitation/:invitationId" component={AcceptInvitationPage} />
      <Route path="/dashboard" component={() => <div>Dashboard</div>} />
      <Route path="/onboarding" component={() => <div>Onboarding</div>} />
      <Route path="/login" component={() => <div>Login</div>} />
    </Router>
  ))
}

describe('AcceptInvitationPage', () => {
  afterEach(() => {
    cleanup()
    clearSession()
    vi.unstubAllGlobals()
  })

  it('accepts explicitly, refreshes the active organization, and continues', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'invitee@example.com',
      name: 'Invitee',
      emailVerified: true,
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/invitations/inv_123/accept') {
        return jsonResponse({
          invitation: { id: 'inv_123', organizationId: 'org_1', status: 'accepted' },
          member: { id: 'member_1', organizationId: 'org_1', role: 'member' },
        })
      }
      if (path === '/api/v1/orgs/switch-active') {
        return jsonResponse({ organization: { id: 'org_1' } })
      }
      if (path === '/api/v1/auth/session?disableCookieCache=true') {
        return jsonResponse({ user: { id: 'user_1', email: 'invitee@example.com', name: 'Invitee', emailVerified: true } })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: { id: 'user_1', email: 'invitee@example.com', name: 'Invitee', emailVerified: true },
          org: { id: 'org_1', name: 'Acme', role: 'member' },
          permissions: [],
          onboardingStatus: 'COMPLETED',
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderInvitation()
    fireEvent.click(screen.getByRole('button', { name: 'Accept invitation' }))

    expect(await screen.findByText('Dashboard')).toBeTruthy()
    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      '/api/v1/orgs/invitations/inv_123/accept',
      '/api/v1/orgs/switch-active',
      '/api/v1/auth/session?disableCookieCache=true',
      '/api/v1/session/current',
    ])
  })

  it('surfaces an expired or already-used invitation without mutating session state', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'invitee@example.com',
      name: 'Invitee',
      emailVerified: true,
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/invitations/inv_123/accept') {
        return jsonResponse({
          code: 'INVITATION_NOT_FOUND',
          message: 'Invitation not found',
        }, 400)
      }
      if (path === '/api/v1/auth/session?disableCookieCache=true') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
          org: null,
          permissions: [],
          onboardingStatus: null,
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderInvitation()
    fireEvent.click(screen.getByRole('button', { name: 'Accept invitation' }))

    await waitFor(() => {
      expect(screen.getByText(/expired, already accepted, or no longer valid/i)).toBeTruthy()
    })
    expect(screen.queryByText('Dashboard')).toBeNull()
    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      '/api/v1/orgs/invitations/inv_123/accept',
    ])
  })

  it('recovers when acceptance committed but its response was lost', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'invitee@example.com',
      name: 'Invitee',
      emailVerified: true,
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/invitations/inv_123/accept') {
        return jsonResponse({
          invitation: { id: 'inv_123', organizationId: 'org_1', status: 'accepted' },
          member: { id: 'member_1', organizationId: 'org_1', role: 'member' },
        })
      }
      if (path === '/api/v1/orgs/switch-active') {
        return jsonResponse({ organization: { id: 'org_1' } })
      }
      if (path === '/api/v1/auth/session?disableCookieCache=true') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
          org: { id: 'org_1', name: 'Acme', role: 'member' },
          permissions: [],
          onboardingStatus: 'COMPLETED',
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderInvitation()
    fireEvent.click(screen.getByRole('button', { name: 'Accept invitation' }))

    expect(await screen.findByText('Dashboard')).toBeTruthy()
    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      '/api/v1/orgs/invitations/inv_123/accept',
      '/api/v1/orgs/switch-active',
      '/api/v1/auth/session?disableCookieCache=true',
      '/api/v1/session/current',
    ])
  })

  it('does not treat an unrelated active-organization switch as accepted invitation evidence', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'invitee@example.com',
      name: 'Invitee',
      emailVerified: true,
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/switch-active') {
        return jsonResponse({ organization: { id: 'org_a' } })
      }
      if (path === '/api/v1/auth/session?disableCookieCache=true') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: {
            id: 'user_1',
            email: 'invitee@example.com',
            name: 'Invitee',
            emailVerified: true,
          },
          org: { id: 'org_b', name: 'Unrelated', role: 'member' },
          permissions: [],
          onboardingStatus: 'COMPLETED',
          status: 'authenticated',
        })
      }
      if (path === '/api/v1/orgs/invitations/inv_123/accept') {
        return jsonResponse({
          code: 'INVITATION_NOT_FOUND',
          message: 'Invitation not found',
        }, 400)
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    await switchActiveOrganization('org_a')
    await loadSession({ disableAuthCookieCache: true })
    renderInvitation()
    fireEvent.click(screen.getByRole('button', { name: 'Accept invitation' }))

    await waitFor(() => {
      expect(screen.getByText(/expired, already accepted, or no longer valid/i)).toBeTruthy()
    })
    expect(screen.queryByText('Dashboard')).toBeNull()
  })
})
