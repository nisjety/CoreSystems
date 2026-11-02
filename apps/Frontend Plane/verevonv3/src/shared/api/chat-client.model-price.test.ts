import { afterEach, describe, expect, it, vi } from 'vitest'
import { filterModels, formatModelListPrice, listModelCatalogue, type ModelInfo } from './chat-client'

// The list price rides on each chat model (`price` on /v1/models, behind the gateway's
// MODEL_GATEWAY_CATALOGUE_PRICES switch). The client keeps a price whole or not at all.

function catalogue(body: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

const priced = (overrides: Record<string, unknown> = {}) => ({
  input_per_million: 3,
  output_per_million: 15,
  currency: 'USD',
  source: 'prefix',
  ...overrides,
})

describe('the list price on a catalogue entry', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('is read from a chat model that names it', async () => {
    catalogue({ models: [{ id: 'claude-sonnet-5-5', provider: 'anthropic', modality: 'chat', price: priced() }] })
    const [model] = (await listModelCatalogue()).models
    expect(model?.price).toEqual({ inputPerMillion: 3, outputPerMillion: 15, currency: 'USD' })
  })

  it('is absent when the gateway sends none', async () => {
    catalogue({ models: [{ id: 'gpt-mini', modality: 'chat' }] })
    const [model] = (await listModelCatalogue()).models
    expect(model?.price).toBeUndefined()
  })

  it('keeps a free model as a price of zero', async () => {
    catalogue({ models: [{ id: 'free-mini', modality: 'chat', price: priced({ input_per_million: 0, output_per_million: 0 }) }] })
    const [model] = (await listModelCatalogue()).models
    expect(model?.price).toEqual({ inputPerMillion: 0, outputPerMillion: 0, currency: 'USD' })
  })

  it('upper-cases the currency it keeps', async () => {
    catalogue({ models: [{ id: 'gpt-mini', modality: 'chat', price: priced({ currency: ' usd ' }) }] })
    const [model] = (await listModelCatalogue()).models
    expect(model?.price?.currency).toBe('USD')
  })

  it('is dropped whenever a part is missing, malformed or unexpected', async () => {
    const malformed = [
      priced({ input_per_million: undefined }),
      priced({ output_per_million: '15' }),
      priced({ input_per_million: -1 }),
      priced({ output_per_million: Number.NaN }),
      priced({ currency: '' }),
      priced({ currency: 'dollars' }),
      priced({ currency: 'U5D' }),
      priced({ currency: undefined }),
      'USD 3 / 15',
      null,
      [],
    ]
    catalogue({ models: malformed.map((price, index) => ({ id: `model-${index}`, modality: 'chat', price })) })
    const models = (await listModelCatalogue()).models
    expect(models).toHaveLength(malformed.length)
    expect(models.map((model) => model.price)).toEqual(malformed.map(() => undefined))
  })
})

describe('filterModels', () => {
  const models: ModelInfo[] = [
    { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', provider: 'anthropic', modality: 'chat' },
    { id: 'gpt-4o-mini', name: 'GPT-4o mini', provider: 'openai', modality: 'chat' },
    { id: 'deepseek-chat', name: 'DeepSeek Chat', provider: 'deepseek', modality: 'chat' },
    { id: 'cafe-model', name: 'Café-modell', provider: 'lab', modality: 'chat' },
    { id: 'soek-model', name: 'Søk Blå', provider: 'lab', modality: 'chat' },
  ]

  it('keeps every model, in order, for an empty query', () => {
    expect(filterModels(models, '  ')).toEqual(models)
  })

  it('matches the name, the id and the provider, case-insensitively', () => {
    expect(filterModels(models, 'SONNET').map((model) => model.id)).toEqual(['claude-sonnet-5-5'])
    expect(filterModels(models, 'gpt-4o').map((model) => model.id)).toEqual(['gpt-4o-mini'])
    expect(filterModels(models, ' anthropic ').map((model) => model.id)).toEqual(['claude-sonnet-5-5'])
    expect(filterModels(models, 'deepseek').map((model) => model.id)).toEqual(['deepseek-chat'])
  })

  it('ignores accents on the other letters', () => {
    expect(filterModels(models, 'cafe').map((model) => model.id)).toEqual(['cafe-model'])
    expect(filterModels(models, 'CAFÉ').map((model) => model.id)).toEqual(['cafe-model'])
  })

  it('keeps the Norwegian letters as they are, so they do not fold onto other letters', () => {
    expect(filterModels(models, 'søk').map((model) => model.id)).toEqual(['soek-model'])
    expect(filterModels(models, 'sok')).toEqual([])
    expect(filterModels(models, 'blå').map((model) => model.id)).toEqual(['soek-model'])
    expect(filterModels(models, 'bla')).toEqual([])
  })
})

describe('formatModelListPrice', () => {
  const price = { inputPerMillion: 3, outputPerMillion: 15.5, currency: 'USD' }

  it('states both rates per million tokens in the reader language', () => {
    expect(formatModelListPrice(price, 'en')).toMatch(/^\$3\.00 in \/ \$15\.50 out per million tokens$/)
    expect(formatModelListPrice(price, 'no')).toMatch(/3,00\s.*inn \/ .*15,50\s.*ut per million tokens$/)
  })
})
