// @vitest-environment jsdom

import { createRouter } from '@solidjs/router'
import { render } from '@/test/render'
import { cleanup, fireEvent, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import { I18nProvider } from '@/shared/i18n'
import { CoreNavbar, shouldDeferGlobalSlashSearch } from './CoreNavbar'
import { commandPaletteEnabled, setCommandPaletteFeature } from '@/features/command/command-registry'

afterEach(() => {
  cleanup()
  document.body.replaceChildren()
})

describe('shouldDeferGlobalSlashSearch', () => {
  it('lets the Inbox own slash when its local queue search is mounted', () => {
    const inboxSearch = document.createElement('input')
    inboxSearch.id = 'verevon-inbox-search'
    document.body.append(inboxSearch)

    expect(shouldDeferGlobalSlashSearch()).toBe(true)
  })

  it('keeps the global shortcut available outside Inbox', () => {
    expect(shouldDeferGlobalSlashSearch()).toBe(false)
  })
})

describe('CoreNavbar workspace control', () => {
  it('opens the shared assistant from the refined top bar', () => {
    const toggle = vi.fn()
    const Router = createRouter({ routes: [{ path: '/*all', component: () => (
      <CoreNavbar activeRoute="/dashboard" workspace={{ initial: 'A', name: 'Acme', plan: 'Expert' }} onSidebarToggle={() => {}} onAssistantToggle={toggle} />
    ) }] })
    render(() => <I18nProvider><Router /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /åpne verevon-assistent|open verevon assistant/i }))
    expect(toggle).toHaveBeenCalledOnce()
  })
  it('returns to the Engagement landing page from a Visibility breadcrumb', () => {
    render(() => <I18nProvider>{(() => { const TestRouter = createRouter({ routes: [{ path: "/*all", component: () => <CoreNavbar activeRoute="/engagement/visibility" offline workspace={{ initial: 'T', name: 'Triodelab', plan: 'Expert' }} /> }] }); return <TestRouter></TestRouter> })()}</I18nProvider>)
    expect(screen.getByRole('link', { name: /Engasjement|Engagement/ }).getAttribute('href')).toBe('/engagement')
    expect(screen.getByRole('link', { name: /Synlighet|Visibility/ }).getAttribute('href')).toBe('/engagement/visibility')
  })
  it('keeps the workspace switcher enabled by default in the authenticated shell', () => {
    render(() => (
      <I18nProvider>
        {(() => { const TestRouter = createRouter({ routes: [{ path: "/*all", component: () => (
              <CoreNavbar
                activeRoute="/studio/canvas"
                workspace={{ initial: 'A', name: 'Acme', plan: 'Trial' }}
              />
            ) }] }); return <TestRouter>{(props) => <>{props.children}</>}</TestRouter> })()}
      </I18nProvider>
    ))

    expect((screen.getByRole('button', { name: /acme.*trial/i }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('renders an explicitly supplied breadcrumb without expanding the product route contract', () => {
    render(() => (
      <I18nProvider>
        {(() => { const TestRouter = createRouter({ routes: [{ path: "/*all", component: () => (
              <CoreNavbar
                activeRoute="/studio/canvas"
                breadcrumbOverride={{ href: '/studio/pulse', moduleLabel: 'Marketing', tabLabel: 'Pulse' }}
                offline
                workspace={{ initial: 'A', name: 'Acme', plan: 'Local' }}
              />
            ) }] }); return <TestRouter>{(props) => <>{props.children}</>}</TestRouter> })()}
      </I18nProvider>
    ))

    expect(screen.getByRole('link', { name: 'Marketing' }).getAttribute('href')).toBe('/studio/pulse')
    expect(screen.getByRole('link', { name: 'Pulse' }).getAttribute('href')).toBe('/studio/pulse')
  })
})

describe('CoreNavbar command palette switch (DEPTH-WS13-S6-1)', () => {
  const json = (body: unknown): Response => new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })

  // The shell reads features.command_palette once at boot, and the palette, once open, reads its room and agent
  // indexes. `features` answers the first; the indexes answer empty lists, so an open palette has real arrays to
  // work with; anything else is a 404. Returns the stub so a test can see the calls it got.
  function stubShell(features: () => Response): ReturnType<typeof vi.fn> {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/spaces/features')) return features()
      if (url.endsWith('/api/v1/spaces')) return json({ data: { spaces: [] } })
      if (url.endsWith('/api/v1/agents/installations')) return json({ data: { definitions: [] } })
      return new Response(null, { status: 404 })
    })
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  const featuresAnswer = (features: Record<string, boolean>) => () => json({ data: { features } })

  function renderNavbar(): void {
    const TestRouter = createRouter({ routes: [{ path: '/*all', component: () => (
      <CoreNavbar activeRoute="/dashboard" workspace={{ initial: 'A', name: 'Acme', plan: 'Expert' }} />
    ) }] })
    render(() => <I18nProvider><TestRouter /></I18nProvider>)
  }

  // Ctrl+K as the browser sends it. Pending Solid work is flushed before the press, so the hotkey layer is in
  // place when the key arrives, and again after it, so the dialog the press opened is in the DOM.
  function pressCtrlK(): void {
    flush()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
    flush()
  }

  afterEach(() => {
    // Unmount before the flag is cleared. A dialog still open would otherwise swap surfaces during teardown and
    // read the palette's indexes through whatever fetch is left.
    cleanup()
    vi.unstubAllGlobals()
    setCommandPaletteFeature(undefined)
  })

  it('keeps the old knowledge search dialog when the gateway says the palette is off, even if the build turns it on', async () => {
    setCommandPaletteFeature(true)
    stubShell(featuresAnswer({ command_palette: false }))
    renderNavbar()
    await waitFor(() => expect(commandPaletteEnabled()).toBe(false))
    await waitFor(() => {
      pressCtrlK()
      expect(screen.getByRole('dialog', { name: /knowledge search|kunnskapssøk/i })).toBeTruthy()
    })
    expect(screen.queryByRole('dialog', { name: /command palette|kommandopalett/i })).toBeNull()
  })

  it('brings the palette and its hotkeys up when the gateway says it is on, after the shell has mounted', async () => {
    setCommandPaletteFeature(false)
    stubShell(featuresAnswer({ command_palette: true }))
    renderNavbar()
    await waitFor(() => expect(commandPaletteEnabled()).toBe(true))
    await waitFor(() => {
      pressCtrlK()
      expect(screen.getByRole('dialog', { name: /command palette|kommandopalett/i })).toBeTruthy()
    })
    expect(screen.queryByRole('dialog', { name: /knowledge search|kunnskapssøk/i })).toBeNull()
  })

  it('leaves the switch as it was when the features read fails', async () => {
    setCommandPaletteFeature(true)
    const fetchMock = stubShell(() => new Response(null, { status: 503 }))
    renderNavbar()
    await waitFor(() => expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/api/v1/spaces/features'))).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(commandPaletteEnabled()).toBe(true)
  })
})
