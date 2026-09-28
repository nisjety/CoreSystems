// @vitest-environment jsdom

/**
 * Transcript code blocks: syntax highlighting and a copy-code control.
 *
 *  - A closed, ordinary-language fence is colored with the same
 *    dependency-free scanner (`highlightCode`) the Artifacts panel already
 *    uses, reusing its `.verevon-code-*` token classes rather than a second
 *    palette.
 *  - A fence still streaming in (`closed: false`) — including one whose
 *    language is `mermaid` — renders as plain, unhighlighted text, exactly
 *    like before: an in-progress fence is frequently invalid/incomplete
 *    source, so the cheap plain render is the honest one until it closes.
 *    A still-streaming mermaid fence in particular must never reach the
 *    real diagram renderer with half-written source.
 *  - A closed fence larger than `MAX_HIGHLIGHT_CHARS` also stays plain, for
 *    the same reason the Artifacts panel does: scanning a very large blob
 *    would visibly stall the render.
 *  - The header's copy button copies the raw block text and reports the
 *    clipboard outcome (Kopiert / Kunne ikke kopiere), the same convention
 *    ChatArtifactPanel's copyContent already uses.
 */

import { render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ChatMarkdown, MarkdownCodeBlock } from './ChatMessages'
import { MAX_HIGHLIGHT_CHARS } from './chat-code-highlight'
import type { MarkdownBlock } from './chat-types'

function codeBlock(
  overrides: Partial<Extract<MarkdownBlock, { kind: 'code' }>> = {},
): Extract<MarkdownBlock, { kind: 'code' }> {
  return {
    kind: 'code',
    lang: 'javascript',
    text: 'const x = 1;',
    closed: true,
    ...overrides,
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('MarkdownCodeBlock', () => {
  it("highlights a closed fence with the Artifacts panel's token classes", () => {
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock()} />)
    expect(container.querySelector('.verevon-code-keyword')?.textContent).toBe('const')
    expect(container.querySelector('.verevon-code-number')?.textContent).toBe('1')
  })

  it('keeps a still-streaming fence as plain, unhighlighted text', () => {
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock({ closed: false })} />)
    expect(container.querySelector('.verevon-code-keyword')).toBeNull()
    expect(container.querySelector('pre code')?.textContent).toBe('const x = 1;')
  })

  it('keeps a still-streaming mermaid fence as a plain code block, never the diagram renderer', () => {
    const { container } = render(() => (
      <MarkdownCodeBlock block={codeBlock({ lang: 'mermaid', text: 'graph TD;\n  A -->', closed: false })} />
    ))
    expect(container.querySelector('.verevon-chat-mermaid')).toBeNull()
    expect(container.querySelector('.verevon-chat-codeblock')).toBeTruthy()
    expect(container.querySelector('pre code')?.textContent).toBe('graph TD;\n  A -->')
  })

  it('falls back to plain text for a closed fence over MAX_HIGHLIGHT_CHARS', () => {
    const huge = 'a'.repeat(MAX_HIGHLIGHT_CHARS + 1)
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock({ text: huge })} />)
    expect(container.querySelectorAll('[class^="verevon-code-"]')).toHaveLength(0)
    expect(container.querySelector('pre code')?.textContent).toBe(huge)
  })

  it('shows the language label in the header', () => {
    render(() => <MarkdownCodeBlock block={codeBlock({ lang: 'python', text: 'x = 1' })} />)
    expect(screen.getByText('python')).toBeTruthy()
  })

  it('copies the raw block text and reports success', async () => {
    const writeText = vi.fn(async () => undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    render(() => <MarkdownCodeBlock block={codeBlock()} />)

    screen.getByText('Kopier kode').click()
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('const x = 1;'))
    await vi.waitFor(() => expect(screen.getByText('Kopiert')).toBeTruthy())
  })

  it('reports a refused clipboard write instead of claiming success', async () => {
    const writeText = vi.fn(async () => {
      throw new Error('NotAllowedError')
    })
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    // Distinct text from the previous test: the copy confirmation is cached
    // by the block's own text (see the next test below), and that cache is
    // module-level, so reusing the same source here would start this test
    // from the previous test's still-warm "Kopiert" instead of idle.
    render(() => <MarkdownCodeBlock block={codeBlock({ text: 'const y = 2;' })} />)

    screen.getByText('Kopier kode').click()
    await vi.waitFor(() => expect(screen.getByText('Kunne ikke kopiere')).toBeTruthy())
    expect(screen.queryByText('Kopiert')).toBeNull()
  })

  it('keeps the copy confirmation up while unrelated content elsewhere in the same streaming message keeps changing', async () => {
    // `ChatMarkdown` reparses the whole message on every token (see
    // `parseMarkdownBlocks`), handing every block — including this already-
    // closed one — a brand-new object identity each time, which remounts
    // `TranscriptCodeBlock`. A component-local timer would lose the
    // confirmation the instant anything else in the reply kept streaming;
    // this fence's own text never changes here; the message content wrapped
    // around it does.
    const writeText = vi.fn(async () => undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    // Own, unused-elsewhere source: see the previous test's note on why the
    // confirmation cache's key must not collide across cases.
    const fence = '```js\nconst streamingConfirmationCase = 3;\n```'
    const [content, setContent] = createSignal(`${fence}\nMore text streaming in`)
    render(() => <ChatMarkdown content={content()} />)

    screen.getByText('Kopier kode').click()
    await vi.waitFor(() => expect(screen.getByText('Kopiert')).toBeTruthy())

    // Simulate more tokens landing elsewhere in the same message, well
    // within the confirmation window.
    setContent(`${fence}\nMore text streaming in, and even more`)

    expect(screen.getByText('Kopiert')).toBeTruthy()
  })
})
