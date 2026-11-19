import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const client = vi.hoisted(() => ({
  getInstallableSpaceAgents: vi.fn(),
  bindSpaceAgent: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...client,
}))

const { SpaceBindAgentDialog } = await import('./SpaceBindAgentDialog')

const organizationAgents = [
  {
    agent_ref: 'agent-ops',
    name: 'Driftsassistent',
    description: 'Svarer på driftsspørsmål',
    already_bound: false,
    add_state: 'addable' as const,
    owner_label: 'Kari Nordmann',
  },
  {
    agent_ref: 'agent-lager',
    name: 'Lagerhjelp',
    already_bound: false,
    add_state: 'blocked' as const,
    reason: 'add_policy_nobody',
  },
  {
    agent_ref: 'agent-faktura',
    name: 'Fakturaagent',
    already_bound: true,
    add_state: 'already_bound' as const,
  },
]

function renderDialog(options: { open?: boolean; onBound?: () => void; onClose?: () => void } = {}) {
  const onBound = options.onBound ?? vi.fn()
  const onClose = options.onClose ?? vi.fn()
  const open = options.open ?? true
  render(() => (
    <SpaceBindAgentDialog spaceRef="space_1" open={() => open} onClose={onClose} onBound={onBound} />
  ))
  return { onBound, onClose }
}

const rowOf = (name: string) => screen.getByText(name).closest('li') as HTMLElement
const addButton = (name: string) => rowOf(name).querySelector('button') as HTMLButtonElement

beforeEach(() => {
  client.getInstallableSpaceAgents.mockReset()
  client.getInstallableSpaceAgents.mockResolvedValue(organizationAgents)
  client.bindSpaceAgent.mockReset()
  client.bindSpaceAgent.mockResolvedValue({ agent_ref: 'agent-ops', subject_id: 'agent-ops-subject', status: 'pending' })
})

afterEach(() => cleanup())

describe('SpaceBindAgentDialog (WS17-S5)', () => {
  it('lists the organization agents with their owner, and marks one already in the room as added', async () => {
    renderDialog()
    expect(await screen.findByText('Driftsassistent')).toBeTruthy()
    expect(screen.getByText('Eier: Kari Nordmann')).toBeTruthy()
    expect(rowOf('Fakturaagent').textContent).toContain('Allerede lagt til')
    expect(rowOf('Fakturaagent').querySelector('button')).toBeNull()
    expect(client.getInstallableSpaceAgents.mock.calls[0]?.[0]).toBe('space_1')
  })

  it('draws nothing while closed, and reads nothing', () => {
    const view = render(() => (
      <SpaceBindAgentDialog spaceRef="space_1" open={() => false} onClose={vi.fn()} />
    ))
    expect(view.container.innerHTML).toBe('')
    expect(client.getInstallableSpaceAgents).not.toHaveBeenCalled()
  })

  it('a blocked row is disabled and says why, in the reason sentence for that reason', async () => {
    renderDialog()
    await screen.findByText('Lagerhjelp')
    expect(addButton('Lagerhjelp').disabled).toBe(true)
    expect(rowOf('Lagerhjelp').textContent).toContain('Eieren har stengt for at denne agenten legges til i rom.')
    // The addable row next to it stays usable.
    expect(addButton('Driftsassistent').disabled).toBe(false)
  })

  it('an older gateway without add_state decides only from already_bound', async () => {
    client.getInstallableSpaceAgents.mockResolvedValue([
      { agent_ref: 'agent-gammel', name: 'Gammel', already_bound: true },
      { agent_ref: 'agent-ny', name: 'Ny', already_bound: false },
    ])
    renderDialog()
    expect(await screen.findByText('Gammel')).toBeTruthy()
    expect(rowOf('Gammel').textContent).toContain('Allerede lagt til')
    expect(addButton('Ny').disabled).toBe(false)
  })

  it('a refused add shows the reason sentence on that row, not the generic line', async () => {
    client.bindSpaceAgent.mockRejectedValue(
      new ApiError('x', 403, 'agent_not_addable', { details: { reason: 'agent_suspended' } }),
    )
    const { onBound } = renderDialog()
    await screen.findByText('Driftsassistent')
    fireEvent.click(addButton('Driftsassistent'))
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Organisasjonen har satt agenten på pause, så den kan ikke legges til nå.')
    expect(rowOf('Driftsassistent').contains(alert)).toBe(true)
    expect(onBound).not.toHaveBeenCalled()
  })

  it('a refusal with a reason the dialog does not know gets the generic "cannot be added" sentence', async () => {
    client.bindSpaceAgent.mockRejectedValue(
      new ApiError('x', 403, 'agent_not_addable', { details: { reason: 'something_new' } }),
    )
    renderDialog()
    await screen.findByText('Driftsassistent')
    fireEvent.click(addButton('Driftsassistent'))
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Denne agenten kan ikke legges til i rommet akkurat nå.',
    )
  })

  it('a failure that is not a refusal keeps the honest partial state: it may already be pending', async () => {
    client.bindSpaceAgent.mockRejectedValue(new ApiError('x', 500, null))
    renderDialog()
    await screen.findByText('Driftsassistent')
    fireEvent.click(addButton('Driftsassistent'))
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Kunne ikke bekreftes som medlem av rommet ennå. Sjekk Agent-fanen.',
    )
  })

  it('a successful add tells the room, and reads the list again', async () => {
    const { onBound } = renderDialog()
    await screen.findByText('Driftsassistent')
    fireEvent.click(addButton('Driftsassistent'))
    await waitFor(() => expect(onBound).toHaveBeenCalledTimes(1))
    expect(client.bindSpaceAgent).toHaveBeenCalledWith('space_1', 'agent-ops')
    await waitFor(() => expect(client.getInstallableSpaceAgents).toHaveBeenCalledTimes(2))
  })

  it('the close button closes it', async () => {
    const { onClose } = renderDialog()
    await screen.findByText('Driftsassistent')
    fireEvent.click(screen.getByRole('button', { name: 'Lukk' }))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(client.bindSpaceAgent).not.toHaveBeenCalled()
  })
})
