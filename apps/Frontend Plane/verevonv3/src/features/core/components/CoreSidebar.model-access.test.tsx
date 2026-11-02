// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { getModelEntitlement, type ModelEntitlementState } from '@/shared/api/model-entitlement-client'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// The Model access tab is gated on the gateway's admin switch. Only an organisation admin is asked,
// and only a settled `enabled: true` shows the tab, which leads to /settings/model-access.
vi.mock('@/shared/api/model-entitlement-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/model-entitlement-client')>()
  return { ...actual, getModelEntitlement: vi.fn() }
})

const getModelEntitlementMock = vi.mocked(getModelEntitlement)

const PAGE = '/settings/model-access'
const ORG_ID = 'org-1'
const SWITCH_ON: ModelEntitlementState = {
  enabled: true,
  organization_id: ORG_ID,
  revision: 0,
  org_default: null,
  roles: {},
  updated_at: null,
  updated_by: 'user-1',
}

// Solid batches store writes until a flush, so the organisation is only set once the user is in place.
function signIn({ orgRole, userRole = 'user' }: { orgRole: string | null; userRole?: string }) {
  setSessionUser({ id: 'user-1', email: 'ada@example.com', name: 'Ada', emailVerified: true, role: userRole })
  flush()
  markSessionOnboardingComplete(orgRole === null ? null : { id: ORG_ID, name: 'Verevon', role: orgRole })
  flush()
}

function renderSidebar() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <CoreSidebar activeRoute={PAGE} expanded onExpandedChange={vi.fn()} onOpenSearch={vi.fn()} /> }],
    history: memoryHistory(PAGE),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

const modelAccessLink = () => screen.queryByRole('link', { name: /^(Modelltilgang|Model access)$/ })
// A settings link that every admin sees, so the panel is known to be on screen when the tab is absent.
const billingLink = () => screen.queryByRole('link', { name: /^(Fakturering|Billing)$/ })

/** Lets a settled entitlement answer reach the sidebar before the panel is inspected. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  getModelEntitlementMock.mockReset()
})

afterEach(() => {
  cleanup()
  clearSession()
  flush()
})

describe('Model access tab in the sidebar settings panel', () => {
  it('is offered to an organisation admin when the gateway reports the switch on', async () => {
    signIn({ orgRole: 'owner' })
    getModelEntitlementMock.mockResolvedValue(SWITCH_ON)
    renderSidebar()

    await waitFor(() => expect(modelAccessLink()).not.toBeNull())
    expect(getModelEntitlementMock).toHaveBeenCalledWith(ORG_ID)
    expect(modelAccessLink()?.getAttribute('href')).toBe(PAGE)
    // The tab marks itself as the current page, so the route resolves to its own section.
    expect(modelAccessLink()?.getAttribute('aria-current')).toBe('page')
  })

  it('stays hidden when the gateway reports the switch off', async () => {
    signIn({ orgRole: 'owner' })
    getModelEntitlementMock.mockResolvedValue({ enabled: false })
    renderSidebar()

    await waitFor(() => expect(getModelEntitlementMock).toHaveBeenCalledWith(ORG_ID))
    await settle()
    expect(billingLink()).not.toBeNull()
    expect(modelAccessLink()).toBeNull()
  })

  it('stays hidden when the gateway cannot answer', async () => {
    signIn({ orgRole: 'owner' })
    getModelEntitlementMock.mockRejectedValue(new Error('gateway unavailable'))
    renderSidebar()

    await waitFor(() => expect(getModelEntitlementMock).toHaveBeenCalledWith(ORG_ID))
    await settle()
    expect(billingLink()).not.toBeNull()
    expect(modelAccessLink()).toBeNull()
  })

  it('is never requested for a member', async () => {
    signIn({ orgRole: 'member' })
    getModelEntitlementMock.mockResolvedValue(SWITCH_ON)
    renderSidebar()

    await settle()
    await settle()
    expect(getModelEntitlementMock).not.toHaveBeenCalled()
    expect(modelAccessLink()).toBeNull()
  })

  it('is never requested by an admin who has no active organisation', async () => {
    signIn({ orgRole: null, userRole: 'admin' })
    getModelEntitlementMock.mockResolvedValue(SWITCH_ON)
    renderSidebar()

    await waitFor(() => expect(billingLink()).not.toBeNull())
    await settle()
    expect(getModelEntitlementMock).not.toHaveBeenCalled()
    expect(modelAccessLink()).toBeNull()
  })

  it('drops the tab when the active organisation is cleared, instead of keeping the last answer', async () => {
    signIn({ orgRole: 'owner', userRole: 'admin' })
    getModelEntitlementMock.mockResolvedValue(SWITCH_ON)
    renderSidebar()

    await waitFor(() => expect(modelAccessLink()).not.toBeNull())
    markSessionOnboardingComplete(null)
    flush()

    await waitFor(() => expect(modelAccessLink()).toBeNull())
  })
})
