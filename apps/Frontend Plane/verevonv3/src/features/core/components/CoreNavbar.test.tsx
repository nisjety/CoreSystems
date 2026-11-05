// @vitest-environment jsdom

import { createRouter } from '@solidjs/router'
import { render } from '@/test/render'
import { cleanup, fireEvent, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
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
