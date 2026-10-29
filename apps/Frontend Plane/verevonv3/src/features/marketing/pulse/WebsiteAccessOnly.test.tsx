import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, expect, it, vi } from 'vitest'
import { WebsiteManager } from './WebsiteManager'
import { sanitizeWebsite } from '@/shared/api/marketing/website-client'
import { marketingWebsiteContracts } from '@/shared/actions/marketing-websites'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_: string,en: string) => en }) }))
vi.mock('./WebsitePreparationPanel', () => ({ WebsitePreparationPanel: () => null }))
afterEach(cleanup)
it('uses provider access rather than Studio DNS ownership to expose scoped GA4 host controls', () => {
  const site = sanitizeWebsite({ id: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', revision: 1, createdAt: '2026-10-23T10:00:00Z', updatedAt: '2026-10-23T10:00:00Z', ownership: { status: 'unverified' }, integrations: { searchConsole: { status: 'connected', accessLevel: 'siteFullUser' }, ga4: { status: 'connected', propertyId: '123', reportingSite: 'https://www.triodelab.no/' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } } })
  render(() => <WebsiteManager websites={[site]} selected={site} onSelect={() => {}} onUpdated={() => {}} onClose={() => {}} />)
  expect(screen.queryByText(/DNS/)).toBeNull(); expect(screen.queryByTestId('website-ownership')).toBeNull()
  expect(screen.getByLabelText('GA4 reporting host')).toBeTruthy()
  expect(Object.keys(marketingWebsiteContracts)).not.toContain('ownershipChallenge'); expect(Object.keys(marketingWebsiteContracts)).not.toContain('ownershipVerify')
})
