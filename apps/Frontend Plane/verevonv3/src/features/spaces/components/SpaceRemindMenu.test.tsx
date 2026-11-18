import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  createSpaceReminder: vi.fn(),
  listPendingSpaceReminders: vi.fn(),
  cancelSpaceReminder: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceRemindMenu } = await import('./SpaceRemindMenu')

const MINUTE = 60 * 1000
const HOUR = 60 * MINUTE
const NOW = Date.parse('2026-11-18T10:00:00Z')

const POST = { spaceRef: 'space-a', threadId: 'thread-1', messageId: 'msg-9' }

/** The list read and the create or cancel settle on a later macrotask; Solid applies the writes at flush. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

function renderMenu() {
  return render(() => <SpaceRemindMenu spaceRef={POST.spaceRef} threadId={POST.threadId} messageId={POST.messageId} />)
}

async function openMenu() {
  fireEvent.click(screen.getByRole('button', { name: 'Påminn meg' }))
  flush()
  await settle()
}

const reminder = (reminderId: string, messageId: string, remindAt: number) => ({
  reminderId,
  spaceRef: POST.spaceRef,
  threadId: POST.threadId,
  messageId,
  remindAt,
  createdAt: NOW,
})

beforeEach(() => {
  // Only Date is faked, so the preset times are exact and waitFor keeps its real timers.
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  for (const mock of Object.values(spacesClient)) mock.mockReset()
  spacesClient.listPendingSpaceReminders.mockResolvedValue([])
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SpaceRemindMenu (WS11-S11)', () => {
  it('a preset creates the reminder for this post at a future time and confirms it', async () => {
    spacesClient.createSpaceReminder.mockResolvedValue({
      reminderId: 'rem-1',
      remindAt: NOW + 20 * MINUTE,
      deduplicated: false,
    })
    renderMenu()
    await openMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Om 20 minutter' }))
    flush()
    await waitFor(() => expect(spacesClient.createSpaceReminder).toHaveBeenCalledTimes(1))
    const sent = spacesClient.createSpaceReminder.mock.calls[0]?.[0]
    expect(sent).toEqual({ spaceRef: POST.spaceRef, threadId: POST.threadId, messageId: POST.messageId, remindAt: NOW + 20 * MINUTE })
    expect(sent?.remindAt).toBeGreaterThan(NOW)
    expect(await screen.findByText(/Påminnelse satt til/)).toBeTruthy()
  })

  it('under zero retention the 412 shows the ZDR note, makes one attempt and creates nothing', async () => {
    spacesClient.createSpaceReminder.mockRejectedValue(new ApiError('Refused.', 412, 'zdr_durable_mutation_forbidden'))
    renderMenu()
    await openMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Om 1 time' }))
    flush()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Organisasjonen lagrer ikke innhold, så påminnelser er ikke tilgjengelige.')
    expect(spacesClient.createSpaceReminder).toHaveBeenCalledTimes(1)
    expect(screen.queryByText(/Påminnelse satt til/)).toBeNull()
    // No re-read after the refusal: the list is still the one the menu opened with.
    expect(spacesClient.listPendingSpaceReminders).toHaveBeenCalledTimes(1)
    expect(window.localStorage.length).toBe(0)
  })

  it('shows the too-many and the invalid-time messages for 429 and 400', async () => {
    spacesClient.createSpaceReminder
      .mockRejectedValueOnce(new ApiError('Too many.', 429, 'too_many_reminders', { details: { max_pending: 50 } }))
      .mockRejectedValueOnce(new ApiError('Bad time.', 400, 'invalid_reminder_time'))
    renderMenu()
    await openMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Om 1 time' }))
    flush()
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'Du har like mange ventende påminnelser som tillatt. Avbryt en for å legge til en ny.',
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Om 1 time' }))
    flush()
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe('Velg et tidspunkt som ligger frem i tid, men innen et år.'),
    )
    expect(spacesClient.createSpaceReminder).toHaveBeenCalledTimes(2)
  })

  it('hides the control when the gateway reports the reminders flag off (404 space_reminders_disabled)', async () => {
    spacesClient.listPendingSpaceReminders.mockRejectedValue(new ApiError('Not enabled.', 404, 'space_reminders_disabled'))
    const view = renderMenu()
    await openMenu()
    expect(screen.queryByRole('button', { name: 'Påminn meg' })).toBeNull()
    expect(view.container.innerHTML).toBe('')
    expect(spacesClient.createSpaceReminder).not.toHaveBeenCalled()
  })

  it("lists this post's pending reminder with Cancel, and Cancel calls cancelSpaceReminder", async () => {
    spacesClient.listPendingSpaceReminders.mockResolvedValue([
      reminder('rem-mine', POST.messageId, NOW + HOUR),
      reminder('rem-other', 'msg-2', NOW + 2 * HOUR),
    ])
    spacesClient.cancelSpaceReminder.mockResolvedValue(undefined)
    renderMenu()
    await openMenu()
    const pending = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    expect(within(pending).getAllByRole('listitem')).toHaveLength(1)
    fireEvent.click(within(pending).getByRole('button', { name: 'Avbryt' }))
    flush()
    await waitFor(() => expect(spacesClient.cancelSpaceReminder).toHaveBeenCalledWith('rem-mine'))
    expect(spacesClient.cancelSpaceReminder).toHaveBeenCalledTimes(1)
    expect(await screen.findByText('Påminnelsen er avbrutt.')).toBeTruthy()
  })

  it('re-reads the list when a cancel answers reminder_not_found, and claims no cancellation', async () => {
    spacesClient.listPendingSpaceReminders
      .mockResolvedValueOnce([reminder('rem-mine', POST.messageId, NOW + HOUR)])
      .mockResolvedValueOnce([])
    spacesClient.cancelSpaceReminder.mockRejectedValue(new ApiError('Gone.', 404, 'reminder_not_found'))
    renderMenu()
    await openMenu()
    const pending = await screen.findByRole('list', { name: 'Ventende påminnelser' })
    fireEvent.click(within(pending).getByRole('button', { name: 'Avbryt' }))
    flush()
    await waitFor(() => expect(spacesClient.listPendingSpaceReminders).toHaveBeenCalledTimes(2))
    await settle()
    expect(screen.queryByRole('list', { name: 'Ventende påminnelser' })).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByText('Påminnelsen er avbrutt.')).toBeNull()
  })
})
