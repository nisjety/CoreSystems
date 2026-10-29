import { describe, expect, it } from 'vitest'
import { createLatestRequest } from './latest'

describe('latest request guard', () => {
  it('accepts only the answer to the newest call', () => {
    const guard = createLatestRequest()

    const first = guard.next()
    const second = guard.next()

    expect(guard.isCurrent(first)).toBe(false)
    expect(guard.isCurrent(second)).toBe(true)
  })
})
