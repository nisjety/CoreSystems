// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { CoreNavbar, shouldDeferGlobalSlashSearch } from './CoreNavbar'

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
  it('returns to the Engagement landing page from a Visibility breadcrumb', () => {
    render(() => <I18nProvider><Router><Route path="/*all" component={() => <CoreNavbar activeRoute="/engagement/visibility" offline workspace={{ initial: 'T', name: 'Triodelab', plan: 'Expert' }} />} /></Router></I18nProvider>)
    expect(screen.getByRole('link', { name: /Engasjement|Engagement/ }).getAttribute('href')).toBe('/engagement')
    expect(screen.getByRole('link', { name: /Synlighet|Visibility/ }).getAttribute('href')).toBe('/engagement/visibility')
  })
  it('keeps the workspace switcher enabled by default in the authenticated shell', () => {
    render(() => (
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route
            path="/*all"
            component={() => (
              <CoreNavbar
                activeRoute="/studio/canvas"
                workspace={{ initial: 'A', name: 'Acme', plan: 'Trial' }}
              />
            )}
          />
        </Router>
      </I18nProvider>
    ))

    expect((screen.getByRole('button', { name: /acme.*trial/i }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('renders an explicitly supplied breadcrumb without expanding the product route contract', () => {
    render(() => (
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route
            path="/*all"
            component={() => (
              <CoreNavbar
                activeRoute="/studio/canvas"
                breadcrumbOverride={{ href: '/studio/pulse', moduleLabel: 'Marketing', tabLabel: 'Pulse' }}
                offline
                workspace={{ initial: 'A', name: 'Acme', plan: 'Local' }}
              />
            )}
          />
        </Router>
      </I18nProvider>
    ))

    expect(screen.getByRole('link', { name: 'Marketing' }).getAttribute('href')).toBe('/studio/pulse')
    expect(screen.getByRole('link', { name: 'Pulse' }).getAttribute('href')).toBe('/studio/pulse')
  })
})
