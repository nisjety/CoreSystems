// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { InboxExpandedSidebarPanel } from './CoreSidebarInboxPanel'
import { I18nProvider } from '@/shared/i18n'

afterEach(cleanup)

describe('InboxExpandedSidebarPanel provider routes', () => {
  it('uses Conversation Core\'s canonical x channel for X mentions', () => {
    window.history.pushState(null, '', '/inbox?view=mentions')
    render(() => (
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route path="/inbox" component={() => <InboxExpandedSidebarPanel onCollapse={() => undefined} />} />
        </Router>
      </I18nProvider>
    ))

    fireEvent.click(screen.getByRole('button', { name: /show mentions|vis omtaler/i }))

    const xMentionsLink = screen.getAllByRole('link', { name: 'Twitter / X' })
      .find((link) => link.getAttribute('href')?.includes('view=mentions'))
    expect(xMentionsLink).toBeTruthy()
    expect(xMentionsLink?.getAttribute('href')).toBe('/inbox?view=mentions&channel=x')
  })
})
