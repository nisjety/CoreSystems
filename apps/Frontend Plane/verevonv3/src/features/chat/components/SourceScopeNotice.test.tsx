// @vitest-environment jsdom

import { render } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { SearchProvenanceNotice, SourceScopeNotice } from './ChatMessages'

/** chat-wave2 scope spec §2.2 and WS18-S3: the source filter note and the Space knowledge chip. */
describe('SourceScopeNotice', () => {
  it('shows the Space knowledge chip with its source count, and no filter note for an auto turn', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="auto" excludedTools={[]} knowledge={{ mode: 'retrieval', sources: 3 }} />
    ))
    expect(container.textContent).toBe('Prosjektkunnskap: 3 kilder')
    unmount()
  })

  it('uses the singular for one source', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="auto" excludedTools={[]} knowledge={{ mode: 'retrieval', sources: 1 }} />
    ))
    expect(container.textContent).toBe('Prosjektkunnskap: 1 kilde')
    unmount()
  })

  it('says the Space knowledge was searched when the count is unknown', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="auto" excludedTools={[]} knowledge={{ mode: 'retrieval' }} />
    ))
    expect(container.textContent).toBe('Prosjektkunnskap: søkt')
    unmount()
  })

  it('says the Space knowledge was not available when the decision was refused', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="auto" excludedTools={[]} knowledge={{ mode: 'not_entitled' }} />
    ))
    expect(container.textContent).toBe('Prosjektkunnskap ikke tilgjengelig for dette svaret')
    unmount()
  })

  it('says the Space knowledge was not used when the turn did not ask for it', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="auto" excludedTools={[]} knowledge={{ mode: 'not_searched' }} />
    ))
    expect(container.textContent).toBe('Prosjektkunnskap ikke brukt i dette svaret')
    unmount()
  })

  it('keeps the filter note and adds the knowledge chip beside it for a filtered Space turn', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="knowledge_only" excludedTools={['web_search']} knowledge={{ mode: 'retrieval', sources: 2 }} />
    ))
    const notes = Array.from(container.querySelectorAll('[role="note"]')).map((note) => note.textContent)
    expect(notes).toEqual(['Kilder: kun organisasjonskunnskap', 'Prosjektkunnskap: 2 kilder'])
    unmount()
  })

  it('renders the unchanged filter note for an ordinary filtered turn with no receipt', () => {
    const { container, unmount } = render(() => (
      <SourceScopeNotice scope="web_only" excludedTools={['knowledge_search']} />
    ))
    expect(container.textContent).toBe('Kilder: kun nett')
    unmount()
  })
})

/** EP-9 W1: the "Searched via" line names the providers that served a turn's searches, and the route that allowed them. */
describe('SearchProvenanceNotice', () => {
  it('names one provider and the free-chain reason', () => {
    const { container, unmount } = render(() => (
      <SearchProvenanceNotice provenance={{ providers: [{ id: 'brave', hits: 3 }], route: 'free_chain' }} />
    ))
    expect(container.textContent).toBe('Søkt via Brave Search. Bare gratis søketjenester er tillatt for dette svaret.')
    unmount()
  })

  it('joins two providers in the order the server sent them', () => {
    const { container, unmount } = render(() => (
      <SearchProvenanceNotice
        provenance={{
          providers: [
            { id: 'tantivy_local', hits: 1 },
            { id: 'brave', hits: 2 },
          ],
          route: 'paid_allowed',
        }}
      />
    ))
    expect(container.textContent).toBe(
      'Søkt via Verevon-indeks, Brave Search. Betalte søketjenester er tillatt for dette svaret.',
    )
    unmount()
  })

  it('words a provider the SPA does not name as other provider', () => {
    const { container, unmount } = render(() => (
      <SearchProvenanceNotice provenance={{ providers: [{ id: 'other', hits: 1 }], route: 'free_chain' }} />
    ))
    expect(container.textContent).toBe(
      'Søkt via annen leverandør. Bare gratis søketjenester er tillatt for dette svaret.',
    )
    unmount()
  })

  it('gives the zero-retention reason on a ZDR turn, which can only have free providers', () => {
    const { container, unmount } = render(() => (
      <SearchProvenanceNotice provenance={{ providers: [{ id: 'verevon', hits: 0 }], route: 'zdr_free_only' }} />
    ))
    expect(container.textContent).toBe(
      'Søkt via Verevon-søk. Nulldatalagring er på, så bare gratis søketjenester ble brukt.',
    )
    unmount()
  })
})
