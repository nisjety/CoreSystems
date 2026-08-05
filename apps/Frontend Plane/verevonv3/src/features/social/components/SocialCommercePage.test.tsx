// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import SocialCommercePage from '@/features/social/components/SocialCommercePage'

function renderCommerce() {
  window.history.pushState(null, '', '/social/commerce')
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={() => <SocialCommercePage />} />
    </Router>
  ))
}

function waitForCommerce(assertion: () => void) {
  return waitFor(assertion, { timeout: 3000 })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('SocialCommercePage', () => {
  it('renders real per-provider metrics and Meta catalogs, then drills into products', async () => {
    stubCommerceFetch()

    renderCommerce()

    // Metrics reuse the shared MetricCard: label + formatted value.
    await waitForCommerce(() => expect(screen.getByText('Facebook · impressions')).toBeTruthy())
    expect(screen.getByText('1,234')).toBeTruthy()

    // Catalog card renders with its real name and offers a product drill-down.
    expect(screen.getByText('Spring Collection')).toBeTruthy()
    const viewProducts = screen.getByRole('button', { name: 'Vis produkter' })
    fireEvent.click(viewProducts)

    await waitForCommerce(() => expect(screen.getByText('Aquatiq Widget')).toBeTruthy())

    const fetchMock = vi.mocked(fetch)
    expect(
      fetchMock.mock.calls.some(([url, init]) =>
        String(url).endsWith('/api/v1/social/metrics') &&
        (init?.headers as Headers).get('x-verevon-org-id') === 'org_acme',
      ),
    ).toBe(true)
    expect(
      fetchMock.mock.calls.some(([url]) =>
        String(url).endsWith('/api/v1/social/catalogs/cat_1/products?accountId=soc_acct_1'),
      ),
    ).toBe(true)
  })

  it('shows an honest empty state when no metric snapshots exist', async () => {
    stubCommerceFetch({ metrics: [], catalogs: [] })

    renderCommerce()

    await waitForCommerce(() =>
      expect(screen.getByText(/Ingen statistikkbilder registrert ennå/)).toBeTruthy(),
    )
    expect(screen.getByText(/Ingen handelskataloger/)).toBeTruthy()
  })
})

function stubCommerceFetch(
  options: { metrics?: unknown[]; catalogs?: unknown[] } = {},
) {
  const metrics = options.metrics ?? [
    {
      accountId: 'soc_acct_1',
      connectionId: 'conn_1',
      providerKey: 'facebook',
      metricName: 'impressions',
      metricValue: 1234,
      dimensions: { campaign: 'launch' },
      snapshotDate: '2026-07-07T00:00:00Z',
    },
  ]
  const catalogs = options.catalogs ?? [
    {
      id: 'cat_1',
      name: 'Spring Collection',
      account_id: 'soc_acct_1',
      provider_key: 'facebook',
      product_count: 12,
    },
  ]

  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({
          user: { id: 'user_acme', email: 'team@acme.test', name: 'Acme Team', emailVerified: true },
        })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({
          userId: 'user_acme',
          email: 'team@acme.test',
          name: 'Acme Team',
          orgId: 'org_acme',
          role: 'owner',
          orgs: [{ id: 'org_acme', name: 'Acme', role: 'owner' }],
        })
      }
      if (url.endsWith('/api/v1/social/metrics')) return jsonResponse({ metrics })
      if (url.endsWith('/api/v1/social/catalogs')) return jsonResponse({ catalogs })
      if (url.endsWith('/api/v1/social/catalogs/cat_1/products?accountId=soc_acct_1')) {
        return jsonResponse({
          products: [{ id: 'prod_1', name: 'Aquatiq Widget', retailer_id: 'sku-1', availability: 'in stock' }],
        })
      }
      return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
    }),
  )
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
