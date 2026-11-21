// @vitest-environment jsdom

// O-10: "Forny godkjenning" renews the schedule in place. With VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED
// on, the renew action opens the dialog in renewal mode and makes one PATCH of the existing
// schedule through cron-client; it never creates a copy. With it off, the old dialog opens.

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
    renewCronScheduleApproval: vi.fn(),
    createCronSchedule: vi.fn(),
    deleteCronSchedule: vi.fn(),
    listScheduleFires: vi.fn(),
  }
})

vi.mock('@/shared/api/organization-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/organization-client')>()
  return { ...actual, listOrganizationMembers: vi.fn() }
})

import { ApiError } from '@/shared/api/api-error'
import {
  createCronSchedule,
  listCronSchedules,
  listScheduleFires,
  renewCronScheduleApproval,
  updateCronSchedule,
  type CronSchedule,
} from '@/shared/api/cron-client'
import { listOrganizationMembers } from '@/shared/api/organization-client'
import { AutomationsList } from './AutomationsList'

const mockedList = vi.mocked(listCronSchedules)
const mockedUpdate = vi.mocked(updateCronSchedule)
const mockedRenew = vi.mocked(renewCronScheduleApproval)
const mockedCreate = vi.mocked(createCronSchedule)

/** An approval that lapsed in the past, naming one read tool, the way the Model Plane stores it. */
const LAPSED_CONTRACT = JSON.stringify({
  v: 1,
  tools: [{ tool: 'web_search', capability: 'cap.tool.web_search', class: 'read', approved_at: '2025-12-01T10:00:00Z' }],
  expires_at: '2026-01-01T00:00:00Z',
  max_rounds: 6,
  max_tokens_per_round: 2048,
})

function lapsedSchedule(): CronSchedule {
  return {
    id: 'cron_expired',
    org_id: 'org-1',
    name: 'Gammel plan',
    description: '',
    schedule_expr: '0 9 * * *',
    timezone: 'Europe/Oslo',
    task_template: { kind: 'cron', title: 'Gammel plan', description: 'summarise the week', approval_contract: LAPSED_CONTRACT },
    enabled: false,
    creator_subject_id: 'user-1',
  }
}

/** The list opens on the active schedules; the lapsed one sits under "Utløpte". */
async function openRenewal(): Promise<void> {
  fireEvent.click(await screen.findByRole('button', { name: 'Utløpte (1)' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Forny godkjenning Gammel plan' }))
}

beforeEach(() => {
  vi.mocked(listScheduleFires).mockResolvedValue([])
  vi.mocked(listOrganizationMembers).mockResolvedValue([])
  mockedList.mockResolvedValue({ schedules: [lapsedSchedule()] })
})

afterEach(() => {
  vi.clearAllMocks()
  vi.restoreAllMocks()
  vi.unstubAllEnvs()
})

describe('renewing an expired approval in place', () => {
  it('sends one PATCH of the existing schedule with the new contract and resume, and creates no copy', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    mockedRenew.mockResolvedValue({ id: 'cron_expired' })

    render(() => <AutomationsList scope="mine" />)
    await openRenewal()

    // Renewal mode: no name or task to enter, and the lapsed approval's tool is selected.
    expect(screen.queryByRole('textbox', { name: 'Navn' })).toBeNull()
    const tool = (await screen.findByRole('checkbox', { name: /web_search/ })) as HTMLInputElement
    expect(tool.checked).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Forny godkjenning' }))

    await waitFor(() => expect(mockedRenew).toHaveBeenCalledTimes(1))
    const [orgId, cronId, contractText] = mockedRenew.mock.calls[0] as [string, string, string]
    expect(orgId).toBe('org-1')
    expect(cronId).toBe('cron_expired')
    const contract = JSON.parse(contractText) as {
      v: number
      tools: { tool: string; capability: string; class: string }[]
      expires_at: string
    }
    expect(contract.v).toBe(1)
    expect(contract.tools.map((entry) => [entry.tool, entry.capability, entry.class])).toEqual([
      ['web_search', 'cap.tool.web_search', 'read'],
    ])
    expect(contract.expires_at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/)
    expect(new Date(contract.expires_at).getTime()).toBeGreaterThan(Date.now())
    expect(mockedCreate).not.toHaveBeenCalled()
    expect(mockedUpdate).not.toHaveBeenCalled()
    // The list is read again, so the renewed schedule shows its new state.
    await waitFor(() => expect(mockedList.mock.calls.length).toBeGreaterThan(1))
  })

  it('renews nothing when every tool is unselected', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')

    render(() => <AutomationsList scope="mine" />)
    await openRenewal()
    fireEvent.click(await screen.findByRole('checkbox', { name: /web_search/ }))
    // A signal write is visible after the flush (Solid 2), so the submit reads the empty selection.
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Forny godkjenning' }))

    expect(await screen.findByText('Velg minst ett verktøy å godkjenne.')).toBeTruthy()
    expect(mockedRenew).not.toHaveBeenCalled()
  })

  it('shows a refusal from the server and keeps the dialog open', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    mockedRenew.mockRejectedValue(new ApiError('Forbidden', 403, null))

    render(() => <AutomationsList scope="mine" />)
    await openRenewal()
    fireEvent.click(await screen.findByRole('button', { name: 'Forny godkjenning' }))

    await waitFor(() => expect(mockedRenew).toHaveBeenCalledTimes(1))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByRole('dialog')).toBeTruthy()
  })

  it('opens the create dialog when this server answers the renewal as not_supported, and creates nothing by itself', async () => {
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    mockedRenew.mockRejectedValue(new ApiError('Renewal is not enabled', 501, 'not_supported'))

    render(() => <AutomationsList scope="mine" />)
    await openRenewal()
    fireEvent.click(await screen.findByRole('button', { name: 'Forny godkjenning' }))

    await waitFor(() => expect(mockedRenew).toHaveBeenCalledTimes(1))
    const name = (await screen.findByRole('textbox', { name: 'Navn' })) as HTMLInputElement
    expect(name.value).toBe('Gammel plan')
    expect(mockedCreate).not.toHaveBeenCalled()
    expect(mockedUpdate).not.toHaveBeenCalled()
  })

  it('keeps the old dialog, which creates a new schedule, while the switch is off', async () => {
    render(() => <AutomationsList scope="mine" />)
    await openRenewal()

    const name = (await screen.findByRole('textbox', { name: 'Navn' })) as HTMLInputElement
    expect(name.value).toBe('Gammel plan')
    expect(mockedRenew).not.toHaveBeenCalled()
  })
})
