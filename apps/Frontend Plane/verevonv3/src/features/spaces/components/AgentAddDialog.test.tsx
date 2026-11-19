import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

import type { AgentAddTarget } from './AgentAddDialog'

const client = vi.hoisted(() => ({ bindSpaceAgent: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => client)

const { AgentAddDialog } = await import('./AgentAddDialog')

function renderAdd(target: AgentAddTarget | undefined) {
  const onClose = vi.fn()
  const onBound = vi.fn()
  const onSendWithout = vi.fn()
  const onRequest = vi.fn()
  const view = render(() => (
    <AgentAddDialog
      spaceRef="space_1"
      target={() => target}
      onClose={onClose}
      onBound={onBound}
      onSendWithout={onSendWithout}
      onRequest={onRequest}
    />
  ))
  return { view, onClose, onBound, onSendWithout, onRequest }
}

const addTarget: AgentAddTarget = { kind: 'add', agentRef: 'agent-ops', name: 'Driftsassistent' }
const requestTarget: AgentAddTarget = { kind: 'request', name: 'Driftsassistent' }

beforeEach(() => {
  client.bindSpaceAgent.mockReset()
})

afterEach(() => cleanup())

describe('AgentAddDialog (WS08-S6)', () => {
  it('draws nothing when there is no target', () => {
    const { view } = renderAdd(undefined)
    expect(view.container.innerHTML).toBe('')
  })

  it('offers to add the agent and send, or to send without it, and names the agent', () => {
    renderAdd(addTarget)
    expect(screen.getByRole('heading', { name: 'Driftsassistent er ikke i rommet' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Legg til og send' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Send uten å legge til' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Avbryt' })).toBeTruthy()
  })

  it('add and send binds the agent through the room, then hands its subject id on without sending itself', async () => {
    client.bindSpaceAgent.mockResolvedValue({ agent_ref: 'agent-ops', subject_id: 'agent-ops-subject', status: 'active' })
    const { onBound, onSendWithout, onClose } = renderAdd(addTarget)
    fireEvent.click(screen.getByRole('button', { name: 'Legg til og send' }))
    await waitFor(() => expect(onBound).toHaveBeenCalledWith({ subjectId: 'agent-ops-subject', name: 'Driftsassistent' }))
    expect(client.bindSpaceAgent).toHaveBeenCalledWith('space_1', 'agent-ops')
    expect(onSendWithout).not.toHaveBeenCalled()
    expect(onClose).not.toHaveBeenCalled()
  })

  it('a failed add says nothing was sent, and hands nothing on', async () => {
    client.bindSpaceAgent.mockRejectedValue(new ApiError('x', 500, null))
    const { onBound, onSendWithout } = renderAdd(addTarget)
    fireEvent.click(screen.getByRole('button', { name: 'Legg til og send' }))
    expect((await screen.findByRole('alert')).textContent).toBe('Agenten kunne ikke legges til. Ingenting er sendt.')
    expect(onBound).not.toHaveBeenCalled()
    expect(onSendWithout).not.toHaveBeenCalled()
  })

  it('while the add is out, both add buttons are off and say that it is adding', async () => {
    client.bindSpaceAgent.mockImplementation(() => new Promise(() => {}))
    renderAdd(addTarget)
    fireEvent.click(screen.getByRole('button', { name: 'Legg til og send' }))
    const adding = (await screen.findByRole('button', { name: 'Legger til …' })) as HTMLButtonElement
    expect(adding.disabled).toBe(true)
    expect((screen.getByRole('button', { name: 'Send uten å legge til' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('send without adding sends the words as they are, binds nothing, and closes', () => {
    const { onSendWithout, onClose, onBound } = renderAdd(addTarget)
    fireEvent.click(screen.getByRole('button', { name: 'Send uten å legge til' }))
    expect(onSendWithout).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(client.bindSpaceAgent).not.toHaveBeenCalled()
    expect(onBound).not.toHaveBeenCalled()
  })

  it('a member who cannot add agents gets only a request, which asks the managers and binds nothing', () => {
    const { onRequest, onClose } = renderAdd(requestTarget)
    expect(screen.getByRole('heading', { name: 'Be en leder legge til en agent' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Legg til og send' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Send og be om det' }))
    expect(onRequest).toHaveBeenCalledWith('Driftsassistent')
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(client.bindSpaceAgent).not.toHaveBeenCalled()
  })

  it('cancel and a click on the dimmed backdrop close without doing anything else', () => {
    const first = renderAdd(addTarget)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    expect(first.onClose).toHaveBeenCalledTimes(1)
    cleanup()
    const second = renderAdd(addTarget)
    fireEvent.click(second.view.container.querySelector('.verevon-space-create-agent') as HTMLElement)
    expect(second.onClose).toHaveBeenCalledTimes(1)
    expect(second.onBound).not.toHaveBeenCalled()
    expect(second.onSendWithout).not.toHaveBeenCalled()
    expect(client.bindSpaceAgent).not.toHaveBeenCalled()
  })
})
