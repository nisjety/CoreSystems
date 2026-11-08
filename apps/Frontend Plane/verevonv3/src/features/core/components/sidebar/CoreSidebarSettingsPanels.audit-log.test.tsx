// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SettingsExpandedSidebarPanel } from './CoreSidebarSettingsPanels'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S4: the "Revisjonslogg" link is offered to owners and admins only. A nav entry must never
// lead a member to a view that shows only an admin-only note.

vi.mock('@/shared/api/space-policy-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/space-policy-client')>()),
  probeSpacePolicyAdminAvailable: vi.fn(async () => false),
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

function signIn(role: string) {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

const auditLink = () => screen.queryByRole('link', { name: /Revisjonslogg|Audit log/ })

afterEach(() => {
  cleanup()
  clearSession()
})

describe('Settings sidebar: audit log link', () => {
  it.each(['owner', 'admin'])('is listed for an %s and points at /settings/audit-log', (role) => {
    signIn(role)
    renderPanel()
    expect(auditLink()).not.toBeNull()
    expect(auditLink()?.getAttribute('href')).toBe('/settings/audit-log')
  })

  it.each(['member', 'viewer', 'guest'])('is not listed for a %s', (role) => {
    signIn(role)
    renderPanel()
    expect(auditLink()).toBeNull()
    // The rest of the list is still there.
    expect(screen.getAllByRole('link').length).toBeGreaterThan(3)
  })
})
