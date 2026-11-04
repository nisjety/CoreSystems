import { describe, expect, it } from 'vitest'

import {
  CRAWL_SITE_APPROVAL_PAGES,
  CRAWL_SITE_MAX_PAGES,
  actionRequiresApproval,
  crawlSiteEstimatedPages,
  crawlSiteRequiresApproval,
  getActionDescriptor,
} from './action-registry'

// E3.7.9a: knowledge.crawl_site needs approval above 50 estimated pages (owner decision 2).
// The static flag stays false; actionRequiresApproval is the input-aware check.

describe('knowledge.crawl_site approval (E3.7.9a)', () => {
  it('keeps the static flag false so the admission schema hash does not move', () => {
    expect(getActionDescriptor('knowledge.crawl_site')?.requiresApproval).toBe(false)
  })

  it('estimates a crawl at its bound, held to the ceiling, and an unbounded one at the ceiling', () => {
    expect(crawlSiteEstimatedPages(25)).toBe(25)
    expect(crawlSiteEstimatedPages(999_999)).toBe(CRAWL_SITE_MAX_PAGES)
    expect(crawlSiteEstimatedPages(undefined)).toBe(CRAWL_SITE_MAX_PAGES)
  })

  it('needs approval only above the threshold', () => {
    expect(CRAWL_SITE_APPROVAL_PAGES).toBe(50)
    expect(crawlSiteRequiresApproval({ url: 'https://a.example/', maxPages: 50 })).toBe(false)
    expect(crawlSiteRequiresApproval({ url: 'https://a.example/', maxPages: 51 })).toBe(true)
    expect(crawlSiteRequiresApproval({ url: 'https://a.example/' })).toBe(true)
  })

  it('treats an input the schema rejects as unbounded', () => {
    expect(crawlSiteRequiresApproval({ maxPages: 10 })).toBe(true)
    expect(crawlSiteRequiresApproval(null)).toBe(true)
  })

  it('asks the input-aware check for crawl_site', () => {
    expect(
      actionRequiresApproval('knowledge.crawl_site', { url: 'https://a.example/', maxPages: 10 }),
    ).toBe(false)
    expect(
      actionRequiresApproval('knowledge.crawl_site', { url: 'https://a.example/', maxPages: 60 }),
    ).toBe(true)
  })
})
