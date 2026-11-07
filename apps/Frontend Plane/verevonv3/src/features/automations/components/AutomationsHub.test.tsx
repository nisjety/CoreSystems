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
    runScheduleNow: vi.fn(),
  }
})

import { ApiError } from '@/shared/api/api-error'
import {
  deleteCronSchedule,
  listCronSchedules,
  listScheduleFires,
  runScheduleNow,
  updateCronSchedule,
  type CronSchedule,
} from '@/shared/api/cron-client'
import { AutomationsHub } from './AutomationsHub'
import { AutomationsList } from './AutomationsList'

const mockedList = vi.mocked(listCronSchedules)
const mockedFires = vi.mocked(listScheduleFires)
const mockedUpdate = vi.mocked(updateCronSchedule)
const mockedDelete = vi.mocked(deleteCronSchedule)
const mockedRun = vi.mocked(runScheduleNow)

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
  // The results read is answered empty unless a test opens a schedule's results.
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

  it('names a pause from the last outcome its row carries, without reading its fires', async () => {
    mockedList.mockResolvedValue({ schedules: [{ ...PAUSED, last_outcome: 'paused_failures' }] })

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Pausede (1)' }))

    expect(await screen.findByText('Pauset etter feil')).toBeTruthy()
    expect(mockedFires).not.toHaveBeenCalled()
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

describe('AutomationsHub Run now (EP-6-S3)', () => {
  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
  const RUN_NOW = 'Kjør nå Daglig rapport'
  const STARTED = { fire_id: 'fire_1', fire_key: 'manual:x', task_id: 'task_1', status: 'pending', deduplicated: false }

  // Each press makes a request id the test can tell apart from the one before it.
  let issued = 0
  beforeEach(() => {
    issued = 0
    vi.stubGlobal('crypto', { randomUUID: () => `00000000-0000-4000-8000-${String(++issued).padStart(12, '0')}` })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('is not offered while the server reports the feature off', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: false } })

    render(() => <AutomationsHub />)

    expect(await screen.findByText('Daglig rapport')).toBeTruthy()
    expect(screen.queryByRole('button', { name: RUN_NOW })).toBeNull()
  })

  it('is offered on the creator’s live schedules, and never on one whose approval has lapsed', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE, EXPIRED], features: { run_now: true } })

    render(() => <AutomationsHub />)
    expect(await screen.findByRole('button', { name: RUN_NOW })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Utløpte (1)' }))
    expect(await screen.findByRole('button', { name: 'Forny godkjenning Gammel plan' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Kjør nå Gammel plan' })).toBeNull()
  })

  it('is never offered for a schedule someone else created', async () => {
    mockedList.mockResolvedValue({ schedules: [ANOTHERS], features: { run_now: true } })

    render(() => <AutomationsHub />)

    expect(await screen.findByText(/Ingen automatiseringer ennå/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Kjør nå Andres plan' })).toBeNull()
  })

  it('is a hub control only, so the Settings list never offers it', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: true } })

    render(() => <AutomationsList scope="org" />)

    expect(await screen.findByText('Daglig rapport')).toBeTruthy()
    expect(screen.queryByRole('button', { name: RUN_NOW })).toBeNull()
  })

  it('reuses the request id when a press is retried after its answer was lost', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: true } })
    mockedRun.mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValueOnce(STARTED)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: RUN_NOW }))
    await screen.findByRole('alert')
    fireEvent.click(screen.getByRole('button', { name: RUN_NOW }))
    await waitFor(() => expect(mockedRun).toHaveBeenCalledTimes(2))

    const [first, second] = mockedRun.mock.calls.map((call) => call[2])
    expect(first).toMatch(UUID)
    expect(second).toBe(first)
    expect(mockedRun).toHaveBeenNthCalledWith(2, 'org-1', 'cron_active', first)
  })

  it('makes a new request id after a refusal, and again after the fire started', async () => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: true } })
    mockedRun
      .mockRejectedValueOnce(new ApiError('refused', 409, 'run_now_in_flight'))
      .mockResolvedValueOnce(STARTED)
      .mockResolvedValueOnce(STARTED)

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: RUN_NOW }))
    await screen.findByRole('alert')
    fireEvent.click(screen.getByRole('button', { name: RUN_NOW }))
    await screen.findByText('Kjøringen er startet.')
    fireEvent.click(screen.getByRole('button', { name: RUN_NOW }))
    await waitFor(() => expect(mockedRun).toHaveBeenCalledTimes(3))

    const [refused, started, next] = mockedRun.mock.calls.map((call) => call[2])
    expect(refused).toMatch(UUID)
    expect(started).not.toBe(refused)
    expect(next).not.toBe(started)
  })

  it.each([
    ['run_now_in_flight', 409, 'Planen kjører allerede. Vent til kjøringen er ferdig, og prøv igjen.'],
    ['run_now_rate_limited', 429, 'Planen har kjørt for mange ganger den siste timen. Prøv igjen senere.'],
    ['approval_expired', 409, 'Godkjenningen er utløpt, så planen kan ikke kjøres nå. Forny godkjenningen først.'],
    ['fire_not_authorized', 403, 'Kjøringen ble ikke godkjent, så planen kjørte ikke. Sjekk planen og godkjenningen.'],
  ])('says what a refused Run now means for %s, in words the person can act on', async (code, status, sentence) => {
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: true } })
    mockedRun.mockRejectedValueOnce(new ApiError('refused', status, code))

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: RUN_NOW }))

    expect(await screen.findByText(sentence)).toBeTruthy()
    expect(screen.queryByText('Kjøringen er startet.')).toBeNull()
  })

  it('reads the open results again after a Run now, so the new fire shows first', async () => {
    vi.stubEnv('VITE_SCHEDULE_RESULTS', 'true')
    mockedList.mockResolvedValue({ schedules: [ACTIVE], features: { run_now: true } })
    mockedRun.mockResolvedValue(STARTED)
    mockedFires.mockResolvedValueOnce([]).mockResolvedValueOnce([
      {
        id: 'fire_m',
        fired_at: '2026-11-04T08:00:00Z',
        status: 'pending',
        outcome: null,
        run_id: null,
        thread_id: null,
        withheld_effects: 0,
        fire_key: 'manual:6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10',
      },
    ])

    render(() => <AutomationsHub />)
    fireEvent.click(await screen.findByRole('button', { name: 'Se resultater' }))
    expect(await screen.findByText('Ingen kjøringer ennå.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: RUN_NOW }))

    expect(await screen.findByText('Manuell')).toBeTruthy()
    expect(mockedFires).toHaveBeenCalledTimes(2)
  })
})
