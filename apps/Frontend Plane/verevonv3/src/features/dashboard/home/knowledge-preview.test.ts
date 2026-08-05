import { describe, expect, it } from 'vitest'
import {
  normalizeUrl,
  splitMarkdownBlocks,
  toScrapePreview,
} from './knowledge-preview'
import type { ScrapePreviewResult, ScrapeResult } from '@/shared/api/knowledge-client'

describe('knowledge preview parsing', () => {
  it('builds selectable blocks from the normalized gateway preview', () => {
    const result: ScrapePreviewResult = {
      description: 'Siste nytt fra Norge og verden.',
      markdown: '# VG\n\nSiste nytt hvert minutt.\n\nSport og underholdning.',
      quarry: {
        fingerprint: 'blake3:abc',
        markdownArtifactId: 'artifact-1',
        runId: 'run-1',
        status: 200,
      },
      source: 'extract',
      title: 'VG',
      url: 'https://www.vg.no/',
    }

    const preview = toScrapePreview('https://vg.no/', result)

    expect(preview.title).toBe('VG')
    expect(preview.description).toBe('Siste nytt fra Norge og verden.')
    expect(preview.source).toBe('extract')
    expect(preview.blocks).toHaveLength(3)
    expect(preview.blocks[0]).toMatchObject({ heading: true, text: 'VG' })
    expect(preview.blocks[1]?.text).toBe('Siste nytt hvert minutt.')
  })

  it('falls back to readable text when a legacy payload only has html', () => {
    const result: ScrapeResult = {
      data: {
        html: '<main><h1>News</h1><p>First paragraph.</p><p>Second paragraph.</p></main>',
        metadata: { title: 'Example news' },
      },
    }
    const preview = toScrapePreview('https://example.com/news', result)

    expect(preview.title).toBe('Example news')
    expect(preview.blocks.map((block) => block.text)).toEqual(['News', 'First paragraph.', 'Second paragraph.'])
  })

  it('keeps artifact-only metadata without inventing selected text', () => {
    const result: ScrapeResult = {
      data: {
        formats: {
          markdown: { artifact_id: 'artifact-1', bytes: 1024 },
        },
        metadata: {
          description: 'Nyheter fra Norges mest leste nettavis.',
          title: 'VG',
        },
        url: {
          final_url: 'https://www.vg.no/',
          requested: 'https://vg.no/',
        },
      },
    }
    const preview = toScrapePreview('https://vg.no/', result)

    expect(preview.title).toBe('VG')
    expect(preview.url).toBe('https://www.vg.no/')
    expect(preview.blocks).toHaveLength(0)
  })

  it('normalizes bare public domains for scraping', () => {
    expect(normalizeUrl('vg.no')).toBe('https://vg.no')
    expect(normalizeUrl('https://vg.no/nyheter')).toBe('https://vg.no/nyheter')
    expect(normalizeUrl('hello')).toBeNull()
  })

  it('splits single-newline markdown into selectable blocks', () => {
    expect(splitMarkdownBlocks('# Title\nFirst line\nSecond line').map((block) => block.text)).toEqual([
      'Title',
      'First line',
      'Second line',
    ])
  })

  it('rejoins multi-line links into one clean block instead of shredding them', () => {
    // quarry/html2md emits teaser-card links with the label split across blank
    // lines — the exact vg.no shape that used to shatter into `[`, the text,
    // and `](url)` as three separate junk blocks.
    const markdown = [
      'Er du sikker på at du vil logge ut? Logg ut Avbryt',
      '[',
      'Se nå: I gang ----------',
      '](https://www.vg.no/stories/366684/vg-lista-topp-40?playlistId=5udeqUMotsPGl37D1ofzZxWA)',
      '----------',
      'Siste nytt hvert minutt på Norges største nettsted.',
    ].join('\n\n')

    const blocks = splitMarkdownBlocks(markdown)
    const texts = blocks.map((block) => block.text)

    expect(texts).toEqual([
      'Er du sikker på at du vil logge ut? Logg ut Avbryt',
      'Se nå: I gang',
      'Siste nytt hvert minutt på Norges største nettsted.',
    ])
    expect(texts).not.toContain('[')
    expect(texts).not.toContain('')
    // The rejoined link keeps its URL so the renderer shows a real anchor.
    expect(blocks[1]?.raw).toContain('https://www.vg.no/stories/366684/')
  })

  it('drops structural-noise blocks (stray brackets, rule dividers)', () => {
    const markdown = ['[', '----------', ']', '| · •', 'Real content here.'].join('\n\n')
    expect(splitMarkdownBlocks(markdown).map((block) => block.text)).toEqual(['Real content here.'])
  })
})
