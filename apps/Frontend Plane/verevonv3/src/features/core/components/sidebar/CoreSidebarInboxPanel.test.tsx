// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { InboxExpandedSidebarPanel } from './CoreSidebarInboxPanel'
import { I18nProvider } from '@/shared/i18n'

afterEach(cleanup)

describe('InboxExpandedSidebarPanel filters', () => {
  it('does not expose a heuristic mentions queue from legacy Inbox navigation', () => {
    window.history.pushState(null, '', '/inbox?view=mentions')
    render(() => (
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route path="/inbox" component={() => <InboxExpandedSidebarPanel onCollapse={() => undefined} />} />
        </Router>
      </I18nProvider>
    ))

    expect(screen.queryByRole('link', { name: /mentions|omtaler/i })).toBeNull()
  })
})
