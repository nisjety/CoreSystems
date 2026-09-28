import { afterEach, describe, expect, it, vi } from 'vitest'
import { getPersonaInstructions, updatePersonaInstructions } from './persona-client'
import { ApiError } from './http'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('persona client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('reads the signed-in user\'s own persona through /api/v1/me/persona', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({ data: { instructions: 'Always be terse.', enabled: true } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const persona = await getPersonaInstructions()

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit | undefined]
    expect(path).toBe('/api/v1/me/persona')
    expect(init?.method ?? 'GET').not.toBe('PATCH')
    expect(persona).toEqual({ instructions: 'Always be terse.', enabled: true })
  })

  it('defaults to an empty, enabled persona when nothing has been authored yet', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ data: { instructions: null, enabled: true } })))
    await expect(getPersonaInstructions()).resolves.toEqual({ instructions: '', enabled: true })
  })

  it('writes persona text and the enabled switch with a PATCH round-trip', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({ data: { instructions: 'Always be terse.', enabled: false } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await updatePersonaInstructions('Always be terse.', false)

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/me/persona')
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(String(init.body))).toEqual({ instructions: 'Always be terse.', enabled: false })
    expect(result).toEqual({ instructions: 'Always be terse.', enabled: false })
  })

  it('translates a gateway error into an ApiError callers can branch on', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          { error: { code: 'persona_instructions_too_long', message: 'Persona instructions must be 1500 characters or fewer.' } },
          400,
        ),
      ),
    )

    await expect(updatePersonaInstructions('x'.repeat(1501), true)).rejects.toMatchObject({
      name: 'ApiError',
      status: 400,
      code: 'persona_instructions_too_long',
    } satisfies Partial<ApiError>)
  })
})
