import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import type { SpaceAgent } from '@/shared/api/spaces-client'

const client = vi.hoisted(() => ({
  getSpaceToolPolicy: vi.fn(),
  setSpaceAgentToolPolicy: vi.fn(),
  setSpaceToolPolicy: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...client,
}))

const { SpaceAgentPolicyEditor } = await import('./SpaceAgentPolicyEditor')

// Only the identity and binding fields this editor reads; the rest of SpaceAgent is not used here.
const agent = {
  subject_id: 'agent-1',
  role: 'editor' as const,
  revision: 3,
  identity_published: true,
  binding_ref: 'binding-1',
  name: 'Ada',
} as SpaceAgent

type Level = 'allow' | 'ask' | 'block'
const RANK: Record<Level, number> = { allow: 1, ask: 2, block: 3 }

// The stored view: the room ceiling sets `*` and this agent's own rule sets one tool. Control merges
// the two layers tighten-only, so the stricter layer wins and names itself as the source.
function storedView(overrides: { ceilingLevel?: Level; agentLevel?: Level } = {}) {
  const ceilingLevel = overrides.ceilingLevel ?? 'ask'
  const agentLevel = overrides.agentLevel ?? 'ask'
  const stricterIsCeiling = RANK[ceilingLevel] > RANK[agentLevel]
  return {
    ceiling: { layer: 'space_ceiling', revision: 2, entries: [{ toolKey: '*', level: ceilingLevel }] },
    agents: [
      {
        layer: 'agent',
        agentSubjectId: 'agent-1',
        revision: 5,
        entries: [{ toolKey: 'mcp__github__create_issue', level: agentLevel }],
      },
    ],
    merged: {
      tools: {
        '*': { level: ceilingLevel, source: 'space_ceiling' as const },
        mcp__github__create_issue: stricterIsCeiling
          ? { level: ceilingLevel, source: 'space_ceiling' as const }
          : { level: agentLevel, source: 'agent' as const },
      },
      ceilingRevision: 2,
      agentRevision: 5,
      digest: 'digest-1',
    },
  }
}

function renderEditor(props: { layer?: 'agent' | 'ceiling'; onSaved?: () => void } = {}) {
  const onSaved = props.onSaved ?? vi.fn()
  const layer = props.layer ?? 'agent'
  render(() => (
    <SpaceAgentPolicyEditor
      spaceRef="space_1"
      layer={layer}
      agent={layer === 'agent' ? agent : undefined}
      onSaved={onSaved}
    />
  ))
  return { onSaved }
}

const openButton = () => screen.getByRole('button', { name: /Verktøyregler for (denne agenten|hele rommet)/ })
const toolInputs = () => screen.queryAllByLabelText('Verktøynavn') as HTMLInputElement[]
const levelSelect = () => screen.getByLabelText('Nivå') as HTMLSelectElement
const saveButton = () => screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement
const open = async () => {
  openButton().click()
  await screen.findAllByLabelText('Verktøynavn')
  flush()
}

beforeEach(() => {
  client.getSpaceToolPolicy.mockReset()
  client.getSpaceToolPolicy.mockResolvedValue(storedView())
  client.setSpaceAgentToolPolicy.mockReset()
  client.setSpaceToolPolicy.mockReset()
})

afterEach(() => cleanup())

