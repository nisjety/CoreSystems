// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { __resetOrgAgentPolicyAvailabilityForTests } from '@/features/customize/AgentModesSection'
import CustomizeHub from '@/features/customize/CustomizeHub'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import { flush } from 'solid-js'

// The hub composes its sections; these tests check the composition and the availability gate, so each section
// is a stub that names itself. The real sections are covered by their own tests.
vi.mock('@/features/customize/AgentModesSection', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/features/customize/AgentModesSection')>()
  return { ...actual, AgentModesSection: () => 'Agent mode section' }
})
vi.mock('@/features/customize/HooksSection', () => ({ HooksSection: () => 'Hooks section' }))
vi.mock('@/features/settings/components/McpServersSection', () => ({ McpServersSection: () => 'MCP section' }))
vi.mock('@/features/settings/components/SkillsSection', () => ({ SkillsSection: () => 'Skills section' }))
vi.mock('@/features/settings/components/OrgInstructionsSection', () => ({ OrgInstructionsSection: () => 'Instructions section' }))
vi.mock('@/features/settings/components/PluginsSection', () => ({ PluginsSection: () => 'Plugins section' }))

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function stubPolicyApi(status: number) {
  vi.stubGlobal('fetch', vi.fn(async () => (status === 200
    ? jsonResponse({ org_id: 'org_1', revision: 0, digest: '', settings: {} })
    : jsonResponse({ error: { code: 'not_found', message: 'off' } }, status))))
}

beforeEach(() => {
  window.localStorage.setItem('verevon.locale', 'en')
  __resetOrgAgentPolicyAvailabilityForTests()
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'admin' })
  flush()
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('CustomizeHub', () => {
  it('composes the agent-mode, hook and existing sections once the policy API is on', async () => {
    stubPolicyApi(200)

    render(() => <I18nProvider><CustomizeHub /></I18nProvider>)

    // The stubs are bare text, so the check reads the page rather than one element.
    await waitFor(() => expect(document.body.textContent).toContain('Agent mode section'))
    for (const section of ['Hooks section', 'MCP section', 'Skills section', 'Instructions section', 'Plugins section']) {
      expect(document.body.textContent).toContain(section)
    }
  })

  it('offers no sections while the deployment has the policy API off', async () => {
    stubPolicyApi(404)

    render(() => <I18nProvider><CustomizeHub /></I18nProvider>)

    expect(await screen.findByText('Customize is not switched on for this deployment yet.')).toBeTruthy()
    expect(screen.queryByText('Agent mode section')).toBeNull()
    expect(screen.queryByText('Hooks section')).toBeNull()
    expect(screen.queryByText('MCP section')).toBeNull()
  })
})
