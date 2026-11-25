// @vitest-environment jsdom

import { createRouter } from '@solidjs/router'
import { render } from '@/test/render'
import { cleanup, fireEvent, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import { I18nProvider } from '@/shared/i18n'
import { CoreNavbar, shouldDeferGlobalSlashSearch } from './CoreNavbar'
import { commandPaletteEnabled, registerCommands, setCommandPaletteFeature } from '@/features/command/command-registry'
import { getCommandPaletteFeature } from '@/shared/api/spaces-client/entries'

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

  it('keeps the old knowledge search dialog when the gateway says the palette is off, even if the palette was on', async () => {
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

  // Seeded off: under vitest the build default is on, so a reset to "no answer" would read true and fail here.
  it('leaves the switch as it was when the features read fails', async () => {
    setCommandPaletteFeature(false)
    const fetchMock = stubShell(() => new Response(null, { status: 503 }))
    renderNavbar()
    const featuresCall = () => fetchMock.mock.calls.findIndex(([url]) => String(url).endsWith('/api/v1/spaces/features'))
    await waitFor(() => expect(featuresCall()).toBeGreaterThanOrEqual(0))
    // A second read through the same client and the same failing fetch, started after the shell's. Both chains
    // have the same depth, so by the time this one settles the shell's read has settled and its continuation has
    // run. That makes the check below independent of how many ticks the read takes.
    await expect(getCommandPaletteFeature()).resolves.toBeUndefined()
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
    expect(commandPaletteEnabled()).toBe(false)
  })

  it('makes no features read on an offline surface and leaves the switch alone', async () => {
    setCommandPaletteFeature(false)
    const fetchMock = stubShell(featuresAnswer({ command_palette: true }))
    const TestRouter = createRouter({ routes: [{ path: '/*all', component: () => (
      <CoreNavbar activeRoute="/dashboard" offline workspace={{ initial: 'A', name: 'Acme', plan: 'Expert' }} />
    ) }] })
    render(() => <I18nProvider><TestRouter /></I18nProvider>)
    // Settle anything the mount started; an online shell has read the features by this point.
    await getCommandPaletteFeature()
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
    const shellFeatureReads = fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/api/v1/spaces/features'))
    // Exactly one read: the settle call above. The offline shell made none.
    expect(shellFeatureReads).toHaveLength(1)
    expect(commandPaletteEnabled()).toBe(false)
  })

  // The link the room search relies on: SpaceRoomSearch registers a `mod+f` command, and only the hotkey layer the
  // navbar installs while the switch is on turns a Ctrl/Cmd+F press into that command. A stand-in command with the
  // same binding proves the switch -> layer -> mod+f step without mounting a room.
  function registerRoomSearchStandIn(): { run: ReturnType<typeof vi.fn>; dispose: () => void } {
    const run = vi.fn()
    const dispose = registerCommands([
      { id: 'test.space.search', labelNo: 'Søk', labelEn: 'Search', group: 'room', hotkey: 'mod+f', kind: 'ui', run },
    ])
    return { run, dispose }
  }

  function pressModF(modifier: 'ctrlKey' | 'metaKey'): KeyboardEvent {
    flush()
    const event = new KeyboardEvent('keydown', { key: 'f', [modifier]: true, bubbles: true, cancelable: true })
    window.dispatchEvent(event)
    flush()
    return event
  }

  it('turns Ctrl+F and Cmd+F into the room search command when the gateway says the palette is on', async () => {
    setCommandPaletteFeature(false)
    stubShell(featuresAnswer({ command_palette: true }))
    const standIn = registerRoomSearchStandIn()
    try {
      renderNavbar()
      await waitFor(() => expect(commandPaletteEnabled()).toBe(true))
      await waitFor(() => {
        expect(pressModF('ctrlKey').defaultPrevented).toBe(true)
        expect(standIn.run).toHaveBeenCalled()
      })
      standIn.run.mockClear()
      expect(pressModF('metaKey').defaultPrevented).toBe(true)
      expect(standIn.run).toHaveBeenCalledOnce()
    } finally {
      standIn.dispose()
    }
  })

  it('leaves Ctrl+F to the browser when the gateway says the palette is off', async () => {
    setCommandPaletteFeature(true)
    stubShell(featuresAnswer({ command_palette: false }))
    const standIn = registerRoomSearchStandIn()
    try {
      renderNavbar()
      await waitFor(() => expect(commandPaletteEnabled()).toBe(false))
      const event = pressModF('ctrlKey')
      expect(event.defaultPrevented).toBe(false)
      expect(standIn.run).not.toHaveBeenCalled()
    } finally {
      standIn.dispose()
    }
  })
})
