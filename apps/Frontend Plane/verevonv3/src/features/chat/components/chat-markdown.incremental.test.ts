/**
 * `parseMarkdownIncremental` must be `parseMarkdownBlocks` that remembers.
 *
 * WS09 S2. The incremental parse keeps the blocks before the last
 * `FROZEN_TAIL_BLOCKS` as the previous parse's own objects and re-reads only
 * the rest. That is only safe if no later text can change a block it froze, and
 * "the parser looks at most two lines ahead" is an argument, not a proof. These
 * tests are the proof: the incremental result is compared with a full parse at
 * EVERY prefix of a corpus of documents built to hit each place a block's end
 * depends on text after it, and again over random chunkings of the same text.
 */

import { describe, expect, it } from 'vitest'
import {
  FROZEN_TAIL_BLOCKS,
  parseMarkdownBlocks,
  parseMarkdownIncremental,
  type ParsedMarkdown,
} from './chat-media-markdown'

/** Documents that put each parser branch, and each lookahead, in play. */
const CORPUS: Record<string, string> = {
  prose: [
    '## Kvartalsoversikt',
    '',
    'Omsetningen endte pa 4,2 millioner kroner.',
    'Veksten kom fra nye kunder.',
    '',
    'Et siste avsnitt uten noe mer.',
  ].join('\n'),
  lists: [
    '- forste punkt',
    '- andre punkt',
    '  - nestet punkt',
    '',
    '1. en',
    '2. to',
    '3) tre',
    '',
    '391. fasit',
    '* stjerne',
    '+ pluss',
    '- [x] ferdig',
    '- [ ] ikke ferdig',
  ].join('\n'),
  tables: [
    'Tabell under:',
    '| Navn | Verdi |',
    '| --- | ---: |',
    '| a | 1 |',
    '| b | 2 |',
    '',
    'Tekst mellom.',
    '',
    'a | b',
    '--|--',
    '1 | 2',
    'tekst rett etter',
  ].join('\n'),
  // A paragraph whose end depends on whether the NEXT line starts a table.
  paragraphBeforeTable: ['Intro linje', '| x | y |', '| - | - |', '| 1 | 2 |', '', 'Slutt'].join('\n'),
  quotes: ['> sitat en', '> sitat to', '>> nestet', '> > nestet to', '', 'etter sitatet', '---', '***', 'slutt'].join('\n'),
  fences: [
    'For',
    '```ts',
    'const a = 1',
    '',
    'const b = 2',
    '```',
    'Mellom',
    '````markdown',
    '```python',
    'print(1)',
    '```',
    '````',
    'Etter',
    '```mermaid',
    'graph TD; A-->B',
    '```',
    '```chart',
    '{"type":"bar"}',
    '```',
  ].join('\n'),
  unterminatedFence: ['Intro', '', '```js', 'let x = 1', '', '## ikke en overskrift', '- ikke en liste'].join('\n'),
  math: ['$$E = mc^2$$', '', '$$', '\\int_0^1 x dx', '$$', '', 'tekst', '', '$$', 'uavsluttet'].join('\n'),
  details: [
    'For',
    '<details><summary>Se alle tilbud</summary>',
    '',
    '| a | b |',
    '|---|---|',
    '| 1 | 2 |',
    '',
    '</details>',
    'Etter',
    '<details>',
    '<summary>Uavsluttet</summary>',
    'innhold uten slutt',
  ].join('\n'),
  // Every comment shape, including the ones that rewrite the line they close on.
  comments: [
    'For',
    '<!-- hel kommentar -->',
    'Midt',
    '',
    '<!--',
    'metadata',
    '-->Etter lukkingen',
    '',
    'Avsnitt A',
    '',
    '<!-- en til',
    '   over flere linjer --> og mer tekst',
    '- liste etter',
    '',
    'Slutt',
    '<!-- uavsluttet',
    'skjult',
  ].join('\n'),
  headingsAndRules: ['# En', '## To', '### Tre', '#### Fire', '---', '# Fem', 'tekst', '___', 'seks'].join('\n'),
  mixed: [
    '# Rapport',
    '',
    'Innledning med **fet** og `kode` og [1].',
    '',
    '- a',
    '- b',
    '',
    '| h1 | h2 |',
    '| -- | -- |',
    '| x | y |',
    '',
    '> sitat',
    '',
    '```sh',
    'echo hei',
    '```',
    '',
    '---',
    '',
    'Avslutning.',
  ].join('\n'),
  blankRuns: ['A', '', '', '', 'B', '   ', '', 'C'].join('\n'),
}

