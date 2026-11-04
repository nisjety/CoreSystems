// @vitest-environment jsdom

// EP-6-S1: the automations hub over the calls the chat panel already makes. Each test pins
// one filter, state or row action against the cron-client function the hub calls.

import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
    listScheduleFires: vi.fn(),
  }
})

import { ApiError } from '@/shared/api/api-error'
import {
  deleteCronSchedule,
  listCronSchedules,
  listScheduleFires,
  updateCronSchedule,
  type CronSchedule,
  type CronScheduleFire,
} from '@/shared/api/cron-client'
import { AutomationsHub } from './AutomationsHub'

const mockedList = vi.mocked(listCronSchedules)
const mockedFires = vi.mocked(listScheduleFires)
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

/** A stored approval the way WS20-S4 writes it, with the goal the schedule was created with. */
function approvalTemplate(expiresAt: string) {
  return {
    kind: 'cron',
    title: 'Gammel plan',
    description: 'summarise the week',
    approval_contract: JSON.stringify({ v: 1, tools: [], expires_at: expiresAt, max_rounds: 6, max_tokens_per_round: 2048 }),
  }
}

function fire(outcome: string): CronScheduleFire {
  return { id: 'fire_1', fired_at: '2026-11-02T07:00:00Z', status: 'paused', outcome, withheld_effects: 0 }
}

const ACTIVE = schedule({
  id: 'cron_active',
  name: 'Daglig rapport',
  task_template: { kind: 'cron', title: 'Daglig rapport', description: 'summarise the day' },
})
const PAUSED = schedule({ id: 'cron_paused', name: 'Ukentlig oversikt', enabled: false, schedule_expr: '0 9 * * 1' })
const EXPIRED = schedule({
  id: 'cron_expired',
  name: 'Gammel plan',
  enabled: false,
  task_template: approvalTemplate('2026-01-01T00:00:00Z'),
})
const ANOTHERS = schedule({ id: 'cron_other', name: 'Andres plan', creator_subject_id: 'user-2' })

beforeEach(() => {
  // A paused row asks for its last fire once it is on screen; a test that does not care about the reason gets none.
  mockedFires.mockResolvedValue([])
})

afterEach(() => {
  vi.clearAllMocks()
  vi.restoreAllMocks()
  vi.unstubAllEnvs()
})

