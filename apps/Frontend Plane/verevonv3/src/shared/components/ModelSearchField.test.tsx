// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, untrack } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ModelSearchField } from './ModelSearchField'

// The field narrows a model menu. Escape clears a query and does not reach the menu, so the menu stays
// open; with no query, Escape is the menu's to handle.

afterEach(() => cleanup())

function Menu(props: { start?: string; onMenuKey: (key: string) => void; onQuery: (query: string) => void }) {
  const [query, setQuery] = createSignal(untrack(() => props.start ?? ''))
  return (
    <div onKeyDown={(event) => props.onMenuKey(event.key)}>
      <ModelSearchField
        value={query()}
        controlsId="model-catalog"
        onChange={(next) => {
          setQuery(next)
          props.onQuery(next)
        }}
      />
    </div>
  )
}

describe('ModelSearchField', () => {
  it('is labelled, names the list it filters, and reports each change', () => {
    const onQuery = vi.fn()
    render(() => <Menu onMenuKey={() => undefined} onQuery={onQuery} />)

    const field = screen.getByRole('searchbox', { name: 'Søk etter modell' })
    expect(field.getAttribute('aria-controls')).toBe('model-catalog')
    fireEvent.input(field, { target: { value: 'sonnet' } })
    expect(onQuery).toHaveBeenLastCalledWith('sonnet')
  })

  it('clears a query on Escape without closing the menu', () => {
    const onMenuKey = vi.fn()
    const onQuery = vi.fn()
    render(() => <Menu start="gpt" onMenuKey={onMenuKey} onQuery={onQuery} />)

    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Escape' })
    expect(onQuery).toHaveBeenLastCalledWith('')
    expect(onMenuKey).not.toHaveBeenCalled()
  })

  it('leaves Escape to the menu when there is no query', () => {
    const onMenuKey = vi.fn()
    const onQuery = vi.fn()
    render(() => <Menu onMenuKey={onMenuKey} onQuery={onQuery} />)

    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Escape' })
    expect(onQuery).not.toHaveBeenCalled()
    expect(onMenuKey).toHaveBeenCalledWith('Escape')
  })

  it('does not clear the query on other keys', () => {
    const onQuery = vi.fn()
    render(() => <Menu start="gpt" onMenuKey={() => undefined} onQuery={onQuery} />)

    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'a' })
    expect(onQuery).not.toHaveBeenCalled()
  })
})
