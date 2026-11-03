// @vitest-environment jsdom

// EP-6-S5: the summary at the top of the agents landing. Each test pins one state of the two reads
// it makes (the org's installations and the person's own schedules) against the copy a person sees.

import { render, screen } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import type { CronSchedule } from '@/shared/api/cron-client'
import type { AgentDefinitionInstallations } from '@/shared/api/spaces-client'

const reads = vi.hoisted(() => ({
  getAgentInstallations: vi.fn(),
  listMySchedules: vi.fn(),
  session: { current: {} as object },
}))

vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  getSession: () => reads.session.current,
}))

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  getAgentInstallations: reads.getAgentInstallations,
}))

vi.mock('@/shared/api/schedule-from-chat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/schedule-from-chat')>()),
  listMySchedules: reads.listMySchedules,
}))

import { AgentsHubSummary } from './AgentsHubSummary'

const ACTIVE_NEXT_RUN = '2026-11-04T08:00:00.000Z'

function schedule(overrides: Partial<CronSchedule>): CronSchedule {
  return {
    id: 'cron_1',
    org_id: 'org-1',
    name: 'Daglig rapport',
    description: '',
    schedule_expr: '0 8 * * *',
    timezone: 'Europe/Oslo',
    task_template: {},
    enabled: true,
    next_fire_at: ACTIVE_NEXT_RUN,
    creator_subject_id: 'user-1',
    ...overrides,
  }
}

/** A stored approval whose expiry has passed, the way WS20-S4 writes it. */
function lapsedTemplate() {
  return {
    kind: 'cron',
    approval_contract: JSON.stringify({ v: 1, tools: [], expires_at: '2020-01-01T00:00:00Z', max_rounds: 6 }),
  }
}

const driftsassistent: AgentDefinitionInstallations = {
  agent_ref: 'agent-1',
  name: 'Driftsassistent',
  description: 'Følger opp drift',
  definition_status: 'active',
  installations: [
    { space_ref: 'space-a', space_name: 'AQUATIQ AS', space_kind: 'room', status: 'active' },
    { space_ref: 'space-b', space_name: 'Personlig rom', space_kind: 'personal', status: 'pending' },
  ],
}

beforeEach(() => {
  reads.session.current = {
    status: 'authenticated',
    user: { id: 'user-1' },
    activeOrg: { id: 'org-1', name: 'Aquatiq', role: 'member' },
    permissions: [],
  }
  reads.getAgentInstallations.mockReset()
  reads.getAgentInstallations.mockResolvedValue([])
  reads.listMySchedules.mockReset()
  reads.listMySchedules.mockResolvedValue([])
})

// No I18nProvider wraps these renders, so useI18n() resolves the Norwegian argument, matching
// defaultLocale ('no'), the same convention the other agent page tests use.
describe('AgentsHubSummary', () => {
  it('says plainly that nothing exists yet, and names the next step for each card', async () => {
    const { container } = render(() => <AgentsHubSummary />)

    expect(await screen.findByText('Ingen agenter ennå.')).toBeTruthy()
    expect(await screen.findByText('Ingen automatiseringer ennå.')).toBeTruthy()
    expect(container.querySelector('a[href="/agents/installations"]')).not.toBeNull()
    expect(container.querySelector('a[href="/chat"]')).not.toBeNull()
  })

  it('counts the agent and the active automation, and shows when the next run is', async () => {
    reads.getAgentInstallations.mockResolvedValue([driftsassistent])
    reads.listMySchedules.mockResolvedValue([schedule({})])
    const { container } = render(() => <AgentsHubSummary />)

    expect(await screen.findByText('Driftsassistent')).toBeTruthy()
    expect(screen.getByText('1 agent')).toBeTruthy()
    expect(screen.getByText('1 aktiv')).toBeTruthy()
    expect(container.querySelector('time')?.getAttribute('datetime')).toBe(ACTIVE_NEXT_RUN)
  })

  it('does not treat a paused or expired automation as the next run', async () => {
    reads.listMySchedules.mockResolvedValue([
      schedule({ id: 'cron_paused', enabled: false, next_fire_at: '2026-11-04T06:00:00.000Z' }),
      schedule({ id: 'cron_lapsed', task_template: lapsedTemplate(), next_fire_at: '2026-11-04T07:00:00.000Z' }),
      schedule({ id: 'cron_active' }),
    ])
    const { container } = render(() => <AgentsHubSummary />)

    expect(await screen.findByText('1 aktiv · 1 pauset · 1 utløpt')).toBeTruthy()
    expect(container.querySelector('time')?.getAttribute('datetime')).toBe(ACTIVE_NEXT_RUN)
  })

  it('reports a failed read as a failure, not as an empty list', async () => {
    reads.getAgentInstallations.mockRejectedValue(new Error('network'))
    render(() => <AgentsHubSummary />)

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText('Ingen agenter ennå.')).toBeNull()
  })

  it('shows the zero-retention refusal for automations instead of an empty list', async () => {
    reads.listMySchedules.mockRejectedValue(
      new ApiError('refused', 403, 'zdr_feature_not_supported', { feature: 'scheduled_step' }),
    )
    render(() => <AgentsHubSummary />)

    expect(await screen.findByText(/støttes ikke med Zero Data Retention/)).toBeTruthy()
    expect(screen.queryByText('Ingen automatiseringer ennå.')).toBeNull()
  })

  it('links each card to the page that holds its detail, and never to the fleet console', async () => {
    const { container } = render(() => <AgentsHubSummary />)
    await screen.findByText('Ingen agenter ennå.')

    for (const href of ['/agents/installations', '/agents/automations', '/agents/runs', '/agents/customize']) {
      expect(container.querySelector(`a[href="${href}"]`), href).not.toBeNull()
    }
    expect(container.querySelector('a[href^="/agents/fleets"]')).toBeNull()
  })
})
