// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { WebsiteStepVisual, snippetPathLabel, visibleCrawlPages } from '@/features/onboarding/components/steps/WebsiteStep'
import type { CrawlSnippet } from '@/features/onboarding/lib/api'
import type { OnboardingState } from '@/features/onboarding/lib/model'

/**
 * Snippets exactly as the gateway streamed them for a real 4-page crawl of
 * aquatiq.com on 2026-09-04 (see the gateway's `stream_e2e` SSE dump). Quarry
 * sends two frames per page — the pre-transform `page_fetched` placeholder
 * and the post-transform `page_extracted` — and `OnboardingPage` appends
 * both, so the card list must collapse them itself.
 */
const streamed: CrawlSnippet[] = [
  // Seed: pre-transform frame carries neither title nor text.
  {
    id: 'https://www.aquatiq.com/no',
    kind: 'text',
    title: 'www.aquatiq.com',
    titleSource: 'host',
    url: 'https://www.aquatiq.com/no',
    source: 'seed',
  },
  // Seed: post-transform frame carries the real title and excerpt.
  {
    id: 'https://www.aquatiq.com/no',
    kind: 'text',
    title: 'Aquatiq - Global leder på Trygg Mat ekspertise og…',
    titleSource: 'html',
    driver: 'static',
    wordCount: 651,
    excerpt:
      'Food Safety Experts Leverandør av kompetanse, rengjøringssystemer, kjemi og hygieniske prosessløsninger til den globale næringsmiddelindustrien.',
    url: 'https://www.aquatiq.com/no',
    source: 'seed',
  },
  {
    id: 'https://www.aquatiq.com/no/chemistry',
    kind: 'text',
    title: 'Kjemiske løsninger for mat, havbruk og industri | Aquatiq',
    titleSource: 'html',
    driver: 'static',
    wordCount: 1025,
    excerpt:
      'Chemistry Services Spesialisert kjemi for matindustrien, transportsektoren og tungindustrien.',
    url: 'https://www.aquatiq.com/no/chemistry',
    source: 'live',
  },
  {
    id: 'https://www.aquatiq.com/no/kurs-and-revisjon',
    kind: 'text',
    title: 'Kurs og revisjon innen mattrygghet og kvalitet | Aquatiq',
    titleSource: 'html',
    driver: 'static',
    wordCount: 427,
    excerpt:
      'Kurs og revisjon Tjenester Vi hjelper matindustrien med å forebygge og minimere risiko knyttet til mattrygghet.',
    url: 'https://www.aquatiq.com/no/kurs-and-revisjon',
    source: 'live',
  },
]

function website(overrides: Partial<OnboardingState['website']> = {}): OnboardingState['website'] {
  return {
    url: 'https://aquatiq.com',
    brief: '',
    snippets: [],
    pages: 0,
    elements: 0,
    status: 'running',
    ...overrides,
  }
}

