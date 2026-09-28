// @vitest-environment jsdom

// chat-wave2 "schedule" spec §2.1, §3 — list/pause/delete a schedule from
// chat. Filters `listCronSchedules` results to `creator_subject_id ===
// session.user.id`; pause/resume and delete call the existing
// `cron-client.ts` functions; "Se resultater" is disabled for every row
// (Phase B's listing endpoint does not exist yet — see the module doc).

import { render, screen, waitFor } from '@solidjs/testing-library'
import { fireEvent } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    activeOrg: { id: 'org-1', name: 'Test', role: 'member' },
    user: { id: 'user-1' },
  }),
}))

vi.mock('@/shared/api/cron-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/cron-client')>()
  return {
    ...actual,
    listCronSchedules: vi.fn(),
    updateCronSchedule: vi.fn(),
    deleteCronSchedule: vi.fn(),
  }
})

import { listCronSchedules, updateCronSchedule, deleteCronSchedule, type CronSchedule } from '@/shared/api/cron-client'
import { ScheduledRunsPanel } from './ScheduledRunsPanel'

const mockedList = vi.mocked(listCronSchedules)
const mockedUpdate = vi.mocked(updateCronSchedule)
const mockedDelete = vi.mocked(deleteCronSchedule)

function schedule(overrides: Partial<CronSchedule>): CronSchedule {
  return {
    id: 'cron_1',
    org_id: 'org-1',
    name: 'Daglig rapport',
    description: '',
    schedule_expr: '0 9 * * *',
    timezone: 'Europe/Oslo',
    task_template: {},
    enabled: true,
    creator_subject_id: 'user-1',
    ...overrides,
  }
}

afterEach(() => {
  vi.clearAllMocks()
  vi.restoreAllMocks()
})

describe('ScheduledRunsPanel', () => {
  it('shows only the schedules the signed-in user created', async () => {
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'mine', name: 'Min plan', creator_subject_id: 'user-1' }),
        schedule({ id: 'theirs', name: 'Andres plan', creator_subject_id: 'user-2' }),
      ],
    })

    render(() => <ScheduledRunsPanel />)

    expect(await screen.findByText('Min plan')).toBeTruthy()
    expect(screen.queryByText('Andres plan')).toBeNull()
  })

  it('renders "Se resultater" disabled for every row', async () => {
    mockedList.mockResolvedValue({ schedules: [schedule({})] })

    render(() => <ScheduledRunsPanel />)

    const button = await screen.findByText('Se resultater')
    expect((button as HTMLButtonElement).disabled).toBe(true)
  })

  it('pauses an active schedule via updateCronSchedule', async () => {
    mockedList.mockResolvedValue({ schedules: [schedule({ enabled: true })] })
    mockedUpdate.mockResolvedValue(undefined)

    render(() => <ScheduledRunsPanel />)
    const pauseButton = await screen.findByText('Pause')
    fireEvent.click(pauseButton)

    await waitFor(() => expect(mockedUpdate).toHaveBeenCalledWith('org-1', 'cron_1', { enabled: false }))
  })

  it('deletes a schedule via deleteCronSchedule after confirmation', async () => {
    mockedList.mockResolvedValue({ schedules: [schedule({})] })
    mockedDelete.mockResolvedValue(undefined)
    vi.spyOn(window, 'confirm').mockReturnValue(true)

    render(() => <ScheduledRunsPanel />)
    const deleteButton = await screen.findByLabelText('Slett Daglig rapport')
    fireEvent.click(deleteButton)

    await waitFor(() => expect(mockedDelete).toHaveBeenCalledWith('org-1', 'cron_1'))
  })

  it('does not delete when the confirmation is declined', async () => {
    mockedList.mockResolvedValue({ schedules: [schedule({})] })
    vi.spyOn(window, 'confirm').mockReturnValue(false)

    render(() => <ScheduledRunsPanel />)
    const deleteButton = await screen.findByLabelText('Slett Daglig rapport')
    fireEvent.click(deleteButton)

    expect(mockedDelete).not.toHaveBeenCalled()
  })
})
