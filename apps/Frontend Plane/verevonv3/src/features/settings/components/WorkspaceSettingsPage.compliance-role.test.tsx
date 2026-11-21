// @vitest-environment jsdom

// O-21: the compliance role in the member list and the invitation form. The picker offers it only for the
// literal "true" of VITE_ORG_COMPLIANCE_ROLE; the "on" case is the control, so an "off" pass cannot come from a
// page that never reaches the members section.

import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' }, status })
}

function stubMembers() {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
    const path = String(input)
    if (path === '/api/v1/orgs/org_1/members') {
      return json({
        members: [
          { user_id: 'u_owner', email: 'owner@example.com', name: 'Owner', role: 'owner', status: 'active' },
          { user_id: 'u_comp', email: 'comp@example.com', name: 'Compliance Person', role: 'compliance', status: 'active' },
          { user_id: 'u_mem', email: 'mem@example.com', name: 'Plain Member', role: 'member', status: 'active' },
        ],
      })
    }
    return json({})
  }))
}

function signInAsOrgOwner() {
  setSessionUser({ id: 'u_owner', email: 'owner@example.com', name: 'Owner', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
  flush()
}

function optionValues(select: HTMLElement): string[] {
  return Array.from(select.querySelectorAll('option')).map((option) => (option as HTMLOptionElement).value)
}

describe('members section role pickers', () => {
  beforeEach(() => {
    signInAsOrgOwner()
    stubMembers()
  })
  afterEach(() => {
    cleanup()
    clearSession()
    vi.unstubAllEnvs()
    vi.unstubAllGlobals()
  })

  it('off: the invitation picker and a plain member row offer member and admin only', async () => {
    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    const memberSelect = await screen.findByLabelText(/Rolle for Plain Member/i)
    expect(optionValues(memberSelect)).toEqual(['member', 'admin'])
    expect(optionValues(screen.getByLabelText('Rolle'))).toEqual(['member', 'admin'])
  })

  it('off: a member who already holds compliance still shows it, so the select is not blank', async () => {
    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    const select = await screen.findByLabelText(/Rolle for Compliance Person/i)
    expect(optionValues(select)).toEqual(['member', 'admin', 'compliance'])
  })

  it('on: the invitation picker and every role picker offer compliance with the Norwegian label', async () => {
    vi.stubEnv('VITE_ORG_COMPLIANCE_ROLE', 'true')
    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    const memberSelect = await screen.findByLabelText(/Rolle for Plain Member/i)
    await waitFor(() => expect(optionValues(memberSelect)).toEqual(['member', 'admin', 'compliance']))
    const invite = screen.getByLabelText('Rolle')
    expect(optionValues(invite)).toEqual(['member', 'admin', 'compliance'])
    expect(invite.textContent).toContain('Etterlevelse (compliance)')
    // The owner row is still fixed.
    const ownerSelect = screen.getByLabelText(/Rolle for Owner/i)
    expect(ownerSelect.hasAttribute('disabled')).toBe(true)
  })
})
