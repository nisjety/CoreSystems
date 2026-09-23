import { createRoot } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { parseInline, parseMarkdownBlocks } from './chat-media-markdown'
import { type Citation, type MarkdownBlock } from './chat-types'
import { ChatMarkdown } from './ChatMessages'

describe('Markdown comment blocks', () => {
  it('hides standalone metadata while preserving following prose', () => {
    expect(parseMarkdownBlocks('<!-- UTKAST — ikke send -->\n\nHei Nora')).toEqual([{ kind: 'paragraph', text: 'Hei Nora' }])
    expect(parseMarkdownBlocks('Før\n<!--\nmetadata\n-->Etter')).toEqual([{ kind: 'paragraph', text: 'Før' }, { kind: 'paragraph', text: 'Etter' }])
  })
  it('keeps comments literal inside code and safely handles an in-progress comment', () => {
    expect(parseMarkdownBlocks('```html\n<!-- eksempel -->\n```')[0]).toMatchObject({ kind: 'code', text: '<!-- eksempel -->', closed: true })
    expect(parseMarkdownBlocks('`<!-- eksempel -->`')[0]).toMatchObject({ kind: 'paragraph', text: '`<!-- eksempel -->`' })
    expect(parseMarkdownBlocks('Synlig\n<!-- metadata under innlasting')).toEqual([{ kind: 'paragraph', text: 'Synlig' }])
  })
})

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

function renderInlineWithCitations(text: string, citations: readonly Citation[]): Array<string | Element> {
  return createRoot((dispose) => {
    const nodes = parseInline(text, citations) as Array<string | Element>
    dispose()
    return nodes
  })
}

// Full block-to-DOM path (parseMarkdownBlocks + the actual ChatMessages
// renderers), for fidelity fixes whose observable behaviour lives in the
// renderer rather than the parser alone (e.g. <ol start>, a task checkbox).
function renderMarkdown(content: string): HTMLElement {
  return createRoot((dispose) => {
    const element = ChatMarkdown({ content }) as unknown as HTMLElement
    dispose()
    return element
  })
}

