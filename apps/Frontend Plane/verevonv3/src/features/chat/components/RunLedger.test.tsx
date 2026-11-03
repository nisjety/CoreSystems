// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createSignal, flush } from 'solid-js'
import type { VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import { RunLedger } from './RunLedger'

// Without a provider the page renders in its default locale, Norwegian.

afterEach(() => {
  cleanup()
  Reflect.deleteProperty(navigator, 'clipboard')
  vi.restoreAllMocks()
})

const at = (seconds: number) => new Date(Date.UTC(2026, 10, 3, 10, 0, seconds)).toISOString()

/** One turn: a connected run, one tool call with its result, and the terminal frame. */
const turnEvents = (): VerevonUiEvent[] => [
  { type: 'run.connected', at: at(0), requestId: 'req-1', runId: 'run-1', turnId: 'turn-1' },
  { type: 'tool.call', at: at(1), id: 'c1', name: 'web_search', runId: 'run-1', turnId: 'turn-1' },
  { type: 'tool.result', at: at(3), id: 'c1', status: 'completed', runId: 'run-1', turnId: 'turn-1' },
  { type: 'message.done', at: at(4), requestId: 'req-1', turnId: 'turn-1' },
]

function rowsOf(container: HTMLElement): HTMLButtonElement[] {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('[data-ledger-row]'))
}

function buttonNamed(container: HTMLElement, label: string): HTMLButtonElement {
  const found = Array.from(container.querySelectorAll('button')).find((element) => element.textContent?.trim() === label)
  if (!found) throw new Error(`no button named "${label}"`)
  return found
}

describe('RunLedger list', () => {
  it('lists the turn with its records, and a call as one row with its duration', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const rows = rowsOf(container)
    expect(rows).toHaveLength(3)
    expect(rows[1]?.textContent).toContain('web_search')
    expect(rows[1]?.textContent).toContain('2.0 s')
    expect(container.querySelector('.verevon-chat-ledger__summary')?.textContent).toBe('1 turer · 1 verktøykall')
  })

  it('says when no event has been recorded yet', () => {
    const { container } = render(() => <RunLedger events={[]} />)
    expect(container.textContent).toContain('Ingen hendelser er registrert ennå.')
    expect(rowsOf(container)).toHaveLength(0)
  })

  it('keeps events no turn owns in an unattributed group, and drops none', () => {
    const events: VerevonUiEvent[] = [
      { type: 'lifecycle.retry', at: at(0) },
      { type: 'lifecycle.compaction', at: at(1) },
    ]
    const { container } = render(() => <RunLedger events={events} />)
    expect(container.textContent).toContain('Ikke knyttet til en tur')
    expect(rowsOf(container)).toHaveLength(2)
  })

  it('collapses a turn from its header without losing its records', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const head = container.querySelector<HTMLButtonElement>('.verevon-chat-ledger__turn-head')
    expect(head?.getAttribute('aria-expanded')).toBe('true')
    if (head) fireEvent.click(head)
    flush()
    expect(head?.getAttribute('aria-expanded')).toBe('false')
    expect(rowsOf(container)).toHaveLength(0)
  })

  it('shows the cap notice when the view was cut', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} replayTruncated />)
    expect(container.querySelector('[role="status"]')?.textContent).toContain('avkortet ved 5 000 hendelser')
  })
})

describe('RunLedger keyboard', () => {
  it('moves focus between rows with the arrow keys, Home and End, and selects nothing', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const rows = rowsOf(container)
    rows[0]?.focus()
    if (rows[0]) fireEvent.keyDown(rows[0], { key: 'ArrowDown' })
    expect(document.activeElement).toBe(rows[1])
    if (rows[1]) fireEvent.keyDown(rows[1], { key: 'End' })
    expect(document.activeElement).toBe(rows[2])
    if (rows[2]) fireEvent.keyDown(rows[2], { key: 'ArrowDown' })
    expect(document.activeElement).toBe(rows[2])
    if (rows[2]) fireEvent.keyDown(rows[2], { key: 'Home' })
    expect(document.activeElement).toBe(rows[0])
    expect(container.querySelector('.verevon-chat-ledger__inspector')).toBeNull()
  })

  it('opens the inspector for a clicked row, and Escape closes it', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const rows = rowsOf(container)
    if (rows[1]) fireEvent.click(rows[1])
    flush()
    expect(container.querySelector('.verevon-chat-ledger__inspector')?.textContent).toContain('c1')
    expect(rows[1]?.getAttribute('aria-current')).toBe('true')
    if (rows[1]) fireEvent.keyDown(rows[1], { key: 'Escape' })
    flush()
    expect(container.querySelector('.verevon-chat-ledger__inspector')).toBeNull()
  })
})

