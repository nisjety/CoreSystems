import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { onCleanup } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { clearSession, getSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import MarketingPulsePage from './MarketingPulsePage'

const mounted = vi.hoisted(() => ({ requests: [] as AbortController[], identities: [] as string[] }))

vi.mock('@/features/marketing/pulse/MarketingPulse', () => ({
  MarketingPulse: (props: { canManageIntegrations?: boolean; canConfigureMonitoring?: boolean }) => {
    const identity = `${getSession().user?.id}:${getSession().activeOrg?.id}`
    const request = new AbortController()
    mounted.requests.push(request)
    mounted.identities.push(identity)
    onCleanup(() => request.abort())
    return <section aria-label="Shared Marketing Visibility" data-manage-integrations={String(props.canManageIntegrations)} data-configure-monitoring={String(props.canConfigureMonitoring)}>Connected workspace {identity}</section>
  },
}))

function signIn(userId = 'user-a', orgId = 'org-a') {
  setSessionUser({ id: userId, name: 'Member', email: 'member@example.com', emailVerified: true })
  markSessionOnboardingComplete({ id: orgId, name: 'Workspace', role: 'member' })
}

beforeEach(() => {
  clearSession()
  mounted.requests = []
  mounted.identities = []
})

afterEach(() => {
  cleanup()
  clearSession()
})

describe('MarketingPulsePage', () => {
  it('renders the shared feature within the existing authenticated shell', () => {
    signIn()
    const { container } = render(() => <MarketingPulsePage />)
    expect(screen.getByRole('region', { name: 'Shared Marketing Visibility' }).textContent).toBe('Connected workspace user-a:org-a')
    expect(container.querySelector('nav')).toBeNull()
    expect(container.querySelector('main')).toBeNull()
  })

  it('does not mount data-fetching panels without an authenticated organization', () => {
    render(() => <MarketingPulsePage />)
    expect(mounted.requests).toHaveLength(0)
    setSessionUser({ id: 'user-a', name: 'Member', email: 'member@example.com', emailVerified: true })
    expect(mounted.requests).toHaveLength(0)
    expect(screen.queryByRole('region', { name: 'Shared Marketing Visibility' })).toBeNull()
    expect(screen.getByRole('status').textContent).toMatch(/arbeidsområde|workspace/i)
  })

  it('only grants integration controls to the active organization owner or admin', () => {
    setSessionUser({ id: 'user-a', name: 'Platform admin', email: 'admin@example.com', emailVerified: true, role: 'admin' })
    markSessionOnboardingComplete({ id: 'org-a', name: 'Workspace', role: 'member' })
    render(() => <MarketingPulsePage />)
    const capability = () => screen.getByRole('region', { name: 'Shared Marketing Visibility' }).getAttribute('data-manage-integrations')
    expect(capability()).toBe('false')
    markSessionOnboardingComplete({ id: 'org-a', name: 'Workspace', role: 'owner' })
    expect(capability()).toBe('true')
    markSessionOnboardingComplete({ id: 'org-a', name: 'Workspace', role: 'admin' })
    expect(capability()).toBe('true')
    markSessionOnboardingComplete({ id: 'org-a', name: 'Workspace', role: 'viewer' })
    expect(capability()).toBe('false')
  })

  it('unmounts old panels and aborts their requests before showing another organization', async () => {
    signIn()
    render(() => <MarketingPulsePage />)
    markSessionOnboardingComplete({ id: 'org-b', name: 'Other workspace', role: 'member' })
    await waitFor(() => expect(mounted.identities).toEqual(['user-a:org-a', 'user-a:org-b']))
    expect(mounted.requests[0]?.signal.aborted).toBe(true)
    expect(mounted.requests[1]?.signal.aborted).toBe(false)
    expect(screen.getByRole('region', { name: 'Shared Marketing Visibility' }).textContent).not.toContain('org-a')
  })

  it('only offers monitoring configuration to active workspace write roles', () => {
    signIn()
    render(() => <MarketingPulsePage />)
    const capability = () => screen.getByRole('region', { name: 'Shared Marketing Visibility' }).getAttribute('data-configure-monitoring')
    expect(capability()).toBe('true')
    for (const role of ['viewer', 'unknown', 'member', 'admin', 'owner']) {
      markSessionOnboardingComplete({ id: 'org-a', name: 'Workspace', role })
      expect(capability()).toBe(String(['member', 'admin', 'owner'].includes(role)))
    }
  })

  it('clears panels when the user signs out or changes identity', async () => {
    signIn()
    render(() => <MarketingPulsePage />)
    clearSession()
    await waitFor(() => expect(screen.queryByRole('region', { name: 'Shared Marketing Visibility' })).toBeNull())
    expect(mounted.requests[0]?.signal.aborted).toBe(true)
    signIn('user-b')
    await waitFor(() => expect(mounted.identities).toEqual(['user-a:org-a', 'user-b:org-a']))
  })
})
