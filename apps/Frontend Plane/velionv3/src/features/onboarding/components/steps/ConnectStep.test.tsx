// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
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

    expect(screen.queryByText('Slack')).toBeNull()
    // Facebook/Instagram/WhatsApp/Meta Ads consolidate into ONE Meta card.
    expect(screen.getByText('Meta')).toBeTruthy()
    expect(screen.queryByText('Instagram')).toBeNull()
    expect(screen.getByText('LinkedIn')).toBeTruthy()
    expect(screen.getByText('TikTok')).toBeTruthy()
    expect(screen.getByText('Snapchat')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /^Meta/i }))

    expect(onConnect).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'meta',
        provider: 'meta',
        category: 'social',
      }),
    )
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
    expect(screen.queryByText('aquatiq.com + integrations')).toBeNull()
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
