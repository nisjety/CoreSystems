import { describe, expect, it } from 'vitest'
import {
  CHAPTER_TITLE_MAX_CHARS,
  CHAPTER_TOC_MAX_ENTRIES,
  chapterEntries,
  headingsIn,
  turnSelector,
  type ChapterSource,
} from './chapter-toc'

function turn(id: string, role: 'user' | 'assistant', content: string, streaming = false): ChapterSource {
  return { id, role, content, streaming }
}

describe('headingsIn', () => {
  it('reads ATX headings at levels one to three, in order, and shows deeper ones at level three', () => {
    expect(headingsIn('# One\ntext\n## Two\n### Three\n#### Four\n###### Five')).toEqual([
      { level: 1, title: 'One' },
      { level: 2, title: 'Two' },
      { level: 3, title: 'Three' },
      { level: 3, title: 'Four' },
      { level: 3, title: 'Five' },
    ])
  })

  it('ignores headings inside fenced code, and a fence closes only on its own character and at least its length', () => {
    const markdown = [
      '```',
      '# inside',
      '```',
      '~~~python',
      '## also inside',
      '~~~',
      '````',
      '# still inside',
      '```',
      '# still inside four',
      '````',
      '# outside',
    ].join('\n')
    expect(headingsIn(markdown)).toEqual([{ level: 1, title: 'outside' }])
  })

  it('reads only lines that are headings: no hashtags, no seven hashes, no indented or quoted lines', () => {
    expect(headingsIn('#hashtag\n#######  seven\n    # indented\n> # quoted\nplain # text')).toEqual([])
  })

  it('drops the closing hashes and the bold, link and code markup, and keeps the words', () => {
    expect(
      headingsIn('## Closing ##\n# **Bold** and [a link](https://example.com/x) and `code`'),
    ).toEqual([
      { level: 2, title: 'Closing' },
      { level: 1, title: 'Bold and a link and code' },
    ])
  })

  it('skips empty headings and reads CRLF line endings', () => {
    expect(headingsIn('#\n# \n## Real\r\n### Also')).toEqual([
      { level: 2, title: 'Real' },
      { level: 3, title: 'Also' },
    ])
  })

  it('keeps letters from any language and cuts a long title with an ellipsis', () => {
    expect(headingsIn('# Økonomi og æbler')).toEqual([{ level: 1, title: 'Økonomi og æbler' }])
    const [long] = headingsIn(`# ${'a'.repeat(200)}`)
    expect(Array.from(long?.title ?? '')).toHaveLength(CHAPTER_TITLE_MAX_CHARS)
    expect(long?.title.endsWith('…')).toBe(true)
  })
})

describe('chapterEntries', () => {
  it('lists the headings of assistant answers in order, each with the turn that holds it', () => {
    const turns = [
      turn('u1', 'user', '# not an answer'),
      turn('a1', 'assistant', '# First\n## Detail'),
      turn('u2', 'user', 'more?'),
      turn('a2', 'assistant', '### Second'),
    ]
    expect(chapterEntries(turns)).toEqual([
      { turnId: 'a1', level: 1, title: 'First' },
      { turnId: 'a1', level: 2, title: 'Detail' },
      { turnId: 'a2', level: 3, title: 'Second' },
    ])
  })

  it('leaves out an answer that is still streaming, until it settles', () => {
    expect(chapterEntries([turn('a1', 'assistant', '# Still arriving', true)])).toEqual([])
    expect(chapterEntries([turn('a1', 'assistant', '# Arrived', false)])).toEqual([
      { turnId: 'a1', level: 1, title: 'Arrived' },
    ])
  })

  it('keeps only the most recent entries', () => {
    const headings = Array.from({ length: CHAPTER_TOC_MAX_ENTRIES + 10 }, (_, index) => `## Part ${index + 1}`).join('\n')
    const entries = chapterEntries([turn('a1', 'assistant', headings)])
    expect(entries).toHaveLength(CHAPTER_TOC_MAX_ENTRIES)
    expect(entries[0]?.title).toBe('Part 11')
    expect(entries.at(-1)?.title).toBe(`Part ${CHAPTER_TOC_MAX_ENTRIES + 10}`)
  })

  it('reads a turn again when its text changes', () => {
    expect(chapterEntries([turn('a1', 'assistant', '# Old')])).toEqual([{ turnId: 'a1', level: 1, title: 'Old' }])
    expect(chapterEntries([turn('a1', 'assistant', '# Old')])).toEqual([{ turnId: 'a1', level: 1, title: 'Old' }])
    expect(chapterEntries([turn('a1', 'assistant', '# New')])).toEqual([{ turnId: 'a1', level: 1, title: 'New' }])
  })
})

describe('turnSelector', () => {
  it('selects the element that holds a turn with a plain id', () => {
    expect(turnSelector('turn_1')).toBe('[data-turn-id="turn_1"]')
  })

  it('selects the element whatever quotes and backslashes the id contains', () => {
    const element = document.createElement('div')
    element.dataset.turnId = 'turn "quoted" \\ id'
    document.body.append(element)
    expect(document.querySelector(turnSelector('turn "quoted" \\ id'))).toBe(element)
    element.remove()
  })
})
