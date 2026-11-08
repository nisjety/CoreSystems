import { describe, expect, it } from 'vitest'
import {
  CRAWL_APPROVAL_PAGES_DEFAULT,
  crawlPageCount,
  crawlPriceRangeText,
  crawlTargetSummary,
  isCrawlPriceBand,
  isDecidableApprovalId,
} from './CrawlApprovalPrompt'

describe('crawl approval prompt helpers (W3-E3.7.9b)', () => {
  it('uses the decided threshold of 50 pages by default', () => {
    expect(CRAWL_APPROVAL_PAGES_DEFAULT).toBe(50)
  })

  it('reads the page count as a whole, non-negative number', () => {
    expect(crawlPageCount(120.7)).toBe(120)
    expect(crawlPageCount(-3)).toBe(0)
    expect(crawlPageCount(Number.NaN)).toBe(0)
  })

  it('accepts only a well-formed price band', () => {
    expect(isCrawlPriceBand({ lowMicroUsd: 10_000, highMicroUsd: 25_000 })).toBe(true)
    expect(isCrawlPriceBand({ lowMicroUsd: 25_000, highMicroUsd: 10_000 })).toBe(false)
    expect(isCrawlPriceBand({ lowMicroUsd: -1, highMicroUsd: 10 })).toBe(false)
    expect(isCrawlPriceBand(null)).toBe(false)
    expect(isCrawlPriceBand(undefined)).toBe(false)
  })

  it('shows one amount for a flat band and a range otherwise', () => {
    expect(crawlPriceRangeText({ lowMicroUsd: 1_500_000, highMicroUsd: 1_500_000 }, 'en-US')).toBe('$1.50')
    expect(crawlPriceRangeText({ lowMicroUsd: 1_000_000, highMicroUsd: 2_000_000 }, 'en-US')).toBe('$1.00 – $2.00')
  })

  it('needs an approval id without whitespace', () => {
    expect(isDecidableApprovalId('appr_1')).toBe(true)
    expect(isDecidableApprovalId('  ')).toBe(false)
    expect(isDecidableApprovalId('appr 1')).toBe(false)
  })

  it('drops the query and fragment of the seed URL', () => {
    const summary = crawlTargetSummary('https://example.com/docs/start?token=secret#top')
    expect(summary).toContain('example.com')
    expect(summary).not.toContain('secret')
    expect(summary).not.toContain('#top')
  })
})
