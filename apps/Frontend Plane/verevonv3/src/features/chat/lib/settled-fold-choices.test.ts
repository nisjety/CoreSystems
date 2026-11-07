import { createMemo, createRoot, flush } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { createSettledFoldChoices } from './settled-fold-choices'

describe('createSettledFoldChoices', () => {
  it('follows the plan default until the reader acts on a turn', () => {
    const choices = createSettledFoldChoices()
    expect(choices.isFolded('a', true)).toBe(true)
    expect(choices.isFolded('a', false)).toBe(false)
  })

  it('holds the reader\'s choice in either direction', () => {
    const choices = createSettledFoldChoices()
    choices.choose('a', false)
    flush()
    expect(choices.isFolded('a', true)).toBe(false)
    choices.choose('a', true)
    flush()
    expect(choices.isFolded('a', false)).toBe(true)
  })

  it('keeps the choice when the plan default changes after it', () => {
    const choices = createSettledFoldChoices()
    choices.choose('a', false)
    flush()
    expect(choices.isFolded('a', true)).toBe(false)
    expect(choices.isFolded('a', false)).toBe(false)
  })

  it('leaves a turn the reader has not touched on the default, so it follows a changed plan', () => {
    const choices = createSettledFoldChoices()
    choices.choose('a', false)
    flush()
    expect(choices.isFolded('b', true)).toBe(true)
    expect(choices.isFolded('b', false)).toBe(false)
  })

  it('keeps two views apart: a choice in one does not reach the other', () => {
    const first = createSettledFoldChoices()
    const second = createSettledFoldChoices()
    first.choose('a', false)
    flush()
    expect(second.isFolded('a', true)).toBe(true)
  })

  it('is reactive: a memo over a choice follows the next choice', () => {
    const choices = createSettledFoldChoices()
    const folded = createRoot(() => createMemo(() => choices.isFolded('a', true)))
    expect(folded()).toBe(true)
    choices.choose('a', false)
    flush()
    expect(folded()).toBe(false)
  })
})
