// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ConnectStepContent } from '@/features/onboarding/components/steps/ConnectStep'

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
    expect(screen.queryByText('Instagram')).toBeNull()

    fireEvent.click(screen.getByRole('tab', { name: /Social channels/i }))

    expect(screen.queryByText('Slack')).toBeNull()
    expect(screen.getByText('Instagram')).toBeTruthy()
    expect(screen.getByText('LinkedIn')).toBeTruthy()
    expect(screen.getByText('TikTok')).toBeTruthy()
    expect(screen.getByText('Snapchat')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /Instagram/i }))

    expect(onConnect).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'instagram',
        provider: 'instagram',
        category: 'social',
      }),
    )
  })
})
