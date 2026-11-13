import { afterEach, describe, expect, it, vi } from 'vitest'
import { listAdmittedOwnerActions, parseAdmittedOwnerActions } from './agent-actions-client'

// W3-EP-6-S11-ui: the admitted owner-action set fails closed. Null (offer no owner action) on any failure
// or any reply that is not exactly the expected shape; never a partial set.

function reply(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('parseAdmittedOwnerActions', () => {
  it('reads the mode and the ids as sent', () => {
    expect(parseAdmittedOwnerActions({ mode: 'table', action_ids: ['tickets.create', 'mail.send'] })).toEqual({
      mode: 'table',
      actionIds: ['tickets.create', 'mail.send'],
    })
    expect(parseAdmittedOwnerActions({ mode: 'tickets_only', action_ids: [] })).toEqual({
      mode: 'tickets_only',
      actionIds: [],
    })
  })

  it.each([
    ['null', null],
    ['a string', 'table'],
    ['an array', [{ mode: 'table', action_ids: [] }]],
    ['no mode', { action_ids: ['tickets.create'] }],
    ['a numeric mode', { mode: 1, action_ids: ['tickets.create'] }],
    ['no ids', { mode: 'table' }],
    ['ids as an object', { mode: 'table', action_ids: { 0: 'tickets.create' } }],
    ['the camelCase key only', { mode: 'table', actionIds: ['tickets.create'] }],
    ['one non-string id among good ones', { mode: 'table', action_ids: ['tickets.create', 7] }],
    ['one empty id among good ones', { mode: 'table', action_ids: ['tickets.create', ''] }],
    ['one blank id among good ones', { mode: 'table', action_ids: ['tickets.create', '   '] }],
    ['a null id', { mode: 'table', action_ids: [null] }],
  ])('refuses %s as a whole, never a partial set', (_label, payload) => {
    expect(parseAdmittedOwnerActions(payload)).toBeNull()
  })
})

describe('listAdmittedOwnerActions', () => {
  it('reads the data envelope the gateway relays from capability-core', async () => {
    const fetchMock = vi.fn().mockResolvedValue(reply(200, { data: { mode: 'table', action_ids: ['tickets.create'] } }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(listAdmittedOwnerActions()).resolves.toEqual({ mode: 'table', actionIds: ['tickets.create'] })
    expect(String(fetchMock.mock.calls[0]![0])).toMatch(/\/api\/v1\/agents\/admitted-actions$/)
  })

  it.each([
    ['a 403 (ZDR principal or no capability)', () => reply(403, { error: { code: 'forbidden', message: 'no' } })],
    ['a 503', () => reply(503, { error: { code: 'unavailable', message: 'down' } })],
    ['a malformed 200', () => reply(200, { data: { mode: 'table', action_ids: ['ok', 1] } })],
    ['a 200 with no body', () => new Response('', { status: 200 })],
  ])('answers null on %s', async (_label, make) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(make()))
    await expect(listAdmittedOwnerActions()).resolves.toBeNull()
  })

  it('answers null when the request itself fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('network')))
    await expect(listAdmittedOwnerActions()).resolves.toBeNull()
  })
})
