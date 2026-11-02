// @vitest-environment jsdom

import { createRouter, memoryHistory, useNavigate } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { QueryProvider } from '@/app/providers/QueryProvider'
import { I18nProvider } from '@/shared/i18n'
import { clearSession } from '@/shared/session/session-store'
import { CoreShell } from './CoreShell'

// The sidebar is a drawer laid over the page at phone width. Picking a room closes it there; at
// desktop width the column stays as it was. `matchMedia` is the shell's phone test.
function stubViewport(phone: boolean) {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: phone && query.includes('max-width'),
    media: query,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
    onchange: null,
  }))
}

function Navigation() {
  const navigate = useNavigate()
  return (
    <>
      <button type="button" onClick={() => navigate('/spaces/room_1')}>go-room</button>
      <button type="button" onClick={() => navigate('/spaces/room_1?thread_id=thr_1#chat')}>go-thread</button>
      <button type="button" onClick={() => navigate('/spaces')}>go-index</button>
      <button type="button" onClick={() => navigate('/settings')}>go-settings</button>
    </>
  )
}

function renderShell(path: string) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <CoreShell><Navigation /></CoreShell> }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  render(() => (
    <QueryProvider>
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    </QueryProvider>
  ))
}

const expanded = () => document.querySelector('.core-product-shell')?.getAttribute('data-sidebar-expanded')

async function go(name: string) {
  fireEvent.click(screen.getByRole('button', { name }))
  flush()
  await Promise.resolve()
  flush()
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

function stubFetch() {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: { spaces: [] } }), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

describe('CoreShell: the drawer after picking a room', () => {
  it('closes the drawer on the rooms index when a room is opened at phone width', async () => {
    stubFetch()
    stubViewport(true)
    renderShell('/spaces')
    expect(expanded()).toBe('true')
    await go('go-room')
    await waitFor(() => expect(expanded()).toBe('false'))
  })

  it('closes the drawer when a conversation is picked in the room that is already open', async () => {
    stubFetch()
    stubViewport(true)
    renderShell('/spaces/room_1')
    // An open room starts with the drawer closed at phone width; the menu button opens it.
    expect(expanded()).toBe('false')
    fireEvent.click(screen.getAllByRole('button', { name: /utvid sidefelt|expand sidebar/i })[0]!)
    flush()
    await waitFor(() => expect(expanded()).toBe('true'))
    await go('go-thread')
    await waitFor(() => expect(expanded()).toBe('false'))
  })

  it('leaves the drawer open for navigation that is not an open room', async () => {
    stubFetch()
    stubViewport(true)
    renderShell('/spaces')
    expect(expanded()).toBe('true')
    await go('go-settings')
    expect(expanded()).toBe('true')
    await go('go-index')
    expect(expanded()).toBe('true')
  })

  it('keeps the desktop sidebar open when a room is opened', async () => {
    stubFetch()
    stubViewport(false)
    renderShell('/spaces')
    expect(expanded()).toBe('true')
    await go('go-room')
    expect(expanded()).toBe('true')
    await go('go-thread')
    expect(expanded()).toBe('true')
  })
})
