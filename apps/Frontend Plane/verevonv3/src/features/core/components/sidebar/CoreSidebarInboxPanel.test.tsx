// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { InboxExpandedSidebarPanel } from './CoreSidebarInboxPanel'
import { I18nProvider } from '@/shared/i18n'

afterEach(cleanup)

describe('InboxExpandedSidebarPanel filters', () => {
  it('does not expose a heuristic mentions queue from legacy Inbox navigation', () => {
    const TestRouter = createRouter({
      routes: [{ path: '/inbox', component: () => <InboxExpandedSidebarPanel onCollapse={() => undefined} /> }],
      history: memoryHistory('/inbox?view=mentions'),
      explicitLinks: true,
    })
    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    expect(screen.queryByRole('link', { name: /mentions|omtaler/i })).toBeNull()
  })
})
