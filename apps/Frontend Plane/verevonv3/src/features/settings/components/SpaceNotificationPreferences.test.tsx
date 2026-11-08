// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const client = vi.hoisted(() => ({
  getNotificationPreferences: vi.fn(),
  updateNotificationPreference: vi.fn(),
}))
vi.mock('@/shared/api/notifications-client', () => client)

const { SpaceNotificationPreferences } = await import('./SpaceNotificationPreferences')

// Titles in the default (Norwegian) locale, in list order, with the default each starts at.
const SIGNALS = [
  ['Nevnt i et Space', 'space.mention', true],
  ['Svar i et Space', 'space.reply', false],
  ['Lagt til i et Space', 'space.invite', true],
  ['Agent ferdig', 'space.agent_done', false],
  ['Agent feilet', 'space.agent_failed', false],
  ['Godkjenning trengs', 'space.approval_needed', true],
  ['Agent venter', 'space.agent_awaiting', false],
  ['Påminnelse', 'space.reminder', false],
] as const

function toggle(title: string): HTMLInputElement {
  return screen.getByRole('switch', { name: title }) as HTMLInputElement
}

beforeEach(() => {
  client.getNotificationPreferences.mockReset()
  client.updateNotificationPreference.mockReset()
  client.updateNotificationPreference.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
})

describe('SpaceNotificationPreferences', () => {
  it('lists the eight Space signals with the migration defaults when nothing is saved', async () => {
    client.getNotificationPreferences.mockResolvedValue([])
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(client.getNotificationPreferences).toHaveBeenCalledTimes(1))
    for (const [title, , defaultOn] of SIGNALS) {
      expect(toggle(title).checked, title).toBe(defaultOn)
    }
    expect(screen.getAllByRole('switch')).toHaveLength(8)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('keeps the defaults when the preferences cannot be loaded', async () => {
    client.getNotificationPreferences.mockRejectedValue(new Error('offline'))
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(client.getNotificationPreferences).toHaveBeenCalledTimes(1))
    expect(toggle('Nevnt i et Space').checked).toBe(true)
    expect(toggle('Svar i et Space').checked).toBe(false)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('lets an explicit saved email row override the default, in either direction', async () => {
    client.getNotificationPreferences.mockResolvedValue([
      { event_type: 'space.mention', channel: 'email', enabled: false },
      { event_type: 'space.reply', channel: 'email', enabled: true },
      // Not email, not a boolean, not a Space type: all ignored.
      { event_type: 'space.invite', channel: 'in_app', enabled: false },
      { event_type: 'space.reminder', channel: 'email', enabled: 'yes' },
      { event_type: 'ticket.assigned', channel: 'email', enabled: true },
    ])
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(toggle('Nevnt i et Space').checked).toBe(false))
    expect(toggle('Svar i et Space').checked).toBe(true)
    expect(toggle('Lagt til i et Space').checked).toBe(true)
    expect(toggle('Påminnelse').checked).toBe(false)
  })

  it('reads the wrapped and camel-case shapes of the list route', async () => {
    client.getNotificationPreferences.mockResolvedValue({
      preferences: [{ eventType: 'space.agent_done', channel: 'email', enabled: true }],
    })
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(toggle('Agent ferdig').checked).toBe(true))

    cleanup()
    client.getNotificationPreferences.mockResolvedValue({
      data: [{ event_type: 'space.agent_failed', channel: 'email', enabled: true }],
    })
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(toggle('Agent feilet').checked).toBe(true))
  })

  it('saves a change on the email channel for that type', async () => {
    client.getNotificationPreferences.mockResolvedValue([])
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(client.getNotificationPreferences).toHaveBeenCalled())

    fireEvent.click(toggle('Svar i et Space'))
    await waitFor(() => expect(client.updateNotificationPreference).toHaveBeenCalledWith('space.reply', 'email', true))
    expect(toggle('Svar i et Space').checked).toBe(true)

    fireEvent.click(toggle('Nevnt i et Space'))
    await waitFor(() => expect(client.updateNotificationPreference).toHaveBeenCalledWith('space.mention', 'email', false))
    expect(client.updateNotificationPreference).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('reverts a failed save and shows an alert; the next save clears it', async () => {
    client.getNotificationPreferences.mockResolvedValue([])
    client.updateNotificationPreference.mockRejectedValueOnce(new Error('503'))
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(client.getNotificationPreferences).toHaveBeenCalled())

    fireEvent.click(toggle('Svar i et Space'))
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Kunne ikke lagre varselvalget.')
    expect(toggle('Svar i et Space').checked, 'the switch is back to its default').toBe(false)

    fireEvent.click(toggle('Agent ferdig'))
    await waitFor(() => expect(client.updateNotificationPreference).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
    expect(toggle('Agent ferdig').checked).toBe(true)
  })

  it('only offers the email channel (the in-app copy is the Space inbox)', async () => {
    client.getNotificationPreferences.mockResolvedValue([])
    render(() => <SpaceNotificationPreferences />)
    await waitFor(() => expect(client.getNotificationPreferences).toHaveBeenCalled())
    fireEvent.click(toggle('Agent venter'))
    await waitFor(() => expect(client.updateNotificationPreference).toHaveBeenCalled())
    for (const call of client.updateNotificationPreference.mock.calls) expect(call[1]).toBe('email')
  })
})
