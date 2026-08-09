import { describe, expect, it } from 'vitest'
import { summarizeOnboardingSources } from '@/features/onboarding/lib/source-summary'

describe('summarizeOnboardingSources', () => {
  it('counts connector source streams separately from the website source', () => {
    const summary = summarizeOnboardingSources({
      websiteUrl: 'https://coresystem.com',
      connectors: [
        {
          id: 'microsoft365',
          label: 'Microsoft 365',
          status: 'connected',
          sources: ['teams', 'outlook', 'sharepoint', 'onedrive'],
        },
        {
          id: 'meta',
          label: 'Meta',
          status: 'connected',
          sources: ['pages', 'instagram_business', 'whatsapp', 'ads'],
        },
        {
          id: 'github',
          label: 'GitHub',
          status: 'connected',
          sources: ['issues'],
        },
      ],
    })

    expect(summary.connectorCount).toBe(3)
    expect(summary.connectedSourceCount).toBe(9)
    expect(summary.websiteSourceCount).toBe(1)
    expect(summary.totalSourceCount).toBe(10)
  })

  it('falls back to one source for older stored connector rows', () => {
    const summary = summarizeOnboardingSources({
      connectors: [{ id: 'slack', label: 'Slack', status: 'connected' }],
    })

    expect(summary.connectedSourceCount).toBe(1)
    expect(summary.totalSourceCount).toBe(1)
  })
})
