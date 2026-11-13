// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'

// W3-EP-6-P3-c: "What this agent remembers". The panel reads one agent's notes only when opened, renders
// each with its Delete, shows the catalogue's ZDR words for named_agent, never shows a failed read as an
// empty memory, and the switch takes only the literal "true".

const client = vi.hoisted(() => ({
  listSpaceAgentMemory: vi.fn(),
  deleteSpaceAgentMemory: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  listSpaceAgentMemory: client.listSpaceAgentMemory,
  deleteSpaceAgentMemory: client.deleteSpaceAgentMemory,
}))

const { SpaceAgentMemoryPanel, spaceAgentMemoryEnabled } = await import('./SpaceAgentMemoryPanel')

function row(id: string, content: string, withheld = false) {
  return {
    memory_id: id,
    content,
    updated_at: '2026-11-01T10:00:00Z',
    provenance: 'agent',
    source_label: '',
    thread_id: 'thread-1',
    withheld,
  }
}

function openPanel() {
  render(() => <SpaceAgentMemoryPanel spaceRef="space-1" agentSubjectId="agent-subject-1" />)
  fireEvent.click(screen.getByRole('button', { name: /Hva agenten husker|What this agent remembers/ }))
}

beforeEach(() => {
  client.listSpaceAgentMemory.mockReset()
  client.deleteSpaceAgentMemory.mockReset()
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

describe('spaceAgentMemoryEnabled', () => {
  it.each([
    [undefined, false],
    ['', false],
    ['false', false],
    ['1', false],
    ['TRUE', false],
    [' true', false],
    ['true', true],
  ])('reads %j as %s', (value, expected) => {
    vi.stubEnv('VITE_SPACE_AGENT_MEMORY', value as string)
    expect(spaceAgentMemoryEnabled()).toBe(expected)
  })
})

describe('SpaceAgentMemoryPanel', () => {
  it('reads nothing while closed', () => {
    render(() => <SpaceAgentMemoryPanel spaceRef="space-1" agentSubjectId="agent-subject-1" />)
    expect(client.listSpaceAgentMemory).not.toHaveBeenCalled()
  })

  it('lists the notes of the one agent asked for, with the quoted-context note', async () => {
    client.listSpaceAgentMemory.mockResolvedValue({
      memories: [row('m-1', 'Prefers short answers'), row('m-2', 'Works on the Q4 bid', true)],
      withheld: 1,
    })
    openPanel()
    await screen.findByText('Prefers short answers')
    expect(client.listSpaceAgentMemory).toHaveBeenCalledWith('space-1', 'agent-subject-1')
    expect(screen.getByText('Works on the Q4 bid')).toBeTruthy()
    expect(document.body.textContent).toMatch(/sitert kontekst|quoted context/)
    expect(document.body.textContent).toMatch(/Holdt tilbake|Held back/)
  })

  it('shows the empty state only for an empty list', async () => {
    client.listSpaceAgentMemory.mockResolvedValue({ memories: [], withheld: 0 })
    openPanel()
    await screen.findByText(/ikke lagret noe|not kept anything/)
  })

  it('deletes a note by its id after the confirm click, and drops it from the list', async () => {
    client.listSpaceAgentMemory.mockResolvedValue({ memories: [row('m-1', 'Prefers short answers')], withheld: 0 })
    client.deleteSpaceAgentMemory.mockResolvedValue({ memory_id: 'm-1', deleted: true })
    openPanel()
    await screen.findByText('Prefers short answers')

    const remove = screen.getByRole('button', { name: /^(Slett|Delete)$/ })
    fireEvent.click(remove)
    expect(client.deleteSpaceAgentMemory).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole('button', { name: /Bekreft sletting|Confirm deleting/ }))

    await waitFor(() => expect(client.deleteSpaceAgentMemory).toHaveBeenCalledWith('space-1', 'm-1'))
    await waitFor(() => expect(screen.queryByText('Prefers short answers')).toBeNull())
  })

  it('keeps the note and says so when the delete fails', async () => {
    client.listSpaceAgentMemory.mockResolvedValue({ memories: [row('m-1', 'Prefers short answers')], withheld: 0 })
    client.deleteSpaceAgentMemory.mockRejectedValue(new ApiError('boom', 500, 'internal'))
    openPanel()
    await screen.findByText('Prefers short answers')
    fireEvent.click(screen.getByRole('button', { name: /^(Slett|Delete)$/ }))
    fireEvent.click(await screen.findByRole('button', { name: /Bekreft sletting|Confirm deleting/ }))
    await screen.findByText(/fikk ikke slettet|could not delete/)
    expect(screen.getByText('Prefers short answers')).toBeTruthy()
  })

  it('shows the catalogue words for a named_agent ZDR refusal, not an empty memory', async () => {
    client.listSpaceAgentMemory.mockRejectedValue(
      // The BFF answers 412 (domains/spaces/memory.rs named_agent_refusal).
      new ApiError('refused', 412, 'zdr_feature_not_supported', { feature: 'named_agent' }),
    )
    openPanel()
    await screen.findByText(/minnet til navngitte agenter|named agent memory/)
    expect(screen.queryByText(/ikke lagret noe|not kept anything/)).toBeNull()
  })

  it('shows a failed read as an error with a retry, never as an empty memory', async () => {
    client.listSpaceAgentMemory.mockRejectedValueOnce(new ApiError('down', 502, 'upstream_unavailable'))
    openPanel()
    await screen.findByRole('alert')
    expect(screen.queryByText(/ikke lagret noe|not kept anything/)).toBeNull()

    client.listSpaceAgentMemory.mockResolvedValueOnce({ memories: [row('m-1', 'Back again')], withheld: 0 })
    fireEvent.click(screen.getByRole('button', { name: /Prøv igjen|Try again/ }))
    await screen.findByText('Back again')
  })
})
