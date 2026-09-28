import { afterEach, describe, expect, it, vi } from 'vitest'
import { rememberAiModelSelection } from '@/shared/ai/model-selection'
import type { ModelInfo } from '@/shared/api/chat-client'
import type { PrivacyTier } from '@/shared/api/privacy-tier'
import {
  CATALOGUE_TIER_WAIT_MS,
  createTurnPrivacyTierResolver,
  FAIL_CLOSED_PRIVACY_TIER,
  rememberedPrivacyTier,
  resolveSubmitPrivacyTier,
  waitForCatalogue,
} from './submit-privacy-tier'

const euModel: ModelInfo = { id: 'm-eu', name: 'EU Model', provider: 'eu-host', privacyTier: 'eu_resident' }
const plainModel: ModelInfo = { id: 'm-plain', name: 'Plain Model', provider: 'plain-host' }

describe('resolveSubmitPrivacyTier', () => {
  it('fails closed to the strictest tier', () => {
    expect(FAIL_CLOSED_PRIVACY_TIER).toBe('sovereign')
  })

  it('gives an intent mode (or no model) no tier, whatever else is known', () => {
    for (const model of ['verevon-balance', 'verevon-genius', '', '  ']) {
      expect(resolveSubmitPrivacyTier({
        model, provider: undefined, catalogue: undefined, lastKnownTier: 'sovereign',
      })).toEqual({ tier: undefined, source: 'intent-mode' })
    }
  })

  it('uses the loaded catalogue, even over a different last-known tier', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-eu', provider: 'eu-host', catalogue: [euModel], lastKnownTier: 'global',
    })).toEqual({ tier: 'eu_resident', source: 'catalogue' })
  })

  it('sends no tier for a listed model that claims none, as before', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-plain', provider: 'plain-host', catalogue: [plainModel], lastKnownTier: 'sovereign',
    })).toEqual({ tier: undefined, source: 'catalogue' })
  })

  it('matches id and provider together, like the picker', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-eu', provider: 'other-host', catalogue: [euModel], lastKnownTier: undefined,
    })).toEqual({ tier: 'sovereign', source: 'fail-closed' })
  })

  it('falls back to the last-known tier while the catalogue is unknown', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-eu', provider: 'eu-host', catalogue: undefined, lastKnownTier: 'zdr_contractual',
    })).toEqual({ tier: 'zdr_contractual', source: 'last-known' })
  })

  it('treats a last-known "claims none" as no tier', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-plain', provider: 'plain-host', catalogue: undefined, lastKnownTier: 'unspecified',
    })).toEqual({ tier: undefined, source: 'last-known' })
  })

  it('fails closed when nothing is known about the model', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-eu', provider: 'eu-host', catalogue: undefined, lastKnownTier: undefined,
    })).toEqual({ tier: 'sovereign', source: 'fail-closed' })
  })

  it('does not trust a loaded catalogue that no longer lists the model', () => {
    expect(resolveSubmitPrivacyTier({
      model: 'm-gone', provider: 'eu-host', catalogue: [euModel], lastKnownTier: undefined,
    })).toEqual({ tier: 'sovereign', source: 'fail-closed' })
    expect(resolveSubmitPrivacyTier({
      model: 'm-gone', provider: 'eu-host', catalogue: [euModel], lastKnownTier: 'eu_resident',
    })).toEqual({ tier: 'eu_resident', source: 'last-known' })
  })
})

describe('rememberedPrivacyTier', () => {
  it('stores "claims none" as unspecified, so it differs from "never seen"', () => {
    expect(rememberedPrivacyTier(plainModel)).toBe('unspecified')
    expect(rememberedPrivacyTier(euModel)).toBe('eu_resident')
  })
})

describe('waitForCatalogue', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('resolves as soon as the request settles, rejected or not', async () => {
    await expect(waitForCatalogue(Promise.resolve([]), CATALOGUE_TIER_WAIT_MS)).resolves.toBeUndefined()
    await expect(waitForCatalogue(Promise.reject(new Error('down')), CATALOGUE_TIER_WAIT_MS)).resolves.toBeUndefined()
  })

  it('stops waiting at the bound when the request never settles', async () => {
    vi.useFakeTimers()
    let done = false
    void waitForCatalogue(new Promise(() => {}), CATALOGUE_TIER_WAIT_MS).then(() => { done = true })
    await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS - 1)
    expect(done).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(done).toBe(true)
  })
})

