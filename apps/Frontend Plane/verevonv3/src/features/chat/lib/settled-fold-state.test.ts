import { createEffect, createRoot, flush } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { createSettledFoldState } from './settled-fold-state'

/** Records what `read` returns each time it changes, as a row in the view would see it. */
function watch(read: () => boolean | undefined) {
  const seen: Array<boolean | undefined> = []
  let dispose!: () => void
  createRoot((done) => {
    dispose = done
    createEffect(read, (value) => {
      seen.push(value)
    })
  })
  flush()
  return { seen, dispose }
}

describe('createSettledFoldState', () => {
  it('has no choice for a turn until the reader makes one', () => {
    expect(createSettledFoldState().choice('a1')).toBeUndefined()
  })

  it('keeps the choice the reader records for each turn', () => {
    const state = createSettledFoldState()
    state.choose('a1', true)
    state.choose('a2', false)
    expect(state.choice('a1')).toBe(true)
    expect(state.choice('a2')).toBe(false)
    state.choose('a1', false)
    expect(state.choice('a1')).toBe(false)
    expect(state.choice('a3')).toBeUndefined()
  })

  it('keeps each view\'s choices to itself', () => {
    const first = createSettledFoldState()
    const second = createSettledFoldState()
    first.choose('a1', false)
    expect(second.choice('a1')).toBeUndefined()
  })

  it('forgets every choice on reset, and resets an empty view without fuss', () => {
    const state = createSettledFoldState()
    state.choose('a1', false)
    state.choose('a2', true)
    state.reset()
    expect(state.choice('a1')).toBeUndefined()
    expect(state.choice('a2')).toBeUndefined()
    expect(() => state.reset()).not.toThrow()
  })

  it('tells a reader when its choice changes, and not when the same choice is recorded again', () => {
    const state = createSettledFoldState()
    const { seen, dispose } = watch(() => state.choice('a1'))
    expect(seen).toEqual([undefined])
    state.choose('a1', false)
    flush()
    expect(seen).toEqual([undefined, false])
    state.choose('a1', false)
    flush()
    expect(seen).toEqual([undefined, false])
    state.reset()
    flush()
    expect(seen).toEqual([undefined, false, undefined])
    dispose()
  })
})
