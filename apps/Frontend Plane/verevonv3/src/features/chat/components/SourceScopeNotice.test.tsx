// @vitest-environment jsdom

import { render } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { SourceScopeNotice } from './ChatMessages'

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