// The helper the Space room composer and Inbox assist send through.
describe('createTurnPrivacyTierResolver', () => {
  const orgId = 'org-tier'

  /** The selection a previous page load stored, with or without its tier. */
  function storeSelection(privacyTier?: PrivacyTier) {
    rememberAiModelSelection(orgId, {
      model: 'm-eu',
      label: 'EU Model',
      provider: 'eu-host',
      ...(privacyTier ? { privacyTier } : {}),
    })
  }

  function deferredCatalogue() {
    let resolve!: (models: ModelInfo[]) => void
    const promise = new Promise<ModelInfo[]>((done) => { resolve = done })
    return { promise, resolve }
  }

  afterEach(() => {
    vi.useRealTimers()
    window.localStorage.clear()
  })

  describe('before the catalogue has loaded', () => {
    it('waits for it and resolves to the model’s own tier', async () => {
      storeSelection('global')
      const catalogue = deferredCatalogue()
      const load = vi.fn(() => catalogue.promise)
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: load })

      let result: unknown
      void tiers.resolve('m-eu', 'eu-host').then((value) => { result = value })
      await Promise.resolve()
      expect(result).toBeUndefined()

      catalogue.resolve([euModel])
      await vi.waitFor(() => expect(result).toEqual({ tier: 'eu_resident', source: 'catalogue' }))
      expect(load).toHaveBeenCalledTimes(1)
    })

    it('falls back to the last-known tier once the bounded wait runs out', async () => {
      storeSelection('zdr_contractual')
      vi.useFakeTimers()
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: () => new Promise(() => {}) })

      let result: unknown
      void tiers.resolve('m-eu', 'eu-host').then((value) => { result = value })
      await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS - 1)
      expect(result).toBeUndefined()
      await vi.advanceTimersByTimeAsync(1)
      expect(result).toEqual({ tier: 'zdr_contractual', source: 'last-known' })
    })

    it('fails closed when the wait runs out and nothing is known about the model', async () => {
      storeSelection()
      vi.useFakeTimers()
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: () => new Promise(() => {}) })

      const result = tiers.resolve('m-eu', 'eu-host')
      await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS)
      await expect(result).resolves.toEqual({ tier: 'sovereign', source: 'fail-closed' })
    })

    it('never starts the request for an intent mode', async () => {
      const load = vi.fn(() => new Promise<ModelInfo[]>(() => {}))
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: load })

      await expect(tiers.resolve('verevon-balance', undefined))
        .resolves.toEqual({ tier: undefined, source: 'intent-mode' })
      expect(load).not.toHaveBeenCalled()
    })
  })

  describe('after the catalogue has loaded', () => {
    it('resolves at once from the one shared request, including "claims none"', async () => {
      storeSelection('sovereign')
      const load = vi.fn(async () => [euModel, plainModel])
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: load })
      // The picker renders from the same request.
      await expect(tiers.catalogue()).resolves.toEqual([euModel, plainModel])

      vi.useFakeTimers()
      await expect(tiers.resolve('m-eu', 'eu-host')).resolves.toEqual({ tier: 'eu_resident', source: 'catalogue' })
      await expect(tiers.resolve('m-plain', 'plain-host')).resolves.toEqual({ tier: undefined, source: 'catalogue' })
      expect(vi.getTimerCount()).toBe(0)
      expect(load).toHaveBeenCalledTimes(1)
    })
  })

  describe('when the catalogue request fails', () => {
    it('uses the last-known tier without waiting', async () => {
      storeSelection('global')
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: async () => { throw new Error('down') } })
      await tiers.catalogue().catch(() => undefined)

      vi.useFakeTimers()
      await expect(tiers.resolve('m-eu', 'eu-host')).resolves.toEqual({ tier: 'global', source: 'last-known' })
      expect(vi.getTimerCount()).toBe(0)
    })

    it('fails closed when nothing is known, a synchronous throw included', async () => {
      storeSelection()
      const tiers = createTurnPrivacyTierResolver({
        orgId,
        loadCatalogue: () => { throw new Error('down') },
      })

      await expect(tiers.resolve('m-eu', 'eu-host')).resolves.toEqual({ tier: 'sovereign', source: 'fail-closed' })
    })

    it('does not borrow the last-known tier of a different stored model', async () => {
      storeSelection('global')
      const tiers = createTurnPrivacyTierResolver({ orgId, loadCatalogue: async () => { throw new Error('down') } })

      await expect(tiers.resolve('m-other', 'eu-host')).resolves.toEqual({ tier: 'sovereign', source: 'fail-closed' })
    })
  })
})
