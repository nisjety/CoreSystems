// @vitest-environment jsdom
import { cleanup, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ProviderOverview } from './ProviderOverview'
import type { GA4Snapshot } from '@/shared/api/marketing/ga4-client'
import type { ClaritySnapshot } from '@/shared/api/marketing/clarity-client'
import type { PulseServiceKey } from './ConnectedServices'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const metrics = { users: 3, sessions: 6, pageViews: 20, engagedSessions: 5, keyEvents: 0, engagementSeconds: 92 }
const ga4: GA4Snapshot = { site: 'https://triodelab.no/', source: 'google_analytics', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-03' }, totals: metrics, days: [{ ...metrics, date: '2026-10-01', sessions: 0 }, { ...metrics, date: '2026-10-03', sessions: 6 }] }
const clarity: ClaritySnapshot = { site: ga4.site, source: 'microsoft_clarity', status: 'connected', scope: 'clarity_project', window: { numOfDays: 3, timeZone: 'UTC' }, metrics: { sessions: 22, users: 20, botSessions: 0, pagesPerSession: 1.09 } }
afterEach(cleanup)
describe('selected provider overview', () => {
  it('uses provider period totals and preserves measured zero versus missing days', () => {
    const view = render(() => <ProviderOverview provider="ga4" ga4={ga4} clarity={null} pageSpeed={null}/>)
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('6')
    expect(screen.getAllByTestId('provider-metric-value').map(item => item.textContent)).toEqual(['3', '6', '0'])
    expect(view.container.querySelector('[data-date="2026-10-01"]')?.hasAttribute('data-measured')).toBe(true)
    expect(view.container.querySelector('[data-date="2026-10-02"]')?.hasAttribute('data-measured')).toBe(false)
    expect(view.container.querySelectorAll('[data-measured]')).toHaveLength(2)
  })
  it('changes metrics and window with the tab, without inventing a Clarity daily series', () => {
    const [provider, setProvider] = createSignal<PulseServiceKey>('ga4')
    render(() => <ProviderOverview provider={provider()} ga4={ga4} clarity={clarity} pageSpeed={null}/>)
    setProvider('clarity')
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('22')
    expect(screen.getAllByTestId('provider-metric-value').map(item => item.textContent)).toEqual(['22', '20', '1.09'])
    expect(screen.queryByRole('img')).toBeNull()
    expect(screen.getByText(/not a daily time series/)).toBeTruthy()
  })
  it('clears values for an unavailable source rather than reusing another provider’s data', () => {
    render(() => <ProviderOverview provider="website" ga4={ga4} clarity={clarity} pageSpeed={null}/>)
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(screen.getAllByTestId('provider-metric-value').map(item => item.textContent)).toEqual(['—', '—', '—'])
    expect(screen.getByText('No saved lab measurement')).toBeTruthy()
  })
})
