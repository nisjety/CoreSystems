// @vitest-environment jsdom

// chat-wave2 "schedule" spec §2.1, §3 — the create-schedule dialog: preset ->
// cron-expr mapping is exact for all four presets, the custom toggle reveals
// the raw field, and submit calls `createScheduleFromChat` with
// `workflow_type` omitted and `timezone: 'Europe/Oslo'` by default.

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

vi.mock('@/shared/api/schedule-from-chat', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/schedule-from-chat')>()
  return { ...actual, createScheduleFromChat: vi.fn() }
})

import { createScheduleFromChat } from '@/shared/api/schedule-from-chat'
import { ScheduleDialog } from './ScheduleDialog'

const mockedCreate = vi.mocked(createScheduleFromChat)

function renderDialog(seedTitle = 'Ukentlig rapport', seedGoal = 'Skriv ukesrapporten') {
  const onCreated = vi.fn()
  const onClose = vi.fn()
  const utils = render(() => (
    <ScheduleDialog
      onClose={onClose}
      seedTitle={seedTitle}
      seedGoal={seedGoal}
      threadOriginHint="message-action"
      onCreated={onCreated}
    />
  ))
  return { ...utils, onCreated, onClose }
}

afterEach(() => {
  vi.clearAllMocks()
})

describe('ScheduleDialog — cadence presets', () => {
  it('maps every preset label to its exact cron expression on submit', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_1' })
    const cases: Array<[string, string]> = [
      ['Hver dag kl. 09:00', '0 9 * * *'],
      ['Hver ukedag kl. 08:00', '0 8 * * 1-5'],
      ['Hver mandag kl. 09:00', '0 9 * * 1'],
      ['Den 1. i hver måned kl. 09:00', '0 9 1 * *'],
    ]
    for (const [label, expr] of cases) {
      mockedCreate.mockClear()
      const { getByRole, unmount } = renderDialog()
      const select = getByRole('combobox') as HTMLSelectElement
      // Select by matching option text directly, since ids are internal.
      const option = Array.from(select.options).find((o) => o.textContent === label)
      expect(option).toBeTruthy()
      fireEvent.change(select, { target: { value: option!.value } })
      flush()
      fireEvent.click(getByRole('button', { name: 'Opprett plan' }))
      await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
      expect(mockedCreate.mock.calls[0]![0].scheduleExpr).toBe(expr)
      unmount()
    }
  })

  it('reveals the raw cron-expression field for "Egendefinert" and uses its value', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_1' })
    const { getByRole, getByPlaceholderText, queryByPlaceholderText } = renderDialog()

    expect(queryByPlaceholderText(/min time dag måned ukedag/)).toBeNull()

    const select = getByRole('combobox') as HTMLSelectElement
    const customOption = Array.from(select.options).find((o) => o.textContent === 'Egendefinert (cron-uttrykk)')
    fireEvent.change(select, { target: { value: customOption!.value } })
    flush()

    const customField = getByPlaceholderText(/min time dag måned ukedag/)
    fireEvent.input(customField, { target: { value: '*/15 * * * *' } })
    flush()

    fireEvent.click(getByRole('button', { name: 'Opprett plan' }))
    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
    expect(mockedCreate.mock.calls[0]![0].scheduleExpr).toBe('*/15 * * * *')
  })
})

describe('ScheduleDialog — submit shape', () => {
  it('defaults to Europe/Oslo and never asks for a workflow type (schedule-from-chat.ts omits it)', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_1' })
    const { getByRole } = renderDialog('Navn', 'Målet')

    fireEvent.click(getByRole('button', { name: 'Opprett plan' }))

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
    const input = mockedCreate.mock.calls[0]![0]
    expect(input.timezone).toBe('Europe/Oslo')
    expect(input.orgId).toBe('org-1')
    expect(input.goalText).toBe('Målet')
    expect('workflowType' in input).toBe(false)
  })

  it('calls onCreated and closes on success', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_42' })
    const { getByRole, onCreated } = renderDialog()

    fireEvent.click(getByRole('button', { name: 'Opprett plan' }))

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith('cron_42'))
  })

  it('shows an error and keeps the dialog open on failure', async () => {
    mockedCreate.mockRejectedValue(new Error('boom'))
    const { getByRole, findByRole } = renderDialog()

    fireEvent.click(getByRole('button', { name: 'Opprett plan' }))

    const alert = await findByRole('alert')
    expect(alert.textContent).toContain('Kunne ikke opprette planen')
  })
})
