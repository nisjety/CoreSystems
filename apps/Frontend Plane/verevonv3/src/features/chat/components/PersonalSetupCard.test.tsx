// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { getPersonalSetupState, getMemoryControls, rememberMemory, executeAction, session } = vi.hoisted(() => ({
  getPersonalSetupState: vi.fn(),
  getMemoryControls: vi.fn(),
  rememberMemory: vi.fn(),
  executeAction: vi.fn(),
  session: { user: { id: 'user-a', name: 'Ima' }, activeOrg: { id: 'org-a', name: 'AQUATIQ AS' } },
}))

vi.mock('@/shared/api/personal-setup-client', () => ({ getPersonalSetupState }))
vi.mock('@/shared/api/memory-client', () => ({ getMemoryControls, rememberMemory }))
vi.mock('@/shared/actions/action-client', () => ({ executeAction }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => session }))

import { PersonalSetupCard } from './PersonalSetupCard'

const actor = { type: 'human', userId: 'user-a', orgId: 'org-a' }
const memoryOn = {
  state: 'on',
  reason: 'on',
  useAllowed: true,
  generationAllowed: true,
  orgMemoryPolicy: '',
  hiddenRows: 0,
  updatedAt: null,
}
const memoryPaused = { ...memoryOn, state: 'paused', reason: 'user_paused', generationAllowed: false }

afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})

function offerSetup(step = 'pending') {
  getPersonalSetupState.mockResolvedValue({ available: true, step })
}

// Lets the card's first reads settle, so a card that should stay hidden has had its chance to appear.
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('PersonalSetupCard', () => {
  it('stays hidden when user-core does not offer the card', async () => {
    getPersonalSetupState.mockResolvedValue({ available: false, step: 'pending' })
    render(() => <PersonalSetupCard threadId={null} />)
    await settle()
    expect(screen.queryByTestId('personal-setup-card')).toBeNull()
  })

  it('stays hidden once the step is completed or skipped', async () => {
    offerSetup('completed')
    render(() => <PersonalSetupCard threadId={null} />)
    await settle()
    expect(screen.queryByTestId('personal-setup-card')).toBeNull()
  })

  it('stays hidden when the read fails, which is the safe reading', async () => {
    getPersonalSetupState.mockRejectedValue(new Error('network'))
    render(() => <PersonalSetupCard threadId={null} />)
    await settle()
    expect(screen.queryByTestId('personal-setup-card')).toBeNull()
  })

  it('saves the four answers through the complete action, then hides', async () => {
    offerSetup()
    executeAction.mockResolvedValue({})
    render(() => <PersonalSetupCard threadId={null} />)
    await screen.findByTestId('personal-setup-card')
    fireEvent.click(screen.getByRole('button', { name: /lagre|save/i }))
    await waitFor(() => expect(executeAction).toHaveBeenCalledTimes(1))

    const [actionId, sentActor, input] = executeAction.mock.calls[0]!
    expect(actionId).toBe('settings.complete_personal_setup')
    expect(sentActor).toEqual(actor)
    expect(input).toEqual({
      displayName: 'Ima',
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      language: expect.stringMatching(/^(nb-NO|en-US)$/),
      tonePreset: 'default',
    })
    await waitFor(() => expect(screen.queryByTestId('personal-setup-card')).toBeNull())
  })

  it('refuses a display name over 80 characters before any write', async () => {
    offerSetup()
    render(() => <PersonalSetupCard threadId={null} />)
    await screen.findByTestId('personal-setup-card')
    fireEvent.input(screen.getByRole('textbox', { name: /visningsnavn|display name/i }), {
      target: { value: 'x'.repeat(81) },
    })
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /lagre|save/i }))
    await screen.findByRole('alert')
    expect(executeAction).not.toHaveBeenCalled()
  })

  it('records a skip, hides the card, and keeps it hidden on the next read', async () => {
    offerSetup()
    executeAction.mockResolvedValue({})
    const first = render(() => <PersonalSetupCard threadId={null} />)
    await screen.findByTestId('personal-setup-card')
    fireEvent.click(screen.getByRole('button', { name: /hopp over|skip/i }))
    await waitFor(() => expect(executeAction).toHaveBeenCalledWith('settings.skip_personal_setup', actor, {}))
    await waitFor(() => expect(screen.queryByTestId('personal-setup-card')).toBeNull())
    first.unmount()

    // The next read returns the recorded skip, so the card never shows again.
    offerSetup('skipped')
    render(() => <PersonalSetupCard threadId={null} />)
    await settle()
    expect(screen.queryByTestId('personal-setup-card')).toBeNull()
  })

  it('hides the role step while memory is paused, even on an open thread', async () => {
    offerSetup()
    getMemoryControls.mockResolvedValue(memoryPaused)
    render(() => <PersonalSetupCard threadId="thread-1" />)
    await screen.findByTestId('personal-setup-card')
    await waitFor(() => expect(getMemoryControls).toHaveBeenCalled())
    await settle()
    expect(screen.queryByTestId('personal-setup-role')).toBeNull()
  })

  it('offers the role step on an empty chat, since the role is held until the first send', async () => {
    offerSetup()
    getMemoryControls.mockResolvedValue(memoryOn)
    render(() => <PersonalSetupCard threadId={null} />)
    await screen.findByTestId('personal-setup-role')
    expect(getMemoryControls).toHaveBeenCalled()
  })

  it('saves a ticked role on the open thread, after the answers', async () => {
    offerSetup()
    getMemoryControls.mockResolvedValue(memoryOn)
    executeAction.mockResolvedValue({})
    rememberMemory.mockResolvedValue({ memoryId: 'memory-1' })
    render(() => <PersonalSetupCard threadId="thread-1" />)
    await screen.findByTestId('personal-setup-role')
    fireEvent.click(screen.getByRole('checkbox'))
    await settle()
    fireEvent.input(screen.getByRole('textbox', { name: /rolle|role/i }), {
      target: { value: 'Anbudsansvarlig' },
    })
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /lagre|save/i }))
    await waitFor(() => expect(rememberMemory).toHaveBeenCalledTimes(1))

    expect(rememberMemory.mock.calls[0]).toEqual(['thread-1', expect.stringContaining('Anbudsansvarlig')])
    expect(executeAction.mock.invocationCallOrder[0]!).toBeLessThan(rememberMemory.mock.invocationCallOrder[0]!)
    await waitFor(() => expect(screen.queryByTestId('personal-setup-card')).toBeNull())
  })

  it('keeps the answers saved and says so when the role cannot be saved', async () => {
    offerSetup()
    getMemoryControls.mockResolvedValue(memoryOn)
    executeAction.mockResolvedValue({})
    rememberMemory.mockRejectedValue(new Error('memory_not_enabled'))
    render(() => <PersonalSetupCard threadId="thread-1" />)
    await screen.findByTestId('personal-setup-role')
    fireEvent.click(screen.getByRole('checkbox'))
    await settle()
    fireEvent.input(screen.getByRole('textbox', { name: /rolle|role/i }), { target: { value: 'Prosjektleder' } })
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /lagre|save/i }))

    await screen.findByRole('status')
    expect(executeAction).toHaveBeenCalledWith('settings.complete_personal_setup', actor, expect.anything())
    expect(screen.getByRole('button', { name: /lukk|close/i })).toBeTruthy()
  })
})