describe('parseMarkdownBlocks — GFM pipe tables', () => {
  it('renders a source table inside an internal-note blockquote as a table', () => {
    const root = renderMarkdown('> **Intern merknad**\n>\n> | Kilde | Brukt til |\n> |---|---|\n> | K3 | Ingen leveringsgaranti |\n>\n> Neste steg: avklar med logistikk.')
    expect(root.querySelectorAll('blockquote table')).toHaveLength(1)
    expect(root.querySelector('blockquote td')?.textContent).toBe('K3')
    expect(root.querySelectorAll('blockquote p')).toHaveLength(2)
  })
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
      'Kommandoen er `verevon sync | grep feil` i terminalen.',
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

  it('promotes only in-range explicit citation markers and preserves unknown markers', () => {
    const citations: Citation[] = [
      { id: 'c1', title: 'Example', url: 'https://example.com/a', snippet: 'A source' },
      { id: 'c2', title: 'Second', url: 'https://example.com/b', snippet: 'B source' },
    ]
    const nodes = renderInlineWithCitations('Supported [1, 2], unknown [3], and [Source](https://example.com).', citations)
    // The component node is intentionally left as a Solid component value in
    // this pure parser test; the mounted ChatMarkdown path renders it as
    // `<details>`. What matters here is that the validated marker is no longer
    // the literal token while an out-of-range marker remains untouched.
    expect(nodes[1]).not.toBe('[1, 2]')
    expect(nodes).toContain('[3]')
    const links = nodes.filter((node): node is Element => node instanceof Element)
    expect(links.some((node) => node.tagName === 'A' && node.textContent === 'Source')).toBe(true)

    const numericLink = renderInlineWithCitations('[3](https://example.com/three)', citations)
    expect(numericLink.some((node): node is Element => node instanceof Element && node.tagName === 'A')).toBe(true)
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

describe('parseMarkdownBlocks — <details> blocks', () => {
  // Regression: the shipping-quote answer wrapped its full offer table in
  // `<details><summary>Se alle tilbud</summary>...</details>`, which rendered
  // as literal tags around the table because the renderer has no HTML pass-
  // through. Only the details/summary tags are interpreted; the body stays
  // ordinary markdown.
  it('parses a details block with a summary and a markdown body', () => {
    const blocks = parseMarkdownBlocks([
      'Billigste er DSV.',
      '<details>',
      '<summary>Se alle tilbud</summary>',
      '',
      '| Transportør | Pris |',
      '|---|---|',
      '| Bring 9300 | 257,83 kr |',
      '</details>',
      'Vil du bestille?',
    ].join('\n'))
    expect(blocks.map((block) => block.kind)).toEqual(['paragraph', 'details', 'paragraph'])
    const details = blocks[1]
    if (details?.kind !== 'details') throw new Error('expected details block')
    expect(details.summary).toBe('Se alle tilbud')
    expect(details.blocks.map((block) => block.kind)).toEqual(['table'])
  })

  it('accepts summary on the opening line and falls back to a default summary', () => {
    const inline = parseMarkdownBlocks('<details><summary>Mer</summary>Skjult tekst</details>')
    expect(inline).toHaveLength(1)
    if (inline[0]?.kind !== 'details') throw new Error('expected details block')
    expect(inline[0].summary).toBe('Mer')
    expect(inline[0].blocks).toEqual([{ kind: 'paragraph', text: 'Skjult tekst' }])

    const bare = parseMarkdownBlocks(['<details>', 'Bare innhold', '</details>'].join('\n'))
    if (bare[0]?.kind !== 'details') throw new Error('expected details block')
    expect(bare[0].summary).toBe('Detaljer')
  })

  it('never treats the body as HTML: tags inside stay text', () => {
    const blocks = parseMarkdownBlocks(['<details>', '<summary>x</summary>', '<script>alert(1)</script>', '</details>'].join('\n'))
    if (blocks[0]?.kind !== 'details') throw new Error('expected details block')
    expect(blocks[0].blocks).toEqual([{ kind: 'paragraph', text: '<script>alert(1)</script>' }])
  })
})

describe('parseInline — backslash escapes', () => {
  // Models escape hashtags ("\#arbeidsplass") so a line is not read as a
  // heading; the backslash used to reach the screen (RUN-LOG finding 17).
  it('renders an escaped hashtag as the bare character', () => {
    const nodes = renderInline('\\#arbeidsplass \\#kontorinnredning')
    expect(nodes).toEqual(['#arbeidsplass #kontorinnredning'])
  })

  it('keeps an escaped emphasis marker literal instead of opening emphasis', () => {
    const nodes = renderInline('pris \\*eks. mva\\* per lampe')
    expect(nodes).toEqual(['pris *eks. mva* per lampe'])
  })

  it('still renders real emphasis and code around escapes', () => {
    const nodes = renderInline('**Pris:** 1 000 kr \\#tilbud') as Array<string | Element>
    expect((nodes[0] as Element).tagName).toBe('STRONG')
    expect(nodes[1]).toBe(' 1 000 kr #tilbud')
  })

  it('leaves a backslash before a letter alone', () => {
    expect(renderInline('C:\\dev\\repo')).toEqual(['C:\\dev\\repo'])
  })
})

// CHAT_PARITY_AUDIT_2026-09-15.md F-02 + §3.2: a leading integer on an
// ordered list's first item ("391. Fasit.") used to be destroyed and
// replaced by "1." — <ol start=null>.
describe('parseMarkdownBlocks — leading list number (F-02)', () => {
  it('parses the leading integer off the first item of an ordered list', () => {
    const blocks = parseMarkdownBlocks('391. Fasit.')
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.ordered).toBe(true)
    expect(list.startNumber).toBe(391)
    expect(list.items.map((item) => item.text)).toEqual(['Fasit.'])
  })

  it('only the first item sets the start — later markers are just item text', () => {
    const blocks = parseMarkdownBlocks(['5. fem', '9. ni'].join('\n'))
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.startNumber).toBe(5)
  })

  it('leaves startNumber unset for a bullet list', () => {
    const blocks = parseMarkdownBlocks('- punkt')
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.startNumber).toBeUndefined()
  })

  it('renders <ol start="391"> instead of renumbering the answer away', () => {
    const root = renderMarkdown('391. Fasit.')
    const ol = root.querySelector('ol')
    expect(ol?.getAttribute('start')).toBe('391')
    expect(ol?.textContent).toContain('Fasit.')
  })

  it('does not set a start attribute when the list already begins at 1', () => {
    const root = renderMarkdown('1. Første')
    expect(root.querySelector('ol')?.hasAttribute('start')).toBe(false)
  })
})

// §3.12: the inline parser only linkified `[text](url)`; a bare URL such as
// the one printed after "URL: " in a web-search answer stayed plain text.
describe('parseInline — bare URL autolinking', () => {
  it('turns a bare https URL into a real link', () => {
    const nodes = renderInline('Se https://www.norges-bank.no/tema/pengepolitikk for detaljer.')
    const link = nodes.find((node): node is Element => node instanceof Element && node.tagName === 'A')
    expect(link?.getAttribute('href')).toBe('https://www.norges-bank.no/tema/pengepolitikk')
    expect(link?.getAttribute('target')).toBe('_blank')
    expect(link?.getAttribute('rel')).toBe('noopener noreferrer')
    expect(link?.textContent).toBe('https://www.norges-bank.no/tema/pengepolitikk')
  })

  it('trims trailing sentence punctuation off the autolinked URL', () => {
    const nodes = renderInline('Kilde: https://example.com/a.')
    const link = nodes.find((node): node is Element => node instanceof Element && node.tagName === 'A')
    expect(link?.getAttribute('href')).toBe('https://example.com/a')
    expect(nodes.at(-1)).toBe('.')
  })

  it('does not double-link a URL already inside a markdown link', () => {
    const nodes = renderInline('[Kilde](https://example.com/a)')
    const links = nodes.filter((node): node is Element => node instanceof Element && node.tagName === 'A')
    expect(links).toHaveLength(1)
  })

  it('does not autolink a URL inside inline code', () => {
    const nodes = renderInline('`https://example.com/a`')
    expect(nodes.some((node) => node instanceof Element && node.tagName === 'A')).toBe(false)
    expect(nodes.some((node) => node instanceof Element && node.tagName === 'CODE')).toBe(true)
  })
})

// §3.2: "- [ ]" / "- [x]" rendered as plain bullets, with no way to tell a
// task list from an ordinary one.
describe('parseMarkdownBlocks — task lists', () => {
  it('marks task-list items with their checked state and strips the marker from the text', () => {
    const blocks = parseMarkdownBlocks(['- [ ] Ubekreftet', '- [x] Bekreftet', '- [X] Også bekreftet'].join('\n'))
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.items.map((item) => item.checked)).toEqual([false, true, true])
    expect(list.items.map((item) => item.text)).toEqual(['Ubekreftet', 'Bekreftet', 'Også bekreftet'])
  })

  it('leaves an ordinary bullet item unchecked (undefined, not false)', () => {
    const blocks = parseMarkdownBlocks('- vanlig punkt')
    const list = blocks[0] as Extract<MarkdownBlock, { kind: 'list' }>
    expect(list.items[0]?.checked).toBeUndefined()
  })

  it('renders a disabled checkbox instead of falling through to a plain bullet', () => {
    const root = renderMarkdown(['- [x] Bekreftet', '- [ ] Ikke bekreftet'].join('\n'))
    const boxes = [...root.querySelectorAll('input[type="checkbox"]')] as HTMLInputElement[]
    expect(boxes.map((box) => box.checked)).toEqual([true, false])
    expect(boxes.every((box) => box.disabled)).toBe(true)
    expect(root.textContent).toContain('Bekreftet')
    expect(root.textContent).toContain('Ikke bekreftet')
  })
})

