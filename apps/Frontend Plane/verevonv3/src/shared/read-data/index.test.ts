import { describe, expect, it } from 'vitest'
import {
  isMeasured,
  listResult,
  notConnectedResult,
  plannedResult,
  resourceResult,
  unavailableResult,
  withResourceTimeout,
  type ResourceResult,
} from '@/shared/read-data'

describe('read-data substrate — state machine', () => {
  it('listResult is live ONLY when rows were produced', () => {
    const live = listResult([{ id: 'a' }], { live: 'rows', empty: 'none' })
    expect(live.state).toBe('live')
    expect(live.message).toBe('rows')
    expect(live.data).toHaveLength(1)

    const empty = listResult([], { live: 'rows', empty: 'none' })
    expect(empty.state).toBe('empty')
    expect(empty.message).toBe('none')
    expect(empty.data).toEqual([])
  })

  it('listResult never aliases the caller array (immutable copy)', () => {
    const source = [1, 2]
    const result = listResult(source, { live: 'l', empty: 'e' })
    source.push(3)
    expect(result.data).toEqual([1, 2])
  })

  it('builders carry their honest, non-live states', () => {
    expect(unavailableResult('down', []).state).toBe('unavailable')
    expect(notConnectedResult('connect first', []).state).toBe('not_connected')
    expect(plannedResult('not built yet', []).state).toBe('planned')
    expect(resourceResult({ n: 1 }, 'live', 'ok').state).toBe('live')
  })

  it('isMeasured is true only for live (the produced-rows guard)', () => {
    expect(isMeasured('live')).toBe(true)
    for (const state of ['empty', 'not_connected', 'unavailable', 'planned'] as const) {
      expect(isMeasured(state)).toBe(false)
    }
  })
})

describe('withResourceTimeout — race transitions', () => {
  const fallback: ResourceResult<number[]> = { data: [], message: 'timed out', state: 'unavailable' }

  it('returns the resource result when it settles before the timeout', async () => {
    const resource = Promise.resolve<ResourceResult<number[]>>({
      data: [1, 2, 3],
      message: 'live rows',
      state: 'live',
    })
    const result = await withResourceTimeout(resource, fallback, 50)
    expect(result.state).toBe('live')
    expect(result.data).toEqual([1, 2, 3])
  })

  it('returns the fallback (unavailable) when the resource is slower than the timeout', async () => {
    const slow = new Promise<ResourceResult<number[]>>((resolve) => {
      setTimeout(() => resolve({ data: [9], message: 'late', state: 'live' }), 80)
    })
    const result = await withResourceTimeout(slow, fallback, 10)
    expect(result.state).toBe('unavailable')
    expect(result.data).toEqual([])
  })

  it('propagates a rejected resource (caller decides) rather than masking it', async () => {
    const failing = Promise.reject<ResourceResult<number[]>>(new Error('boom'))
    await expect(withResourceTimeout(failing, fallback, 50)).rejects.toThrow('boom')
  })
})
