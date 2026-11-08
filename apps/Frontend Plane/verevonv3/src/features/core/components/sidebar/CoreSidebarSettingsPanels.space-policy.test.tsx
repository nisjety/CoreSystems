// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SettingsExpandedSidebarPanel } from './CoreSidebarSettingsPanels'
import { I18nProvider } from '@/shared/i18n'
import { workspaceSettingsSectionIds, workspaceSettingsSections } from '@/features/settings/lib/settings-sections'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS03-S6: the "Rom-policy" link is offered only to an admin, and only while the gateway reports the
// admin API on. Any error, a 404 or a non-admin means it is not listed: a nav entry must never lead to
// a console that cannot load.

const spacePolicy = vi.hoisted(() => ({ probeSpacePolicyAdminAvailable: vi.fn() }))

vi.mock('@/shared/api/space-policy-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/space-policy-client')>()),
  probeSpacePolicyAdminAvailable: spacePolicy.probeSpacePolicyAdminAvailable,
}))

function renderPanel() {
  const TestRouter = createRouter({
    routes: [
      {
        path: '/settings/:section?',
        component: () => <SettingsExpandedSidebarPanel modelAccessEnabled={false} onCollapse={() => undefined} />,
      },
    ],
    history: memoryHistory('/settings/profile'),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

function signIn(role: 'owner' | 'admin' | 'member', orgId = 'org_1') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: orgId, name: 'Acme', role })
  flush()
}

const spacePolicyLink = () => screen.queryByRole('link', { name: /Rom-policy|Room policy/ })

describe('Settings sidebar: Rom-policy link', () => {
  beforeEach(() => {
    spacePolicy.probeSpacePolicyAdminAvailable.mockReset()
  })
  afterEach(() => {
    cleanup()
    clearSession()
  })

  it('is listed for an admin when the probe says the console is available', async () => {
    signIn('owner')
    spacePolicy.probeSpacePolicyAdminAvailable.mockResolvedValue(true)
    renderPanel()
    await waitFor(() => expect(spacePolicyLink()).not.toBeNull())
    expect(spacePolicyLink()?.getAttribute('href')).toBe('/settings/space-policy')
    expect(spacePolicy.probeSpacePolicyAdminAvailable).toHaveBeenCalledWith('org_1')
  })

  it('is not listed when the probe says the admin API is off', async () => {
    signIn('admin')
    spacePolicy.probeSpacePolicyAdminAvailable.mockResolvedValue(false)
    renderPanel()
    await waitFor(() => expect(spacePolicy.probeSpacePolicyAdminAvailable).toHaveBeenCalledTimes(1))
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
    expect(spacePolicyLink()).toBeNull()
    // The rest of the settings list is unaffected.
    expect(screen.getAllByRole('link').length).toBeGreaterThan(3)
  })

  it('is not listed when the probe itself fails', async () => {
    signIn('owner')
    spacePolicy.probeSpacePolicyAdminAvailable.mockRejectedValue(new Error('down'))
    renderPanel()
    await waitFor(() => expect(spacePolicy.probeSpacePolicyAdminAvailable).toHaveBeenCalledTimes(1))
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
    expect(spacePolicyLink()).toBeNull()
  })

  it('never asks, and never lists it, for a non-admin', async () => {
    signIn('member')
    spacePolicy.probeSpacePolicyAdminAvailable.mockResolvedValue(true)
    renderPanel()
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
    expect(spacePolicy.probeSpacePolicyAdminAvailable).not.toHaveBeenCalled()
    expect(spacePolicyLink()).toBeNull()
  })
})

describe('space-policy section registration', () => {
  it('is a known workspace settings section with its own title and save label', () => {
    expect(workspaceSettingsSectionIds).toContain('space-policy')
    const section = workspaceSettingsSections.find((entry) => entry.id === 'space-policy')
    expect(section?.label).toBe('Rom-policy')
    expect(section?.title).toBe('Rom-policy')
    expect(section?.saveLabel).toBeTruthy()
  })
})