const asCrlf = (text: string) => text.replace(/\n/g, '\r\n')

/** mulberry32: a deterministic PRNG, so a failing chunking can be replayed from its seed. */
function rng(seed: number) {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** Feeds `chunks` in order, asserting the equivalence after each, and returns the final parse. */
function streamAndCompare(label: string, chunks: string[]): ParsedMarkdown | undefined {
  let source = ''
  let parsed: ParsedMarkdown | undefined
  for (const chunk of chunks) {
    source += chunk
    const previous = parsed
    parsed = parseMarkdownIncremental(previous, source)
    const expected = parseMarkdownBlocks(source)
    if (JSON.stringify(parsed.blocks) !== JSON.stringify(expected)) {
      // toEqual for the readable diff; the stringify above keeps the happy path fast.
      expect(parsed.blocks, `${label}: prefix of ${source.length} chars`).toEqual(expected)
      throw new Error(`${label}: blocks differ at ${source.length} chars`)
    }
    // Plain checks, not `expect`, on the per-chunk hot path: this runs over a
    // million chunks and an assertion object per element exhausts the heap.
    if (parsed.restartAt.length !== parsed.blocks.length) {
      throw new Error(`${label}: restartAt does not parallel blocks at ${source.length} chars`)
    }
    if (previous) {
      for (let index = 0; index < parsed.reused; index += 1) {
        // Sealed identity: a frozen block is the previous parse's own object.
        if (parsed.blocks[index] !== previous.blocks[index]) {
          throw new Error(`${label}: block ${index} was reparsed though it was frozen, at ${source.length} chars`)
        }
      }
    }
  }
  return parsed
}

const chars = (text: string) => [...text]

describe('parseMarkdownIncremental equals a full parse', () => {
  for (const [name, text] of Object.entries(CORPUS)) {
    it(`at every character prefix of "${name}"`, () => {
      streamAndCompare(name, chars(text))
    })
    it(`at every character prefix of "${name}" with CRLF line endings`, () => {
      streamAndCompare(`${name} (crlf)`, chars(asCrlf(text)))
    })
  }

  it('at every character prefix of the whole corpus joined, so blocks of every kind sit next to each other', () => {
    streamAndCompare('corpus', chars(Object.values(CORPUS).join('\n\n')))
  })

  it('under random chunkings of the corpus, including chunks that split a line and a CRLF', () => {
    const documents = [...Object.values(CORPUS), Object.values(CORPUS).join('\n\n'), asCrlf(Object.values(CORPUS).join('\n'))]
    for (let seed = 1; seed <= 40; seed += 1) {
      const next = rng(seed)
      for (const [documentIndex, document] of documents.entries()) {
        const chunks: string[] = []
        let at = 0
        while (at < document.length) {
          // Mostly a few characters, sometimes a whole line or more: tokens vary.
          const size = next() < 0.8 ? 1 + Math.floor(next() * 8) : 1 + Math.floor(next() * 80)
          chunks.push(document.slice(at, at + size))
          at += size
        }
        streamAndCompare(`seed ${seed}, document ${documentIndex}`, chunks)
      }
    }
  })

  it('when every token is a whole line', () => {
    const text = Object.values(CORPUS).join('\n\n')
    streamAndCompare('lines', text.split(/(?<=\n)/u))
  })
})

describe('parseMarkdownIncremental freezing', () => {
  const blockCount = (parsed: ParsedMarkdown | undefined) => parsed?.blocks.length ?? 0

  it('keeps every block before the last FROZEN_TAIL_BLOCKS as the same object while the tail grows', () => {
    const sealed = Array.from({ length: 12 }, (_, index) => `Avsnitt ${index} med litt tekst.`).join('\n\n')
    const first = parseMarkdownIncremental(undefined, `${sealed}\n\nHale`)
    expect(blockCount(first)).toBe(13)
    const second = parseMarkdownIncremental(first, `${sealed}\n\nHale som vokser`)
    expect(second.reused).toBe(blockCount(first) - FROZEN_TAIL_BLOCKS)
    for (let index = 0; index < second.reused; index += 1) {
      expect(second.blocks[index]).toBe(first.blocks[index])
    }
    // The trailing blocks are re-read, so they are new objects.
    expect(second.blocks.at(-1)).not.toBe(first.blocks.at(-1))
    expect(second.blocks.at(-1)).toEqual({ kind: 'paragraph', text: 'Hale som vokser' })
  })

  it('keeps a re-read block that came out unchanged as the previous object', () => {
    const first = parseMarkdownIncremental(undefined, 'Foerste\n\nAndre\n\nTredje')
    const second = parseMarkdownIncremental(first, 'Foerste\n\nAndre\n\nTredje og mer')
    expect(second.reused).toBe(1)
    // Block 1 was parsed again, but is the same block: no new object, no DOM work.
    expect(second.blocks[1]).toBe(first.blocks[1])
    expect(second.blocks[2]).not.toBe(first.blocks[2])
  })

  it('re-reads only from the first unfrozen block, never from the top', () => {
    const body = Array.from({ length: 30 }, (_, index) => `Avsnitt ${index} med litt tekst.`).join('\n\n')
    let parsed = parseMarkdownIncremental(undefined, body)
    let source = body
    for (const token of [' en', ' to', ' tre']) {
      source += token
      parsed = parseMarkdownIncremental(parsed, source)
      // 30 paragraphs, the last two re-read: everything else carried over.
      expect(parsed.reused).toBe(30 - FROZEN_TAIL_BLOCKS)
    }
  })

  it('returns the previous parse itself when nothing was added', () => {
    const parsed = parseMarkdownIncremental(undefined, 'Hei\n\nverden')
    expect(parseMarkdownIncremental(parsed, 'Hei\n\nverden')).toBe(parsed)
    expect(parseMarkdownIncremental(parsed, 'Hei\r\n\r\nverden')).toBe(parsed)
  })

  it('parses a message that is not an extension of the previous one in full', () => {
    const paragraphs = Array.from({ length: 8 }, (_, index) => `Avsnitt ${index}.`).join('\n\n')
    const previous = parseMarkdownIncremental(undefined, paragraphs)
    for (const replaced of ['Helt ny tekst\n\nmed to avsnitt', paragraphs.slice(0, 30), '']) {
      const next = parseMarkdownIncremental(previous, replaced)
      expect(next.reused).toBe(0)
      expect(next.blocks).toEqual(parseMarkdownBlocks(replaced))
      // A fresh parse shares nothing with the one it replaced.
      for (const block of next.blocks) expect(previous.blocks).not.toContain(block)
    }
  })

  it('treats a partial CRLF at the end of the previous source as a different source, then converges', () => {
    const head = Array.from({ length: 6 }, (_, index) => `Avsnitt ${index}.`).join('\r\n\r\n')
    const midLineBreak = parseMarkdownIncremental(undefined, `${head}\r`)
    const complete = parseMarkdownIncremental(midLineBreak, `${head}\r\n\r\nNeste`)
    expect(complete.blocks).toEqual(parseMarkdownBlocks(`${head}\r\n\r\nNeste`))
  })

  it('does not restart on a block that begins on the remainder of a comment line', () => {
    // The `-->` line is rewritten to its remainder, so a restart at that line
    // would parse the original text, comment tail included, and diverge.
    const source = [
      'Foerste',
      '',
      '<!-- a',
      'b --> Etter kommentar',
      '',
      'Tredje',
    ].join('\n')
    const parsed = parseMarkdownIncremental(undefined, source)
    const rewrittenBlock = parsed.blocks.findIndex((block) => block.kind === 'paragraph' && block.text === 'Etter kommentar')
    expect(rewrittenBlock).toBeGreaterThan(-1)
    expect(parsed.restartAt[rewrittenBlock]).toBeNull()
    // Three blocks, two frozen: the rewritten one would be the first re-read,
    // so the restart has to step back to the top instead.
    const next = parseMarkdownIncremental(parsed, `${source} mer`)
    expect(next.blocks).toEqual(parseMarkdownBlocks(`${source} mer`))
    expect(next.reused).toBe(0)
  })
})

describe('parseMarkdownBlocks is unchanged by the refactor', () => {
  it('reads CRLF text as it reads LF text', () => {
    for (const text of Object.values(CORPUS)) {
      expect(parseMarkdownBlocks(asCrlf(text))).toEqual(parseMarkdownBlocks(text))
    }
  })

  it('terminates on a line a stray CR leaves half a heading or list item', () => {
    // A stream chunk can end between a CR and its LF. The anchored heading and
    // list patterns stop at that CR while the unanchored block-start check does
    // not, and the paragraph branch used to spin on the line forever.
    expect(parseMarkdownBlocks('## Rubrikk\r')).toEqual([{ kind: 'paragraph', text: '## Rubrikk' }])
    expect(parseMarkdownBlocks('- punkt\r')).toEqual([{ kind: 'paragraph', text: '- punkt' }])
    expect(parseMarkdownBlocks('## Rubrikk\r\n')).toEqual([{ kind: 'heading', level: 2, text: 'Rubrikk' }])
  })
})
