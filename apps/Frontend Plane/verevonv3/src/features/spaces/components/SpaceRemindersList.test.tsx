import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  listPendingSpaceReminders: vi.fn(),
  cancelSpaceReminder: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceRemindersList } = await import('./SpaceRemindersList')

const NOW = Date.parse('2026-11-18T10:00:00Z')
const HOUR = 60 * 60 * 1000

const row = (reminderId: string, messageId: string | null, remindAt: number) => ({
  reminderId,
  spaceRef: 'space-a',
  threadId: 'thread-1',
  messageId,
  remindAt,
  createdAt: NOW,
})

/** The list read and the cancel settle on a later macrotask; Solid applies the writes at flush. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  spacesClient.listPendingSpaceReminders.mockReset()
  spacesClient.cancelSpaceReminder.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceRemindersList (WS11-S11)', () => {
  it('lists each pending reminder with its time, a link to its post and a Cancel button, in the order read', async () => {
    spacesClient.listPendingSpaceReminders.mockResolvedValue([
      row('rem-1', 'msg-9', NOW + HOUR),
      row('rem-2', null, NOW + 2 * HOUR),
    ])
    render(() => <SpaceRemindersList />)
    const list = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(2)
    const [message, post] = items as [HTMLElement, HTMLElement]
    // A reminder on one message opens that message in its thread; a post-level one opens the thread.
    expect(within(message).getByRole('link', { name: 'Åpne innlegget' }).getAttribute('href')).toContain('m=msg-9')
    expect(within(post).getByRole('link', { name: 'Åpne innlegget' }).getAttribute('href')).toBe(
      '/spaces/space-a?thread_id=thread-1#chat',
    )
    expect(within(message).getByRole('button', { name: 'Avbryt' })).toBeTruthy()
    expect(within(post).getByRole('button', { name: 'Avbryt' })).toBeTruthy()
  })

  it('says so when nothing is pending', async () => {
    spacesClient.listPendingSpaceReminders.mockResolvedValue([])
    render(() => <SpaceRemindersList />)
    expect(await screen.findByText('Du har ingen ventende påminnelser.')).toBeTruthy()
    expect(screen.queryByRole('list', { name: 'Ventende påminnelser' })).toBeNull()
  })

  it('draws nothing when the gateway reports the reminders flag off (404 space_reminders_disabled)', async () => {
    spacesClient.listPendingSpaceReminders.mockRejectedValue(
      new ApiError('Not enabled.', 404, 'space_reminders_disabled'),
    )
    const view = render(() => <SpaceRemindersList />)
    await settle()
    expect(spacesClient.listPendingSpaceReminders).toHaveBeenCalledTimes(1)
    expect(view.container.innerHTML).toBe('')
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('draws nothing, and no load error, for a session with no active organization (403 active_membership_required)', async () => {
    spacesClient.listPendingSpaceReminders.mockRejectedValue(
      new ApiError('No active organization.', 403, 'active_membership_required'),
    )
    const view = render(() => <SpaceRemindersList />)
    await settle()
    expect(spacesClient.listPendingSpaceReminders).toHaveBeenCalledTimes(1)
    expect(view.container.innerHTML).toBe('')
    expect(screen.queryByText('Påminnelsene kunne ikke hentes.')).toBeNull()
  })

  it('Cancel cancels that reminder, reads the list again and says so', async () => {
    spacesClient.listPendingSpaceReminders
      .mockResolvedValueOnce([row('rem-1', 'msg-9', NOW + HOUR)])
      .mockResolvedValueOnce([])
    spacesClient.cancelSpaceReminder.mockResolvedValue(undefined)
    render(() => <SpaceRemindersList />)
    const list = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    fireEvent.click(within(list).getByRole('button', { name: 'Avbryt' }))
    flush()
    await waitFor(() => expect(spacesClient.cancelSpaceReminder).toHaveBeenCalledWith('rem-1'))
    expect(await screen.findByText('Påminnelsen er avbrutt.')).toBeTruthy()
    expect(await screen.findByText('Du har ingen ventende påminnelser.')).toBeTruthy()
    expect(spacesClient.cancelSpaceReminder).toHaveBeenCalledTimes(1)
  })

  it('a reminder that already fired reads as gone: the list is read again and no cancellation is claimed', async () => {
    spacesClient.listPendingSpaceReminders
      .mockResolvedValueOnce([row('rem-1', 'msg-9', NOW + HOUR)])
      .mockResolvedValueOnce([])
    spacesClient.cancelSpaceReminder.mockRejectedValue(new ApiError('Gone.', 404, 'reminder_not_found'))
    render(() => <SpaceRemindersList />)
    const list = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    fireEvent.click(within(list).getByRole('button', { name: 'Avbryt' }))
    flush()
    await waitFor(() => expect(spacesClient.listPendingSpaceReminders).toHaveBeenCalledTimes(2))
    await settle()
    expect(screen.queryByText('Påminnelsen er avbrutt.')).toBeNull()
    expect(screen.queryByRole('status')).toBeNull()
    expect(await screen.findByText('Du har ingen ventende påminnelser.')).toBeTruthy()
  })

  it('a failed cancel keeps the row and says the reminder was not cancelled', async () => {
    spacesClient.listPendingSpaceReminders.mockResolvedValue([row('rem-1', 'msg-9', NOW + HOUR)])
    spacesClient.cancelSpaceReminder.mockRejectedValue(new ApiError('Down.', 400, 'reminder_cancel_refused'))
    render(() => <SpaceRemindersList />)
    const list = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    fireEvent.click(within(list).getByRole('button', { name: 'Avbryt' }))
    flush()
    await waitFor(() => expect((screen.getByRole('status').textContent ?? '').length).toBeGreaterThan(0))
    expect(screen.queryByText('Påminnelsen er avbrutt.')).toBeNull()
    expect(screen.getByRole('list', { name: 'Ventende påminnelser' })).toBeTruthy()
  })
})
