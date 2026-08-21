// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import SupportPage from './SupportPage'
import { I18nProvider } from '@/shared/i18n'

vi.mock('@/features/inbox/components/InboxPage', () => ({
  default: () => <div>Conversation workspace</div>,
}))

vi.mock('@/features/tickets/components/TicketingPage', () => ({
  default: () => <div>Ticket workspace</div>,
}))

vi.mock('@/features/support/components/AiReviewQueue', () => ({
  AiReviewQueue: () => <div>Legacy AI review</div>,
}))

afterEach(cleanup)

describe('SupportPage navigation', () => {
  it('keeps Conversations, Ticketing, and Outbound as the three primary work surfaces', () => {
    const TestRouter = createRouter({
      routes: [{ path: '/support', component: SupportPage }],
      history: memoryHistory('/support'),
      explicitLinks: true,
    })
    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    expect(screen.getByRole('tab', { name: /conversations|samtaler/i }).getAttribute('href')).toBe('/support')
    expect(screen.getByRole('tab', { name: /ticketing|saksbehandling/i }).getAttribute('href')).toBe('/support?surface=tickets')
    expect(screen.getByRole('tab', { name: /outbound|utgående/i }).getAttribute('href')).toBe('/support?surface=outbound')
    expect(screen.queryByRole('tab', { name: /AI review|AI-gjennomgang/i })).toBeNull()
  })

  it('keeps functional center and right-rail tabs in the three-pane Outbound workspace', () => {
    const TestRouter = createRouter({
      routes: [{ path: '/support', component: SupportPage }],
      history: memoryHistory('/support?surface=outbound'),
      explicitLinks: true,
    })
    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    const centerTabs = screen.getByRole('tablist', { name: /outbound content|utgående innhold/i })
    fireEvent.click(within(centerTabs).getByRole('tab', { name: /delivery|levering/i }))
    flush()
    expect(screen.getByRole('heading', { name: /select a receipt|velg en kvittering/i })).toBeTruthy()

    const railTabs = screen.getByRole('tablist', { name: /outbound context|utgående kontekst/i })
    fireEvent.click(within(railTabs).getByRole('tab', { name: 'Verevon' }))
    flush()
    // OutboundVerevonRail's empty-state copy is identical on both branches:
    // i18n.tr('Velg en kvittering før Verevon får utgående kontekst.',
    //         'Select a receipt before Verevon receives outbound context.')
    // so match both locales exactly rather than main's looser variant list.
    expect(screen.getByText(/select a receipt before verevon receives outbound context|velg en kvittering før verevon får utgående kontekst/i)).toBeTruthy()
  })

})
