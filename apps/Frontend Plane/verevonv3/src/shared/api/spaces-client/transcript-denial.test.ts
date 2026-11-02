import { afterEach, describe, expect, it, vi } from 'vitest'

import { readSpaceDenial } from '../space-denials'
import { loadSpaceTranscript } from './index'

// The real client, not a mock of it: a typed 403 body goes through requestJson, becomes an ApiError and
// must come out of loadSpaceTranscript as `restricted` carrying the denial the room shows.
function stubFetch(status: number, body: unknown): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('loadSpaceTranscript: the typed denial of a 403 (D1-259)', () => {
  it('carries the code and details of a typed 403 so the room can say why', async () => {
    stubFetch(403, {
      error: {
        code: 'space_thread_not_authorized',
        message: 'no',
        details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
      },
    })
    const load = await loadSpaceTranscript('room', 't1')
    expect(load.state).toBe('restricted')
    const denial = load.state === 'restricted' ? load.denial : undefined
    expect(denial).toBeDefined()
    expect(readSpaceDenial(denial)).toMatchObject({
      reason: 'space_role_below_floor',
      known: true,
      resolvableBy: 'space_manager',
    })
  })

  it('keeps a reason this build does not know, for the presenter’s generic line', async () => {
    stubFetch(403, { error: { code: 'forbidden', message: 'no', details: { reason: 'a_newer_reason' } } })
    const load = await loadSpaceTranscript('room', 't1')
    const denial = load.state === 'restricted' ? load.denial : undefined
    expect(readSpaceDenial(denial)).toMatchObject({ reason: 'a_newer_reason', known: false })
  })

  it('returns a bare restricted for a 403 that names no reason', async () => {
    stubFetch(403, { error: { code: 'forbidden', message: 'no' } })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'restricted' })
    const load = await loadSpaceTranscript('room', 't1')
    expect('denial' in load).toBe(false)
  })

  it('never attaches a denial to a 404 or a 500, even when the body is typed', async () => {
    const error = { code: 'space_thread_not_authorized', message: 'x', details: { reason: 'space_role_below_floor' } }
    stubFetch(404, { error })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'missing' })
    stubFetch(500, { error })
    expect(await loadSpaceTranscript('room', 't1')).toEqual({ state: 'error' })
  })
})
