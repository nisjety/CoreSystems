import { fireEvent, render, screen, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { I18nProvider, localeStorageKey } from '@/shared/i18n'
import { SpaceCockpit, useActiveSpaceTab } from './SpaceCockpit'

afterEach(() => {
  vi.unstubAllEnvs()
  if (typeof window !== 'undefined') {
    window.location.hash = ''
    window.localStorage.removeItem(localeStorageKey)
  }
})

describe('SpaceCockpit', () => {
  it('renders the six tabs the adoption plan specifies', () => {
    render(() => <SpaceCockpit />)
    for (const label of ['Samtaler', 'Arbeid', 'Kunnskap', 'Aktivitet', 'Agent', 'Medlemmer']) {
      expect(screen.getByRole('tab', { name: label })).toBeTruthy()
    }
  })

  describe('locale', () => {
    it('renders English tab labels and copy under an English locale, not hardcoded Norwegian', () => {
      window.localStorage.setItem(localeStorageKey, 'en')
      render(() => (
        <I18nProvider>
          <SpaceCockpit initialTab="kunnskap" />
        </I18nProvider>
      ))
      for (const label of ['Chat', 'Work', 'Knowledge', 'Activity', 'Agent', 'Members']) {
        expect(screen.getByRole('tab', { name: label })).toBeTruthy()
      }
      expect(screen.getByRole('tablist').getAttribute('aria-label')).toBe('Space views')
      expect(screen.getByText(/Data Plane has not published/)).toBeTruthy()
      expect(within(screen.getByRole('tabpanel')).getByText(/missing connection, not an empty Space/)).toBeTruthy()
    })
  })

  it('shows supplied content for the active tab', () => {
    render(() => (
      <SpaceCockpit initialTab="aktivitet" tabs={{ aktivitet: <p>Ekte aktivitet</p> }} />
    ))
    expect(screen.getByText('Ekte aktivitet')).toBeTruthy()
  })

  describe('an unsupplied tab is honest rather than merely empty', () => {
    it('names the owner plane that has not published yet', () => {
      render(() => <SpaceCockpit initialTab="kunnskap" />)
      expect(screen.getByText(/Data Plane har ikke publisert/)).toBeTruthy()
    })

    it('says plainly that this is a missing connection, not an empty room', () => {
      render(() => <SpaceCockpit initialTab="arbeid" />)
      expect(within(screen.getByRole('tabpanel')).getByText(/manglende kobling, ikke et tomt rom/)).toBeTruthy()
    })

    it('attributes Members to Control Plane, which owns membership', () => {
      render(() => <SpaceCockpit initialTab="medlemmer" />)
      expect(screen.getByText(/Control Plane har ikke publisert/)).toBeTruthy()
    })
  })

  describe('tab selection', () => {
    it('switches panels on click', () => {
      render(() => (
        <SpaceCockpit tabs={{ chat: <p>Tråder</p>, medlemmer: <p>Roster</p> }} />
      ))
      expect(screen.getByText('Tråder')).toBeTruthy()
      fireEvent.click(screen.getByRole('tab', { name: 'Medlemmer' }))
      expect(screen.getByText('Roster')).toBeTruthy()
    })

    it('marks exactly one tab selected', () => {
      render(() => <SpaceCockpit initialTab="agent" />)
      const selected = screen.getAllByRole('tab').filter((tab) => tab.getAttribute('aria-selected') === 'true')
      expect(selected).toHaveLength(1)
      expect(selected[0]?.textContent).toBe('Agent')
    })

    it('keeps only the active tab in the tab order, so the strip is one stop', () => {
      render(() => <SpaceCockpit initialTab="agent" />)
      const focusable = screen.getAllByRole('tab').filter((tab) => tab.getAttribute('tabindex') === '0')
      expect(focusable).toHaveLength(1)
    })

    it('moves between tabs with the arrow keys', () => {
      render(() => <SpaceCockpit initialTab="chat" />)
      const list = screen.getByRole('tablist')
      fireEvent.keyDown(list, { key: 'ArrowRight' })
      flush()
      expect(screen.getByRole('tab', { name: 'Arbeid' }).getAttribute('aria-selected')).toBe('true')
    })

    it('moves focus with its roving selection when using arrow keys', () => {
      render(() => <SpaceCockpit initialTab="chat" />)
      const first = screen.getByRole('tab', { name: 'Samtaler' })
      first.focus()
      fireEvent.keyDown(screen.getByRole('tablist'), { key: 'ArrowRight' })
      expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Arbeid' }))
    })

    it('wraps around at the ends rather than dead-ending', () => {
      render(() => <SpaceCockpit initialTab="chat" />)
      const list = screen.getByRole('tablist')
      fireEvent.keyDown(list, { key: 'ArrowLeft' })
      flush()
      expect(screen.getByRole('tab', { name: 'Medlemmer' }).getAttribute('aria-selected')).toBe('true')
    })
  })

  describe('deep links', () => {
    it('opens the tab named in the hash', () => {
      window.location.hash = '#aktivitet'
      render(() => <SpaceCockpit tabs={{ aktivitet: <p>Fra dyplenke</p> }} />)
      expect(screen.getByText('Fra dyplenke')).toBeTruthy()
    })

    it('falls back to the default tab for an unknown hash instead of rendering nothing', () => {
      window.location.hash = '#noe-som-ikke-finnes'
      render(() => <SpaceCockpit tabs={{ chat: <p>Standard</p> }} />)
      expect(screen.getByText('Standard')).toBeTruthy()
    })

    it.each([
      ['members', 'medlemmer', 'Medlemsinnhold'],
      ['work', 'arbeid', 'Arbeidsinnhold'],
      ['activity', 'aktivitet', 'Aktivitetsinnhold'],
    ] as const)('keeps the legacy #%s link pointed at %s', (legacyHash, tab, label) => {
      window.location.hash = `#${legacyHash}`
      render(() => <SpaceCockpit tabs={{ [tab]: <p>{label}</p> }} />)
      expect(screen.getByText(label)).toBeTruthy()
    })

    it('respects initialTab when the URL carries no hash', () => {
      render(() => <SpaceCockpit initialTab="agent" />)
      expect(screen.getByRole('tab', { name: 'Agent' }).getAttribute('aria-selected')).toBe('true')
    })
  })

  it('links the panel to its tab for assistive technology', () => {
    render(() => <SpaceCockpit initialTab="chat" />)
    const panel = screen.getByRole('tabpanel')
    expect(panel.getAttribute('aria-labelledby')).toBe('space-tab-chat')
    expect(screen.getByRole('tab', { name: 'Samtaler' }).getAttribute('aria-controls')).toBe(
      'space-panel-chat',
    )
  })

  // Measured live before this was fixed: one context resolve produced six
  // identical `/work` and six identical `/knowledge` requests inside 3ms, and
  // another six on every membership recheck. `props.tabs` is a getter over the
  // caller's object literal, this `For` read it twice per tab across six tabs,
  // and Solid JSX constructs a component eagerly — so every read mounted every
  // panel again.
  it('constructs each panel once, however many tabs read the tabs object', () => {
    let mounted = 0
    const Counting = () => {
      mounted += 1
      return <p>panel</p>
    }
    render(() => <SpaceCockpit initialTab="chat" tabs={{ chat: <Counting /> }} />)
    expect(mounted).toBe(1)
  })

  it('keeps every aria-controls target in the DOM, including inactive tabs', () => {
    render(() => <SpaceCockpit initialTab="chat" />)
    for (const tab of screen.getAllByRole('tab')) {
      const panelId = tab.getAttribute('aria-controls')
      expect(panelId).toBeTruthy()
      expect(document.getElementById(panelId!)).toBeTruthy()
    }
  })
})

describe('SpaceCockpit artifacts tab (WS07-S12)', () => {
  it('shows seven tabs with the flag on, artefakter after kunnskap', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
    render(() => <SpaceCockpit />)
    const labels = screen.getAllByRole('tab').map((tab) => tab.textContent)
    expect(labels).toEqual(['Samtaler', 'Arbeid', 'Kunnskap', 'Artefakter', 'Aktivitet', 'Agent', 'Medlemmer'])
  })

  it('shows six tabs and no artifacts panel when VITE_SPACE_ROOM_ARTIFACTS_UI=false', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'false')
    render(() => <SpaceCockpit />)
    const labels = screen.getAllByRole('tab').map((tab) => tab.textContent)
    expect(labels).toEqual(['Samtaler', 'Arbeid', 'Kunnskap', 'Aktivitet', 'Agent', 'Medlemmer'])
    expect(screen.queryByRole('tab', { name: 'Artefakter' })).toBeNull()
    expect(document.getElementById('space-panel-artefakter')).toBeNull()
  })

  it('lets a panel built from the tabs prop see useActiveSpaceTab() change on tab select', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
    const Probe = () => {
      const active = useActiveSpaceTab()
      return <p data-testid="active-tab">{active()}</p>
    }
    render(() => <SpaceCockpit initialTab="chat" tabs={{ artefakter: <Probe /> }} />)
    expect(screen.getByTestId('active-tab').textContent).toBe('chat')
    fireEvent.click(screen.getByRole('tab', { name: 'Artefakter' }))
    flush()
    expect(screen.getByTestId('active-tab').textContent).toBe('artefakter')
    fireEvent.click(screen.getByRole('tab', { name: 'Kunnskap' }))
    flush()
    expect(screen.getByTestId('active-tab').textContent).toBe('kunnskap')
  })

  it('reports the default tab to a panel rendered outside a cockpit', () => {
    const Probe = () => <p data-testid="outside">{useActiveSpaceTab()()}</p>
    render(() => <Probe />)
    expect(screen.getByTestId('outside').textContent).toBe('chat')
  })

  it('covers the new tab in roving focus, both directions', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
    render(() => <SpaceCockpit initialTab="kunnskap" />)
    const list = screen.getByRole('tablist')
    screen.getByRole('tab', { name: 'Kunnskap' }).focus()
    fireEvent.keyDown(list, { key: 'ArrowRight' })
    flush()
    expect(screen.getByRole('tab', { name: 'Artefakter' }).getAttribute('aria-selected')).toBe('true')
    expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Artefakter' }))
    fireEvent.keyDown(list, { key: 'ArrowRight' })
    flush()
    expect(screen.getByRole('tab', { name: 'Aktivitet' }).getAttribute('aria-selected')).toBe('true')
    fireEvent.keyDown(list, { key: 'ArrowLeft' })
    flush()
    fireEvent.keyDown(list, { key: 'ArrowLeft' })
    flush()
    expect(screen.getByRole('tab', { name: 'Kunnskap' }).getAttribute('aria-selected')).toBe('true')
  })

  it('opens #artefakter from a deep link when the flag is on, and falls back to chat when off', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
    window.location.hash = '#artefakter'
    const first = render(() => <SpaceCockpit />)
    expect(screen.getByRole('tab', { name: 'Artefakter' }).getAttribute('aria-selected')).toBe('true')
    first.unmount()
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'false')
    render(() => <SpaceCockpit />)
    expect(screen.getByRole('tab', { name: 'Samtaler' }).getAttribute('aria-selected')).toBe('true')
  })
})
