// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { workspaceSettingsSectionIds, workspaceSettingsSections } from '@/features/settings/lib/settings-sections'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S4: the audit-log settings section exists, and the org-security "Open audit log" button
// navigates for owners and admins and stays inert for everyone else.

function signIn(role: string) {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

function stubFetch() {
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).startsWith('/api/v1/audit')) {
      return new Response(JSON.stringify({ data: [], meta: { count: 0 }, error: null }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }
    return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' }, status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

const openAuditLogButton = () => screen.queryByRole('button', { name: /åpne revisjonslogg|open audit log/i })

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('audit-log settings section', () => {
  it('is a registered section with the fields every settings tab needs', () => {
    expect(workspaceSettingsSectionIds).toContain('audit-log')
    const section = workspaceSettingsSections.find((candidate) => candidate.id === 'audit-log')
    expect(section).toBeDefined()
    expect(section!.title).toBeTruthy()
    expect(section!.description).toMatch(/owners and admins only/i)
    expect(section!.saveLabel.length).toBeGreaterThan(0)
    expect(workspaceSettingsSections.filter((candidate) => candidate.id === 'audit-log')).toHaveLength(1)
  })

  it('renders the log for an admin and the admin-only note for a member', async () => {
    signIn('admin')
    stubFetch()
    render(() => <VerevonWorkspaceSettingsPage section="audit-log" />)
    expect(await screen.findByTestId('audit-log-filters')).toBeTruthy()
    expect(screen.queryByTestId('audit-log-admin-only')).toBeNull()
    cleanup()

    clearSession()
    signIn('member')
    const fetchMock = stubFetch()
    render(() => <VerevonWorkspaceSettingsPage section="audit-log" />)
    expect(await screen.findByTestId('audit-log-admin-only')).toBeTruthy()
    expect(screen.queryByTestId('audit-log-filters')).toBeNull()
    expect(fetchMock.mock.calls.filter((call) => String(call[0]).startsWith('/api/v1/audit'))).toHaveLength(0)
  })
})

describe('org-security "Open audit log" button', () => {
  it('navigates to /settings/audit-log for an admin', async () => {
    signIn('admin')
    stubFetch()
    const assign = vi.fn()
    vi.stubGlobal('location', { ...window.location, assign })

    render(() => <VerevonWorkspaceSettingsPage section="org-security" />)
    const button = await waitFor(() => {
      const found = openAuditLogButton()
      expect(found).not.toBeNull()
      return found!
    })
    fireEvent.click(button)
    expect(assign).toHaveBeenCalledTimes(1)
    expect(assign).toHaveBeenCalledWith('/settings/audit-log')
  })

  it('is inert for a member: no navigation happens', async () => {
    signIn('member')
    stubFetch()
    const assign = vi.fn()
    vi.stubGlobal('location', { ...window.location, assign })

    render(() => <VerevonWorkspaceSettingsPage section="org-security" />)
    expect(await screen.findByRole('heading', { name: /nylige sikkerhetshendelser|recent security events/i })).toBeTruthy()
    const button = openAuditLogButton()
    if (button) fireEvent.click(button)
    expect(assign).not.toHaveBeenCalled()
  })
})