describe('RunLedger inspector', () => {
  it('copies the request id and the run id exactly as recorded', async () => {
    const writeText = vi.fn(async (_value: string) => undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const rows = rowsOf(container)
    if (rows[0]) fireEvent.click(rows[0])
    flush()

    fireEvent.click(buttonNamed(container, 'Kopier forespørsel-id'))
    await vi.waitFor(() => {
      expect(container.querySelector('.verevon-chat-ledger__copy-status')?.textContent).toBe('Forespørsel-id er kopiert.')
    })
    expect(writeText).toHaveBeenLastCalledWith('req-1')

    fireEvent.click(buttonNamed(container, 'Kopier kjørings-id'))
    await vi.waitFor(() => {
      expect(container.querySelector('.verevon-chat-ledger__copy-status')?.textContent).toBe('Kjørings-id er kopiert.')
    })
    expect(writeText).toHaveBeenLastCalledWith('run-1')
  })

  it('says when the clipboard refuses, and leaves the id in view', async () => {
    const writeText = vi.fn(async (_value: string) => {
      throw new Error('denied')
    })
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const { container } = render(() => <RunLedger events={turnEvents()} />)
    const rows = rowsOf(container)
    if (rows[0]) fireEvent.click(rows[0])
    flush()
    fireEvent.click(buttonNamed(container, 'Kopier forespørsel-id'))
    await vi.waitFor(() => {
      expect(container.querySelector('.verevon-chat-ledger__copy-status')?.textContent).toContain('Kunne ikke kopiere')
    })
    expect(container.querySelector('.verevon-chat-ledger__inspector code')?.textContent).toBe('req-1')
  })

  it('says a call has no recorded timing rather than showing an empty field', () => {
    const events: VerevonUiEvent[] = [{ type: 'tool.call', at: at(0), id: 'c9', name: 'read_file', turnId: 'turn-1' }]
    const { container } = render(() => <RunLedger events={events} />)
    const rows = rowsOf(container)
    if (rows[0]) fireEvent.click(rows[0])
    flush()
    const inspector = container.querySelector('.verevon-chat-ledger__inspector')
    expect(inspector?.textContent).toContain('Varigheten er ikke registrert for dette kallet.')
    expect(inspector?.textContent).toContain('ikke registrert')
  })

  it('says a result with no recorded call is unmatched', () => {
    const events: VerevonUiEvent[] = [{ type: 'tool.result', at: at(0), id: 'orphan', status: 'completed', turnId: 'turn-1' }]
    const { container } = render(() => <RunLedger events={events} />)
    const rows = rowsOf(container)
    if (rows[0]) fireEvent.click(rows[0])
    flush()
    expect(container.querySelector('.verevon-chat-ledger__inspector')?.textContent).toContain(
      'Ingen kall med denne id-en er registrert i turen.',
    )
  })
})

describe('RunLedger Inspect request', () => {
  it('selects the call the request names, and takes no focus', () => {
    const [request, setRequest] = createSignal<{ callId: string } | null>(null)
    const { container } = render(() => <RunLedger events={turnEvents()} inspectRequest={request()} />)
    expect(container.querySelector('.verevon-chat-ledger__inspector')).toBeNull()
    setRequest({ callId: 'c1' })
    flush()
    expect(container.querySelector('.verevon-chat-ledger__inspector')?.textContent).toContain('c1')
    expect(document.activeElement).toBe(document.body)
  })

  it('says so when the requested call is not in this view', () => {
    const { container } = render(() => <RunLedger events={turnEvents()} inspectRequest={{ callId: 'gone' }} />)
    flush()
    expect(container.textContent).toContain('Kallet er ikke i denne visningen.')
    expect(container.querySelector('.verevon-chat-ledger__inspector')).toBeNull()
  })
})
