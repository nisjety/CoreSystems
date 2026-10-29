import { fireEvent, render, screen, cleanup } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { BusinessOutcomesPanel } from './BusinessOutcomesPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_n: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'a'.repeat(32)}`, site: 'https://triodelab.no/', revision: 8, quarter: 'Q4' as const }
describe('business outcome UI', () => {
 afterEach(() => {cleanup(); vi.unstubAllGlobals()})
 it('keeps production separate and only shows dev receipts after explicit environment selection', async () => {
  const fetcher = vi.fn().mockImplementation((url: string) => {
    const dev = url.includes('environment=development')
    return Promise.resolve(Response.json({ data: { websiteId: context.websiteId, websiteRevision: 8, site: context.site, quarter: 'Q4', environment: dev ? 'development' : 'production', source: 'leads_core', status: dev ? 'connected' : 'unavailable', ...(dev ? {} : { reason: 'not_connected' }), timeZone: 'Europe/Oslo', period: { startDate: '2026-10-01', endDate: '2026-10-28' }, metrics: dev ? { inquiries: 1, leads: 1, bookings: null } : null, lastReceivedAt: null, ...(dev ? { asOf: '2026-10-28T12:00:00Z' } : {}) } }))
  });vi.stubGlobal('fetch', fetcher);render(() => <BusinessOutcomesPanel {...context} />)
  await screen.findByText('Website form delivery is not connected for this environment.')
  expect(screen.queryByText(/Local development only/)).toBeNull()
  fireEvent.change(screen.getByRole('combobox', {name: 'Environment'}), { target: { value: 'development' } })
  await screen.findByText(/Local development only/); await screen.findAllByText('1')
  expect(screen.queryByText('Confirmed bookings')).toBeNull()
  expect(screen.queryByText('Booking system is not connected')).toBeNull()
  expect(screen.getAllByRole('article')).toHaveLength(2)
  expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
 })
})
