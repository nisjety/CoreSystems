// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ConnectStepContent, ConnectStepVisual } from '@/features/onboarding/components/steps/ConnectStep'

describe('ConnectStepContent', () => {
  afterEach(() => cleanup())

  it('switches between work systems and social channel integrations', () => {
    const onConnect = vi.fn()

    render(() => (
      <ConnectStepContent
        connectedSources={[]}
        onConnect={onConnect}
        onContinue={vi.fn()}
        onSkip={vi.fn()}
      />
    ))

    expect(screen.getByText('Slack')).toBeTruthy()
    expect(screen.queryByText('Meta')).toBeNull()

    fireEvent.click(screen.getByRole('tab', { name: /Social channels/i }))
    flush()

    expect(screen.queryByText('Slack')).toBeNull()
    // Facebook/Instagram/WhatsApp/Meta Ads consolidate into ONE Meta card.
    expect(screen.getByText('Meta')).toBeTruthy()
    expect(screen.queryByText('Instagram')).toBeNull()
    expect(screen.getByText('LinkedIn')).toBeTruthy()
    expect(screen.getByText('TikTok')).toBeTruthy()
    expect(screen.getByText('Snapchat')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /^Meta/i }))
    flush()

    expect(onConnect).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'meta',
        provider: 'meta',
        category: 'social',
      }),
    )
  })

  it('offers ChatGPT subscription sign-in in the AI tab without treating it as a source connector', () => {
    render(() => (
      <ConnectStepContent
        connectedSources={[]}
        orgId="org-coresystem"
        onConnect={vi.fn()}
        onContinue={vi.fn()}
        onSkip={vi.fn()}
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: /^AI/i }))
    flush()

    expect(screen.getByRole('heading', { name: /chatgpt-abonnementet/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /koble til chatgpt/i })).toBeTruthy()
    expect(screen.queryByText('Slack')).toBeNull()
  })
})

describe('ConnectStepContent connected accounts', () => {
  afterEach(() => cleanup())

  const microsoftAccount = {
    connectionId: 'conn_f019f69a',
    connectorId: 'microsoft365',
    provider: 'microsoft',
    providerLabel: 'Microsoft 365',
    account: 'ima.dacosta@aquatiq.com',
    status: 'active' as const,
    grants: ['Outlook', 'Teams', 'SharePoint', 'OneDrive'],
    lanes: [
      { key: 'mail' as const, label: 'Outlook', status: 'synced' as const, lastSyncAt: '2026-09-04T02:00:00.000Z' },
      { key: 'documents' as const, label: 'SharePoint · OneDrive', status: 'pending' as const },
    ],
    nextStep: {
      kind: 'pick_library' as const,
      message: 'Velg hvilket SharePoint- eller OneDrive-bibliotek Verevon skal lese.',
    },
  }

  it('shows integration-core truth prominently and registers a library inline', async () => {
    const library = {
      listSites: vi.fn().mockResolvedValue([
        { id: 'site-1', name: 'Aquatiq', display_name: 'Aquatiq AS', web_url: 'https://aquatiq.sharepoint.com' },
        { id: 'site-2', name: 'Support', web_url: 'https://aquatiq.sharepoint.com/sites/support' },
      ]),
      listDrives: vi.fn().mockResolvedValue([
        { id: 'drive-1', name: 'Dokumenter', drive_type: 'documentLibrary', web_url: 'https://aquatiq.sharepoint.com/Shared%20Documents' },
      ]),
      register: vi.fn().mockResolvedValue(undefined),
    }

    render(() => (
      <ConnectStepContent
        connectedSources={[{ id: 'microsoft365', label: 'Microsoft 365', status: 'connected', sources: ['teams'] }]}
        onConnect={vi.fn()}
        onContinue={vi.fn()}
        onSkip={vi.fn()}
        accounts={[microsoftAccount]}
        library={library}
      />
    ))

    const panel = screen.getByRole('region', { name: 'Tilkoblede kontoer' })
    expect(panel.textContent).toContain('Microsoft 365')
    expect(panel.textContent).toContain('ima.dacosta@aquatiq.com')
    const grants = screen.getByRole('list', { name: 'Tilganger gitt' })
    expect(Array.from(grants.querySelectorAll('li')).map((item) => item.textContent)).toEqual(['Outlook', 'Teams', 'SharePoint', 'OneDrive'])
    expect(panel.textContent).toContain('Velg hvilket SharePoint- eller OneDrive-bibliotek')
    expect(panel.textContent).toContain('Synkronisert')
    expect(panel.textContent).toContain('Venter på første synk')

    // The picker opens by itself for a connection that still needs a library
    // and preselects the first document library of the first site.
    await waitFor(() => expect(library.listDrives).toHaveBeenCalledWith('site-1'))
    const registerButton = await screen.findByRole('button', { name: 'Legg til bibliotek' })
    await waitFor(() => expect(registerButton.hasAttribute('disabled')).toBe(false))
    fireEvent.click(registerButton)

    await waitFor(() => expect(library.register).toHaveBeenCalledWith('conn_f019f69a', {
      kind: 'drive',
      siteId: 'site-1',
      siteWebUrl: 'https://aquatiq.sharepoint.com',
      driveId: 'drive-1',
      driveName: 'Dokumenter',
      driveType: 'documentLibrary',
    }))
  })

  it('offers re-authorization when integration-core says the connection needs refresh', () => {
    const onReconnect = vi.fn()
    render(() => (
      <ConnectStepContent
        connectedSources={[]}
        onConnect={vi.fn()}
        onContinue={vi.fn()}
        onSkip={vi.fn()}
        accounts={[{
          ...microsoftAccount,
          status: 'needs_refresh',
          nextStep: { kind: 'reconnect', message: 'Tilgangen må godkjennes på nytt før noe kan synkroniseres.' },
        }]}
        onReconnect={onReconnect}
      />
    ))

    fireEvent.click(screen.getByRole('button', { name: 'Godkjenn på nytt' }))
    expect(onReconnect).toHaveBeenCalledWith(expect.objectContaining({ connectionId: 'conn_f019f69a' }))
    expect(screen.queryByRole('group', { name: 'Velg SharePoint-bibliotek' })).toBeNull()
  })

  it('renders nothing about accounts before integration-core has answered', () => {
    render(() => (
      <ConnectStepContent connectedSources={[]} onConnect={vi.fn()} onContinue={vi.fn()} onSkip={vi.fn()} />
    ))
    expect(screen.queryByRole('region', { name: 'Tilkoblede kontoer' })).toBeNull()
  })
})

