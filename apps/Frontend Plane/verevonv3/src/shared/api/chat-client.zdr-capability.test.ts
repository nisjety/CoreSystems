import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { hasZeroRetentionModel, listModels, modelAttestsZeroRetention } from './chat-client'

// The org ZDR switch has three readers that must agree on one question: does the
// provider catalogue show at least one CHAT model whose provider has a recorded
// zero-retention attestation?
//
//   - the composer (DashboardComposer) decides whether Temporary chat is offered,
//   - the workspace settings toggle decides whether org ZDR can be switched on,
//   - the gateway (`apps/gateway/src/domains/orgs/settings.rs`) decides whether the
//     PATCH is allowed at all.
//
// The first two are `hasZeroRetentionModel`; the third is `catalogue_attests_zdr_chat_model`.
// They read ONE file of cases, so a change to either side that makes them diverge fails
// here or in the gateway's own suite. The file is read, never skipped: a missing file
// throws and the suite fails.

const here = dirname(fileURLToPath(import.meta.url))
const FIXTURE = join(
  here, '..', '..', '..', 'apps', 'gateway', 'tests', 'fixtures', 'zdr-model-catalogue-cases.json',
)

type Case = { name: string; attested_chat_model: boolean; body: unknown }

function fixtureCases(): Case[] {
  const parsed = JSON.parse(readFileSync(FIXTURE, 'utf8')) as { cases?: Case[] }
  if (!Array.isArray(parsed.cases)) throw new Error(`${FIXTURE} has no cases array`)
  return parsed.cases
}

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })
}

describe('org ZDR switch: one attestation predicate for the composer, the toggle and the gateway', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('answers every shared fixture case, read through the real listModels(), as the gateway gate does', async () => {
    const cases = fixtureCases()
    expect(cases.length, 'the fixture lost cases').toBeGreaterThanOrEqual(10)
    let attested = 0
    let refused = 0
    for (const testCase of cases) {
      vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(testCase.body)))
      const models = await listModels()
      expect(hasZeroRetentionModel(models), testCase.name).toBe(testCase.attested_chat_model)
      if (testCase.attested_chat_model) attested += 1
      else refused += 1
    }
    expect(attested).toBeGreaterThanOrEqual(3)
    expect(refused).toBeGreaterThanOrEqual(5)
  })

  it('is false for an empty catalogue, so a gateway outage never switches the control on', () => {
    expect(hasZeroRetentionModel([])).toBe(false)
  })

  it('counts a chat model that carries the zdr feature', () => {
    expect(modelAttestsZeroRetention({ modality: 'chat', capabilities: ['usage', 'zdr'] })).toBe(true)
  })

  it('counts a chat model whose tier only an attested provider can have, when its feature list omits zdr', () => {
    // The Anthropic provider's list_models hardcodes tools/vision/reasoning and
    // never lists zdr, but classifies its tier from the same supports_zdr bit.
    expect(modelAttestsZeroRetention({ modality: 'chat', capabilities: ['tools'], privacyTier: 'zdr_contractual' })).toBe(true)
    expect(modelAttestsZeroRetention({ modality: 'chat', capabilities: [], privacyTier: 'sovereign' })).toBe(true)
  })

  it('does not count residency alone, a lookalike tier, or a model that says nothing', () => {
    expect(modelAttestsZeroRetention({ modality: 'chat', capabilities: ['usage'], privacyTier: 'eu_resident' })).toBe(false)
    expect(modelAttestsZeroRetention({ modality: 'chat', capabilities: ['usage'], privacyTier: 'global' })).toBe(false)
    expect(modelAttestsZeroRetention({ modality: 'chat' })).toBe(false)
  })

  it('does not count a model that is not a chat model, even when it advertises zdr', () => {
    for (const modality of ['image', 'embedding', 'audio', 'video', 'other'] as const) {
      expect(
        modelAttestsZeroRetention({ modality, capabilities: ['zdr'], privacyTier: 'zdr_contractual' }),
        modality,
      ).toBe(false)
    }
    expect(modelAttestsZeroRetention({ capabilities: ['zdr'], privacyTier: 'zdr_contractual' })).toBe(false)
  })
})
