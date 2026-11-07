// @vitest-environment jsdom

// EP-6-S4: the one schedule list. Each test pins one scope or mode of the list against the
// cron-client calls it makes. The WS20 assertions stay in ScheduledRunsPanel.test.tsx and
// CronSchedulesSection.test.tsx, which mount this list.

import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
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

vi.mock('@/shared/api/organization-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/organization-client')>()
  return { ...actual, listOrganizationMembers: vi.fn() }
})

import { listCronSchedules, listScheduleFires, type CronSchedule, type CronScheduleFire } from '@/shared/api/cron-client'
import { listOrganizationMembers } from '@/shared/api/organization-client'
import { AutomationsList } from './AutomationsList'

const mockedList = vi.mocked(listCronSchedules)
const mockedFires = vi.mocked(listScheduleFires)
const mockedMembers = vi.mocked(listOrganizationMembers)

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

/** One member of the org roster, as the member list reports them. */
function memberNamed(userId: string, name: string) {
  return { userId, name, email: `${userId}@example.test`, role: 'member', status: 'active' }
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
  mockedMembers.mockResolvedValue([])
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

  it('asks the server for the caller’s own schedules when it lists mine, and for the whole org when it lists org', async () => {
    mockedList.mockResolvedValue({ schedules: [] })

    const mine = render(() => <AutomationsList scope="mine" />)
    await waitFor(() => expect(mockedList).toHaveBeenCalledWith('org-1', { creator: 'self' }))
    mine.unmount()

    session.current = sessionAs('owner')
    render(() => <AutomationsList scope="org" />)
    await waitFor(() => expect(mockedList).toHaveBeenLastCalledWith('org-1'))
  })

  it('shows an admin who made each schedule by name, from the member list', async () => {
    session.current = sessionAs('owner')
    mockedMembers.mockResolvedValue([memberNamed('user-2', 'Kari Nordmann')])
    mockedList.mockResolvedValue({
      schedules: [schedule({ id: 'theirs', name: 'Andres plan', creator_subject_id: 'user-2' })],
    })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByText(/Kari Nordmann/)).toBeTruthy()
    expect(mockedMembers).toHaveBeenCalledWith('org-1')
  })

  it('reads the member list for an admin only, and shows a member no creator name', async () => {
    mockedMembers.mockResolvedValue([memberNamed('user-2', 'Kari Nordmann')])
    mockedList.mockResolvedValue({
      schedules: [schedule({ id: 'theirs', name: 'Andres plan', creator_subject_id: 'user-2' })],
    })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByText('Andres plan')).toBeTruthy()
    expect(mockedMembers).not.toHaveBeenCalled()
    expect(screen.queryByText(/Kari Nordmann/)).toBeNull()
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

describe('AutomationsList scheduled-run results', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function fire(overrides: Partial<CronScheduleFire>): CronScheduleFire {
    return {
      id: 'fire_1',
      fired_at: '2026-11-06T09:00:00Z',
      status: 'completed',
      run_id: null,
      thread_id: null,
      withheld_effects: 0,
      ...overrides,
    }
  }

  it('rates a completed fire by its run id, inside its result row, with the switch on only', async () => {
    vi.stubEnv('VITE_SCHEDULE_RESULTS', 'true')
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) =>
      new Response(JSON.stringify({ accepted: true }), { headers: { 'Content-Type': 'application/json' } }),
    )
    vi.stubGlobal('fetch', fetchMock)
    mockedList.mockResolvedValue({ schedules: [schedule({ id: 'cron_1', name: 'Daglig rapport' })] })
    mockedFires.mockResolvedValue([
      fire({ id: 'fire_done', run_id: 'run_fire_done' }),
      fire({ id: 'fire_failed', status: 'failed', run_id: 'run_fire_failed' }),
      fire({ id: 'fire_unrecorded', run_id: null }),
    ])

    render(() => <AutomationsList scope="mine" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Se resultater' }))

    const groups = await screen.findAllByRole('group', { name: 'Vurder svaret' })
    expect(groups).toHaveLength(1)
    fireEvent.click(within(groups[0]!).getByRole('button', { name: 'Godt svar' }))

    await waitFor(() => expect(fetchMock.mock.calls.filter((call) => call[0] === '/api/v1/chat/feedback')).toHaveLength(1))
    const rating = fetchMock.mock.calls.find((call) => call[0] === '/api/v1/chat/feedback')
    expect(JSON.parse(String(rating?.[1]?.body))).toEqual({ run_id: 'run_fire_done', rating: 'positive' })
  })

  it('shows no rating on a fire while the switch is off', async () => {
    vi.stubEnv('VITE_SCHEDULE_RESULTS', 'true')
    mockedList.mockResolvedValue({ schedules: [schedule({ id: 'cron_1', name: 'Daglig rapport' })] })
    mockedFires.mockResolvedValue([fire({ run_id: 'run_fire_done' })])

    render(() => <AutomationsList scope="mine" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Se resultater' }))

    await waitFor(() => expect(document.querySelector('.verevon-scheduled-runs-panel__fire')).toBeTruthy())
    expect(screen.queryByRole('group', { name: 'Vurder svaret' })).toBeNull()
  })
})
