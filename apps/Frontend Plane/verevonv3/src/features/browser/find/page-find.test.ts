import { findInDomNodes, stepFindIndex } from './page-find'
import { describe, expect, it } from 'vitest'

const nodes = [
  { kind: 'heading', text: 'Flights to Oslo' },
  { kind: 'button', text: 'Book now' },
  { kind: 'paragraph', text: 'All flights include luggage.' },
]

describe('findInDomNodes', () => {
  it('matches case-insensitively on node text', () => {
    expect(findInDomNodes(nodes, 'FLIGHT')).toEqual([
      { index: 0, kind: 'heading', text: 'Flights to Oslo' },
      { index: 2, kind: 'paragraph', text: 'All flights include luggage.' },
    ])
  })

  it('returns no matches for an empty or whitespace query', () => {
    expect(findInDomNodes(nodes, '')).toEqual([])
    expect(findInDomNodes(nodes, '   ')).toEqual([])
  })

  it('returns no matches when nothing matches', () => {
    expect(findInDomNodes(nodes, 'zzz')).toEqual([])
  })
})

describe('stepFindIndex', () => {
  it('steps forward with wrap-around', () => {
    expect(stepFindIndex(0, 3, 1)).toBe(1)
    expect(stepFindIndex(2, 3, 1)).toBe(0)
  })

  it('steps backward with wrap-around', () => {
    expect(stepFindIndex(1, 3, -1)).toBe(0)
    expect(stepFindIndex(0, 3, -1)).toBe(2)
  })

  it('stays at 0 when there are no matches', () => {
    expect(stepFindIndex(0, 0, 1)).toBe(0)
  })
})
