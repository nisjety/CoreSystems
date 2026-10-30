import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadWebsitePreparation, loadSourceCatalog, prepareWebsite, sanitizeWebsitePreparation, sanitizeSourceCatalog, sourceKeys } from './preparation-client'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3 }
const steps = ['discovery', 'pagespeed_mobile', 'pagespeed_desktop'].map((key) => ({ key, status: 'queued' }))
const idle = { ...context, source: 'marketing_preparation', status: 'idle', steps }
const queued = { ...idle, status: 'queued', jobId: `prep_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const running = { ...queued, status: 'running', startedAt: '2026-10-23T10:00:01Z' }
const complete = { ...running, status: 'complete', completedAt: '2026-10-23T10:01:00Z', steps: steps.map((step) => ({ ...step, status: 'complete', observedAt: '2026-10-23T10:00:59.123456789Z' })) }

describe('website preparation boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('rebuilds idle and completed receipts without upstream or secret fields', () => {
    expect(sanitizeWebsitePreparation({ ...idle, token: 'private' }, context)).toEqual(idle)
    expect(sanitizeWebsitePreparation({ ...complete, raw: { token: 'private' } }, context)).toEqual(complete)
  })
  it.each([
    { ...idle, site: 'https://agenci.no/' }, { ...idle, websiteRevision: 4 },
    { ...idle, steps: [steps[0], steps[0], steps[2]] }, { ...idle, steps: steps.slice(0, 2) },
    { ...queued, jobId: 'anything' }, { ...running, startedAt: '2026-10-22T10:00:00Z' },
    { ...running, steps: steps.map((item) => ({ ...item, status: 'running' })) },
    { ...complete, completedAt: '2026-02-30T10:01:00Z' }, { ...complete, steps },
    { ...complete, steps: complete.steps.map((step) => ({ ...step, observedAt: '2026-10-23T10:01:01Z' })) },
    { ...complete, steps: complete.steps.map((step) => ({ ...step, reason: 'secret_failure' })) },
  ])('rejects invalid context, lifecycle or closed fields %#', (value) => {
    expect(() => sanitizeWebsitePreparation(value, context)).toThrow()
  })
  it('accepts an explicit partial receipt and rejects a false success', () => {
    const partial = { ...complete, status: 'partial', steps: [complete.steps[0], complete.steps[1], { key: 'pagespeed_desktop', status: 'unavailable', reason: 'quota_limited', observedAt: '2026-10-23T10:00:59Z' }] }
    expect(sanitizeWebsitePreparation(partial, context).status).toBe('partial')
    expect(() => sanitizeWebsitePreparation({ ...partial, status: 'complete' }, context)).toThrow()
  })
  it('accepts recovered queued jobs and reports actual failed steps without replay assumptions', () => {
    const failedStep = { key: 'discovery', status: 'failed', reason: 'interrupted', observedAt: '2026-10-23T10:00:59Z' }
    const recovered = { ...queued, steps: [failedStep, steps[1], steps[2]] }
    expect(sanitizeWebsitePreparation(recovered, context)).toEqual(recovered)
    const failed = { ...complete, status: 'failed', steps: steps.map((item) => ({ ...failedStep, key: item.key })) }
    expect(sanitizeWebsitePreparation(failed, context)).toEqual(failed)
    expect(() => sanitizeWebsitePreparation({ ...failed, status: 'partial' }, context)).toThrow()
  })
  it('uses scoped read-only GET; only explicit prepare sends revision-bound POST', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: idle }))).mockResolvedValueOnce(new Response(JSON.stringify({ data: queued }), { status: 202 }))
    vi.stubGlobal('fetch', fetcher)
    await loadWebsitePreparation(context); await prepareWebsite(context)
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' })
    expect(new URL(fetcher.mock.calls[0]?.[0] ?? '', 'https://local.test').searchParams.get('websiteId')).toBe(context.websiteId)
    expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ method: 'POST', credentials: 'include', redirect: 'error', body: JSON.stringify(context) })
  })
  it('requires the expected HTTP status and does not expose provider errors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ token: 'private' }), { status: 500 })))
    await expect(loadWebsitePreparation(context)).rejects.toThrow('preparation_unavailable')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: queued }), { status: 200 })))
    await expect(prepareWebsite(context)).rejects.toThrow('preparation_unavailable')
  })
  it('accepts the exact catalogue and rejects duplicate, missing or misleading planned entries', () => {
    const catalog = { ...context, source: 'marketing_source_catalog', items: sourceKeys.map((key) => ({ key, implemented: false, mode: 'oauth', status: 'not_implemented', refresh: 'not_available' })) }
    expect(sanitizeSourceCatalog({ ...catalog, token: 'private' }, context)).toEqual(catalog)
    expect(() => sanitizeSourceCatalog({ ...catalog, items: catalog.items.slice(1) }, context)).toThrow()
    expect(() => sanitizeSourceCatalog({ ...catalog, items: catalog.items.map(() => catalog.items[0]) }, context)).toThrow()
    expect(() => sanitizeSourceCatalog({ ...catalog, items: catalog.items.map((item) => ({ ...item, status: 'connected' })) }, context)).toThrow()
  })
  it('accepts saved snapshots only for implemented Search Console, GA4 and Clarity sources', () => {
    const catalog = { ...context, source: 'marketing_source_catalog', items: sourceKeys.map((key) => ['search_console', 'ga4', 'clarity'].includes(key) ? { key, implemented: true, mode: 'oauth', status: 'connected', refresh: 'snapshot_read' } : { key, implemented: false, mode: 'oauth', status: 'not_implemented', refresh: 'not_available' }) }
    expect(sanitizeSourceCatalog(catalog, context).items.filter((item) => item.refresh === 'snapshot_read')).toHaveLength(3)
    expect(() => sanitizeSourceCatalog({ ...catalog, items: catalog.items.map((item) => item.key === 'bing_webmaster' ? { ...item, implemented: true, status: 'connected', refresh: 'snapshot_read' } : item) }, context)).toThrow()
  })
  it('accepts public signal jobs only for implemented CrUX and Common Crawl', () => {
    const catalog = { ...context, source: 'marketing_source_catalog', items: sourceKeys.map((key) => ['crux', 'common_crawl'].includes(key) ? { key, implemented: true, mode: 'public_check', status: 'available', refresh: 'public_signal_job' } : { key, implemented: false, mode: 'oauth', status: 'not_implemented', refresh: 'not_available' }) }
    expect(sanitizeSourceCatalog(catalog, context).items.filter((item) => item.refresh === 'public_signal_job')).toHaveLength(2)
    expect(() => sanitizeSourceCatalog({ ...catalog, items: catalog.items.map((item) => item.key === 'google_trends' ? { ...item, implemented: true, status: 'available', refresh: 'public_signal_job' } : item) }, context)).toThrow()
  })
  it('reads the catalogue with cookie-scoped GET and strips extra provider fields', async () => {
    const catalog = { ...context, source: 'marketing_source_catalog', items: sourceKeys.map((key) => ({ key, implemented: true, mode: 'public_check', status: 'available', refresh: 'preparation_job', token: 'private' })) }
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: catalog })))
    vi.stubGlobal('fetch', fetcher)
    expect(JSON.stringify(await loadSourceCatalog(context))).not.toContain('private')
    expect(fetcher.mock.calls[0]?.[0]).toContain('/api/v1/visibility/sources?')
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' })
  })
})
