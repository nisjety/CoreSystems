// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'

// A search belongs to one opening of the model menu. Closing the menu, by its button or by choosing a
// model, clears the query, so the next opening lists the whole catalogue again.

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

import { AiModelPicker } from './AiModelPicker'

const chat = (id: string): ModelInfo => ({ id, name: id, provider: 'openai', modality: 'chat' })
const catalogue = ['gpt-mini', 'gpt-nano', 'gpt-large', 'gpt-vision', 'gpt-audio', 'gpt-turbo', 'gpt-omni'].map(chat)
const loadCatalogue = async () => catalogue

async function searchField(): Promise<HTMLInputElement> {
  return (await screen.findByRole('searchbox', { name: /søk etter modell/i })) as HTMLInputElement
}

function searchValue(): string | undefined {
  return document.querySelector<HTMLInputElement>('input[type="search"]')?.value
}

afterEach(() => {
  cleanup()
  window.localStorage.clear()
})

describe('AiModelPicker search resets with the menu', () => {
  it('lists the whole catalogue again after the menu is closed and reopened', async () => {
    render(() => <AiModelPicker orgId="org-1" catalogue={loadCatalogue} />)
    const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })

    fireEvent.click(trigger)
    fireEvent.input(await searchField(), { target: { value: 'claude' } })
    expect(await screen.findByText('Ingen modeller passer')).toBeTruthy()

    fireEvent.click(trigger)
    await waitFor(() => expect(trigger.getAttribute('aria-expanded')).toBe('false'))
    fireEvent.click(trigger)
    await waitFor(() => expect(searchValue()).toBe(''))
    expect(screen.queryByText('Ingen modeller passer')).toBeNull()
    expect(screen.getByText('gpt-nano')).toBeTruthy()
  })

  it('clears the query when a model is chosen', async () => {
    render(() => <AiModelPicker orgId="org-1" catalogue={loadCatalogue} />)
    const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })

    fireEvent.click(trigger)
    fireEvent.input(await searchField(), { target: { value: 'gpt-nano' } })
    await waitFor(() => expect(screen.queryByText('gpt-mini')).toBeNull())
    fireEvent.click(screen.getByText('gpt-nano'))
    await waitFor(() => expect(trigger.getAttribute('aria-expanded')).toBe('false'))

    fireEvent.click(trigger)
    await waitFor(() => expect(searchValue()).toBe(''))
    expect(screen.getByText('gpt-mini')).toBeTruthy()
  })
})
