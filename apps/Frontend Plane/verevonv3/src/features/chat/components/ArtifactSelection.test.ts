// @vitest-environment jsdom

import { webcrypto } from 'node:crypto'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  quoteSha256,
  readSourceView,
  selectionRange,
  sourceSelectionRange,
} from './ArtifactSelection'

afterEach(() => {
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

/** A root element holding `html`, attached to the document so its selection behaves as a page's does. */
function mount(html: string): HTMLElement {
  const root = document.createElement('div')
  root.innerHTML = html
  document.body.appendChild(root)
  return root
}

/** The text nodes under `root`, in document order. */
function textNodes(root: Element): Text[] {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const nodes: Text[] = []
  for (let node = walker.nextNode(); node; node = walker.nextNode()) nodes.push(node as Text)
  return nodes
}

/** Makes the page report one selection, as a drag over `anchor`..`focus` would. */
function selectOnPage(anchorNode: Node, anchorOffset: number, focusNode: Node, focusOffset: number): void {
  const selection = {
    isCollapsed: false,
    anchorNode,
    anchorOffset,
    focusNode,
    focusOffset,
  } as unknown as Selection
  // A test may move the selection more than once: reuse the spy the first call made.
  if (vi.isMockFunction(document.getSelection)) {
    vi.mocked(document.getSelection).mockReturnValue(selection)
  } else {
    vi.spyOn(document, 'getSelection').mockReturnValue(selection)
  }
}

describe('ArtifactSelection (EP-3 S6)', () => {
  it('source_view_offsets_exact', () => {
    const content = 'Første linje\nAndre linje'
    const root = mount(`<pre><code>${content}</code></pre>`)
    expect(readSourceView(root).text).toBe(content)

    // "linje" on the second line is characters 19 to 24 of the artifact.
    const text = textNodes(root)[0] as Text
    selectOnPage(text, 19, text, 24)
    expect(sourceSelectionRange(root, content)).toEqual({ start: 19, end: 24, quote: 'linje' })

    // A backwards drag names the same range.
    selectOnPage(text, 24, text, 19)
    expect(sourceSelectionRange(root, content)).toEqual({ start: 19, end: 24, quote: 'linje' })
  })

  it('code_view_skips_the_gutter_and_counts_each_line_break', () => {
    // The markup has no whitespace between elements, as the panel's code view renders none.
    const content = 'const a = 1\nconst b = 2'
    const root = mount(
      '<div class="verevon-chat-artifact-code"><pre><code>'
        + '<span class="verevon-chat-artifact-code__line"><span class="verevon-chat-artifact-code__gutter" aria-hidden="true">1</span>'
        + '<span class="verevon-chat-artifact-code__text">const a = 1</span></span>'
        + '<span class="verevon-chat-artifact-code__line"><span class="verevon-chat-artifact-code__gutter" aria-hidden="true">2</span>'
        + '<span class="verevon-chat-artifact-code__text">const b = 2</span></span>'
        + '</code></pre></div>',
    )
    expect(readSourceView(root).text).toBe(content)

    const second = textNodes(root).find((node) => node.data === 'const b = 2') as Text
    selectOnPage(second, 0, second, 11)
    expect(sourceSelectionRange(root, content)).toEqual({ start: 12, end: 23, quote: 'const b = 2' })

    // A gutter number is not part of the artifact, so a selection that starts there is not offered.
    const gutter = root.querySelectorAll('.verevon-chat-artifact-code__gutter')[1]?.firstChild as Text
    selectOnPage(gutter, 0, second, 11)
    expect(sourceSelectionRange(root, content)).toBeNull()
  })

  it('ambiguous_rendered_selection_falls_back', () => {
    // The rendered Markdown shows "Et fet ord." and no markup, so its text is not the artifact's text.
    const content = '# Tittel\n\nEt **fet** ord.'
    const root = mount('<h1>Tittel</h1><p>Et <strong>fet</strong> ord.</p>')
    expect(readSourceView(root).text).not.toBe(content)

    const paragraph = textNodes(root).find((node) => node.data.startsWith('Et ')) as Text
    selectOnPage(paragraph, 0, paragraph, 2)
    expect(sourceSelectionRange(root, content)).toBeNull()
  })

  it('a_selection_outside_the_view_is_not_offered', () => {
    const content = 'Kort tekst'
    const view = mount(`<pre><code>${content}</code></pre>`)
    const elsewhere = mount('<p>Annen tekst på siden</p>')
    const text = textNodes(elsewhere)[0] as Text
    selectOnPage(text, 0, text, 4)
    expect(sourceSelectionRange(view, content)).toBeNull()
  })

  it('selection_range_counts_code_points', () => {
    // "😀" is two UTF-16 units and one code point: the DOM offset 13 is character 12.
    const content = 'Hei 😀 verden'
    expect(selectionRange(content, 7, 13)).toEqual({ start: 6, end: 12, quote: 'verden' })
    expect(selectionRange(content, 5, 5)).toBeNull()
    expect(selectionRange(content, -1, 3)).toBeNull()
    expect(selectionRange(content, 0, 99)).toBeNull()
  })

  it('quote_is_hashed_as_the_gateway_checks_it', async () => {
    // jsdom does not provide SubtleCrypto, so the test supplies Node's implementation of the same API.
    vi.stubGlobal('crypto', webcrypto)
    try {
      // SHA-256 of "abc", the standard test vector.
      expect(await quoteSha256('abc')).toBe('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
