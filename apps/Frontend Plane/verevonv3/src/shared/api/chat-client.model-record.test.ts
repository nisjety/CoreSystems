import { afterEach, describe, expect, it, vi } from 'vitest'
import { formatContextWindow, groupChatModels, listModelCatalogue, type ModelInfo } from './chat-client'

// The record rides on each chat model (`context_window_tokens`, `max_output_tokens` and
// `lifecycle` on /v1/models). A field the gateway does not send, or sends in a shape the
// client does not know, is absent rather than guessed.

function catalogue(body: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

const chat = (overrides: Partial<ModelInfo>): ModelInfo => ({
  id: 'claude-sonnet-5-5',
  name: 'claude-sonnet-5-5',
  modality: 'chat',
  provider: 'anthropic',
  ...overrides,
})

describe('the record on a catalogue entry', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('is read from a chat model that names it', async () => {
    catalogue({
      models: [{
        id: 'claude-sonnet-5-5',
        provider: 'anthropic',
        modality: 'chat',
        context_window_tokens: 1_000_000,
        max_output_tokens: 128_000,
        lifecycle: 'ga',
      }],
    })
    const [model] = (await listModelCatalogue()).models
    expect(model).toMatchObject({ contextWindowTokens: 1_000_000, maxOutputTokens: 128_000, lifecycle: 'ga' })
  })

  it('is absent when the gateway sends none', async () => {
    catalogue({ models: [{ id: 'gpt-mini', modality: 'chat' }] })
    const [model] = (await listModelCatalogue()).models
    expect(model?.contextWindowTokens).toBeUndefined()
    expect(model?.maxOutputTokens).toBeUndefined()
    expect(model?.lifecycle).toBeUndefined()
  })

  it('drops a token count that is not a positive whole number, and a lifecycle it does not know', async () => {
    catalogue({
      models: [{
        id: 'odd-model',
        modality: 'chat',
        context_window_tokens: 0,
        max_output_tokens: -4,
        lifecycle: 'beta',
      }],
    })
    const [model] = (await listModelCatalogue()).models
    expect(model?.contextWindowTokens).toBeUndefined()
    expect(model?.maxOutputTokens).toBeUndefined()
    expect(model?.lifecycle).toBeUndefined()
  })
})

describe('the context window label', () => {
  it('states a whole million tokens as 1M and anything smaller in thousands', () => {
    expect(formatContextWindow(1_000_000)).toBe('1M kontekst')
    expect(formatContextWindow(200_000)).toBe('200k kontekst')
  })

  it('is absent when no window is known', () => {
    expect(formatContextWindow(undefined)).toBeUndefined()
  })
})

describe('the deprecated fold in the picker groups', () => {
  it('moves a deprecated model into a folded last group and keeps current models in their provider group', () => {
    const groups = groupChatModels([
      chat({ id: 'claude-sonnet-5-5', name: 'claude-sonnet-5-5', lifecycle: 'ga' }),
      chat({ id: 'claude-sonnet-4-5', name: 'claude-sonnet-4-5', lifecycle: 'deprecated' }),
    ])
    expect(groups.map((group) => group.label)).toEqual(['Claude', 'Eldre modeller'])
    expect(groups[0]?.models.map((model) => model.id)).toEqual(['claude-sonnet-5-5'])
    expect(groups[1]).toMatchObject({ folded: true })
    expect(groups[1]?.models.map((model) => model.id)).toEqual(['claude-sonnet-4-5'])
  })

  it('adds no fold when no model is deprecated', () => {
    const groups = groupChatModels([chat({ lifecycle: 'ga' })])
    expect(groups.map((group) => group.label)).toEqual(['Claude'])
    expect(groups[0]).not.toHaveProperty('folded')
  })
})
