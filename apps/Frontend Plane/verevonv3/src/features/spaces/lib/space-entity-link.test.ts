import { describe, expect, it } from 'vitest'

import { parseSpaceEntityLocation, spaceEntityHref, type SpaceEntityRef } from './space-entity-link'

// Reads a link back the way the router does: its path and its search string.
function readHref(href: string) {
  const url = new URL(href, 'http://localhost')
  return parseSpaceEntityLocation(url.pathname, url.search, url.hash)
}

function parseSearch(search: string) {
  return parseSpaceEntityLocation('/spaces/room-a', search)
}

// WS13-S4a: the one grammar for pointing at a message, a thread or a run. It fails closed.
describe('space entity links (WS13-S4a)', () => {
  it('round-trips a thread, a message with its thread, and a run', () => {
    const thread: SpaceEntityRef = { kind: 'thread', spaceRef: 'room-a', threadId: 'thr-1' }
    const message: SpaceEntityRef = { kind: 'message', spaceRef: 'room-a', messageId: 'msg-1', threadId: 'thr-1' }
    const run: SpaceEntityRef = { kind: 'run', spaceRef: 'room-a', runId: 'run-1', threadId: 'thr-1' }
    for (const ref of [thread, message, run]) expect(readHref(spaceEntityHref(ref))).toEqual(ref)
    expect(spaceEntityHref(thread)).toBe('/spaces/room-a?thread_id=thr-1#chat')
    expect(spaceEntityHref(message)).toBe('/spaces/room-a?thread_id=thr-1&m=msg-1#chat')
    expect(spaceEntityHref(run)).toBe('/spaces/room-a?thread_id=thr-1&run_id=run-1#chat')
  })

  it('a message with no thread uses the /m/{id} form, which reads back to the same message', () => {
    const ref: SpaceEntityRef = { kind: 'message', spaceRef: 'room-a', messageId: 'msg-1' }
    expect(spaceEntityHref(ref)).toBe('/spaces/room-a/m/msg-1')
    expect(readHref(spaceEntityHref(ref))).toEqual(ref)
    expect(parseSpaceEntityLocation('/spaces/room-a/m/msg-1', '')).toEqual({
      kind: 'message',
      spaceRef: 'room-a',
      messageId: 'msg-1',
    })
  })

  it('a thread_id and a threadId that disagree name nothing, and threadId alone is read', () => {
    expect(parseSearch('?thread_id=thr-1&threadId=thr-2')).toBeNull()
    expect(parseSearch('?threadId=thr-3')).toEqual({ kind: 'thread', spaceRef: 'room-a', threadId: 'thr-3' })
  })

  it('a repeated message or run parameter, or a repeated thread id with another value, names nothing', () => {
    expect(parseSearch('?thread_id=thr-1&thread_id=thr-2')).toBeNull()
    expect(parseSearch('?thread_id=thr-1&m=msg-1&m=msg-2')).toBeNull()
    expect(parseSearch('?thread_id=thr-1&run_id=run-1&run_id=run-2')).toBeNull()
  })

  it('control characters in the room, thread or message id name nothing', () => {
    expect(parseSearch('?thread_id=thr%0A1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room%00a', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room-a/m/msg%090', '')).toBeNull()
  })

  it('an over-length id names nothing, and the length limit is inclusive', () => {
    expect(parseSearch(`?thread_id=${'x'.repeat(201)}`)).toBeNull()
    expect(parseSearch(`?thread_id=${'x'.repeat(200)}`)).toEqual({
      kind: 'thread',
      spaceRef: 'room-a',
      threadId: 'x'.repeat(200),
    })
    expect(parseSpaceEntityLocation(`/spaces/${'r'.repeat(201)}`, '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation(`/spaces/room-a/m/${'m'.repeat(65)}`, '')).toBeNull()
    expect(parseSpaceEntityLocation(`/spaces/room-a/m/${'m'.repeat(64)}`, '')).toEqual({
      kind: 'message',
      spaceRef: 'room-a',
      messageId: 'm'.repeat(64),
    })
  })

  it('a bad message id on its own names nothing, and an unreadable id never reaches a link', () => {
    expect(parseSpaceEntityLocation('/spaces/room-a/m/bad.id', '')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room-a/m/', '')).toBeNull()
    expect(spaceEntityHref({ kind: 'message', spaceRef: 'room-a', messageId: 'bad.id', threadId: 'thr-1' })).toBe(
      '/spaces/room-a?thread_id=thr-1#chat',
    )
  })

  it('unknown shapes parse to null', () => {
    expect(parseSpaceEntityLocation('/other/room-a', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('spaces/room-a', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room-a/x', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room-a/m/msg-1/extra', '')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/a%2Fb', '?thread_id=thr-1')).toBeNull()
    expect(parseSpaceEntityLocation('/spaces/room-a', '')).toBeNull()
    expect(parseSearch('?m=msg-1')).toBeNull()
  })

  it('a malformed m still yields the thread ref', () => {
    const thread = { kind: 'thread', spaceRef: 'room-a', threadId: 'thr-1' }
    expect(parseSearch('?thread_id=thr-1&m=bad.id')).toEqual(thread)
    expect(parseSearch('?thread_id=thr-1&m=')).toEqual(thread)
    expect(parseSearch('?thread_id=thr-1&m=msg%0A1')).toEqual(thread)
  })
})
