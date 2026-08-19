// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { render, screen, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import ChiefCoreRoutingPage from './ChiefCoreRoutingPage'

const spacesClient = vi.hoisted(() => ({
  getAgentInstallations: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  getAgentInstallations: spacesClient.getAgentInstallations,
}))

function renderPage() {
  window.history.replaceState({}, '', '/agents/chief-core')
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/agents/chief-core" component={ChiefCoreRoutingPage} />
    </Router>
  ))
}

// No I18nProvider wraps these renders, so useI18n() falls back to the
// module's default context, whose `tr()` always resolves the Norwegian
// argument — matching defaultLocale ('no'), the same convention
// AgentInstallationsPage's own tests use.
describe('ChiefCoreRoutingPage', () => {
  beforeEach(() => {
    spacesClient.getAgentInstallations.mockReset()
  })

  it('groups agents by destination room instead of by definition', async () => {
    spacesClient.getAgentInstallations.mockResolvedValue([
      {
        agent_ref: 'agent-1',
        name: 'Driftsassistent',
        installations: [
          { space_ref: 'space-a', space_name: 'AQUATIQ AS', space_kind: 'room', status: 'active' },
        ],
      },
      {
        agent_ref: 'agent-2',
        name: 'Statusagent',
        installations: [
          { space_ref: 'space-a', space_name: 'AQUATIQ AS', space_kind: 'room', status: 'pending' },
        ],
      },
    ])
    renderPage()

    expect(await screen.findByRole('link', { name: 'AQUATIQ AS' })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'AQUATIQ AS' }).getAttribute('href')).toBe('/spaces/space-a')
    expect(screen.getByText('Driftsassistent')).toBeTruthy()
    expect(screen.getByText('Statusagent')).toBeTruthy()
    expect(screen.getByText('2 agenter')).toBeTruthy()
  })

  it('says plainly when no agent is published anywhere yet', async () => {
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
