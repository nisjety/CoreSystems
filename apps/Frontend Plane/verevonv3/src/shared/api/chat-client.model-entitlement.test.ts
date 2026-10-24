import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  effortLevelWithin,
  enforcedMaxEffort,
  getModelEntitlementMeta,
  listModelCatalogue,
  listModels,
  parseModelEntitlementMeta,
  selectionIsStale,
  type ModelEntitlementMeta,
  type ModelInfo,
} from './chat-client'

// The organisation's model policy rides beside the catalogue (model-gateway
// `entitlement` on /v1/models): counts and a ceiling, never a model list. The
// client reads it whole or not at all, and only an enforced policy changes what
// the pickers show.

const DECLARED = {
  state: 'role',
  enforced: true,
  revision: 7,
  restricted: true,
  hidden_count: 3,
  default_model: 'gpt-mini',
  max_effort: 'medium',
}

function catalogue(body: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

const chat = (id: string, provider = 'openai'): ModelInfo => ({ id, name: id, provider, modality: 'chat' })

const meta = (overrides: Partial<ModelEntitlementMeta> = {}): ModelEntitlementMeta => ({
  state: 'role',
  enforced: true,
  revision: 1,
  restricted: true,
  hiddenCount: 1,
  ...overrides,
})

describe('the declared model entitlement', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('comes back beside the models from listModelCatalogue', async () => {
    catalogue({ models: [{ id: 'gpt-mini', modality: 'chat' }], entitlement: DECLARED })
    const result = await listModelCatalogue()
    expect(result.models.map((model) => model.id)).toEqual(['gpt-mini'])
    expect(result.entitlement).toEqual({
      state: 'role',
      enforced: true,
      revision: 7,
      restricted: true,
      hiddenCount: 3,
      defaultModel: 'gpt-mini',
      maxEffort: 'medium',
    })
  })

  it('keeps listModels returning the models alone, and leaves the meta for callers that read it', async () => {
    catalogue({ models: [{ id: 'gpt-mini' }], entitlement: DECLARED })
    expect(await listModels()).toHaveLength(1)
    expect(getModelEntitlementMeta()?.revision).toBe(7)
  })

  it('is absent when no policy applies, so the catalogue is shown as served', async () => {
    catalogue({ models: [{ id: 'm1' }], entitlement: DECLARED })
    await listModels()
    expect(getModelEntitlementMeta()).toBeDefined()
    catalogue({ models: [{ id: 'm1' }] })
    const result = await listModelCatalogue()
    expect(result.entitlement).toBeUndefined()
    expect(getModelEntitlementMeta()).toBeUndefined()
    catalogue([{ id: 'legacy-array-shape' }])
    await listModels()
    expect(getModelEntitlementMeta()).toBeUndefined()
  })

  it('is never half-trusted: one malformed field drops the whole meta', () => {
    expect(parseModelEntitlementMeta(DECLARED)).toBeDefined()
    expect(parseModelEntitlementMeta({ ...DECLARED, enforced: 'true' })).toBeUndefined()
    expect(parseModelEntitlementMeta({ ...DECLARED, hidden_count: '3' })).toBeUndefined()
    expect(parseModelEntitlementMeta({ ...DECLARED, state: 7 })).toBeUndefined()
    expect(parseModelEntitlementMeta([DECLARED])).toBeUndefined()
  })

  it('drops a ceiling it cannot name rather than guessing one', () => {
    expect(parseModelEntitlementMeta({ ...DECLARED, max_effort: 'turbo' })?.maxEffort).toBeUndefined()
  })
})

describe('the effort ceiling', () => {
  it('applies only while the policy is enforced', () => {
    expect(enforcedMaxEffort(meta({ maxEffort: 'low' }))).toBe('low')
    expect(enforcedMaxEffort(meta({ enforced: false, maxEffort: 'low' }))).toBeUndefined()
    expect(enforcedMaxEffort(undefined)).toBeUndefined()
  })

  it('allows every level up to and including the ceiling, and all of them without one', () => {
    expect(effortLevelWithin('low', 'medium')).toBe(true)
    expect(effortLevelWithin('medium', 'medium')).toBe(true)
    expect(effortLevelWithin('high', 'medium')).toBe(false)
    expect(effortLevelWithin('max', undefined)).toBe(true)
  })
})

describe('a stale selection', () => {
  const listed = [chat('gpt-mini'), chat('claude-x', 'anthropic')]

  it('is a pinned model the enforced policy no longer lists', () => {
    expect(selectionIsStale({ model: 'gpt-big', provider: 'openai' }, listed, meta())).toBe(true)
  })

  it('is not stale while the catalogue still lists it, by id and provider', () => {
    expect(selectionIsStale({ model: 'gpt-mini', provider: 'openai' }, listed, meta())).toBe(false)
    expect(selectionIsStale({ model: 'gpt-mini', provider: 'azure' }, listed, meta())).toBe(true)
  })

  it('never resets a Verevon mode or an empty selection, which the server resolves', () => {
    expect(selectionIsStale({ model: 'verevon-balance' }, listed, meta())).toBe(false)
    expect(selectionIsStale({ model: '' }, listed, meta())).toBe(false)
  })

  it('never resets on shadow mode, an unrestricted policy or no policy', () => {
    const gone = { model: 'gpt-big', provider: 'openai' }
    expect(selectionIsStale(gone, listed, meta({ enforced: false }))).toBe(false)
    expect(selectionIsStale(gone, listed, meta({ restricted: false }))).toBe(false)
    expect(selectionIsStale(gone, listed, undefined)).toBe(false)
  })

  it('never resets against an empty catalogue, which is an outage and not a policy', () => {
    expect(selectionIsStale({ model: 'gpt-big', provider: 'openai' }, [], meta())).toBe(false)
  })
})
