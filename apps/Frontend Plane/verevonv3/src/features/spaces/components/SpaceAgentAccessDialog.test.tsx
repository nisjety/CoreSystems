import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import type { PersonalAgentAccess } from '@/shared/api/spaces-client'

// The dialog reads one agent's access and saves the whole list it shows. Both calls are answered here, so no request
// leaves the test. The mount case guards a crash: a one-argument createEffect threw MISSING_EFFECT_FN on mount and halted
// the reactive system, which is why the access is drawn only once the read's effect has seeded the drafts.
// The people picker reads the organization's directory; it is answered here too.
const access = vi.hoisted(() => ({
  getPersonalAgentAccess: vi.fn(),
  updatePersonalAgentAccess: vi.fn(),
  searchDirectory: vi.fn(),
}))
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

const person = (subjectId: string, displayName: string, email: string) => ({
  kind: 'person' as const,
  subjectId,
  displayName,
  email,
  isSelf: false,
})

beforeEach(() => {
  access.getPersonalAgentAccess.mockReset()
  access.updatePersonalAgentAccess.mockReset()
  access.searchDirectory.mockReset()
  access.searchDirectory.mockResolvedValue({ results: [], nextCursor: null })
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

  // WS17: a person is found by name in the organization's directory and picked, not typed as an id.
  describe('the people picker', () => {
    it('lists the people the directory finds, but not people already on the list, and asks for people only', async () => {
      access.getPersonalAgentAccess.mockResolvedValue(accessOf())
      access.searchDirectory.mockResolvedValue({
        results: [person('sub-kari', 'Kari Nordmann', 'kari@example.com'), person('sub-ola', 'Ola Hansen', 'ola@example.com')],
        nextCursor: null,
      })
      renderDialog()
      await screen.findByText('sub-kari')

      expect(await screen.findByText('Ola Hansen')).toBeTruthy()
      // sub-kari is already on the list, so she is not offered again.
      expect(screen.queryByText('Kari Nordmann')).toBeNull()
      expect(access.searchDirectory).toHaveBeenCalled()
      expect(access.searchDirectory.mock.calls[0]?.[1]).toMatchObject({ kinds: ['person'] })
    })

    it('adds the picked person by their subject id, named, with the role chosen for a new person, and saves it', async () => {
      access.getPersonalAgentAccess.mockResolvedValue(accessOf())
      access.updatePersonalAgentAccess.mockResolvedValue(accessOf({ revision: 8 }))
      access.searchDirectory.mockResolvedValue({
        results: [person('sub-ola', 'Ola Hansen', 'ola@example.com')],
        nextCursor: null,
      })
      renderDialog()
      await screen.findByText('sub-kari')
      await screen.findByText('Ola Hansen')

      fireEvent.change(screen.getByRole('combobox', { name: 'Rolle for ny person' }), { target: { value: 'manager' } })
      flush()
      fireEvent.click(screen.getByRole('button', { name: 'Gi tilgang' }))
      flush()

      // The person is on the draft list by name, and is no longer offered by the picker.
      expect(await screen.findByRole('combobox', { name: 'Rolle for Ola Hansen' })).toBeTruthy()
      expect(screen.getAllByText('Ola Hansen')).toHaveLength(1)
      expect(access.updatePersonalAgentAccess).not.toHaveBeenCalled()

      fireEvent.click(screen.getByRole('button', { name: 'Lagre tilgang' }))
      flush()
      await waitFor(() => expect(access.updatePersonalAgentAccess).toHaveBeenCalledTimes(1))
      const [, body] = access.updatePersonalAgentAccess.mock.calls[0] as [
        string,
        { entries: readonly { principalId: string; role: string }[]; expectedRevision: number },
      ]
      expect(body.expectedRevision).toBe(7)
      expect(body.entries).toEqual([
        { principalId: 'sub-kari', role: 'manager' },
        { principalId: 'sub-ola', role: 'manager' },
      ])
    })

    // The search box sits inside the form whose default button saves the draft. jsdom has no implicit submission, so this
    // does what a browser does with an Enter that nothing took: it presses the form's default button, if it can be pressed.
    it('does not save the draft when Enter is pressed in the people search', async () => {
      access.getPersonalAgentAccess.mockResolvedValue(accessOf())
      access.updatePersonalAgentAccess.mockResolvedValue(accessOf({ revision: 8 }))
      access.searchDirectory.mockResolvedValue({
        results: [person('sub-ola', 'Ola Hansen', 'ola@example.com')],
        nextCursor: null,
      })
      renderDialog()
      await screen.findByText('sub-kari')
      await screen.findByText('Ola Hansen')

      // A draft worth saving: anyone who can see the agent may now add it to a room. Nothing is stored until Save.
      fireEvent.click(screen.getByRole('radio', { name: 'Alle som kan se den' }))
      flush()
      const save = screen.getByRole('button', { name: 'Lagre tilgang' }) as HTMLButtonElement
      expect(save.disabled).toBe(false)

      const search = screen.getByRole('searchbox') as HTMLInputElement
      fireEvent.input(search, { target: { value: 'ola' } })
      flush()
      const taken = !fireEvent.keyDown(search, { key: 'Enter' })
      if (!taken && !save.disabled) save.click()
      flush()
      await new Promise((resolve) => setTimeout(resolve, 0))
      flush()

      expect(taken).toBe(true)
      expect(access.updatePersonalAgentAccess).not.toHaveBeenCalled()
      expect(screen.queryByText('Tilgangen er lagret.')).toBeNull()

      // Saving is still the person's own act, on the button.
      fireEvent.click(save)
      flush()
      await waitFor(() => expect(access.updatePersonalAgentAccess).toHaveBeenCalledTimes(1))
      expect(
        (access.updatePersonalAgentAccess.mock.calls[0] as [string, { addPolicy: string }])[1].addPolicy,
      ).toBe('anyone')
    })

    it('says the search failed when the directory does, and the id box still adds a person', async () => {
      access.getPersonalAgentAccess.mockResolvedValue(accessOf())
      access.searchDirectory.mockRejectedValue(new ApiError('off', 404, 'feature_disabled'))
      renderDialog()
      await screen.findByText('sub-kari')
      // The failure is said, not shown as "no results"; the person can still add by id.
      expect(await screen.findByRole('alert')).toBeTruthy()

      addPerson('sub-ola')
      expect(screen.getByText('sub-ola')).toBeTruthy()
    })

    it('is not offered to someone who may only see the access', async () => {
      access.getPersonalAgentAccess.mockResolvedValue(accessOf({ role: 'collaborator' }))
      renderDialog()
      await screen.findByText('sub-kari')
      expect(screen.queryByRole('searchbox')).toBeNull()
      expect(access.searchDirectory).not.toHaveBeenCalled()
    })
  })
})
