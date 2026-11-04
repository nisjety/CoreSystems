// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { ArtifactProvenance } from './ArtifactProvenance'

afterEach(cleanup)

/** A stamp in the wire shape the gateway streams and stores. */
const stamp = (overrides: Record<string, unknown> = {}) => ({
  question: 'Hva koster laksefôr i Norge?',
  sub_queries: ['laksefôr pris 2026', 'fôrkostnad oppdrett'],
  sources: [
    { number: 1, url: 'https://example.no/a', title: 'Fôrpriser' },
    { number: 2, url: 'javascript:alert(1)', title: 'Skadelig' },
  ],
  model: 'verevon-research',
  coverage: { found: 6, read: 2, unread: 4, deadline_hit: false },
  generated_at: '2026-11-04T10:20:30Z',
  pipeline: 'deep_research_v1',
  ...overrides,
})

describe('ArtifactProvenance', () => {
  it('states what the report was built from, with its model', () => {
    const { container } = render(() => <ArtifactProvenance provenance={stamp()} />)
    const summary = container.querySelector('.verevon-chat-artifact-provenance__summary')?.textContent ?? ''
    expect(summary).toContain('Bygget fra 2 av 6 kilder')
    expect(summary).toContain('verevon-research')
    expect(container.querySelectorAll('.verevon-chat-artifact-provenance__queries li')).toHaveLength(2)
    expect(container.querySelector('.verevon-chat-artifact-provenance__warning')).toBeNull()
  })

  it('links only a safe source address', () => {
    const { container } = render(() => <ArtifactProvenance provenance={stamp()} />)
    const links = Array.from(container.querySelectorAll('.verevon-chat-artifact-provenance__sources a'))
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['https://example.no/a'])
    expect(container.textContent).toContain('Skadelig')
  })

  it('says when the time ran out', () => {
    const { container } = render(() => (
      <ArtifactProvenance provenance={stamp({ coverage: { found: 6, read: 2, unread: 4, deadline_hit: true } })} />
    ))
    expect(container.querySelector('.verevon-chat-artifact-provenance__warning')).not.toBeNull()
  })

  it('renders nothing without a complete stamp, so a report without one is unchanged', () => {
    for (const provenance of [undefined, null, {}, stamp({ coverage: undefined }), stamp({ pipeline: '' })]) {
      const { container, unmount } = render(() => <ArtifactProvenance provenance={provenance} />)
      expect(container.innerHTML).toBe('')
      unmount()
    }
  })
})
