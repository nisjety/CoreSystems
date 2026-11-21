// @vitest-environment jsdom

// F-f05-20: with VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED on, the consent card asks for the org's delegable read tools
// once per tab, when the dialog opens, shows a short loading line meanwhile, and offers only the tools the list named.
// Off, it asks for nothing. A chat that keeps no data asks for nothing either.

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

vi.mock('@/shared/api/schedule-from-chat', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/schedule-from-chat')>()
  return { ...actual, createScheduleFromChat: vi.fn(), loadDelegableScheduleTools: vi.fn() }
})

import { createScheduleFromChat, loadDelegableScheduleTools } from '@/shared/api/schedule-from-chat'
import { ScheduleDialog } from './ScheduleDialog'

const mockedCreate = vi.mocked(createScheduleFromChat)
const mockedLoad = vi.mocked(loadDelegableScheduleTools)

function renderDialog(zeroRetention?: boolean) {
  return render(() => (
    <ScheduleDialog
      onClose={vi.fn()}
      seedTitle="Ukentlig rapport"
      seedGoal="Skriv ukesrapporten"
      threadOriginHint="message-action"
      zeroRetention={zeroRetention}
    />
  ))
}

afterEach(() => {
  vi.clearAllMocks()
  vi.unstubAllEnvs()
})

describe('ScheduleDialog — delegable read tools (F-f05-20)', () => {
  it('asks for nothing while the switch is off, and keeps the draft-only copy', () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', '')
    const { getByTestId, queryByTestId } = renderDialog()

    expect(mockedLoad).not.toHaveBeenCalled()
    expect(queryByTestId('schedule-consent-loading')).toBeNull()
    expect(getByTestId('schedule-consent-empty').textContent).toContain('forblir kun utkast')
  })

  it('asks for the list once when it opens, and offers the tools it names', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    mockedLoad.mockResolvedValue([{ tool: 'web_search', capability: 'web' }])
    const { getByRole } = renderDialog()

    expect(mockedLoad).toHaveBeenCalledTimes(1)
    expect(mockedLoad).toHaveBeenCalledWith('org-1')
    await waitFor(() => expect(getByRole('checkbox', { name: /web_search/ })).toBeTruthy())
  })

  it('shows a loading line until the list answers, then the draft-only copy when the list is empty', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    let answer: (tools: never[]) => void = () => {}
    mockedLoad.mockReturnValue(new Promise((resolve) => {
      answer = resolve
    }))
    const { getByTestId, queryByTestId } = renderDialog()

    expect(getByTestId('schedule-consent-loading')).toBeTruthy()
    answer([])
    await waitFor(() => expect(queryByTestId('schedule-consent-loading')).toBeNull())
    expect(getByTestId('schedule-consent-empty').textContent).toContain('forblir kun utkast')
  })

  it('creates the schedule with exactly the tool and capability of each ticked row', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    mockedLoad.mockResolvedValue([
      { tool: 'web_search', capability: 'web' },
      { tool: 'calendar_read', capability: 'calendar' },
    ])
    mockedCreate.mockResolvedValue({ id: 'cron_1' })
    const { getByRole } = renderDialog()

    const webSearch = await waitFor(() => getByRole('checkbox', { name: /web_search/ }))
    fireEvent.click(webSearch)
    flush()
    fireEvent.click(getByRole('button', { name: 'Opprett plan' }))

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
    expect(mockedCreate.mock.calls[0]![0].contract?.tools).toEqual([{ tool: 'web_search', capability: 'web' }])
  })

  it('asks for nothing in a temporary or zero-retention chat, and shows no consent card', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    const { getByTestId, queryByTestId } = renderDialog(true)

    expect(mockedLoad).not.toHaveBeenCalled()
    expect(queryByTestId('schedule-consent-loading')).toBeNull()
    expect(queryByTestId('schedule-consent-card')).toBeNull()
    expect(getByTestId('schedule-consent-empty').textContent).toContain('forblir kun utkast')
  })
})
