// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import SupportOutboundPage from './SupportOutboundPage'
import { I18nProvider } from '@/shared/i18n'

const { listOrganizationOutboundIntents, runAssist } = vi.hoisted(() => ({
  listOrganizationOutboundIntents: vi.fn(),
  runAssist: vi.fn(),
}))

vi.mock('@/shared/api/inbox-client', () => ({ listOrganizationOutboundIntents }))
vi.mock('@/features/inbox/lib/inbox-ai', () => ({ runAssist: runAssist }))
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-coresystem' }, user: { id: 'user-coresystem' } }),
}))

afterEach(cleanup)

beforeEach(() => {
  listOrganizationOutboundIntents.mockResolvedValue([])
  runAssist.mockResolvedValue({
    text: 'The provider accepted the submission, but delivery remains unconfirmed.',
    sources: [],
    zdr: false,
    supportAiMode: 'assist',
    threadId: 'support_outbound_1',
  })
})

describe('SupportOutboundPage', () => {
  it('keeps creation disabled while presenting the canonical receipt-ledger empty state', () => {
    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    expect((screen.getByRole('button', { name: /create outbound message|opprett utgående melding/i }) as HTMLButtonElement).disabled).toBe(true)
    expect(screen.queryByText('0')).toBeNull()
    expect(screen.getByText(/all delivery receipts|alle leveringskvitteringer/i)).toBeTruthy()
    expect(screen.queryByText(/campaign workspace|kampanjearbeidsområde/i)).toBeNull()
  })

  it('opens the newest canonical receipt after the ledger finishes loading', async () => {
    listOrganizationOutboundIntents.mockResolvedValue([
      {
        id: 'outbound_1', conversation_id: 'conv_1', status: 'submitted', provider: 'whatsapp',
        delivery_status: 'unconfirmed', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:01:00.000Z',
      },
    ])

    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    expect(await screen.findByRole('heading', { name: /provider accepted|godtatt av leverandør/i })).toBeTruthy()
    expect(screen.getAllByText(/conv_1/).some((element) => element.textContent?.includes('conv_1'))).toBe(true)
    expect(screen.queryByText(/select an outbound receipt|velg en utgående kvittering/i)).toBeNull()
  })

  it('passes only the supported provider and delivery filters to the canonical ledger', async () => {
    listOrganizationOutboundIntents.mockResolvedValue([])

    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/?outbound_provider=whatsapp&outbound_delivery_status=failed'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    await vi.waitFor(() => expect(listOrganizationOutboundIntents).toHaveBeenCalledWith('org-coresystem', {
      provider: 'whatsapp', deliveryStatus: 'failed', limit: 100,
    }))
  })

  it('takes an unknown outcome back to its source conversation for manual reconciliation without exposing retry', async () => {
    listOrganizationOutboundIntents.mockResolvedValue([{
      id: 'outbound_unknown', conversation_id: 'conv_unknown', status: 'unknown', provider: 'gmail',
      delivery_status: 'unconfirmed', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:10:00.000Z',
    }])

    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    expect(await screen.findByRole('heading', { name: /unknown outcome|ukjent utfall/i })).toBeTruthy()
    const reconcile = screen.getByRole('link', { name: /open source conversation|åpne kildesamtale/i })
    expect(reconcile.getAttribute('href')).toBe('/support?view=all&conversation_id=conv_unknown')
    expect(screen.getByText(/requires reconciliation|krever avstemming/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /retry|prøv igjen/i })).toBeNull()
  })

  it('shows the selected receipt facts, safe reconciliation route, and content-free audit in the rail', async () => {
    listOrganizationOutboundIntents.mockResolvedValue([{
      id: 'outbound_unknown', conversation_id: 'conv_unknown', status: 'unknown', provider: 'gmail',
      provider_message_id: 'gmail-message-42', delivery_status: 'unconfirmed', error_code: 'provider_timeout',
      created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:10:00.000Z',
    }])

    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    await screen.findByRole('heading', { name: /unknown outcome|ukjent utfall/i })
    expect(screen.getByText(/provider receipt id|leverandørkvittering/i)).toBeTruthy()
    expect(screen.getByText('gmail-message-42')).toBeTruthy()
    expect(screen.getByText(/provider timeout|provider_timeout/i)).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: /actions|handlinger/i }))
    flush()
    const reconciliationLinks = screen.getAllByRole('link', { name: /open source conversation|åpne kildesamtale/i })
    expect(reconciliationLinks.some((link) => link.getAttribute('href') === '/support?view=all&conversation_id=conv_unknown')).toBe(true)
    expect(screen.getByText(/do not retry automatically|ikke prøv automatisk på nytt/i)).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: /audit|revisjon/i }))
    flush()
    expect(screen.getByText(/content-free audit context|innholdsfri revisjonskontekst/i)).toBeTruthy()
    expect(screen.getByText(/outbound_unknown/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /retry|prøv igjen/i })).toBeNull()
  })

  it('gives Verevon only a selected receipt context and keeps the outbound rail read-only', async () => {
    listOrganizationOutboundIntents.mockResolvedValue([{
      id: 'outbound_1', conversation_id: 'conv_1', status: 'submitted', provider: 'gmail',
      delivery_status: 'unconfirmed', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:01:00.000Z',
    }])

    const TestRouter = createRouter({
      routes: [{ path: '/', component: SupportOutboundPage }],
      history: memoryHistory('/'),
      explicitLinks: true,
    })

    render(() => (
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    ))

    await screen.findByRole('heading', { name: /provider accepted|godtatt av leverandør/i })
    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    fireEvent.click(await screen.findByRole('button', { name: /explain receipt|forklar kvittering/i }))

    await waitFor(() => expect(runAssist).toHaveBeenCalledWith(
      'org-coresystem',
      'outbound',
      [],
      expect.objectContaining({
        contextPack: expect.objectContaining({
          selectedEntity: expect.objectContaining({ id: 'outbound_1', status: 'work=submitted; delivery=unconfirmed; error=none' }),
          support: expect.objectContaining({ availableActions: [] }),
        }),
      }),
    ))

    expect((await screen.findByRole('region', { name: /verevon answer|verevon-svar/i })).textContent).toMatch(/delivery remains unconfirmed/i)
    expect(screen.getByRole('link', { name: /open in chat|åpne i chat/i }).getAttribute('href')).toBe('/chat?thread_id=support_outbound_1')
    expect(screen.queryByRole('button', { name: /prepare customer reply|forbered kundesvar|retry|prøv igjen/i })).toBeNull()
    expect(screen.getByText(/cannot send|kan ikke sende/i)).toBeTruthy()
  })
})
