// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EmailAccountHealthBadge, SupportExpandedSidebarPanel } from './CoreSidebarSupportPanel'
import { I18nProvider } from '@/shared/i18n'

const { listConnectionsMock } = vi.hoisted(() => ({
  listConnectionsMock: vi.fn(),
}))

vi.mock('@/shared/api/integrations-client', () => ({
  listConnections: listConnectionsMock,
}))

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-aquatiq', name: 'Aquatiq', role: 'owner' } }),
}))

beforeEach(() => {
  listConnectionsMock.mockResolvedValue([])
})

afterEach(() => {
  cleanup()
  listConnectionsMock.mockReset()
})

function renderSupportSidebar(path: string) {
  window.history.pushState(null, '', path)
  return render(() => (
    <I18nProvider>
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/support" component={() => <SupportExpandedSidebarPanel onCollapse={() => undefined} />} />
      </Router>
    </I18nProvider>
  ))
}

describe('SupportExpandedSidebarPanel', () => {
  it('keeps the unified Support sidebar compact while retaining its actionable ticket filters', () => {
    renderSupportSidebar('/support?surface=tickets&queue=all')

    expect(screen.getByRole('button', { name: /Queues|Køer/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Workflow|Arbeidsflyt/i })).toBeTruthy()

    const myTicketsLink = screen.getByRole('link', { name: /my tickets|mine saker/i })
    expect(myTicketsLink.getAttribute('href')).toBe('/support?surface=tickets&queue=my')

    expect(screen.queryByRole('tablist', { name: /support workspaces|supportarbeidsområder/i })).toBeNull()
    expect(screen.queryByRole('tab', { name: /conversations|samtaler|ticketing|saksbehandling|outbound|utgående/i })).toBeNull()
    expect(screen.queryByRole('link', { name: /AI review|AI-gjennomgang/i })).toBeNull()
  })

  it('does not expose a heuristic, non-user-specific mentions queue', () => {
    renderSupportSidebar('/support?view=mine')

    expect(screen.queryByRole('link', { name: /mentions|omtaler/i })).toBeNull()
  })

  it('keeps the primary workspace switcher out of the sidebar', () => {
    renderSupportSidebar('/support?view=mine')

    expect(screen.queryByRole('tablist', { name: /support workspaces|supportarbeidsområder/i })).toBeNull()
    expect(screen.getByRole('link', { name: /my conversations|mine samtaler/i })).toBeTruthy()
  })

  it('shows conversation queues, statuses, and channel routes in the sidebar', () => {
    renderSupportSidebar('/support?view=mine')

    expect(screen.getByRole('link', { name: /my conversations|mine samtaler/i }).getAttribute('href')).toBe('/support?view=mine')
    expect(screen.getByRole('link', { name: /all conversations|alle samtaler/i }).getAttribute('href')).toBe('/support?view=all')
    expect(screen.getByRole('link', { name: /waiting|venter/i }).getAttribute('href')).toBe('/support?view=all&status=pending')
    expect(screen.getByRole('link', { name: 'Messenger' }).getAttribute('href')).toBe('/support?view=all&channel=messenger')
    expect(screen.getByRole('link', { name: 'Instagram' }).getAttribute('href')).toBe('/support?view=all&channel=instagram')
    expect(screen.getByRole('link', { name: 'WhatsApp' }).getAttribute('href')).toBe('/support?view=all&channel=whatsapp')
    expect(screen.getByRole('link', { name: 'Threads' }).getAttribute('href')).toBe('/support?view=all&channel=threads')
    expect(screen.getByRole('link', { name: 'LinkedIn' }).getAttribute('href')).toBe('/support?view=all&channel=linkedin')
    expect(screen.getByRole('link', { name: 'SMS' }).getAttribute('href')).toBe('/support?view=all&channel=sms')
    // Email is intentionally connection-driven and absent without an active
    // Gmail/Outlook mailbox; the remaining static channel filters stay intact.
    expect(screen.getByRole('navigation', { name: /support navigation|supportnavigasjon/i }).querySelectorAll('svg.verevon-support-provider-icon')).toHaveLength(10)
  })

  it('shows the complete operational ticket queues in the sidebar', () => {
    renderSupportSidebar('/support?surface=tickets&queue=all')

    expect(screen.getByRole('link', { name: /suggested by ai|foreslått av ai/i }).getAttribute('href')).toContain('queue=suggested')
    expect(screen.getByRole('link', { name: /waiting on team|venter på team/i }).getAttribute('href')).toContain('queue=waiting-team')
    expect(screen.getByRole('link', { name: /resolved|løst/i }).getAttribute('href')).toContain('queue=resolved')
    expect(screen.getByRole('link', { name: /rules.*queues|regler.*køer/i }).getAttribute('href')).toContain('queue=rules')
  })

  it('adds only canonical outbound and delivery-receipt filters to the sidebar', () => {
    renderSupportSidebar('/support?surface=outbound&outbound_status=submitted')

    expect(screen.getByRole('link', { name: /provider accepted|godtatt av leverandør/i }).getAttribute('href')).toContain('outbound_status=submitted')
    expect(screen.getByRole('link', { name: /unknown outcome|ukjent utfall/i }).getAttribute('href')).toContain('outbound_status=unknown')
    expect(screen.getByRole('link', { name: /delivery failed|levering mislyktes/i }).getAttribute('href')).toContain('outbound_delivery_status=failed')
    expect(screen.queryByRole('link', { name: /drafts|utkast|scheduled|planlagt/i })).toBeNull()
  })

  it('shows email sync health without calling a completed sync a delivery receipt', () => {
    render(() => (
      <I18nProvider>
        <EmailAccountHealthBadge account={{
          id: 'conn-outlook',
          providerKey: 'microsoft',
          label: 'ima.dacosta@aquatiq.com',
          sharedMailboxes: [],
          syncHealth: 'synced',
          lastSyncAt: '2026-08-05T11:00:00.000Z',
        }} />
      </I18nProvider>
    ))

    expect(screen.getByText(/synced|synkronisert/i)).toBeTruthy()
    expect(screen.getByTitle(/not proof of delivery|ikke bevis på levering/i)).toBeTruthy()
  })

  it('marks only the selected mailbox filter as the current page', async () => {
    listConnectionsMock.mockResolvedValueOnce([
      {
        id: 'conn-outlook',
        providerId: 'microsoft',
        providerKey: 'microsoft',
        providerEmail: 'ima.dacosta@aquatiq.com',
        status: 'connected',
        scopes: ['mail.read'],
        lastSyncStatus: 'synced',
        createdAt: '2026-08-05T10:00:00.000Z',
      },
      {
        id: 'conn-gmail',
        providerId: 'google',
        providerKey: 'google',
        providerEmail: 'imamzambi64@gmail.com',
        status: 'connected',
        scopes: ['gmail.read'],
        lastSyncStatus: 'synced',
        createdAt: '2026-08-05T10:00:00.000Z',
      },
    ])
    renderSupportSidebar('/support?view=all&channel=email&connection_id=conn-gmail')

    const outlook = await screen.findByRole('link', { name: 'ima.dacosta@aquatiq.com' })
    const gmail = screen.getByRole('link', { name: 'imamzambi64@gmail.com' })

    expect(outlook.getAttribute('aria-current')).toBeNull()
    expect(gmail.getAttribute('aria-current')).toBe('page')
  })

  it('keeps support queues usable and offers an explicit retry when connection status is unavailable', async () => {
    listConnectionsMock.mockRejectedValueOnce(new Error('integration catalogue unavailable'))
    renderSupportSidebar('/support?view=all&channel=email&connection_id=conn-gmail')

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(/connection status could not be loaded|tilkoblingsstatus kunne ikke lastes/i)
    expect(screen.getByRole('link', { name: /all conversations|alle samtaler/i })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /retry|prøv igjen/i }))
    await waitFor(() => expect(listConnectionsMock).toHaveBeenCalledTimes(2))
  })
})
