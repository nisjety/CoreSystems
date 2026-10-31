// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { omniboxHistoryStorageKey } from '@/shared/session/home-local-content'

const identity = vi.hoisted(() => ({ user: { id: 'user-a' } as { id: string } | null, activeOrg: { id: 'org-a' } as { id: string } | null }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { addOmniboxVisit, readOmniboxHistory } from './omnibox-history'
import { intentForRow, nextOmniboxIndex, OmniboxField } from './OmniboxField'
import type { OmniboxIntent } from './omnibox-resolver'

const keyA = omniboxHistoryStorageKey('user-a', 'org-a')!

function Harness(props: { onIntent: (intent: OmniboxIntent) => void; searchEnabled?: boolean; onHistoryChange?: () => void }) {
  const [value, setValue] = createSignal('')
  return (
    <div onKeyDown={parentKeyDown}>
      <OmniboxField
        value={value()}
        label="Nettleseradresse"
        searchEnabled={props.searchEnabled ?? true}
        askEnabled
        fallbackUrl="https://current.example/"
        historyKey={() => keyA}
        zdr={() => false}
        onValueChange={setValue}
        onIntent={props.onIntent}
        onHistoryChange={props.onHistoryChange}
      />
    </div>
  )
}

let parentKeyDown = vi.fn()

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  __resetRetentionForTests()
  identity.user = { id: 'user-a' }
  identity.activeOrg = { id: 'org-a' }
  parentKeyDown = vi.fn()
})

afterEach(() => vi.restoreAllMocks())

describe('keyboard model', () => {
  it('moves the highlight down and up, wrapping at both ends', () => {
    expect(nextOmniboxIndex(-1, 3, 'ArrowDown')).toBe(0)
    expect(nextOmniboxIndex(0, 3, 'ArrowDown')).toBe(1)
    expect(nextOmniboxIndex(2, 3, 'ArrowDown')).toBe(0)
    expect(nextOmniboxIndex(-1, 3, 'ArrowUp')).toBe(2)
    expect(nextOmniboxIndex(0, 3, 'ArrowUp')).toBe(2)
    expect(nextOmniboxIndex(0, 0, 'ArrowDown')).toBe(-1)
  })

  it('turns a suggestion into a search only while search is on', () => {
    expect(intentForRow({ kind: 'suggestion', id: 's', text: 'laks' }, true)).toEqual({ kind: 'search', query: 'laks' })
    expect(intentForRow({ kind: 'suggestion', id: 's', text: 'laks' }, false)).toEqual({
      kind: 'noop',
      reason: 'search_disabled',
    })
    expect(intentForRow({ kind: 'history', id: 'h', url: 'https://a.example/', title: 'A' }, false)).toEqual({
      kind: 'navigate',
      url: 'https://a.example/',
    })
  })
})

describe('OmniboxField', () => {
  it('offers the explicit rows for typed text and runs the highlighted one with Enter', async () => {
    const onIntent = vi.fn()
    render(() => <Harness onIntent={onIntent} />)
    const input = screen.getByRole('combobox', { name: 'Nettleseradresse' })

    fireEvent.input(input, { target: { value: 'norwegian salmon' } })
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())
    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([
      'Søk på nettet etter «norwegian salmon»',
      'Spør Verevon om «norwegian salmon»',
    ])

    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onIntent).toHaveBeenLastCalledWith({ kind: 'search', query: 'norwegian salmon' })
  })

  it('submits the typed address with Enter when no row is highlighted', async () => {
    const onIntent = vi.fn()
    render(() => <Harness onIntent={onIntent} />)
    const input = screen.getByRole('combobox', { name: 'Nettleseradresse' })

    fireEvent.input(input, { target: { value: 'example.com' } })
    flush()
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onIntent).toHaveBeenLastCalledWith({ kind: 'navigate', url: 'https://example.com' })
  })

  it('navigates an empty field to the current page on Enter', () => {
    const onIntent = vi.fn()
    render(() => <Harness onIntent={onIntent} />)
    fireEvent.keyDown(screen.getByRole('combobox', { name: 'Nettleseradresse' }), { key: 'Enter' })
    expect(onIntent).toHaveBeenLastCalledWith({ kind: 'navigate', url: 'https://current.example/' })
  })

  it('consumes Escape: the dropdown closes and the event does not bubble', async () => {
    render(() => <Harness onIntent={vi.fn()} />)
    const input = screen.getByRole('combobox', { name: 'Nettleseradresse' })
    fireEvent.input(input, { target: { value: 'norwegian salmon' } })
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())

    fireEvent.keyDown(input, { key: 'Escape' })
    expect(parentKeyDown).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.queryByRole('listbox')).toBeNull())
  })

  it('forgets the highlighted remembered page on Shift+Delete', async () => {
    addOmniboxVisit(keyA, { url: 'https://www.vg.no/', title: 'VG' })
    const onHistoryChange = vi.fn()
    render(() => <Harness onIntent={vi.fn()} onHistoryChange={onHistoryChange} />)
    const input = screen.getByRole('combobox', { name: 'Nettleseradresse' })

    fireEvent.input(input, { target: { value: 'vg' } })
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    fireEvent.keyDown(input, { key: 'Delete', shiftKey: true })

    expect(readOmniboxHistory(keyA)).toEqual([])
    expect(onHistoryChange).toHaveBeenCalledTimes(1)
  })

  it('says so when free text is typed while search is off', async () => {
    render(() => <Harness onIntent={vi.fn()} searchEnabled={false} />)
    fireEvent.input(screen.getByRole('combobox', { name: 'Nettleseradresse' }), { target: { value: 'norwegian salmon' } })
    await waitFor(() => expect(screen.getByText('Søk er ikke slått på her.')).toBeTruthy())
  })
})
