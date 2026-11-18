import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import type { PersonalAgentAccess } from '@/shared/api/spaces-client'

// The dialog reads one agent's access and saves the whole list it shows. Both calls are answered here, so no request
// leaves the test. The mount case guards a crash: a one-argument createEffect threw MISSING_EFFECT_FN on mount and halted
// the reactive system, which is why the access is drawn only once the read's effect has seeded the drafts.
const access = vi.hoisted(() => ({ getPersonalAgentAccess: vi.fn(), updatePersonalAgentAccess: vi.fn() }))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...access,
}))

const { SpaceAgentAccessDialog } = await import('./SpaceAgentAccessDialog')

function accessOf(over: Partial<PersonalAgentAccess> = {}): PersonalAgentAccess {
  return {
    agentRef: 'agent-a',
    subjectId: 'sub-kari',
    role: 'owner',
    visibility: 'owner',
    addPolicy: 'owner_only',
    revision: 7,
    ownerSubjectId: 'sub-kari',
    entries: [{ principalId: 'sub-kari', role: 'manager' }],
    ...over,
  }
}

function renderDialog(): void {
  render(() => <SpaceAgentAccessDialog agentRef="agent-a" open={() => true} onClose={() => {}} />)
  flush()
}

// Adds one person the way the person does: type the id, press Enter.
function addPerson(principalId: string): void {
  const input = screen.getByRole('textbox', { name: 'Person-id' }) as HTMLInputElement
  fireEvent.input(input, { target: { value: principalId } })
  flush()
  fireEvent.keyDown(input, { key: 'Enter' })
  flush()
}

beforeEach(() => {
  access.getPersonalAgentAccess.mockReset()
  access.updatePersonalAgentAccess.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceAgentAccessDialog (WS17-S5)', () => {
  it('mounts without throwing, and draws the access it reads once the read has seeded the drafts', async () => {
    access.getPersonalAgentAccess.mockResolvedValue(accessOf())
    expect(() => renderDialog()).not.toThrow()
    expect(await screen.findByText('sub-kari')).toBeTruthy()
    expect(access.getPersonalAgentAccess).toHaveBeenCalledWith('agent-a')
  })

  it('saves the whole list shown, against the revision that was read', async () => {
    access.getPersonalAgentAccess.mockResolvedValue(accessOf())
    access.updatePersonalAgentAccess.mockResolvedValue(accessOf({ revision: 8 }))
    renderDialog()
    await screen.findByText('sub-kari')

    addPerson('sub-ola')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre tilgang' }))
    flush()

    await waitFor(() => expect(access.updatePersonalAgentAccess).toHaveBeenCalledTimes(1))
    const [agentRef, body] = access.updatePersonalAgentAccess.mock.calls[0] as [
      string,
      { entries: readonly { principalId: string }[]; expectedRevision: number },
    ]
    expect(agentRef).toBe('agent-a')
    expect(body.expectedRevision).toBe(7)
    expect(body.entries.map((entry) => entry.principalId)).toEqual(['sub-kari', 'sub-ola'])
  })

  it('a 409 agent_state_changed keeps the unsaved draft on screen and resends nothing', async () => {
    access.getPersonalAgentAccess.mockResolvedValue(accessOf())
    access.updatePersonalAgentAccess.mockRejectedValue(new ApiError('Conflict', 409, 'agent_state_changed'))
    renderDialog()
    await screen.findByText('sub-kari')

    addPerson('sub-ola')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre tilgang' }))
    flush()

    await waitFor(() => expect(access.updatePersonalAgentAccess).toHaveBeenCalledTimes(1))
    flush()
    expect(screen.getByText('sub-ola')).toBeTruthy()
    expect(access.updatePersonalAgentAccess).toHaveBeenCalledTimes(1)
  })
})
