// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelEntitlementMeta, ModelInfo } from '@/shared/api/chat-client'

// A member whose remembered model the organisation's policy no longer lists is
// moved to the default and told, instead of being left with a send that refuses.

const { mockMeta } = vi.hoisted(() => ({ mockMeta: vi.fn() }))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, getModelEntitlementMeta: mockMeta }
})

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

import { AiModelPicker } from './AiModelPicker'

const STORAGE_KEY = 'verevon.ai-model-selection.v1:org-1'
const chat = (id: string): ModelInfo => ({ id, name: id, provider: 'openai', modality: 'chat' })
const meta = (overrides: Partial<ModelEntitlementMeta> = {}): ModelEntitlementMeta => ({
  state: 'role',
  enforced: true,
  revision: 2,
  restricted: true,
  hiddenCount: 1,
  ...overrides,
})

beforeEach(() => {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ model: 'gpt-big', label: 'GPT Big', provider: 'openai' }))
})

afterEach(() => {
  cleanup()
  window.localStorage.clear()
  vi.clearAllMocks()
})

describe('AiModelPicker under an organisation model policy', () => {
  it('replaces a stale selection and says so', async () => {
    mockMeta.mockReturnValue(meta())
    const onChange = vi.fn()
    render(() => <AiModelPicker orgId="org-1" catalogue={async () => [chat('gpt-mini')]} onChange={onChange} />)

    expect(await screen.findByText(/Organisasjonen din tillater ikke lenger GPT Big/)).toBeTruthy()
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ model: 'verevon-balance' })))
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}').model).toBe('verevon-balance')
  })

  it('uses the policy default when the catalogue lists it', async () => {
    mockMeta.mockReturnValue(meta({ defaultModel: 'gpt-mini' }))
    const onChange = vi.fn()
    render(() => <AiModelPicker orgId="org-1" catalogue={async () => [chat('gpt-mini')]} onChange={onChange} />)

    await waitFor(() => expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ model: 'gpt-mini', provider: 'openai' })))
  })

  it('leaves the selection and shows no notice when no policy applies', async () => {
    mockMeta.mockReturnValue(undefined)
    const onChange = vi.fn()
    render(() => <AiModelPicker orgId="org-1" catalogue={async () => [chat('gpt-mini')]} onChange={onChange} />)

    await screen.findByRole('button', { name: /velg ai-modell/i })
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(onChange).not.toHaveBeenCalled()
    expect(screen.queryByText(/tillater ikke lenger/)).toBeNull()
  })
})
