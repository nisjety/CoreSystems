// @vitest-environment jsdom

// EP-6-S4: the one schedule list. Each test pins one scope or mode of the list against the
// cron-client calls it makes. The WS20 assertions stay in ScheduledRunsPanel.test.tsx and
// CronSchedulesSection.test.tsx, which mount this list.

import { render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const session = vi.hoisted(() => ({ current: {} as object }))

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => session.current,
}))

vi.mock('@/shared/api/cron-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/cron-client')>()
  return {
    ...actual,
    listCronSchedules: vi.fn(),
    updateCronSchedule: vi.fn(),
    deleteCronSchedule: vi.fn(),
    listScheduleFires: vi.fn(),
  }
})

import { listCronSchedules, listScheduleFires, type CronSchedule } from '@/shared/api/cron-client'
import { AutomationsList } from './AutomationsList'

const mockedList = vi.mocked(listCronSchedules)
const mockedFires = vi.mocked(listScheduleFires)

/** A member of the org, or an org owner, who is an admin for the Settings page. */
function sessionAs(orgRole: 'member' | 'owner') {
  return {
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1', name: 'Test', role: orgRole },
    permissions: [],
  }
}

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

/** A stored approval the way WS20-S4 writes it, with the goal the schedule was created with. */
function approvalTemplate(expiresAt: string) {
  return {
    kind: 'cron',
    title: 'Gammel plan',
    description: 'summarise the week',
    approval_contract: JSON.stringify({ v: 1, tools: [], expires_at: expiresAt, max_rounds: 6, max_tokens_per_round: 2048 }),
  }
}

beforeEach(() => {
  session.current = sessionAs('member')
  mockedFires.mockResolvedValue([])
})

afterEach(() => {
  vi.clearAllMocks()
  vi.restoreAllMocks()
  vi.unstubAllEnvs()
})

describe('AutomationsList', () => {
  it("mine never renders another creator's row", async () => {
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'mine', name: 'Min plan' }),
        schedule({ id: 'theirs', name: 'Andres plan', creator_subject_id: 'user-2' }),
      ],
    })

    render(() => <AutomationsList scope="mine" />)

    expect(await screen.findByText('Min plan')).toBeTruthy()
    expect(screen.queryByText('Andres plan')).toBeNull()
  })

  it('org scope lists every creator’s schedule, and no row shows the goal it was created with', async () => {
    session.current = sessionAs('owner')
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'mine', name: 'Min plan', task_template: { kind: 'cron', description: 'summarise the week' } }),
        schedule({ id: 'theirs', name: 'Andres plan', creator_subject_id: 'user-2' }),
      ],
    })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByText('Andres plan')).toBeTruthy()
    expect(screen.getByText('Min plan')).toBeTruthy()
    expect(screen.queryByText(/summarise the week/)).toBeNull()
  })

  it('org scope offers a member no pause and no delete', async () => {
    mockedList.mockResolvedValue({ schedules: [schedule({ name: 'Daglig rapport' })] })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByText('Daglig rapport')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Pause Daglig rapport' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Slett Daglig rapport' })).toBeNull()
  })

  it('org scope lets an admin pause and delete, never re-enables a lapsed approval, and offers no renewal, copy or results', async () => {
    session.current = sessionAs('owner')
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'cron_paused', name: 'Pauset plan', enabled: false }),
        schedule({
          id: 'cron_lapsed',
          name: 'Utløpt plan',
          enabled: false,
          task_template: approvalTemplate('2026-01-01T00:00:00Z'),
        }),
        schedule({ id: 'cron_own', name: 'Egen plan', creator_subject_id: 'user-1' }),
      ],
    })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByRole('button', { name: 'Aktiver Pauset plan' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Aktiver Utløpt plan' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Forny godkjenning Utløpt plan' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Slett Utløpt plan' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Dupliser og endre Egen plan' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Se resultater' })).toBeNull()
  })

  it('compact mount shows at most the limit, reads no pause reason, and offers no filters, time change or copy', async () => {
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'cron_paused', name: 'Pauset plan', enabled: false }),
        ...Array.from({ length: 6 }, (_, index) => schedule({ id: `cron_${index}`, name: `Plan ${index}` })),
      ],
    })

    render(() => <AutomationsList scope="mine" compact limit={5} />)

    expect(await screen.findByText('Plan 3')).toBeTruthy()
    expect(screen.queryByText('Plan 4')).toBeNull()
    expect(screen.queryByRole('button', { name: /^Aktive \(/ })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Endre tidspunkt Plan 0' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Dupliser og endre Plan 0' })).toBeNull()
    expect(mockedFires).not.toHaveBeenCalled()
  })

  it('reads the org again when its parent bumps the revision', async () => {
    session.current = sessionAs('owner')
    mockedList.mockResolvedValueOnce({ schedules: [schedule({ name: 'Første plan' })] })
    const [revision, setRevision] = createSignal(0)

    render(() => <AutomationsList scope="org" revision={revision()} />)
    expect(await screen.findByText('Første plan')).toBeTruthy()

    mockedList.mockResolvedValueOnce({ schedules: [schedule({ id: 'cron_2', name: 'Ny plan' })] })
    setRevision(1)
    flush()

    expect(await screen.findByText('Ny plan')).toBeTruthy()
    expect(mockedList).toHaveBeenCalledTimes(2)
  })
})