describe('AutomationsHub', () => {
  it('shows the three filters with their counts, counting only the signed-in person’s schedules', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE, PAUSED, EXPIRED, ANOTHERS] })

    render(() => <AutomationsHub />)

    expect(await screen.findByRole('button', { name: 'Aktive (1)', pressed: true })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Pausede (1)', pressed: false })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Utløpte (1)', pressed: false })).toBeTruthy()
    expect(screen.queryByText('Andres plan')).toBeNull()
  })

  it('shows only the rows of the chosen filter', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE, PAUSED, EXPIRED] })

    render(() => <AutomationsHub />)
    expect(await screen.findByText('Daglig rapport')).toBeTruthy()
    expect(screen.queryByText('Ukentlig oversikt')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Pausede (1)' }))
    expect(await screen.findByText('Ukentlig oversikt')).toBeTruthy()
    expect(screen.queryByText('Daglig rapport')).toBeNull()
  })

  it('pauses an active schedule with an enabled-false update and nothing else', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })
    mockedUpdate.mockResolvedValue(undefined)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Pause Daglig rapport' }))

    await waitFor(() => expect(mockedUpdate).toHaveBeenCalledWith('org-1', 'cron_active', { enabled: false }))
    expect(mockedUpdate).toHaveBeenCalledTimes(1)
  })

  it('resumes a paused schedule whose approval is still live', async () => {
    mockedList.mockResolvedValue({ schedules: [PAUSED] })
    mockedUpdate.mockResolvedValue(undefined)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Pausede (1)' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Aktiver Ukentlig oversikt' }))

    await waitFor(() => expect(mockedUpdate).toHaveBeenCalledWith('org-1', 'cron_paused', { enabled: true }))
  })

  it('offers renewal, not resume, for a schedule whose approval has expired', async () => {
    mockedList.mockResolvedValue({ schedules: [EXPIRED] })

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Utløpte (1)' }))

    expect(await screen.findByRole('button', { name: 'Forny godkjenning Gammel plan' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Aktiver Gammel plan' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Endre tidspunkt Gammel plan' })).toBeNull()
    expect(screen.getByText(/godkjenning utløpt/)).toBeTruthy()
  })

  it('opens the create dialog seeded with the expired schedule when renewing it', async () => {
    mockedList.mockResolvedValue({ schedules: [EXPIRED] })

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Utløpte (1)' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Forny godkjenning Gammel plan' }))

    const name = (await screen.findByRole('textbox', { name: 'Navn' })) as HTMLInputElement
    expect(name.value).toBe('Gammel plan')
    // The goal is carried over, so the dialog never asks for a new task.
    expect(screen.queryByRole('textbox', { name: 'Oppgave' })).toBeNull()
  })

  it('changes only the cadence when the time is edited', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })
    mockedUpdate.mockResolvedValue(undefined)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Endre tidspunkt Daglig rapport' }))
    const cadence = await screen.findByRole('combobox', { name: 'Gjentakelse' })
    fireEvent.change(cadence, { target: { value: 'weekday-08' } })
    // A signal write is visible after the flush (Solid 2), so the save reads the new preset.
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre tidspunkt' }))

    await waitFor(() =>
      expect(mockedUpdate).toHaveBeenCalledWith('org-1', 'cron_active', { schedule_expr: '0 8 * * 1-5' }),
    )
    expect(mockedUpdate).toHaveBeenCalledTimes(1)
  })

  it('asks before a delete and deletes only after the confirmation', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })
    mockedDelete.mockResolvedValue(undefined)
    const confirm = vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Slett Daglig rapport' }))
    expect(mockedDelete).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Slett Daglig rapport' }))
    await waitFor(() => expect(mockedDelete).toHaveBeenCalledWith('org-1', 'cron_active'))
    expect(confirm).toHaveBeenCalledTimes(2)
  })

  it('hides "See results" while VITE_SCHEDULE_RESULTS is not the literal true', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })

    render(() => <AutomationsHub />)
    await screen.findByText('Daglig rapport')

    expect(screen.queryByRole('button', { name: 'Se resultater' })).toBeNull()
    expect(screen.queryByText('Se resultater')).toBeNull()
    expect(mockedFires).not.toHaveBeenCalled()
  })

  it('opens a schedule’s results behind VITE_SCHEDULE_RESULTS', async () => {
    vi.stubEnv('VITE_SCHEDULE_RESULTS', 'true')
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })
    mockedFires.mockResolvedValue([])

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Se resultater' }))

    await waitFor(() => expect(mockedFires).toHaveBeenCalledWith('org-1', 'cron_active'))
    expect(await screen.findByText('Ingen kjøringer ennå.')).toBeTruthy()
  })

  it('reads the reason a schedule is paused from its last fire, once, and names it', async () => {
    mockedList.mockResolvedValue({ schedules: [PAUSED] })
    mockedFires.mockResolvedValue([fire('paused_failures')])

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Pausede (1)' }))

    expect(await screen.findByText('Pauset etter feil')).toBeTruthy()
    expect(mockedFires).toHaveBeenCalledTimes(1)
    expect(mockedFires).toHaveBeenCalledWith('org-1', 'cron_paused', 1)
  })

  it('shows the scheduled-runs refusal when the list is refused under zero retention', async () => {
    mockedList.mockRejectedValue(new ApiError('refused', 403, 'zdr_feature_not_supported', { feature: 'scheduled_step' }))

    render(() => <AutomationsHub />)

    expect(await screen.findByText(/planlagte kjøringer/)).toBeTruthy()
    expect(screen.queryByText(/Ingen automatiseringer ennå/)).toBeNull()
  })

  it('says so when there are no schedules yet, and links to the conversations to schedule from', async () => {
    mockedList.mockResolvedValue({ schedules: [] })

    render(() => <AutomationsHub />)

    expect(await screen.findByText(/Ingen automatiseringer ennå/)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Gå til samtalene' }).getAttribute('href')).toBe('/chat')
  })

  it('states the cadence of a schedule in words', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })

    render(() => <AutomationsHub />)

    expect(await screen.findByText(/Hver dag kl\. 09:00/)).toBeTruthy()
  })

  it('opens a copy with the name prefilled, and keeps the goal the schedule was created with', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Dupliser og endre Daglig rapport' }))

    const name = (await screen.findByRole('textbox', { name: 'Navn' })) as HTMLInputElement
    expect(name.value).toBe('Kopi av Daglig rapport')
    // A seeded goal is carried over; the create dialog hides its field rather than asking again.
    expect(screen.queryByRole('textbox', { name: 'Oppgave' })).toBeNull()
  })

  it('places the filters before each row’s actions in the focus order', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE] })

    render(() => <AutomationsHub />)
    await screen.findByRole('button', { name: 'Pause Daglig rapport' })

    const buttons = Array.from(document.querySelectorAll('button'))
    const indexOf = (name: string) => buttons.findIndex((button) => button.getAttribute('aria-label') === name || button.textContent?.trim() === name)
    const filterIndex = indexOf('Aktive (1)')
    const pauseIndex = indexOf('Pause Daglig rapport')
    const deleteIndex = indexOf('Slett Daglig rapport')

    expect(filterIndex).toBeGreaterThanOrEqual(0)
    expect(filterIndex).toBeLessThan(pauseIndex)
    expect(pauseIndex).toBeLessThan(deleteIndex)
  })
})
