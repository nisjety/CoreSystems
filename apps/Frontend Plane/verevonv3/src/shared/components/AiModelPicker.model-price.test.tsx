// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'

// A chat model the catalogue prices shows its list price under its name. A model it does not price
// shows none.

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

import { AiModelPicker } from './AiModelPicker'

afterEach(() => {
  cleanup()
  window.localStorage.clear()
})

describe('AiModelPicker list prices', () => {
  it('shows the list price under a model that carries one, and none under the rest', async () => {
    const priced: ModelInfo = {
      id: 'claude-sonnet-5-5',
      name: 'Claude Sonnet 5.5',
      provider: 'anthropic',
      modality: 'chat',
      price: { inputPerMillion: 3, outputPerMillion: 15, currency: 'USD' },
    }
    const unpriced: ModelInfo = { id: 'gpt-mini', name: 'GPT mini', provider: 'openai', modality: 'chat' }
    const loadCatalogue = async () => [priced, unpriced]
    render(() => <AiModelPicker orgId="org-1" catalogue={loadCatalogue} />)

    fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
    const lines = await screen.findAllByText(/per million tokens/)
    expect(lines).toHaveLength(1)
    expect(lines[0]?.textContent).toMatch(/3,00\s.*inn \/ .*15,00\s.*ut per million tokens$/)
    expect(lines[0]?.closest('button')?.textContent).toContain('Claude Sonnet 5.5')
  })
})
