// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { render, screen, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import AgentInstallationsPage from './AgentInstallationsPage'

const spacesClient = vi.hoisted(() => ({
  getAgentInstallations: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  getAgentInstallations: spacesClient.getAgentInstallations,
}))

function renderPage() {
  window.history.replaceState({}, '', '/agents/installations')
  const TestRouter = createRouter({
    routes: [{ path: '/agents/installations', component: AgentInstallationsPage }],
    history: memoryHistory('/agents/installations'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

// No I18nProvider wraps these renders, so useI18n() falls back to the
// module's default context, whose `tr()` always resolves the Norwegian
// argument — matching defaultLocale ('no'), the same convention SpacePage's
// own tests use.
describe('AgentInstallationsPage', () => {
  beforeEach(() => {
    spacesClient.getAgentInstallations.mockReset()
  })

  it('groups one definition\'s installations across rooms with each room\'s own status', async () => {
    spacesClient.getAgentInstallations.mockResolvedValue([
      {
        agent_ref: 'agent-1',
        name: 'Driftsassistent',
        description: 'Følger opp drift',
        definition_status: 'active',
        installations: [
          { space_ref: 'space-a', space_name: 'AQUATIQ AS', space_kind: 'room', status: 'active' },
          { space_ref: 'space-b', space_name: 'Personlig rom', space_kind: 'personal', status: 'pending' },
        ],
      },
    ])
    renderPage()

    expect(await screen.findByText('Driftsassistent')).toBeTruthy()
    expect(screen.getByText('Lagt til i 2 rom')).toBeTruthy()
    expect(screen.getByRole('link', { name: 'AQUATIQ AS' }).getAttribute('href')).toBe('/spaces/space-a')
    expect(screen.getByText('Aktiv')).toBeTruthy()
    expect(screen.getByText('Venter på bekreftelse')).toBeTruthy()
  })

  it('says plainly when no agent is published anywhere yet, instead of an empty list', async () => {
    spacesClient.getAgentInstallations.mockResolvedValue([])
    renderPage()

    expect(await screen.findByText(/Ingen agenter er publisert/)).toBeTruthy()
  })

  it('reports a load failure distinctly from an empty result', async () => {
    spacesClient.getAgentInstallations.mockRejectedValue(new Error('network'))
    renderPage()

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(screen.queryByText(/Ingen agenter er publisert/)).toBeNull()
  })
})
