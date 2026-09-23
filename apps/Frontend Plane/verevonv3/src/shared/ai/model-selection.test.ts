import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listChatGptSubscriptions } from '@/shared/api/chatgpt-subscription-client'
import { rememberAiModelSelection, readAiModelSelection, resolveAiModelSelection } from './model-selection'

vi.mock('@/shared/api/chatgpt-subscription-client', () => ({
  OPENAI_CODEX_SUBSCRIPTION_PROVIDER: 'openai-codex-subscription',
  listChatGptSubscriptions: vi.fn(),
}))

const selection = { model: 'gpt-5.6-terra', provider: 'openai-codex-subscription', label: 'ChatGPT Terra', subscriptionConnectionId: 'connected' } as const
describe('explicit subscription selection', () => {
  beforeEach(() => { localStorage.clear(); vi.resetAllMocks() })
  it('fails before inference when the subscription is unavailable and preserves the choice', async () => {
    rememberAiModelSelection('org', selection)
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([])
    await expect(resolveAiModelSelection('org')).rejects.toThrow('ikke tilkoblet')
    expect(readAiModelSelection('org')).toEqual(selection)
  })
  it('keeps Terra on the active connection after reconnecting', async () => {
    rememberAiModelSelection('org', selection)
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([{ id: 'reconnected', providerKey: selection.provider, status: 'active' }])
    expect(await resolveAiModelSelection('org')).toEqual({ ...selection, subscriptionConnectionId: 'reconnected' })
  })
  it('does not treat a failed connection lookup as permission to use a different model', async () => {
    rememberAiModelSelection('org', selection)
    vi.mocked(listChatGptSubscriptions).mockRejectedValue(new Error('unavailable'))
    await expect(resolveAiModelSelection('org')).rejects.toThrow('unavailable')
    expect(readAiModelSelection('org')).toEqual(selection)
  })
})
