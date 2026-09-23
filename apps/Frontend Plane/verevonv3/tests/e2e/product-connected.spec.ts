import { expect, test } from '@playwright/test'
import { openFresh, send } from './product-acceptance-helpers'

test('Q07 authenticated read contracts and quote provenance (not business-data sign-off)', async ({ page }, info) => {
  await openFresh(page)
  const baseline: Record<string, unknown> = {}
  for (const path of ['/api/v1/mcp/servers', '/api/v1/inbox/inboxes', '/api/v1/inbox/conversations?limit=1', '/api/v1/knowledge/documents?limit=20', '/api/v1/shipping/carriers']) {
    const response = await page.request.get(path)
    expect(response.status(), path).toBe(200)
    baseline[path] = await response.json()
  }
  await info.attach('connected-read-baseline', { body: JSON.stringify(baseline, null, 2), contentType: 'application/json' })
  // Explicitly a generic ONE-parcel contract probe, not a quote for the
  // five-parcel Aquatiq order or evidence that its contents are eligible.
  const shipment = {
    from: { name: 'Q07 test sender', postal_code: '0150', city: 'Oslo', country: 'NO', is_business: true },
    to: { name: 'Q07 test recipient', postal_code: '7010', city: 'Trondheim', country: 'NO', is_business: true },
    package: { weight_kg: 5, length_cm: 30, width_cm: 20, height_cm: 15, dangerous_good: false },
    segment: 'b2b',
  }
  const response = await page.request.post('/api/v1/shipping/quotes', { data: shipment })
  expect(response.status()).toBe(200)
  const result = await response.json()
  const quotes = result.quotes ?? result.data?.quotes ?? []
  expect(quotes.length, 'at least one real response is needed to verify per-quote provenance').toBeGreaterThan(0)
  for (const quote of quotes) {
    expect(['production', 'sandbox', 'mock', 'unknown']).toContain(quote.environment)
    expect(quote.is_mock).toBe(quote.environment === 'mock')
    expect(Number.isNaN(Date.parse(quote.quoted_at))).toBe(false)
    expect(quote.package_count).toBe(1)
  }
  await info.attach('one-parcel-quote-probe', { body: JSON.stringify({ shipment, result }, null, 2), contentType: 'application/json' })
  const multiple = await page.request.post('/api/v1/shipping/quotes', { data: { ...shipment, packages: [shipment.package, shipment.package] } })
  expect(multiple.status()).toBe(400)
  const inboxResponse = page.waitForResponse(r => r.url().endsWith('/api/v1/chat/stream') && r.request().method() === 'POST')
  const answer = await send(page, 'Bruk inbox_search til å sjekke om arbeidsområdet har noen kundesamtaler nå. Hvis det er tomt, si det tydelig. Ikke lag en eksempelhenvendelse og ikke send noe.', info, 'inbox-read')
  expect(await (await inboxResponse).text()).toMatch(/"name":"inbox_search"/)
  await info.attach('inbox-read-transcript', { body: JSON.stringify(answer, null, 2), contentType: 'application/json' })
  await page.screenshot({ path: info.outputPath('inbox-read.png'), fullPage: true })
})
