// @vitest-environment jsdom

// W3-EP-5-S7-ui: the composer's deck template chip. Offered only while the page says the draft talks about a deck
// (the page's heuristic) AND the gateway lists at least one template: the list route answers 404 (null) while
// SKILL_DECK_TEMPLATES_ENABLED is off, and null in a zero-retention session. The chip opens the picker; a pick is
// handed to the page for the next send, shown as its own row, and can be removed (null = Standard, which carries none).

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeckTemplate } from '@/shared/api/chat-client'

const { listDeckTemplates } = vi.hoisted(() => ({ listDeckTemplates: vi.fn() }))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, listDeckTemplates }
})

import { ComposerTrays } from './ComposerTrays'

const TEMPLATE: DeckTemplate = {
  skillId: 'skill.deck.aquatiq',
  name: 'Kundemal',
  description: 'Merkevarens mal',
  colors: { primary: '#003366', secondary: '#0099cc', text: '#111111', background: '#ffffff', accent: ['#ff6600'] },
  fonts: { heading: 'Inter', body: 'Inter' },
}

const CHIP = /^(Velg mal|Choose template)$/

function mount(entry: boolean) {
  const [wanted, setWanted] = createSignal(entry)
  const [pick, setPick] = createSignal<DeckTemplate | null>(null)
  const onDeckTemplateChange = vi.fn((template: DeckTemplate | null) => setPick(template))
  const view = render(() => (
    <ComposerTrays
      trays={[]}
      onApprovalDecision={vi.fn()}
      onApprovePlan={vi.fn()}
      onRejectPlan={vi.fn()}
      deckTemplateEntry={wanted()}
      deckTemplate={pick()}
      onDeckTemplateChange={onDeckTemplateChange}
    />
  ))
  flush()
  return { view, setWanted, onDeckTemplateChange, pick }
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  listDeckTemplates.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('the composer deck template chip', () => {
  it('asks nothing and offers nothing while the draft does not talk about a deck', async () => {
    listDeckTemplates.mockResolvedValue([TEMPLATE])
    mount(false)
    await settle()
    expect(listDeckTemplates).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()
  })

  it('stays hidden when the gateway lists no templates (switch off answers null)', async () => {
    listDeckTemplates.mockResolvedValue(null)
    mount(true)
    await waitFor(() => expect(listDeckTemplates).toHaveBeenCalled())
    await settle()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()
  })

  it('stays hidden when the organisation has no template, and when the list fails', async () => {
    listDeckTemplates.mockResolvedValueOnce([])
    const first = mount(true)
    await waitFor(() => expect(listDeckTemplates).toHaveBeenCalledTimes(1))
    await settle()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()
    first.view.unmount()

    listDeckTemplates.mockRejectedValueOnce(new Error('network'))
    mount(true)
    await waitFor(() => expect(listDeckTemplates).toHaveBeenCalledTimes(2))
    await settle()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()
  })

  it('offers the chip with a listed template, opens the picker, and hands the pick to the page', async () => {
    listDeckTemplates.mockResolvedValue([TEMPLATE])
    const { onDeckTemplateChange } = mount(true)
    const chip = await screen.findByRole('button', { name: CHIP })
    fireEvent.click(chip)
    flush()

    const option = await screen.findByRole('radio', { name: /Kundemal/ })
    fireEvent.click(option)
    flush()
    expect(onDeckTemplateChange).toHaveBeenCalledWith(TEMPLATE)

    // The pick shows as its own row, and the chip is gone while a pick is held.
    expect(await screen.findByRole('button', { name: /(Mal|Template): Kundemal/ })).toBeTruthy()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()

    // Removing it hands null back: the next send carries no template.
    fireEvent.click(screen.getByRole('button', { name: /Fjern malen|Remove the template/ }))
    flush()
    expect(onDeckTemplateChange).toHaveBeenLastCalledWith(null)
  })

  it('withdraws the chip when the draft stops talking about a deck, even if the list answers late', async () => {
    let answer: (value: DeckTemplate[]) => void = () => undefined
    listDeckTemplates.mockImplementation(() => new Promise<DeckTemplate[]>((resolve) => { answer = resolve }))
    const { setWanted } = mount(true)
    await waitFor(() => expect(listDeckTemplates).toHaveBeenCalledTimes(1))
    setWanted(false)
    flush()
    answer([TEMPLATE])
    await settle()
    expect(screen.queryByRole('button', { name: CHIP })).toBeNull()
  })
})
