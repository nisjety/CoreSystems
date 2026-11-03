import { afterEach, describe, expect, it, vi } from 'vitest'
import { listModelCatalogue } from './chat-client'

// `role_routing` rides beside `models` on /api/v1/models. The gateway leaves it out while the switch is off,
// so the client reads only a declared `true` and never guesses one.

function catalogue(body: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

describe('role routing on the model catalogue', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('is read when the gateway declares it on', async () => {
    catalogue({ models: [], role_routing: true })

    expect((await listModelCatalogue()).roleRouting).toBe(true)
  })

  it('is absent while the gateway leaves it out or declares it off', async () => {
    catalogue({ models: [] })
    expect((await listModelCatalogue()).roleRouting).toBeUndefined()

    catalogue({ models: [], role_routing: false })
    expect((await listModelCatalogue()).roleRouting).toBeUndefined()
  })
})
