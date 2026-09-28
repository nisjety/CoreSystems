// @vitest-environment jsdom

/**
 * "Eksporter til Word/Excel/PowerPoint" / "Lagre som PDF" wiring in
 * `AssistantMessage`: the four choices always live in the overflow "..."
 * menu, and a finished (non-streaming) answer of at least 600 visible
 * characters — Microsoft Copilot's own threshold — additionally gets a
 * dedicated, always-offered export button.
 *
 * `chat-export` itself (the OOXML builders, the zip writer, the markdown
 * tokenizer) has its own tests next to it in `lib/chat-export/`; this file
 * only tests that `AssistantMessage` calls it correctly and reports failure.
 */

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn, Citation } from './chat-types'

const exportChatAnswerMock = vi.fn()

vi.mock('../lib/chat-export', () => ({
  exportChatAnswer: (...args: unknown[]) => exportChatAnswerMock(...args),
  answerExportSize: (content: string) => content.length,
  EXPORT_BUTTON_MIN_CHARS: 600,
}))

afterEach(() => {
  exportChatAnswerMock.mockReset()
})

const SHORT_ANSWER = 'Svaret er ferdig.'
const LONG_ANSWER = 'Dette er et langt svar. '.repeat(30) // > 600 characters

const citation = (overrides: Partial<Citation> = {}): Citation => ({
  id: 'c1',
  title: 'NRK-artikkel',
  url: 'https://nrk.no/x',
  snippet: '',
  ...overrides,
})

const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: SHORT_ANSWER,
  createdAt: '2026-09-28T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const props = (message: ChatTurn) => ({
  copied: false,
  message,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

function openOverflowMenu() {
  fireEvent.click(screen.getByRole('button', { name: 'Flere handlinger' }))
  flush()
}

describe('export wiring on an assistant answer', () => {
  it('offers all four export choices in the overflow menu even for a short answer', () => {
    render(() => <AssistantMessage {...props(turn())} />)
    openOverflowMenu()
    expect(screen.getByRole('button', { name: 'Eksporter til Word' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Eksporter til Excel' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Eksporter til PowerPoint' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Lagre som PDF' })).toBeTruthy()
  })

  it('does not show the dedicated export button for a short answer', () => {
    render(() => <AssistantMessage {...props(turn())} />)
    expect(screen.queryByRole('button', { name: 'Eksporter svar' })).toBeNull()
  })

  it('shows the dedicated export button once the answer passes the visible-text threshold', () => {
    render(() => <AssistantMessage {...props(turn({ content: LONG_ANSWER }))} />)
    expect(screen.getByRole('button', { name: 'Eksporter svar' })).toBeTruthy()
  })

  it('never shows the dedicated export button while the answer is still streaming', () => {
    render(() => <AssistantMessage {...props(turn({ content: LONG_ANSWER, streaming: true }))} />)
    expect(screen.queryByRole('button', { name: 'Eksporter svar' })).toBeNull()
  })

  it('calls exportChatAnswer with the format, content and citations-as-sources', () => {
    render(() => (
      <AssistantMessage
        {...props(
          turn({
            content: 'Svaret er **ferdig**.',
            citations: [citation()],
          }),
        )}
      />
    ))
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter til Word' }))
    flush()

    expect(exportChatAnswerMock).toHaveBeenCalledTimes(1)
    const [format, input] = exportChatAnswerMock.mock.calls[0] as [string, Record<string, unknown>]
    expect(format).toBe('docx')
    expect(input.content).toBe('Svaret er **ferdig**.')
    expect(input.sources).toEqual([{ title: 'NRK-artikkel', url: 'https://nrk.no/x' }])
    expect(input.element).toBeUndefined()
  })

  it('passes the rendered answer element for a pdf export', () => {
    render(() => <AssistantMessage {...props(turn())} />)
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre som PDF' }))
    flush()

    expect(exportChatAnswerMock).toHaveBeenCalledTimes(1)
    const [format, input] = exportChatAnswerMock.mock.calls[0] as [string, { element?: HTMLElement | null }]
    expect(format).toBe('pdf')
    expect(input.element).toBeInstanceOf(HTMLElement)
  })

  it('falls back to a title without a url when a citation has none', () => {
    render(() => (
      <AssistantMessage
        {...props(turn({ citations: [citation({ url: 'https://example.com/artikkel', title: '' })] }))}
      />
    ))
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter til Excel' }))
    flush()
    const [, input] = exportChatAnswerMock.mock.calls[0] as [string, { sources?: Array<{ title: string }> }]
    expect(input.sources?.[0]?.title).toBe('example.com')
  })

  it('shows a brief failure note when the export throws, and the reply stays on screen', async () => {
    exportChatAnswerMock.mockImplementation(() => {
      throw new Error('boom')
    })
    const { container } = render(() => <AssistantMessage {...props(turn())} />)
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter til PowerPoint' }))
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(/Eksport mislyktes/)
    expect(container.textContent).toContain('Svaret er')
  })

  // Verevon's drafts live in a document artifact while the reply is a note.
  const draft = {
    id: 'draft-1',
    kind: 'document',
    title: 'Utkast til salgsrapport – uke 37',
    content: '## Salgsrapport uke 37\n\nOmsetningen økte med 12 %.',
    version: 2,
  }

  it('exports the turn’s document, under its own title, when the reply itself is short', () => {
    render(() => (
      <AssistantMessage {...props(turn({ content: 'Utkastet er klart for gjennomgang i Resultat.', artifacts: [draft] }))} />
    ))
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter til Word' }))
    flush()
    const [, input] = exportChatAnswerMock.mock.calls[0] as [string, { content: string; title?: string }]
    expect(input.content).toBe(draft.content)
    expect(input.title).toBe(draft.title)
  })

  it('shows the dedicated export button for a short reply that produced a document', () => {
    render(() => <AssistantMessage {...props(turn({ content: 'Utkastet er klart.', artifacts: [draft] }))} />)
    expect(screen.getByRole('button', { name: 'Eksporter svar' })).toBeTruthy()
  })

  it('exports a long reply as written even when it also produced a document', () => {
    render(() => <AssistantMessage {...props(turn({ content: LONG_ANSWER, artifacts: [draft] }))} />)
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter til Word' }))
    flush()
    const [, input] = exportChatAnswerMock.mock.calls[0] as [string, { content: string; title?: string }]
    expect(input.content).toBe(LONG_ANSWER)
    expect(input.title).toBeUndefined()
  })

  it('renders the document off-screen for a pdf export of a short reply, not the note', () => {
    // Read at call time: the off-screen render is disposed right after the
    // call, which is safe only because print.ts copies the element at once.
    let printed: string | null | undefined
    exportChatAnswerMock.mockImplementation((_format: string, input: { element?: HTMLElement | null }) => {
      printed = input.element?.textContent
    })
    render(() => <AssistantMessage {...props(turn({ content: 'Utkastet er klart.', artifacts: [draft] }))} />)
    openOverflowMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre som PDF' }))
    flush()
    expect(exportChatAnswerMock.mock.calls[0]?.[0]).toBe('pdf')
    expect(printed).toContain('Omsetningen økte med 12 %.')
    expect(printed).not.toContain('Utkastet er klart.')
  })
})