describe('ConnectStepVisual', () => {
  afterEach(() => cleanup())

  it('renders graph nodes without permanent summary cards', () => {
    render(() => (
      <ConnectStepVisual
        connectedSources={[
          {
            id: 'slack',
            label: 'Slack',
            status: 'connected',
            sources: ['messages', 'threads'],
            sourceCount: 2,
          },
          {
            id: 'microsoft365',
            label: 'Microsoft 365',
            status: 'connected',
            sources: ['teams', 'sharepoint', 'onedrive'],
            sourceCount: 3,
          },
        ]}
        organizationName="AQUATIQ AS"
        currentUserId="user-42"
        currentUserName="Ima Fernandes"
      />
    ))

    expect(screen.getByLabelText('Integration knowledge graph')).toBeTruthy()
    expect(screen.getByText('Slack')).toBeTruthy()
    expect(screen.getByText('Microsoft 365')).toBeTruthy()
    expect(screen.getByText('Ima Fernandes')).toBeTruthy()
    expect(screen.queryByText('coresystem.com + integrations')).toBeNull()
    expect(screen.queryByText('5 connected sources')).toBeNull()
  })

  it('keeps the graph empty until an integration is confirmed connected', () => {
    render(() => (
      <ConnectStepVisual
        connectedSources={[
          {
            id: 'slack',
            label: 'Slack',
            status: 'pending',
            sources: ['messages'],
          },
        ]}
        organizationName="AQUATIQ AS"
        currentUserId="user-42"
        currentUserName="Ima Fernandes"
      />
    ))

    expect(screen.getByLabelText('Integration graph with no connected sources')).toBeTruthy()
    expect(screen.getByText('No connected integrations. The graph is empty.')).toBeTruthy()
    expect(screen.queryByLabelText('Zoom in')).toBeNull()
    expect(screen.queryByText('Slack')).toBeNull()
    expect(screen.queryByText('AQUATIQ AS')).toBeNull()
    expect(screen.queryByText('Ima Fernandes')).toBeNull()
  })
})