// §3.2: the fence parser closed at the next ``` line regardless of the
// opening fence's backtick count, so a ```markdown block containing a
// nested ```python block closed early (3 <pre> blocks, one empty).
describe('parseMarkdownBlocks — fence-length matching (nested fences)', () => {
  it('keeps a shorter nested fence as literal content of a longer outer fence', () => {
    const blocks = parseMarkdownBlocks([
      '````markdown',
      '# Title',
      '```python',
      'print("hi")',
      '```',
      '````',
    ].join('\n'))
    expect(blocks).toHaveLength(1)
    expect(blocks[0]?.kind).toBe('code')
    const code = blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>
    expect(code.lang).toBe('markdown')
    expect(code.text).toBe(['# Title', '```python', 'print("hi")', '```'].join('\n'))
  })

  it('still closes a fence on a bare closing line of the same length', () => {
    const blocks = parseMarkdownBlocks(['```', 'a', '```', 'b'].join('\n'))
    expect(blocks.map((block) => block.kind)).toEqual(['code', 'paragraph'])
    expect((blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>).text).toBe('a')
  })

  it('does not close on a shorter backtick run than the one that opened the fence', () => {
    const blocks = parseMarkdownBlocks(['````', '``` still inside', 'done', '````'].join('\n'))
    expect(blocks).toHaveLength(1)
    expect((blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>).text).toBe(['``` still inside', 'done'].join('\n'))
  })
})

