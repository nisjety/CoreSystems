import { createRoot } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { parseInline, parseMarkdownBlocks } from './chat-media-markdown'
import { type MarkdownBlock } from './chat-types'

function tableBlock(blocks: MarkdownBlock[], at = 0): Extract<MarkdownBlock, { kind: 'table' }> {
  const block = blocks[at]
  if (block?.kind !== 'table') throw new Error(`expected table at ${at}, got ${block?.kind}`)
  return block
}

function renderInline(text: string): Array<string | Element> {
  return createRoot((dispose) => {
    const nodes = parseInline(text) as Array<string | Element>
    dispose()
    return nodes
  })
}

describe('parseMarkdownBlocks — GFM pipe tables', () => {
  it('parses a source-citation table into header, alignment, and rows', () => {
    const blocks = parseMarkdownBlocks([
      '| Kilde | Innhold |',
      '|---|---|',
      '| 706407ad | Selskapsprofil, etablering, ansatte, geografi |',
      '| 8449dc6f | Ekspansjon til Island |',
    ].join('\n'))
    expect(blocks).toHaveLength(1)
    const table = tableBlock(blocks)
    expect(table.header).toEqual(['Kilde', 'Innhold'])
    expect(table.align).toEqual([null, null])
    expect(table.rows).toEqual([
      ['706407ad', 'Selskapsprofil, etablering, ansatte, geografi'],
      ['8449dc6f', 'Ekspansjon til Island'],
    ])
  })

  it('parses every alignment variant in the delimiter row', () => {
    const table = tableBlock(parseMarkdownBlocks([
      '| a | b | c | d |',
      '|:---|:--:|---:|---|',
      '| 1 | 2 | 3 | 4 |',
    ].join('\n')))
    expect(table.align).toEqual(['left', 'center', 'right', null])
  })

  it('accepts rows without leading/trailing pipes', () => {
    const table = tableBlock(parseMarkdownBlocks([
      'Kilde | Innhold',
      '--- | ---',
      '706407ad | Selskapsprofil',
    ].join('\n')))
    expect(table.header).toEqual(['Kilde', 'Innhold'])
    expect(table.rows).toEqual([['706407ad', 'Selskapsprofil']])
  })

  it('normalizes ragged body rows to the header width', () => {
    const table = tableBlock(parseMarkdownBlocks([
      '| a | b |',
      '|---|---|',
      '| only |',
      '| one | two | three |',
    ].join('\n')))
    expect(table.rows).toEqual([
      ['only', ''],
      ['one', 'two'],
    ])
  })

  it('does NOT turn pipe-containing prose into a table without a delimiter row', () => {
    const blocks = parseMarkdownBlocks([
      'Kommandoen er `velion sync | grep feil` i terminalen.',
      'Dette er vanlig prosa med en pipe: a | b.',
    ].join('\n'))
    expect(blocks).toHaveLength(1)
    expect(blocks[0]?.kind).toBe('paragraph')
  })

  it('does NOT start a table when header and delimiter column counts differ', () => {
    const blocks = parseMarkdownBlocks([
      '| a | b | c |',
      '|---|---|',
    ].join('\n'))
    expect(blocks.every((block) => block.kind !== 'table')).toBe(true)
  })

  it('splits a table off a preceding paragraph even without a blank line', () => {
    const blocks = parseMarkdownBlocks([
      'Her er kildene:',
      '| Kilde | Innhold |',
      '|---|---|',
      '| 706407ad | Selskapsprofil |',
    ].join('\n'))
    expect(blocks.map((block) => block.kind)).toEqual(['paragraph', 'table'])
    expect((blocks[0] as Extract<MarkdownBlock, { kind: 'paragraph' }>).text).toBe('Her er kildene:')
  })

  it('keeps a bare --- line as a horizontal rule, not a table fragment', () => {
    const blocks = parseMarkdownBlocks('før\n\n---\n\netter')
    expect(blocks.map((block) => block.kind)).toEqual(['paragraph', 'hr', 'paragraph'])
  })

  it('unescapes \\| inside a cell instead of splitting on it', () => {
    const table = tableBlock(parseMarkdownBlocks([
      '| Uttrykk | Betydning |',
      '|---|---|',
      '| a \\| b | union |',
    ].join('\n')))
    expect(table.rows).toEqual([['a | b', 'union']])
  })

  it('preserves inline markers in cells and renders them via the shared inline path', () => {
    const table = tableBlock(parseMarkdownBlocks([
      '| Kilde | Innhold |',
      '|---|---|',
      '| `8449dc6f` | **Ekspansjon** til [Island](https://example.com) |',
    ].join('\n')))
    expect(table.rows[0]).toEqual(['`8449dc6f`', '**Ekspansjon** til [Island](https://example.com)'])

    const idCell = renderInline(table.rows[0]?.[0] ?? '')
    expect(idCell.some((node) => node instanceof Element && node.tagName === 'CODE')).toBe(true)

    const proseCell = renderInline(table.rows[0]?.[1] ?? '')
    const tags = proseCell.filter((node): node is Element => node instanceof Element).map((node) => node.tagName)
    expect(tags).toContain('STRONG')
    expect(tags).toContain('A')
  })
})

describe('parseMarkdownBlocks — nested lists', () => {
  it('annotates list items with their indentation depth', () => {
    const blocks = parseMarkdownBlocks([
      '- topp',
      '  - under',
      '    - dypest',
      '- topp igjen',
    ].join('\n'))
    expect(blocks).toHaveLength(1)
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.ordered).toBe(false)
    expect(list.items.map((item) => item.depth)).toEqual([0, 1, 2, 0])
    expect(list.items.map((item) => item.text)).toEqual(['topp', 'under', 'dypest', 'topp igjen'])
  })

  it('keeps nested items of the other marker family inside the parent list', () => {
    const blocks = parseMarkdownBlocks([
      '1. første',
      '   - detalj',
      '2. andre',
    ].join('\n'))
    expect(blocks).toHaveLength(1)
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.ordered).toBe(true)
    expect(list.items.map((item) => [item.depth, item.ordered])).toEqual([[0, true], [1, false], [0, true]])
  })

  it('still splits sibling top-level lists of different marker families', () => {
    const blocks = parseMarkdownBlocks('- punkt\n1. nummer')
    expect(blocks.map((block) => block.kind)).toEqual(['list', 'list'])
  })
})
