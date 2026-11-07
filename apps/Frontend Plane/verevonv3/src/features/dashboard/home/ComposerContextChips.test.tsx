// @vitest-environment jsdom

/**
 * EP-3 S5 context chips: a chip is removed whole, no character opens the picker, and `/kontekst`
 * (registered in the composer's slash registry) does. The picker's reads are mocked; the chips are a
 * controlled prop, as the composer wires them.
 */

import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createRoot, createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatContextRef } from '@/shared/actions/chat-context-refs'

vi.mock('@/shared/api/knowledge-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/knowledge-client')>()
  return {
    ...actual,
    listDocuments: vi.fn().mockResolvedValue([{ id: 'doc-9', title: 'Prisliste', sourceId: 'src-1' }]),
  }
})

vi.mock('@/shared/api/skills-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/skills-client')>()
  return { ...actual, listAvailableSkills: vi.fn().mockResolvedValue([]) }
})

import { ComposerContextChips } from './ComposerContextChips'
import { createSlashCommandRegistry, type SlashCommandRegistry } from './composer-slash-registry'

const budget: ChatContextRef = { kind: 'document', id: 'doc-1', label: 'Budsjett' }
const writing: ChatContextRef = { kind: 'skill', id: 'skill.a', label: 'Skriv' }

let dispose: (() => void) | undefined

function mount(options: { available?: boolean; refs?: ChatContextRef[] } = {}) {
  let registry!: SlashCommandRegistry
  createRoot((rootDispose) => {
    dispose = rootDispose
    registry = createSlashCommandRegistry()
  })
  const changes: ChatContextRef[][] = []
  const [refs, setRefs] = createSignal<ChatContextRef[]>(options.refs ?? [])
  render(() => (
    <ComposerContextChips
      available={options.available ?? true}
      refs={refs()}
      onChange={(next) => {
        changes.push(next)
        setRefs(next)
      }}
      registry={registry}
      orgId="org-1"
    />
  ))
  flush()
  return { registry, changes }
}

afterEach(() => {
  dispose?.()
  dispose = undefined
})

describe('ComposerContextChips', () => {
  it('chip_is_atomic_on_backspace', () => {
    const { changes } = mount({ refs: [budget, writing] })
    const chip = screen.getByRole('listitem', { name: 'Budsjett' })
    chip.focus()
    fireEvent.keyDown(chip, { key: 'Backspace' })
    flush()
    expect(changes.at(-1)).toEqual([writing])
    expect(screen.queryByRole('listitem', { name: 'Budsjett' })).toBeNull()
    // The rest of the row is untouched: no chip is ever edited down to part of its label.
    expect(screen.getByRole('listitem', { name: 'Skriv' })).toBeTruthy()
  })

  it('at_sign_opens_nothing', () => {
    const { registry } = mount({ refs: [budget] })
    const row = screen.getByRole('list', { name: /kontekst|context/i })
    fireEvent.keyDown(row, { key: '@' })
    fireEvent.keyDown(screen.getByRole('listitem', { name: 'Budsjett' }), { key: '@' })
    flush()
    expect(screen.queryByRole('dialog')).toBeNull()
    // The only trigger this row adds is a slash command; `@` stays reserved for Space agents.
    for (const command of registry.commands()) {
      expect(command.name.startsWith('@')).toBe(false)
      expect(command.aliases.some((alias) => alias.startsWith('@'))).toBe(false)
    }
  })

  it('slash_kontekst_opens_picker', async () => {
    const { registry } = mount()
    const command = registry.commands().find((item) => item.name === 'kontekst')
    expect(command).toBeDefined()
    command?.run()
    flush()
    expect(screen.getByRole('dialog')).toBeTruthy()
    await waitFor(() => expect(screen.getByRole('option', { name: 'Prisliste' })).toBeTruthy())
  })

  it('picking adds one atomic chip and closes the picker', async () => {
    const { changes } = mount()
    fireEvent.click(screen.getByRole('button', { name: /kontekst|context/i }))
    flush()
    await waitFor(() => expect(screen.getByRole('option', { name: 'Prisliste' })).toBeTruthy())
    fireEvent.click(screen.getByRole('option', { name: 'Prisliste' }))
    flush()
    expect(changes.at(-1)).toEqual([{ kind: 'document', id: 'doc-9', label: 'Prisliste' }])
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('is absent, and registers no command, where references are not available', () => {
    const { registry } = mount({ available: false, refs: [budget] })
    expect(screen.queryByRole('list', { name: /kontekst|context/i })).toBeNull()
    expect(registry.commands()).toHaveLength(0)
  })
})