describe('WebsiteStepVisual', () => {
  it('shows the real page titles and excerpts instead of a bare counter', () => {
    render(() => <WebsiteStepVisual website={website({ snippets: streamed, pages: 4 })} />)

    expect(screen.getByText('Kjemiske løsninger for mat, havbruk og industri | Aquatiq')).toBeTruthy()
    expect(screen.getByText(/Spesialisert kjemi for matindustrien/)).toBeTruthy()
    expect(screen.getByText('Kurs og revisjon innen mattrygghet og kvalitet | Aquatiq')).toBeTruthy()
    expect(screen.getByText(/forebygge og minimere risiko/)).toBeTruthy()
    expect(screen.getByText(/Food Safety Experts/)).toBeTruthy()
    // Word counts make the card feel like real captured knowledge.
    expect(screen.getByText('1025 ord')).toBeTruthy()
  })

  it('collapses the placeholder frame so no host-label card is left on screen', () => {
    render(() => <WebsiteStepVisual website={website({ snippets: streamed, pages: 4 })} />)

    expect(screen.queryByText('www.aquatiq.com')).toBeNull()
    expect(screen.getByRole('list', { name: 'Sider funnet på nettstedet' }).children).toHaveLength(3)
  })

  it('counts pages, not appended frames', () => {
    render(() => <WebsiteStepVisual website={website({ snippets: streamed, pages: 4 })} />)

    // Four streamed snippets, three distinct pages.
    expect(screen.getByText('3 sider lest')).toBeTruthy()
  })

  it('reports progress before the first page arrives', () => {
    render(() => <WebsiteStepVisual website={website({ status: 'starting' })} />)

    expect(screen.getByText('Leser nettsiden …')).toBeTruthy()
  })

  it('marks a page whose text has not arrived yet instead of showing it as empty', () => {
    const pending: CrawlSnippet = {
      id: 'https://www.aquatiq.com/no/kontakt',
      kind: 'text',
      title: 'Kontakt oss | Aquatiq',
      titleSource: 'html',
      url: 'https://www.aquatiq.com/no/kontakt',
      source: 'live',
    }
    render(() => <WebsiteStepVisual website={website({ snippets: [pending] })} />)

    expect(screen.getByText('Kontakt oss | Aquatiq')).toBeTruthy()
    expect(screen.getByText('Henter tekst …')).toBeTruthy()
  })

  it('badges a page whose name the model proposed', () => {
    const modelNamed: CrawlSnippet = {
      id: 'https://example.test/p',
      kind: 'text',
      title: 'Hygieneløsninger for matindustrien',
      titleSource: 'model',
      excerpt: 'Vi leverer kompetanse og systemer til næringsmiddelindustrien.',
      url: 'https://example.test/p',
      source: 'live',
    }
    render(() => <WebsiteStepVisual website={website({ snippets: [modelNamed] })} />)

    expect(screen.getByText('AI-navn')).toBeTruthy()
  })
})

describe('snippetPathLabel', () => {
  it('shows the host for a root page and host + path otherwise', () => {
    expect(snippetPathLabel({ url: 'https://www.aquatiq.com/' })).toBe('aquatiq.com')
    expect(snippetPathLabel({ url: 'https://www.aquatiq.com/no/chemistry' })).toBe('aquatiq.com/no/chemistry')
    expect(snippetPathLabel({ url: 'not a url' })).toBe('not a url')
  })
})

describe('visibleCrawlPages', () => {
  it('keeps the richest card per page, newest first, capped', () => {
    const pages = visibleCrawlPages(streamed, 2)
    expect(pages).toHaveLength(2)
    expect(pages[0]?.url).toBe('https://www.aquatiq.com/no/kurs-and-revisjon')
    expect(pages.every((page) => Boolean(page.excerpt))).toBe(true)
  })
})

describe('WebsiteStep.css', () => {
  const css = readFileSync(
    `${process.cwd()}/src/features/onboarding/components/steps/WebsiteStep.css`,
    'utf8',
  )

  // jsdom does not apply imported stylesheets, so the cascade cannot be
  // asserted by rendering. Observed live: `.onboarding-folder-card p` in
  // global.css styles the card's eyebrow label as 10px uppercase with 0.16em
  // tracking, and being class+type it outranks a bare class here — every page
  // excerpt rendered as tiny uppercase text.
  it('scopes the excerpt rule so the folder-card eyebrow style cannot win', () => {
    expect(css).toMatch(/\.onboarding-folder-card\s+\.onboarding-page-card__excerpt\s*\{/)
  })

  it('restores prose casing and tracking on the excerpt', () => {
    const rule = css.slice(
      css.indexOf('.onboarding-folder-card .onboarding-page-card__excerpt {'),
    )
    const body = rule.slice(0, rule.indexOf('}'))
    expect(body).toContain('text-transform: none')
    expect(body).toContain('letter-spacing: normal')
  })
})
