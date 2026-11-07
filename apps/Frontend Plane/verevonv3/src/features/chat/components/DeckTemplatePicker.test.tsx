// @vitest-environment jsdom

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import type { DeckTemplate } from '@/shared/api/chat-client'
import { DeckTemplatePicker, deckTemplateSwatches } from './DeckTemplatePicker'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, listDeckTemplates: vi.fn() }
})

afterEach(() => {
  cleanup()
  vi.mocked(chatClient.listDeckTemplates).mockReset()
})

function template(overrides: Partial<DeckTemplate> = {}): DeckTemplate {
  return {
    skillId: 'skill-brand',
    name: 'Aquatiq brand',
    description: 'Blue on white.',
    colors: { primary: '#0a3d62', secondary: '#3c6382', text: '#1e272e', background: '#ffffff', accent: ['#e58e26'] },
    fonts: { heading: 'Carlito', body: 'Liberation Sans' },
    ...overrides,
  }
}

const options = (container: HTMLElement) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>('.verevon-deck-template-picker__option'))

describe('deckTemplateSwatches', () => {
  it('lists the four named colours first, then the accents', () => {
    expect(deckTemplateSwatches(template())).toEqual(['#0a3d62', '#3c6382', '#1e272e', '#ffffff', '#e58e26'])
  })
})

describe('DeckTemplatePicker', () => {
  it('shows Standard first, then each template with its swatches and font names', async () => {
    vi.mocked(chatClient.listDeckTemplates).mockResolvedValue([
      template(),
      template({ skillId: 'skill-plain', name: 'Plain', fonts: { heading: 'Carlito', body: 'Carlito' } }),
    ])
    const { container } = render(() => <DeckTemplatePicker selected={null} onSelect={() => {}} />)
    await waitFor(() => expect(options(container)).toHaveLength(3))
    const [standard, brand, plain] = options(container)
    expect(standard?.textContent).toContain('Standard')
    expect(standard?.getAttribute('aria-checked')).toBe('true')
    expect(brand?.textContent).toContain('Aquatiq brand')
    expect(brand?.textContent).toContain('Carlito / Liberation Sans')
    expect(brand?.getAttribute('aria-checked')).toBe('false')
    expect(brand?.querySelectorAll('.verevon-deck-template-picker__swatch')).toHaveLength(5)
    // One font for both reads as one name.
    expect(plain?.querySelector('.verevon-deck-template-picker__fonts')?.textContent).toBe('Carlito')
  })

  it('marks the selected template and hands the pick back, Standard as null', async () => {
    vi.mocked(chatClient.listDeckTemplates).mockResolvedValue([template()])
    const onSelect = vi.fn()
    const { container } = render(() => <DeckTemplatePicker selected="skill-brand" onSelect={onSelect} />)
    await waitFor(() => expect(options(container)).toHaveLength(2))
    const [standard, brand] = options(container)
    expect(brand?.getAttribute('aria-checked')).toBe('true')
    expect(standard?.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(brand as HTMLButtonElement)
    fireEvent.click(standard as HTMLButtonElement)
    expect(onSelect.mock.calls).toEqual([[template()], [null]])
  })

  it('picks nothing while disabled', async () => {
    vi.mocked(chatClient.listDeckTemplates).mockResolvedValue([template()])
    const onSelect = vi.fn()
    const { container } = render(() => <DeckTemplatePicker selected={null} onSelect={onSelect} disabled />)
    await waitFor(() => expect(options(container)).toHaveLength(2))
    fireEvent.click(options(container)[1] as HTMLButtonElement)
    expect(onSelect).not.toHaveBeenCalled()
  })

  it('hides itself while the route is off or refused, and when the organisation has no template', async () => {
    for (const listed of [null, []]) {
      vi.mocked(chatClient.listDeckTemplates).mockResolvedValue(listed)
      const { container, unmount } = render(() => <DeckTemplatePicker selected={null} onSelect={() => {}} />)
      await waitFor(() => expect(chatClient.listDeckTemplates).toHaveBeenCalled())
      await Promise.resolve()
      expect(container.querySelector('.verevon-deck-template-picker')).toBeNull()
      expect(container.textContent).toBe('')
      unmount()
      vi.mocked(chatClient.listDeckTemplates).mockReset()
    }
  })

  it('says so when the list cannot be read, without offering a choice', async () => {
    vi.mocked(chatClient.listDeckTemplates).mockRejectedValue(new Error('down'))
    const { container } = render(() => <DeckTemplatePicker selected={null} onSelect={() => {}} />)
    await waitFor(() => expect(container.textContent).toContain('Kunne ikke hente malene nå'))
    expect(options(container)).toHaveLength(0)
  })
})
