// @vitest-environment jsdom
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CONSOLE_FILTER_DEBOUNCE_MS,
  CONSOLE_FILTER_MAX_CHARS,
  ConsoleFilterBar,
  consoleEntryMatches,
  emptyConsoleFilter,
  filterConsoleEntries,
  normalizeConsoleLevel,
} from './ConsoleFilterBar'

const entries = [
  { level: 'error', text: 'Boom at checkout' },
  { level: 'warning', text: 'Deprecated API' },
  { level: 'log', text: 'boom again' },
]

describe('normalizeConsoleLevel', () => {
  it('maps the server aliases onto the four chips', () => {
    expect(normalizeConsoleLevel('warning')).toBe('warn')
    expect(normalizeConsoleLevel('log')).toBe('info')
    expect(normalizeConsoleLevel('verbose')).toBe('debug')
    expect(normalizeConsoleLevel('ERROR')).toBe('error')
    expect(normalizeConsoleLevel(undefined)).toBe('info')
  })
})

describe('filterConsoleEntries', () => {
  it('keeps every entry for the empty filter', () => {
    expect(filterConsoleEntries(entries, emptyConsoleFilter())).toHaveLength(3)
  })

  it('filters by level and by case-insensitive text', () => {
    expect(filterConsoleEntries(entries, { levels: ['error'], text: '' })).toEqual([entries[0]])
    expect(filterConsoleEntries(entries, { levels: [], text: 'BOOM' })).toHaveLength(2)
    expect(filterConsoleEntries(entries, { levels: ['info'], text: 'boom' })).toEqual([entries[2]])
  })

  it('matches the text only, never a level name', () => {
    expect(consoleEntryMatches({ level: 'error', text: 'ok' }, { levels: [], text: 'error' })).toBe(false)
  })
})

describe('ConsoleFilterBar', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('applies a level chip at once and shows the count', () => {
    const onChange = vi.fn()
    render(() => <ConsoleFilterBar value={emptyConsoleFilter()} total={3} shown={3} onChange={onChange} />)

    expect(screen.getByRole('status').textContent).toContain('3')
    const errorChip = document.querySelector('[data-level="error"]') as HTMLButtonElement
    fireEvent.click(errorChip)
    expect(onChange).toHaveBeenCalledWith({ levels: ['error'], text: '' })
  })

  it('debounces the text and clears it on Escape', () => {
    const onChange = vi.fn()
    render(() => <ConsoleFilterBar value={emptyConsoleFilter()} total={3} shown={3} onChange={onChange} />)

    const input = screen.getByRole('searchbox') as HTMLInputElement
    fireEvent.input(input, { target: { value: 'boom' } })
    expect(onChange).not.toHaveBeenCalled()
    vi.advanceTimersByTime(CONSOLE_FILTER_DEBOUNCE_MS)
    expect(onChange).toHaveBeenLastCalledWith({ levels: [], text: 'boom' })

    fireEvent.keyDown(input, { key: 'Escape' })
    expect(onChange).toHaveBeenLastCalledWith({ levels: [], text: '' })
    expect(input.value).toBe('')
  })

  it('caps the text at the maximum length', () => {
    const onChange = vi.fn()
    render(() => <ConsoleFilterBar value={emptyConsoleFilter()} total={0} shown={0} onChange={onChange} />)

    fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'a'.repeat(CONSOLE_FILTER_MAX_CHARS + 50) } })
    vi.advanceTimersByTime(CONSOLE_FILTER_DEBOUNCE_MS)
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as { text: string }
    expect(last.text).toHaveLength(CONSOLE_FILTER_MAX_CHARS)
  })
})