// §3.2: only one level of blockquote rendered — nested ">" markers stayed
// stuck to the text of a single flat <blockquote> instead of nesting.
describe('parseMarkdownBlocks — nested blockquotes', () => {
  it('annotates quote lines with their nesting depth for doubled ">>" markers', () => {
    const blocks = parseMarkdownBlocks(['> topp', '>> under'].join('\n'))
    const quote = blocks[0] as Extract<MarkdownBlock, { kind: 'quote' }>
    expect(quote.lines.map((line) => line.depth)).toEqual([1, 2])
    expect(quote.lines.map((line) => line.text)).toEqual(['topp', 'under'])
  })

  it('annotates quote lines with their nesting depth for space-separated "> >" markers', () => {
    const blocks = parseMarkdownBlocks(['> topp', '> > under'].join('\n'))
    const quote = blocks[0] as Extract<MarkdownBlock, { kind: 'quote' }>
    expect(quote.lines.map((line) => line.depth)).toEqual([1, 2])
  })

  it('renders a nested <blockquote> instead of collapsing to one level', () => {
    const root = renderMarkdown(['> topp', '>> under', '> tilbake'].join('\n'))
    const outer = root.querySelector('blockquote')
    const inner = outer?.querySelector('blockquote')
    expect(inner).toBeTruthy()
    expect(inner?.textContent).toContain('under')
    expect(outer?.textContent).toContain('topp')
    expect(outer?.textContent).toContain('tilbake')
  })
})

// A fenced code block now reports whether its closing fence actually
// arrived, so a fence still mid-stream (no closing backticks yet) can be
// told apart from a finished one — the mermaid renderer below only ever
// hands a CLOSED block to `mermaid.render`.
describe('parseMarkdownBlocks — fence completion (closed)', () => {
  it('marks a fence that closed before the source ran out', () => {
    const blocks = parseMarkdownBlocks(['```js', 'const a = 1', '```'].join('\n'))
    const code = blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>
    expect(code.closed).toBe(true)
  })

  it('marks a fence still open when the source ends before a closing line', () => {
    const blocks = parseMarkdownBlocks(['```js', 'const a = 1'].join('\n'))
    const code = blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>
    expect(code.closed).toBe(false)
    // The in-progress content is still there — the fence not having closed
    // yet must not lose or hide what has streamed in so far.
    expect(code.text).toBe('const a = 1')
  })
})

describe('parseMarkdownBlocks — inline math ($...$)', () => {
  it('renders a valid inline expression through KaTeX', () => {
    const nodes = renderInline('Formelen er $x = \\frac{-b}{2a}$ for andregradslikninger.')
    const math = nodes.find((node): node is Element => node instanceof Element && node.classList.contains('verevon-chat-math--inline'))
    expect(math).toBeTruthy()
    expect(math?.querySelector('.katex')).toBeTruthy()
  })

  // Pandoc's own heuristic for this exact ambiguity: an opening `$`
  // immediately followed by a digit is read as a currency figure, not a
  // formula opening, so "$5 and $10" never gets its first `$` treated as
  // math (which would otherwise swallow "5 and " into a bogus expression).
  it('does not mistake a currency amount for math', () => {
    const nodes = renderInline('Prisen er $5 og $10 for de to variantene.')
    expect(nodes.some((node) => node instanceof Element)).toBe(false)
    expect(nodes.join('')).toBe('Prisen er $5 og $10 for de to variantene.')
  })

  it('leaves a lone, unmatched $ literal instead of guessing at math', () => {
    const nodes = renderInline('Kostnaden starter på $ og øker derfra.')
    expect(nodes.some((node) => node instanceof Element)).toBe(false)
    expect(nodes.join('')).toBe('Kostnaden starter på $ og øker derfra.')
  })

  it('falls back to the raw $...$ source when KaTeX rejects the expression', () => {
    const nodes = renderInline('Ugyldig: $\\frac{1}{$ er feil.')
    expect(nodes.some((node) => node instanceof Element)).toBe(false)
    expect(nodes.join('')).toBe('Ugyldig: $\\frac{1}{$ er feil.')
  })
})

