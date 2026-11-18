import { describe, expect, it } from 'vitest'

import { normalizeSnippet } from './space-search-snippet'

// WS13-S5: the server sends a snippet as text segments, never HTML. These only tidy the list.
describe('search snippet segments (WS13-S5)', () => {
  it('normalizeSnippet merges same-kind neighbours and drops empties', () => {
    expect(
      normalizeSnippet([
        { text: 'Hei ', matched: false },
        { text: 'kar', matched: true },
        { text: 'i', matched: true },
        { text: '', matched: false },
        { text: ' og ', matched: false },
        { text: '', matched: true },
        { text: 'Ola', matched: false },
      ]),
    ).toEqual([
      { text: 'Hei ', matched: false },
      { text: 'kari', matched: true },
      { text: ' og Ola', matched: false },
    ])
  })

  it('an empty segment does not split a run of the same kind, and nothing left is an empty list', () => {
    expect(
      normalizeSnippet([
        { text: 'a', matched: true },
        { text: '', matched: false },
        { text: 'b', matched: true },
      ]),
    ).toEqual([{ text: 'ab', matched: true }])
    expect(normalizeSnippet([])).toEqual([])
    expect(normalizeSnippet([{ text: '', matched: true }])).toEqual([])
  })
})
