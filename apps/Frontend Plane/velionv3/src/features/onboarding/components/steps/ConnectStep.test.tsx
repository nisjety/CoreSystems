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
        graphNodes={[
          {
            id: 'org',
            label: 'AQUATIQ AS',
            group: 'org',
            position: { left: '50%', top: '50%' },
          },
          {
            id: 'knowledge-1',
            label: 'Cleaning systems and chemical product catalog',
            group: 'knowledge',
            position: { left: '64%', top: '44%' },
          },
        ]}
        organizationName="AQUATIQ AS"
        websiteUrl="https://aquatiq.com"
      />
    ))

    expect(screen.getByLabelText('Integration knowledge graph')).toBeTruthy()
    expect(screen.getByText('Slack')).toBeTruthy()
    expect(screen.getByText('Microsoft 365')).toBeTruthy()
    expect(screen.queryByText('aquatiq.com + integrations')).toBeNull()
    expect(screen.queryByText('5 connected sources')).toBeNull()
  })
})
