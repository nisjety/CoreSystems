import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('../http', () => http)

const { SPACE_QUICK_REACTIONS, setSpaceReaction, spaceReactionsFrom } = await import('./reactions')

beforeEach(() => http.requestJson.mockReset())
afterEach(() => vi.restoreAllMocks())

describe('spaceReactionsFrom', () => {
  it('keeps well-formed chips in the order given', () => {
    expect(
      spaceReactionsFrom([
        { emoji: '\u{1F44D}', count: 2, reacted: true },
        { emoji: '\u{1F389}', count: 1, reacted: false },
      ]),
    ).toEqual([
      { emoji: '\u{1F44D}', count: 2, reacted: true },
      { emoji: '\u{1F389}', count: 1, reacted: false },
    ])
  })

  it('drops malformed chips and keeps the good ones', () => {
    expect(
      spaceReactionsFrom([
        null,
        'x',
        7,
        [],
        {},
        { emoji: '', count: 1 },
        { emoji: 5, count: 1 },
        { emoji: '\u{1F44D}', count: '2' },
        { emoji: '\u{1F44D}', count: 0 },
        { emoji: '\u{1F44D}', count: -3 },
        { emoji: '\u{1F44D}', count: Number.NaN },
        { emoji: '\u{1F44D}', count: Number.POSITIVE_INFINITY },
        { emoji: '\u{2705}', count: 4, reacted: true, extra: 'dropped' },
      ]),
    ).toEqual([{ emoji: '\u{2705}', count: 4, reacted: true }])
  })

  it('floors a fractional count and treats a missing or non-true reacted as false', () => {
    expect(spaceReactionsFrom([{ emoji: 'a', count: 2.9 }])).toEqual([{ emoji: 'a', count: 2, reacted: false }])
    expect(spaceReactionsFrom([{ emoji: 'a', count: 1, reacted: 'true' }])).toEqual([
      { emoji: 'a', count: 1, reacted: false },
    ])
  })

  it('treats anything that is not an array as no reactions', () => {
    for (const value of [undefined, null, 'x', 3, {}, { emoji: 'a', count: 1 }]) {
      expect(spaceReactionsFrom(value)).toEqual([])
    }
  })
})

describe('SPACE_QUICK_REACTIONS', () => {
  it('is the 24-emoji curated picker with no duplicates', () => {
    expect(SPACE_QUICK_REACTIONS).toHaveLength(24)
    expect(new Set(SPACE_QUICK_REACTIONS).size).toBe(24)
    expect(SPACE_QUICK_REACTIONS[0]).toBe('\u{1F44D}')
  })
})

describe('setSpaceReaction', () => {
  const messagePath = '/api/v1/spaces/space%201/threads/thread%2F1/messages/msg%3A1/reactions/'

  it('PUTs the encoded emoji path to add and returns the parsed chips', async () => {
    http.requestJson.mockResolvedValue({
      message_id: 'msg:1',
      reactions: [{ emoji: '\u{1F44D}', count: 1, reacted: true }, { emoji: 'bad' }],
    })
    const chips = await setSpaceReaction('space 1', 'thread/1', 'msg:1', '\u{1F44D}', true)
    expect(http.requestJson).toHaveBeenCalledTimes(1)
    expect(http.requestJson).toHaveBeenCalledWith(`${messagePath}${encodeURIComponent('\u{1F44D}')}`, { method: 'PUT' })
    expect(chips).toEqual([{ emoji: '\u{1F44D}', count: 1, reacted: true }])
  })

  it('DELETEs the same path to remove', async () => {
    http.requestJson.mockResolvedValue({ reactions: [] })
    const chips = await setSpaceReaction('space 1', 'thread/1', 'msg:1', '\u{2764}\u{FE0F}', false)
    expect(http.requestJson).toHaveBeenCalledWith(`${messagePath}${encodeURIComponent('\u{2764}\u{FE0F}')}`, {
      method: 'DELETE',
    })
    expect(chips).toEqual([])
  })

  it('percent-encodes an emoji so it can never add a path segment or query', async () => {
    http.requestJson.mockResolvedValue({})
    await setSpaceReaction('s', 't', 'm', '../x?y#z', true)
    const path = http.requestJson.mock.calls[0]?.[0] as string
    expect(path.endsWith('/reactions/..%2Fx%3Fy%23z')).toBe(true)
    expect(path.includes('?')).toBe(false)
  })

  it('answers no chips when the response has none, and lets an error through', async () => {
    http.requestJson.mockResolvedValueOnce(undefined)
    expect(await setSpaceReaction('s', 't', 'm', 'a', true)).toEqual([])
    const failure = Object.assign(new Error('nope'), { code: 'reaction_limit' })
    http.requestJson.mockRejectedValueOnce(failure)
    await expect(setSpaceReaction('s', 't', 'm', 'a', true)).rejects.toBe(failure)
  })
})
