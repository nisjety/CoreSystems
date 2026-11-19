import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import type { SpaceAgent, SpaceRosterMember } from '@/shared/api/spaces-client'

const client = vi.hoisted(() => ({ setSpaceAgentWakePolicy: vi.fn() }))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...client,
}))

const { SpaceAgentWakePolicyEditor } = await import('./SpaceAgentWakePolicyEditor')

const roster: SpaceRosterMember[] = [
  { subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'user_2', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
  { subject_type: 'user', subject_id: 'user_3', role: 'viewer', revision: 1, display_name: 'Nina Berg' },
  { subject_type: 'service', subject_id: 'agent-2', role: 'editor', revision: 1, display_name: 'Driftsassistent' },
]

function agentWith(overrides: Partial<SpaceAgent> = {}): SpaceAgent {
  return {
    subject_id: 'agent-1',
    role: 'editor',
    revision: 1,
    identity_published: true,
    binding_ref: 'binding-1',
    name: 'Ada',
    respond_to: 'members',
    respond_to_subjects: [],
    updated_at: 1700000000,
    ...overrides,
  } as SpaceAgent
}

function renderEditor(agent: SpaceAgent = agentWith()) {
  const onSaved = vi.fn()
  render(() => (
    <SpaceAgentWakePolicyEditor spaceRef="space_1" agent={agent} roster={() => roster} onSaved={onSaved} />
  ))
  screen.getByRole('button', { name: 'Hvem kan tilkalle den' }).click()
  flush()
  return { onSaved }
}

const radio = (name: RegExp) => screen.getByRole('radio', { name }) as HTMLInputElement
const saveButton = () => screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement
const discardButton = () => screen.getByRole('button', { name: 'Angre endringer' }) as HTMLButtonElement
const choose = (name: RegExp) => {
  radio(name).click()
  flush()
}

beforeEach(() => {
  client.setSpaceAgentWakePolicy.mockReset()
  client.setSpaceAgentWakePolicy.mockResolvedValue({
    bindingRef: 'binding-1',
    respondTo: 'managers',
    respondToSubjects: [],
    projectionVersion: 2,
    updatedAt: 1700000100,
    changed: true,
  })
})

afterEach(() => cleanup())

describe('SpaceAgentWakePolicyEditor (WS08-S8)', () => {
  it('stays closed until it is opened, then offers the three modes with the stored one chosen', () => {
    render(() => <SpaceAgentWakePolicyEditor spaceRef="space_1" agent={agentWith()} roster={() => roster} />)
    expect(screen.queryByRole('radio')).toBeNull()
    screen.getByRole('button', { name: 'Hvem kan tilkalle den' }).click()
    flush()
    expect(radio(/Alle med skrivetilgang/).checked).toBe(true)
    expect(radio(/Bare eiere og ledere/).checked).toBe(false)
    expect(radio(/Bare utvalgte personer/).checked).toBe(false)
    // The people list belongs to the chosen-people mode only.
    expect(screen.queryByText('Personer')).toBeNull()
  })

  it('choosing a mode changes the draft, and the people list appears only for the chosen-people mode', () => {
    renderEditor()
    choose(/Bare utvalgte personer/)
    expect(radio(/Bare utvalgte personer/).checked).toBe(true)
    expect(screen.getByText('Personer')).toBeTruthy()
    choose(/Bare eiere og ledere/)
    expect(screen.queryByText('Personer')).toBeNull()
    expect(radio(/Bare eiere og ledere/).checked).toBe(true)
  })

  it('the people list is drawn from the roster, and leaves out viewers and agents', () => {
    renderEditor()
    choose(/Bare utvalgte personer/)
    expect(screen.getByRole('checkbox', { name: 'Kari Nordmann' })).toBeTruthy()
    expect(screen.getByRole('checkbox', { name: 'Ola Hansen' })).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: 'Nina Berg' })).toBeNull()
    expect(screen.queryByRole('checkbox', { name: 'Driftsassistent' })).toBeNull()
  })

  it('a chosen-people mode with nobody picked is a problem, and Save waits for a choice', () => {
    renderEditor()
    choose(/Bare utvalgte personer/)
    expect(screen.getByRole('alert').textContent).toBe('Velg minst én person, eller velg en annen modus.')
    expect(saveButton().disabled).toBe(true)
  })

  it('Save and Discard are off until the draft differs from what is stored', () => {
    renderEditor()
    expect(saveButton().disabled).toBe(true)
    expect(discardButton().disabled).toBe(true)
    choose(/Bare eiere og ledere/)
    expect(saveButton().disabled).toBe(false)
    expect(discardButton().disabled).toBe(false)
    choose(/Alle med skrivetilgang/)
    expect(saveButton().disabled).toBe(true)
  })

  it('Discard puts back the stored mode', () => {
    renderEditor()
    choose(/Bare eiere og ledere/)
    fireEvent.click(discardButton())
    flush()
    expect(radio(/Alle med skrivetilgang/).checked).toBe(true)
    expect(saveButton().disabled).toBe(true)
  })

  it('Save sends the chosen people with the binding’s updated_at, and says Saved', async () => {
    const { onSaved } = renderEditor()
    choose(/Bare utvalgte personer/)
    fireEvent.click(screen.getByRole('checkbox', { name: 'Kari Nordmann' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Ola Hansen' }))
    flush()
    fireEvent.click(saveButton())

    await waitFor(() =>
      expect(client.setSpaceAgentWakePolicy).toHaveBeenCalledWith('space_1', 'binding-1', {
        respondTo: 'allowlist',
        respondToSubjects: ['user_1', 'user_2'],
        expectedUpdatedAt: 1700000000,
      }),
    )
    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(onSaved).toHaveBeenCalledTimes(1)
  })

  it('a mode other than the chosen-people list sends no people at all', async () => {
    renderEditor()
    choose(/Bare eiere og ledere/)
    fireEvent.click(saveButton())
    await waitFor(() => expect(client.setSpaceAgentWakePolicy).toHaveBeenCalledTimes(1))
    const body = client.setSpaceAgentWakePolicy.mock.calls[0]?.[2] as Record<string, unknown>
    expect(body).toEqual({ respondTo: 'managers', expectedUpdatedAt: 1700000000 })
    expect(body.respondToSubjects).toBeUndefined()
  })

  it('a former member on the stored list is kept, shown as one, and can be taken off', async () => {
    renderEditor(agentWith({ respond_to: 'allowlist', respond_to_subjects: ['user_2', 'user_9'] }))
    expect((screen.getByRole('checkbox', { name: 'Ola Hansen' }) as HTMLInputElement).checked).toBe(true)
    expect((screen.getByRole('checkbox', { name: 'Kari Nordmann' }) as HTMLInputElement).checked).toBe(false)
    const former = screen.getByRole('checkbox', { name: 'Tidligere medlem' }) as HTMLInputElement
    expect(former.checked).toBe(true)

    fireEvent.click(former)
    flush()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(client.setSpaceAgentWakePolicy).toHaveBeenCalledWith('space_1', 'binding-1', {
        respondTo: 'allowlist',
        respondToSubjects: ['user_2'],
        expectedUpdatedAt: 1700000000,
      }),
    )
  })

  it('without an updated_at from the binding it cannot save, rather than sending a blind write', () => {
    renderEditor(agentWith({ updated_at: undefined }))
    choose(/Bare eiere og ledere/)
    expect(saveButton().disabled).toBe(true)
    expect(client.setSpaceAgentWakePolicy).not.toHaveBeenCalled()
  })

  it('a conflict says someone else changed it, and asks the parent to read the binding again', async () => {
    client.setSpaceAgentWakePolicy.mockRejectedValue(new ApiError('x', 409, 'policy_conflict'))
    const { onSaved } = renderEditor()
    choose(/Bare eiere og ledere/)
    fireEvent.click(saveButton())
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Noen andre har endret hvem som kan tilkalle agenten. Den nyeste innstillingen er lastet inn; se over og lagre på nytt.',
    )
    expect(onSaved).toHaveBeenCalledTimes(1)
  })

  it('a fresh read of the binding replaces the draft, so a save never sends an old choice', () => {
    const [agent, setAgent] = createSignal(agentWith())
    render(() => <SpaceAgentWakePolicyEditor spaceRef="space_1" agent={agent()} roster={() => roster} />)
    screen.getByRole('button', { name: 'Hvem kan tilkalle den' }).click()
    flush()
    choose(/Bare eiere og ledere/)
    setAgent(agentWith({ respond_to: 'managers', updated_at: 1700000100 }))
    flush()
    expect(radio(/Bare eiere og ledere/).checked).toBe(true)
    expect(saveButton().disabled).toBe(true)
  })
})