describe('parseMarkdownBlocks — block math ($$...$$)', () => {
  it('parses a same-line $$...$$ as a closed math block', () => {
    const blocks = parseMarkdownBlocks('$$E = mc^2$$')
    expect(blocks).toHaveLength(1)
    const math = blocks[0] as Extract<MarkdownBlock, { kind: 'math' }>
    expect(math.kind).toBe('math')
    expect(math.text).toBe('E = mc^2')
    expect(math.closed).toBe(true)
  })

  it('parses a multi-line $$ ... $$ block with the delimiters on their own lines', () => {
    const blocks = parseMarkdownBlocks(['$$', 'E = mc^2', '$$'].join('\n'))
    const math = blocks[0] as Extract<MarkdownBlock, { kind: 'math' }>
    expect(math.text).toBe('E = mc^2')
    expect(math.closed).toBe(true)
  })

  it('splits a math block off a preceding paragraph even without a blank line', () => {
    const blocks = parseMarkdownBlocks(['Her er formelen:', '$$E = mc^2$$'].join('\n'))
    expect(blocks.map((block) => block.kind)).toEqual(['paragraph', 'math'])
  })

  it('marks a block still open when the closing $$ has not arrived yet (mid-stream)', () => {
    const blocks = parseMarkdownBlocks(['$$', 'E = mc^'].join('\n'))
    expect(blocks).toHaveLength(1)
    const math = blocks[0] as Extract<MarkdownBlock, { kind: 'math' }>
    expect(math.closed).toBe(false)
    expect(math.text).toBe('E = mc^')
  })

  it('renders a closed block as KaTeX display math', () => {
    const root = renderMarkdown('$$E = mc^2$$')
    const math = root.querySelector('.verevon-chat-math--block')
    expect(math).toBeTruthy()
    expect(math?.querySelector('.katex-display, .katex')).toBeTruthy()
  })

  it('renders an unclosed (still streaming) block as raw text, not KaTeX', () => {
    const root = renderMarkdown(['$$', 'E = mc^'].join('\n'))
    expect(root.querySelector('.katex')).toBeNull()
    const raw = root.querySelector('.verevon-chat-math-raw')
    expect(raw?.textContent).toContain('E = mc^')
  })

  it('falls back to raw text when a closed block is malformed LaTeX', () => {
    const root = renderMarkdown('$$\\frac{1}{$$')
    expect(root.querySelector('.katex')).toBeNull()
    const raw = root.querySelector('.verevon-chat-math-raw')
    expect(raw?.textContent).toContain('\\frac{1}{')
  })
})

// ```mermaid fenced blocks are a special case of the existing fenced-code
// path (see chat-media-markdown.tsx's FENCE_OPEN_PATTERN handling) — the
// diagram is only ever attempted once `closed` is true (ChatMessages.tsx's
// `MarkdownCodeBlock`); actually invoking `mermaid.render` needs a real
// browser (jsdom has no SVG `getBBox`), so that path is covered by the
// live-browser check, not here.
describe('parseMarkdownBlocks — mermaid fenced blocks', () => {
  it('parses a closed ```mermaid block as a code block with lang "mermaid"', () => {
    const blocks = parseMarkdownBlocks(['```mermaid', 'graph TD', 'A-->B', '```'].join('\n'))
    const code = blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>
    expect(code.kind).toBe('code')
    expect(code.lang).toBe('mermaid')
    expect(code.text).toBe('graph TD\nA-->B')
    expect(code.closed).toBe(true)
  })

  it('marks a ```mermaid block still open mid-stream (no closing fence yet)', () => {
    const blocks = parseMarkdownBlocks(['```mermaid', 'graph TD', 'A-->B'].join('\n'))
    const code = blocks[0] as Extract<MarkdownBlock, { kind: 'code' }>
    expect(code.lang).toBe('mermaid')
    expect(code.closed).toBe(false)
  })

  it('renders an unclosed mermaid block as a plain code block, never attempting the diagram', () => {
    const root = renderMarkdown(['```mermaid', 'graph TD', 'A-->B'].join('\n'))
    // No diagram (and no loading placeholder) — the fence hasn't closed, so
    // `MarkdownCodeBlock` takes the same path as any other language.
    expect(root.querySelector('.verevon-chat-mermaid')).toBeNull()
    expect(root.querySelector('.verevon-chat-mermaid-loading')).toBeNull()
    const code = root.querySelector('.verevon-chat-codeblock pre code')
    expect(code?.textContent).toBe('graph TD\nA-->B')
  })
})
