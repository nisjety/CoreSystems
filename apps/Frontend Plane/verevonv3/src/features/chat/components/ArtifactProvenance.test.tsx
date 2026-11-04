// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { ArtifactProvenance, researchPassFacts } from './ArtifactProvenance'

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

describe('ArtifactProvenance second pass (EP-2-S6)', () => {
  const twoPass = () => stamp({
    passes: 2,
    claims_by_status: { supported: 3, single_source: 1, conflict: 1, unsupported: 2 },
    sources: [
      { number: 1, url: 'https://example.no/a', title: 'Fôrpriser', pass: 1 },
      { number: 2, url: 'https://example.no/b', title: 'Gap', pass: 2 },
    ],
  })

  it('shows the passes and the claim counts by status', () => {
    const { container } = render(() => <ArtifactProvenance provenance={twoPass()} />)
    expect(container.querySelector('.verevon-chat-artifact-provenance__passes')?.textContent).toContain('To runder')
    const claims = container.querySelector('.verevon-chat-artifact-provenance__claims')?.textContent ?? ''
    expect(claims).toContain('3 støttet')
    expect(claims).toContain('1 fra én kilde')
    expect(claims).toContain('1 motstridende')
    expect(claims).toContain('2 ikke verifisert')
  })

  it('shows no pass or claim line on a stamp without them', () => {
    const { container } = render(() => <ArtifactProvenance provenance={stamp()} />)
    expect(container.querySelector('.verevon-chat-artifact-provenance__passes')).toBeNull()
    expect(container.querySelector('.verevon-chat-artifact-provenance__claims')).toBeNull()
  })

  it('reads the second pass numbers and drops a partial tally', () => {
    expect(researchPassFacts(twoPass())).toEqual({
      passes: 2,
      claims: { supported: 3, single_source: 1, conflict: 1, unsupported: 2 },
      secondPassNumbers: [2],
    })
    expect(researchPassFacts(stamp())).toBeUndefined()
    expect(researchPassFacts(null)).toBeUndefined()
    const partial = researchPassFacts(stamp({ passes: 1, claims_by_status: { supported: 1 } }))
    expect(partial).toEqual({ passes: 1, claims: undefined, secondPassNumbers: [] })
  })
})
