import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import { ThreadReplayView } from './ThreadReplayView'
import { readThreadExport, THREAD_EXPORT_FORMAT, type ThreadExport } from './thread-export-schema'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))

afterEach(() => {
  cleanup()
})

// A redacted v1 export: two turns, the second with a document, and one API run. Read through the schema so the
// fixture is the same typed document the export menu hands the viewer.
function documentOf(overrides: Record<string, unknown> = {}): ThreadExport {
  const reading = readThreadExport(
    JSON.stringify({
      format: THREAD_EXPORT_FORMAT,
      version: 1,
      exported_at: '2026-11-05T10:00:00Z',
      profile: 'redacted',
      thread: { id: 'thread-1', title: 'Plan', created_at: '2026-11-04T09:00:00Z' },
      messages: [
        {
          id: 'm-1',
          role: 'user',
          content: 'Hei fra brukeren',
          created_at: '2026-11-04T09:00:01Z',
          artifacts: [],
          citations: [],
          metadata: {},
        },
        {
          id: 'm-2',
          role: 'assistant',
          content: 'Svar fra Verevon',
          created_at: '2026-11-04T09:00:05Z',
          artifacts: [{ kind: 'document', title: 'Notat' }],
          citations: [],
          metadata: {},
        },
      ],
      runs: [
        {
          id: 'run-1',
          status: 'completed',
          origin: 'api',
          model: 'model-a',
          input_tokens: 120,
          output_tokens: 40,
          created_at: '2026-11-04T09:00:02Z',
        },
      ],
      truncated: false,
      ...overrides,
    }),
  )
  if (reading.status !== 'ok') throw new Error(`the fixture is not a valid export: ${reading.status}`)
  return reading.document
}

describe('ThreadReplayView', () => {
  it('shows the first turn, the runs behind the conversation and what the copy leaves out', () => {
    render(() => <ThreadReplayView conversation={documentOf()} onClose={() => {}} />)

    const dialog = screen.getByRole('dialog', { name: 'Plan' })
    expect(dialog.textContent).toContain('Hei fra brukeren')
    expect(screen.getByText('Completed')).toBeTruthy()
    expect(screen.getByText('API')).toBeTruthy()
    expect(screen.getByText('in 120 · out 40')).toBeTruthy()
    expect(screen.getByText('Message 1 of 2')).toBeTruthy()
    expect(
      screen.getByText(
        'Tool arguments and results, connector data, citation snippets, URL query strings and attachment names are left out.',
      ),
    ).toBeTruthy()
  })

  it('steps through the turns and stops at either end', () => {
    render(() => <ThreadReplayView conversation={documentOf()} onClose={() => {}} />)
    const previous = screen.getByRole('button', { name: 'Previous message' }) as HTMLButtonElement
    const next = screen.getByRole('button', { name: 'Next message' }) as HTMLButtonElement

    expect(previous.disabled).toBe(true)
    fireEvent.click(next)
    flush()

    expect(screen.getByText('Message 2 of 2')).toBeTruthy()
    expect(screen.getByText('Svar fra Verevon')).toBeTruthy()
    expect(screen.getByText('document: Notat')).toBeTruthy()
    expect(next.disabled).toBe(true)

    fireEvent.click(previous)
    flush()
    expect(screen.getByText('Message 1 of 2')).toBeTruthy()
  })

  it('shows the turn text as plain text, never as markup', () => {
    const markup = '<img src=x onerror="alert(1)">'
    const turn = {
      id: 'm-1',
      role: 'user',
      content: markup,
      created_at: '2026-11-04T09:00:01Z',
      artifacts: [],
      citations: [],
      metadata: {},
    }
    render(() => <ThreadReplayView conversation={documentOf({ messages: [turn] })} onClose={() => {}} />)

    expect(document.querySelector('img')).toBeNull()
    expect(screen.getByText(markup)).toBeTruthy()
  })

  it('closes on Escape and from the close button', () => {
    const onClose = vi.fn()
    render(() => <ThreadReplayView conversation={documentOf()} onClose={onClose} />)
    // The Escape listener is attached by an effect after mount.
    flush()

    fireEvent.keyDown(document, { key: 'Escape' })
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))

    expect(onClose).toHaveBeenCalledTimes(2)
  })

  it('says so when the conversation has no messages', () => {
    render(() => <ThreadReplayView conversation={documentOf({ messages: [] })} onClose={() => {}} />)

    expect(screen.getByText('This conversation has no messages.')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Next message' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('says when the export stops at its size limit', () => {
    render(() => <ThreadReplayView conversation={documentOf({ truncated: true })} onClose={() => {}} />)

    expect(screen.getByRole('status').textContent).toContain('size limit')
  })
})
