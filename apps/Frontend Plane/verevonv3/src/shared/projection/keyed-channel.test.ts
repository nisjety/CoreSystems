import { describe, expect, it, vi } from 'vitest'
import { openProjectionChannel } from './keyed-channel'

describe('openProjectionChannel', () => {
  it('accepts while it owns both the key and the generation', () => {
    const channel = openProjectionChannel({
      ownsKey: () => true,
      generation: 1,
      activeGeneration: () => 1,
    })
    expect(channel.accepts()).toBe(true)
    expect(channel.drops()).toEqual({ key: 0, generation: 0 })
  })

  it('rejects once its key is no longer on screen', () => {
    let active = true
    const channel = openProjectionChannel({
      ownsKey: () => active,
      generation: 1,
      activeGeneration: () => 1,
    })
    expect(channel.accepts()).toBe(true)
    active = false
    expect(channel.accepts()).toBe(false)
    expect(channel.drops()).toEqual({ key: 1, generation: 0 })
  })

  /**
   * The case a key check cannot see: same thread, older connection. Sending a
   * second message does not abort the first stream, so both are live on one
   * thread and both pass the id check — and the first stream's late `done` would
   * otherwise flip the status to idle while the second is still answering.
   */
  it('rejects a superseded connection on the SAME key', () => {
    let newest = 1
    const first = openProjectionChannel({
      ownsKey: () => true,
      generation: 1,
      activeGeneration: () => newest,
    })
    expect(first.accepts()).toBe(true)

    newest = 2 // a second send opened a newer stream on the same thread
    expect(first.accepts()).toBe(false)
    expect(first.drops()).toEqual({ key: 0, generation: 1 })
  })

  it('the newest channel still accepts after superseding an older one', () => {
    const second = openProjectionChannel({
      ownsKey: () => true,
      generation: 2,
      activeGeneration: () => 2,
    })
    // Equal generation is this channel, not a superseding one.
    expect(second.accepts()).toBe(true)
    expect(second.drops()).toEqual({ key: 0, generation: 0 })
  })

  it('reports the reason for each rejection so an unexpected rate is visible', () => {
    const onDrop = vi.fn()
    let owns = true
    let newest = 1
    const channel = openProjectionChannel({
      ownsKey: () => owns,
      generation: 1,
      activeGeneration: () => newest,
      onDrop,
    })
    owns = false
    expect(channel.accepts()).toBe(false)
    owns = true
    newest = 5
    expect(channel.accepts()).toBe(false)

    expect(onDrop.mock.calls.map((call) => call[0])).toEqual(['key', 'generation'])
    expect(onDrop.mock.calls.every((call) => call[1] === 1)).toBe(true)
    expect(channel.drops()).toEqual({ key: 1, generation: 1 })
  })

  /** A lost key takes precedence: it is the more specific fact about the write. */
  it('reports key loss ahead of generation when both apply', () => {
    const channel = openProjectionChannel({
      ownsKey: () => false,
      generation: 1,
      activeGeneration: () => 9,
    })
    expect(channel.accepts()).toBe(false)
    expect(channel.drops()).toEqual({ key: 1, generation: 0 })
  })
})