describe('SpaceAgentPolicyEditor (WS10-S5b)', () => {
  it('stays closed until it is opened, and reads nothing before then', () => {
    renderEditor()
    const toggle = screen.getByRole('button', { name: 'Verktøyregler for denne agenten' })
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(client.getSpaceToolPolicy).not.toHaveBeenCalled()
    expect(screen.queryAllByLabelText('Verktøynavn')).toHaveLength(0)
  })

  it('on open, reads the room view for this agent: the merged rules and this agent’s own layer', async () => {
    renderEditor()
    await open()
    expect(client.getSpaceToolPolicy).toHaveBeenCalledWith('space_1', 'agent-1')
    expect(screen.getByRole('button', { name: 'Skjul verktøyregler' }).getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByText('Slik gjelder det nå')).toBeTruthy()
    // The merged table lists `*` first, with the layer that decided each row.
    expect(screen.getByText('* (alle verktøy)')).toBeTruthy()
    expect(screen.getByText('Fra rommets tak')).toBeTruthy()
    expect(screen.getByText('Fra agentens egen regel')).toBeTruthy()
    // This agent's rows are the editable ones.
    expect(toolInputs()).toHaveLength(1)
    expect(toolInputs()[0]?.value).toBe('mcp__github__create_issue')
    expect(levelSelect().value).toBe('ask')
  })

  it('a row keeps its field, and the focus in it, while the name is typed', async () => {
    renderEditor()
    await open()
    const field = toolInputs()[0] as HTMLInputElement
    field.focus()
    fireEvent.input(field, { target: { value: 'mcp__github__close_issue' } })
    flush()
    expect(toolInputs()[0]).toBe(field)
    expect(field.isConnected).toBe(true)
    expect(document.activeElement).toBe(field)
    expect(field.value).toBe('mcp__github__close_issue')
  })

  it('Save and Discard are off until the rules differ from what is stored, and Discard puts the stored rules back', async () => {
    renderEditor()
    await open()
    expect(saveButton().disabled).toBe(true)
    const discard = () => screen.getByRole('button', { name: 'Angre endringer' }) as HTMLButtonElement
    expect(discard().disabled).toBe(true)

    fireEvent.input(toolInputs()[0] as HTMLInputElement, { target: { value: 'mcp__github__close_issue' } })
    flush()
    expect(saveButton().disabled).toBe(false)
    expect(discard().disabled).toBe(false)

    fireEvent.click(discard())
    flush()
    expect(toolInputs()[0]?.value).toBe('mcp__github__create_issue')
    expect(saveButton().disabled).toBe(true)
  })

  it('a duplicate, an empty or a malformed tool name is named, and blocks Save', async () => {
    renderEditor()
    await open()

    // A second row starts as `*`, so a duplicate needs the same name typed into it.
    fireEvent.click(screen.getByRole('button', { name: 'Legg til regel' }))
    flush()
    expect(toolInputs()[1]?.value).toBe('*')
    fireEvent.input(toolInputs()[1] as HTMLInputElement, { target: { value: 'mcp__github__create_issue' } })
    flush()
    expect((await screen.findByRole('alert')).textContent).toBe('Samme verktøy er nevnt to ganger.')
    expect(saveButton().disabled).toBe(true)

    fireEvent.input(toolInputs()[1] as HTMLInputElement, { target: { value: '' } })
    flush()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Hver regel trenger et verktøynavn, eller * for alle.',
    )
    expect(saveButton().disabled).toBe(true)

    fireEvent.input(toolInputs()[1] as HTMLInputElement, { target: { value: 'mcp github' } })
    flush()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Verktøynavn kan ikke ha mellomrom og kan være opptil 200 tegn.',
    )
    expect(saveButton().disabled).toBe(true)
    expect(client.setSpaceAgentToolPolicy).not.toHaveBeenCalled()
  })

  it('Save sends the rules with the layer revision it read, and then says Saved', async () => {
    client.setSpaceAgentToolPolicy.mockResolvedValue({
      policy: { layer: 'agent', agentSubjectId: 'agent-1', revision: 6, entries: [{ toolKey: 'mcp__github__create_issue', level: 'block' }] },
      merged: storedView({ agentLevel: 'block' }).merged,
      projection: 'synced',
    })
    const { onSaved } = renderEditor()
    await open()
    fireEvent.change(levelSelect(), { target: { value: 'block' } })
    flush()
    fireEvent.click(saveButton())

    await waitFor(() =>
      expect(client.setSpaceAgentToolPolicy).toHaveBeenCalledWith(
        'space_1',
        'binding-1',
        [{ toolKey: 'mcp__github__create_issue', level: 'block' }],
        5,
      ),
    )
    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(onSaved).toHaveBeenCalledTimes(1)
  })

  it('says the room view will catch up when Control saved but the projection is still pending', async () => {
    client.setSpaceAgentToolPolicy.mockResolvedValue({
      policy: { layer: 'agent', agentSubjectId: 'agent-1', revision: 6, entries: [{ toolKey: 'mcp__github__create_issue', level: 'block' }] },
      merged: storedView({ agentLevel: 'block' }).merged,
      projection: 'pending',
    })
    renderEditor()
    await open()
    fireEvent.change(levelSelect(), { target: { value: 'block' } })
    flush()
    fireEvent.click(saveButton())
    expect(await screen.findByText('Lagret. Visningen i rommet oppdateres om litt.')).toBeTruthy()
  })

  it('a stale write says the rules changed under the person, reloads what is stored, and keeps saying so', async () => {
    client.setSpaceAgentToolPolicy.mockRejectedValue(new ApiError('x', 409, 'tool_policy_revision_conflict'))
    renderEditor()
    await open()
    fireEvent.change(levelSelect(), { target: { value: 'block' } })
    flush()
    fireEvent.click(saveButton())

    // Reading the layer again is what the conflict asks for, and the copy must survive that reload.
    await waitFor(() => expect(client.getSpaceToolPolicy).toHaveBeenCalledTimes(2))
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(
      'Reglene ble endret av noen andre. De nyeste er lastet inn; se over og lagre på nytt.',
    )
    // The reload replaced the unsaved edit with what is stored: nothing was overwritten blind.
    expect(levelSelect().value).toBe('ask')
  })

  it('a row the room ceiling holds down says so, and stops saying so once the row is as strict', async () => {
    client.getSpaceToolPolicy.mockResolvedValue(storedView({ ceilingLevel: 'block', agentLevel: 'ask' }))
    renderEditor()
    await open()
    const note = 'Rommets tak er strengere og gjelder i stedet for dette nivået.'
    expect(screen.getByText(note)).toBeTruthy()

    fireEvent.change(levelSelect(), { target: { value: 'block' } })
    flush()
    await waitFor(() => expect(screen.queryByText(note)).toBeNull())
  })

  it('a failed read says nothing was changed and shows no rules to edit', async () => {
    client.getSpaceToolPolicy.mockRejectedValue(new Error('down'))
    renderEditor()
    openButton().click()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Verktøyreglene kunne ikke hentes. Ingenting er endret.',
    )
    expect(screen.queryAllByLabelText('Verktøynavn')).toHaveLength(0)
  })

  it('the room ceiling edits the whole room: it reads the room with no agent, and saves its own layer', async () => {
    client.setSpaceToolPolicy.mockResolvedValue({
      policy: { layer: 'space_ceiling', revision: 3, entries: [{ toolKey: '*', level: 'block' }] },
      merged: storedView({ ceilingLevel: 'block' }).merged,
      projection: 'synced',
    })
    renderEditor({ layer: 'ceiling' })
    openButton().click()
    await screen.findAllByLabelText('Verktøynavn')
    expect(client.getSpaceToolPolicy).toHaveBeenCalledWith('space_1', undefined)
    expect(screen.getByRole('button', { name: 'Skjul verktøyregler' })).toBeTruthy()

    fireEvent.change(levelSelect(), { target: { value: 'block' } })
    flush()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(client.setSpaceToolPolicy).toHaveBeenCalledWith('space_1', [{ toolKey: '*', level: 'block' }], 2),
    )
    expect(client.setSpaceAgentToolPolicy).not.toHaveBeenCalled()
  })
})
