import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamChat } from './chat-client'

// WS03-S7: a typed Space denial (policy, entitlement, role or ZDR refusal) on the chat stream is the
// server's answer about the room, not a sign-in problem. It keeps its code and details so the room can
// say why. An untyped 403 still reads as lost access, a 401 is unchanged, and the zero-retention codes
// keep winning.

function refusal(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

async function errorFor(status: number, body: unknown) {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(status, body)))
  const onError = vi.fn()
  await streamChat({ content: 'hi' }, { onError })
  expect(onError).toHaveBeenCalledTimes(1)
  return onError.mock.calls[0]?.[0] as Record<string, unknown>
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('a typed Space denial as the HTTP answer to the chat stream', () => {
  it('keeps the code and details of a typed 403', async () => {
    const event = await errorFor(403, {
      error: {
        code: 'forbidden',
        message: 'role below floor',
        details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
      },
    })
    expect(event).toMatchObject({
      code: 'forbidden',
      details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
    })
    expect(event.code).not.toBe('connection_error')
    expect(String(event.message)).not.toMatch(/Logg inn på nytt/)
  })

  it('keeps the details of a typed 503', async () => {
    const event = await errorFor(503, {
      error: {
        code: 'space_authority_unavailable',
        message: 'cannot confirm',
        details: { reason: 'space_authority_unavailable', resolvable_by: 'retry' },
      },
    })
    expect(event).toMatchObject({ code: 'space_authority_unavailable', details: { reason: 'space_authority_unavailable' } })
  })

  it('accepts a pass-through code with no details', async () => {
    const event = await errorFor(403, { error: { code: 'space_membership_required', message: 'x' } })
    expect(event.code).toBe('space_membership_required')
    expect(event).not.toHaveProperty('details')
  })

  it('an untyped 403 still reads as lost access', async () => {
    const event = await errorFor(403, { error: { code: 'forbidden', message: 'no' } })
    expect(event.code).toBe('forbidden')
    expect(String(event.message)).toMatch(/Logg inn på nytt/)
    expect(event).not.toHaveProperty('details')
  })

  it('an untyped 503 is still a lost connection', async () => {
    const event = await errorFor(503, { error: { code: 'upstream_error', message: 'down' } })
    expect(event.code).toBe('connection_error')
  })

  it('does not treat a typed 500 as a denial', async () => {
    const event = await errorFor(500, {
      error: { code: 'internal', message: 'boom', details: { reason: 'space_role_below_floor' } },
    })
    expect(event.code).toBe('connection_error')
  })

  it('a 401 is unchanged', async () => {
    const event = await errorFor(401, { error: { code: 'unauthorized', message: 'no session' } })
    expect(event.code).toBe('unauthorized')
    expect(String(event.message)).toMatch(/Økten din er utløpt/)
  })

  it('a 401 stays a sign-in problem even if its body carries a denial reason', async () => {
    const event = await errorFor(401, {
      error: { code: 'unauthorized', message: 'x', details: { reason: 'space_role_below_floor' } },
    })
    expect(event.code).toBe('unauthorized')
  })

  it('zero-retention codes still win over a Space denial in the same body', async () => {
    const event = await errorFor(403, {
      error: {
        code: 'zdr_durable_mutation_forbidden',
        message: 'zdr',
        details: { reason: 'space_zdr_forbids_effect', effect: 'thread-write' },
      },
    })
    expect(event.code).toBe('zdr_durable_mutation_forbidden')
    expect(event).not.toHaveProperty('details')
  })
})
