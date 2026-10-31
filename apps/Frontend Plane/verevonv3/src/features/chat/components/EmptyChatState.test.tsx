// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { listNotifications, listConversations } = vi.hoisted(() => ({
  listNotifications: vi.fn(),
  listConversations: vi.fn(),
}))

vi.mock('@/shared/api/notifications-client', () => ({ listNotifications }))
vi.mock('@/shared/api/inbox-client', () => ({ listConversations }))

import { EmptyChatState } from './ChatMessages'

afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})

describe('EmptyChatState', () => {
  it('shows the requested introduction only after the activity feeds return empty', async () => {
    listNotifications.mockResolvedValue([])
    listConversations.mockResolvedValue({ tickets: [] })
    render(() => (
      <EmptyChatState orgId="org-a" orgName="AQUATIQ AS" userId="user-a" onSelectPrompt={() => undefined} onResumeThread={() => undefined}>
        <div>Composer</div>
      </EmptyChatState>
    ))

    expect(await screen.findByText(/Spør om noe dere jobber med\. Verevon finner svar i AQUATIQs kunnskap og systemer/)).toBeTruthy()
    expect(screen.queryByText('Dette har skjedd')).toBeNull()
  })

  it('replaces the introduction with a sourced suggestion that only fills the composer', async () => {
    const selected = vi.fn()
    listNotifications.mockResolvedValue([{
      id: 'alert-1', organization_id: 'org-a', recipient_id: 'user-a', event_type: 'case.alert',
      channel: 'in_app', title: 'Leveransen trenger avklaring', body: 'Ny hendelse',
      read: false, archived: false, submitted_at: new Date().toISOString(),
    }])
    // A slow inbox must not delay a useful alert suggestion.
    listConversations.mockImplementation(() => new Promise(() => undefined))
    render(() => (
      <EmptyChatState orgId="org-a" orgName="AQUATIQ AS" userId="user-a" onSelectPrompt={selected} onResumeThread={() => undefined}>
        <div>Composer</div>
      </EmptyChatState>
    ))

    const suggestion = await screen.findByRole('button', { name: /Leveransen trenger avklaring/ })
    expect(screen.queryByText(/Spør om noe dere jobber med/)).toBeNull()
    fireEvent.click(suggestion)
    expect(selected).toHaveBeenCalledOnce()
    expect(selected.mock.calls[0]?.[0]).toContain('Leveransen trenger avklaring')
  })

  it('refreshes quiet suggestions when the page becomes visible again', async () => {
    listNotifications.mockResolvedValueOnce([]).mockResolvedValueOnce([{
      id: 'alert-2', organization_id: 'org-a', recipient_id: 'user-a', event_type: 'case.alert',
      channel: 'in_app', title: 'Ny sak trenger oppfølging', body: 'En hendelse har kommet inn.',
      read: false, archived: false, submitted_at: new Date().toISOString(),
    }])
    listConversations.mockResolvedValue({ tickets: [] })
    render(() => (
      <EmptyChatState orgId="org-a" orgName="AQUATIQ AS" userId="user-a" onSelectPrompt={() => undefined} onResumeThread={() => undefined}>
        <div>Composer</div>
      </EmptyChatState>
    ))

    await screen.findByText(/Spør om noe dere jobber med/)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(await screen.findByRole('button', { name: /Ny sak trenger oppfølging/ })).toBeTruthy()
    expect(screen.queryByText(/Spør om noe dere jobber med/)).toBeNull()
  })

  it('shows activity above the composer and keeps example prompts available below it', async () => {
    const submittedAt = new Date().toISOString()
    listNotifications.mockResolvedValue([
      { id: 'alert-1', organization_id: 'org-a', recipient_id: 'user-a', event_type: 'case.alert', channel: 'in_app', title: 'Backup failed', body: 'System details', read: false, archived: false, submitted_at: submittedAt },
      { id: 'alert-2', organization_id: 'org-a', recipient_id: 'user-a', event_type: 'case.alert', channel: 'in_app', title: 'Another incident', body: 'More system details', read: false, archived: false, submitted_at: submittedAt },
    ])
    listConversations.mockResolvedValue({ tickets: [] })
    render(() => (
      <EmptyChatState orgId="org-a" userId="user-a" onSelectPrompt={() => undefined} onResumeThread={() => undefined}>
        <div>Composer</div>
      </EmptyChatState>
    ))

    expect(await screen.findByRole('button', { name: /Backup failed/ })).toBeTruthy()
    expect(screen.getByText('Dette kan trenge oppfølging')).toBeTruthy()
    expect(screen.getByRole('button', { name: /Another incident/ })).toBeTruthy()
    expect(screen.getByText('System details')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Oppsummer et dokument' })).toBeNull()
    const activity = screen.getByRole('group', { name: 'Forslag basert på ny aktivitet' })
    const composer = screen.getByText('Composer')
    expect(activity.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Vis eksempler' }))
    expect(await screen.findByRole('button', { name: 'Oppsummer et dokument' })).toBeTruthy()
    expect(await screen.findByRole('button', { name: 'Lag en dagsbrief' })).toBeTruthy()
  })

  it('groups inbox items and other alerts behind separate chevrons', async () => {
    const now = new Date().toISOString()
    listNotifications.mockResolvedValue([{
      id: 'alert-1', organization_id: 'org-a', recipient_id: 'user-a', event_type: 'case.alert',
      channel: 'in_app', title: 'Leveransen trenger avklaring', body: 'Ny hendelse',
      read: false, archived: false, submitted_at: now,
    }])
    listConversations.mockResolvedValue({ tickets: [{
      conversationId: 'conversation-1', title: 'Backup failed', lastMessagePreview: 'Network share failed',
      state: { name: 'open' }, updated_at: now,
    }] })
    render(() => (
      <EmptyChatState orgId="org-a" userId="user-a" onSelectPrompt={() => undefined} onResumeThread={() => undefined}>
        <div>Composer</div>
      </EmptyChatState>
    ))

    expect(await screen.findByRole('button', { name: /Følg opp Backup failed/ })).toBeTruthy()
    const inbox = screen.getByRole('button', { name: 'Innboks' })
    const other = screen.getByRole('button', { name: 'Annet' })
    expect(inbox.getAttribute('aria-expanded')).toBe('true')
    expect(other.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('button', { name: /Leveransen trenger avklaring/ })).toBeNull()

    fireEvent.click(other)
    expect(await screen.findByRole('button', { name: /Leveransen trenger avklaring/ })).toBeTruthy()
    fireEvent.click(inbox)
    await waitFor(() => expect(screen.queryByRole('button', { name: /Følg opp Backup failed/ })).toBeNull())
  })
})
