import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listChatGptSubscriptions } from '@/shared/api/chatgpt-subscription-client'
import { LocalizedError, translateApiError } from '@/shared/i18n/errors'
import {
  lastKnownAiModelPrivacyTier,
  readAiModelSelection,
  rememberAiModelPrivacyTier,
  rememberAiModelSelection,
  resolveAiModelSelection,
  resolveReplaySubscriptionConnectionId,
} from './model-selection'

vi.mock('@/shared/api/chatgpt-subscription-client', () => ({
  OPENAI_CODEX_SUBSCRIPTION_PROVIDER: 'openai-codex-subscription',
  listChatGptSubscriptions: vi.fn(),
}))

const selection = { model: 'gpt-5.6-terra', provider: 'openai-codex-subscription', label: 'ChatGPT Terra', subscriptionConnectionId: 'connected' } as const
const NOT_CONNECTED_NO = 'ChatGPT-abonnementet er ikke tilkoblet. Koble til under Profil → ChatGPT-abonnement, eller velg en annen modell.'
const NOT_CONNECTED_EN = 'The ChatGPT subscription is not connected. Connect it under Profile → ChatGPT subscription, or choose another model.'
describe('explicit subscription selection', () => {
  beforeEach(() => { localStorage.clear(); vi.resetAllMocks() })
  it('fails before inference when the subscription is unavailable and preserves the choice', async () => {
    rememberAiModelSelection('org', selection)
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([])
    const failure = await resolveAiModelSelection('org').catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(LocalizedError)
    expect((failure as LocalizedError).copy).toEqual({ no: NOT_CONNECTED_NO, en: NOT_CONNECTED_EN })
    expect(readAiModelSelection('org')).toEqual(selection)
  })
  it('shows its own sentence in each language, not the caller fallback', async () => {
    rememberAiModelSelection('org', selection)
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([])
    const failure = await resolveAiModelSelection('org').catch((error: unknown) => error)
    const fallback = { no: 'Kunne ikke sende meldingen.', en: 'Could not send the message.' }
    expect(translateApiError(failure, (no) => no, fallback)).toBe(NOT_CONNECTED_NO)
    expect(translateApiError(failure, (_no, en) => en, fallback)).toBe(NOT_CONNECTED_EN)
  })
  it('leaves a selection that is not the subscription untouched, with no connection lookup', async () => {
    const catalogue = { model: 'verevon-balance', label: 'Verevon Balance' } as const
    rememberAiModelSelection('org', catalogue)
    expect(await resolveAiModelSelection('org')).toEqual(catalogue)
    expect(listChatGptSubscriptions).not.toHaveBeenCalled()
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

describe('the connection a subscription turn is sent on again', () => {
  const active = (id: string) => ({ id, providerKey: selection.provider, status: 'active' })
  beforeEach(() => { vi.resetAllMocks() })

  it('keeps the stored connection while it is active', async () => {
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([active('other'), active('stored')])
    expect(await resolveReplaySubscriptionConnectionId('org', 'stored')).toBe('stored')
  })
  it('moves to the active connection after a reconnect left the stored one needing renewal', async () => {
    vi.mocked(listChatGptSubscriptions).mockResolvedValue([
      { id: 'stored', providerKey: selection.provider, status: 'needs_refresh' },
      active('reconnected'),
    ])
    expect(await resolveReplaySubscriptionConnectionId('org', 'stored')).toBe('reconnected')
  })
  it('keeps the stored connection when none is active or the lookup fails, so the broker says why', async () => {
    vi.mocked(listChatGptSubscriptions).mockResolvedValueOnce([{ id: 'stored', providerKey: selection.provider, status: 'needs_refresh' }])
    expect(await resolveReplaySubscriptionConnectionId('org', 'stored')).toBe('stored')
    vi.mocked(listChatGptSubscriptions).mockRejectedValueOnce(new Error('unavailable'))
    expect(await resolveReplaySubscriptionConnectionId('org', 'stored')).toBe('stored')
  })
  it('looks nothing up without an organization', async () => {
    expect(await resolveReplaySubscriptionConnectionId(' ', 'stored')).toBe('stored')
    expect(listChatGptSubscriptions).not.toHaveBeenCalled()
  })
})

describe('last-known privacy tier', () => {
  const euSelection = { model: 'm-eu', provider: 'eu-host', label: 'EU Model' } as const
  beforeEach(() => { localStorage.clear() })

  it('keeps a valid tier and drops a garbled one', () => {
    rememberAiModelSelection('org', { ...euSelection, privacyTier: 'eu_resident' })
    expect(readAiModelSelection('org')).toEqual({ ...euSelection, privacyTier: 'eu_resident' })
    localStorage.setItem('verevon.ai-model-selection.v1:org', JSON.stringify({ ...euSelection, privacyTier: 'top-secret' }))
    expect(readAiModelSelection('org')).toEqual(euSelection)
  })

  it('is returned only for the same model and provider', () => {
    rememberAiModelSelection('org', { ...euSelection, privacyTier: 'sovereign' })
    expect(lastKnownAiModelPrivacyTier('org', { model: 'm-eu', provider: 'eu-host' })).toBe('sovereign')
    expect(lastKnownAiModelPrivacyTier('org', { model: 'm-eu', provider: 'other-host' })).toBeUndefined()
    expect(lastKnownAiModelPrivacyTier('org', { model: 'm-other', provider: 'eu-host' })).toBeUndefined()
    expect(lastKnownAiModelPrivacyTier('other-org', { model: 'm-eu', provider: 'eu-host' })).toBeUndefined()
  })

  it('is updated only on the selection it belongs to', () => {
    rememberAiModelSelection('org', euSelection)
    rememberAiModelPrivacyTier('org', { model: 'm-eu', provider: 'eu-host' }, 'zdr_contractual')
    expect(readAiModelSelection('org')).toEqual({ ...euSelection, privacyTier: 'zdr_contractual' })
    // Another tab stored a different model since: leave it alone.
    rememberAiModelPrivacyTier('org', { model: 'm-other', provider: 'eu-host' }, 'global')
    expect(readAiModelSelection('org')).toEqual({ ...euSelection, privacyTier: 'zdr_contractual' })
  })
})
