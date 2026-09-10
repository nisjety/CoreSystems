import { describe, expect, it } from 'vitest'
import type { CrawlSnippet } from '@/features/onboarding/lib/api'
import {
  dedupeCrawlSnippets,
  isRicherCrawlSnippet,
  mergeCrawlSnippet,
  snippetQuality,
} from '@/features/onboarding/lib/crawl-preview'

const bare: CrawlSnippet = {
  id: 'https://aquatiq.com/',
  kind: 'text',
  title: 'aquatiq.com',
  url: 'https://aquatiq.com/',
  titleSource: 'host',
  source: 'seed',
}

const rich: CrawlSnippet = {
  id: 'https://aquatiq.com/',
  kind: 'text',
  title: 'Aquatiq – hygiene for matindustrien',
  titleSource: 'model',
  excerpt: 'Vi leverer hygieneløsninger, kjemikalier og kompetanse til næringsmiddelindustrien.',
  summary: 'Leverandør av hygieneløsninger.',
  wordCount: 412,
  url: 'https://aquatiq.com/',
  source: 'live',
}

const other: CrawlSnippet = {
  id: 'https://aquatiq.com/om-oss',
  kind: 'text',
  title: 'Om oss',
  titleSource: 'html',
  excerpt: 'Aquatiq ble etablert i 1990.',
  url: 'https://aquatiq.com/om-oss',
  source: 'live',
}

describe('snippetQuality', () => {
  it('ranks text above a specific title above a bare host card', () => {
    expect(snippetQuality(bare)).toBe(0)
    expect(snippetQuality({ ...bare, title: 'Real', titleSource: 'html' })).toBe(1)
    expect(snippetQuality({ ...bare, excerpt: 'text' })).toBe(2)
    expect(snippetQuality(rich)).toBe(3)
  })

  it('treats whitespace-only excerpts as no text', () => {
    expect(snippetQuality({ ...bare, excerpt: '   ' })).toBe(0)
  })
})

describe('mergeCrawlSnippet', () => {
  it('appends a snippet for a URL it has not seen', () => {
    expect(mergeCrawlSnippet([], bare)).toEqual([bare])
    expect(mergeCrawlSnippet([bare], other)).toEqual([bare, other])
  })

  it('updates the existing card in place when a richer snippet for the same page arrives', () => {
    const merged = mergeCrawlSnippet([bare, other], rich)
    expect(merged).toHaveLength(2)
    expect(merged[0]).toEqual(rich)
    expect(merged[1]).toEqual(other)
  })

  it('matches by URL when ids differ (seed vs live emit different ids)', () => {
    const liveRich = { ...rich, id: 'page-42', url: 'https://aquatiq.com' }
    const merged = mergeCrawlSnippet([bare], liveRich)
    expect(merged).toHaveLength(1)
    expect(merged[0]?.title).toBe(rich.title)
  })

  it('keeps the richer card when a poorer duplicate arrives later', () => {
    const merged = mergeCrawlSnippet([rich], bare)
    expect(merged).toEqual([rich])
  })

  it('caps the list at the limit, dropping the oldest', () => {
    const many = Array.from({ length: 14 }, (_, i) => ({
      ...other,
      id: `https://aquatiq.com/p${i}`,
      url: `https://aquatiq.com/p${i}`,
    }))
    const merged = many.reduce<CrawlSnippet[]>((acc, s) => mergeCrawlSnippet(acc, s, 12), [])
    expect(merged).toHaveLength(12)
    expect(merged[0]?.url).toBe('https://aquatiq.com/p2')
  })
})

describe('dedupeCrawlSnippets', () => {
  it('collapses appended duplicates to one card per page, keeping the richest', () => {
    const appended = [bare, other, rich, { ...bare, source: 'live' as const }]
    const deduped = dedupeCrawlSnippets(appended)
    expect(deduped).toHaveLength(2)
    expect(deduped.find((s) => s.url === 'https://aquatiq.com/')).toEqual(rich)
    expect(deduped.find((s) => s.url === 'https://aquatiq.com/om-oss')).toEqual(other)
  })

  it('preserves first-seen order', () => {
    const deduped = dedupeCrawlSnippets([other, bare, rich])
    expect(deduped.map((s) => s.url)).toEqual(['https://aquatiq.com/om-oss', 'https://aquatiq.com/'])
  })
})

describe('isRicherCrawlSnippet', () => {
  it('is strict', () => {
    expect(isRicherCrawlSnippet(rich, bare)).toBe(true)
    expect(isRicherCrawlSnippet(bare, rich)).toBe(false)
    expect(isRicherCrawlSnippet(rich, rich)).toBe(false)
  })
})
