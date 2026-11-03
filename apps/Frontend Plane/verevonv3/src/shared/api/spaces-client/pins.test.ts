import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('../http', () => http)

const { pinSpaceThread, spacePinsFrom, unpinSpaceThread } = await import('./pins')

beforeEach(() => http.requestJson.mockReset())
afterEach(() => vi.restoreAllMocks())

describe('spacePinsFrom', () => {
  it('keeps well-formed pins in the order given', () => {
    expect(
      spacePinsFrom([
        { thread_id: 't2', pinned_by: 'user-2', pinned_at: 20 },
        { thread_id: 't1', pinned_by: 'user-1', pinned_at: 10 },
      ]),
    ).toEqual([
      { thread_id: 't2', pinned_by: 'user-2', pinned_at: 20 },
      { thread_id: 't1', pinned_by: 'user-1', pinned_at: 10 },
    ])
  })

  it('drops malformed rows and keeps the good ones', () => {
    expect(
      spacePinsFrom([
        null,
        'x',
        7,
        {},
        { thread_id: '', pinned_by: 'u', pinned_at: 1 },
        { thread_id: 't', pinned_by: 5, pinned_at: 1 },
        { thread_id: 't', pinned_by: 'u', pinned_at: '1' },
        { thread_id: 't', pinned_by: 'u', pinned_at: Number.NaN },
        { thread_id: 't', pinned_by: 'u', pinned_at: Number.POSITIVE_INFINITY },
        { thread_id: 'ok', pinned_by: 'u', pinned_at: 3, extra: 'dropped' },
      ]),
    ).toEqual([{ thread_id: 'ok', pinned_by: 'u', pinned_at: 3 }])
  })

  it('treats absent or non-array input as no pins', () => {
    for (const raw of [undefined, null, 'pins', 5, {}, { thread_id: 't' }]) {
      expect(spacePinsFrom(raw)).toEqual([])
    }
  })
})

describe('pinSpaceThread / unpinSpaceThread', () => {
  it('PUTs the encoded pin URL and returns the pin time', async () => {
    http.requestJson.mockResolvedValue({ pinned: true, pinned_at: 1760000000000 })
    await expect(pinSpaceThread('space/1', 'thread 1')).resolves.toEqual({ pinned_at: 1760000000000 })
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/space%2F1/pins/thread%201', { method: 'PUT' })
  })

  it('returns a null time when the response carries none', async () => {
    http.requestJson.mockResolvedValue({ pinned: true })
    await expect(pinSpaceThread('s', 't')).resolves.toEqual({ pinned_at: null })
    http.requestJson.mockResolvedValue({ pinned_at: 'soon' })
    await expect(pinSpaceThread('s', 't')).resolves.toEqual({ pinned_at: null })
  })

  it('DELETEs the encoded pin URL', async () => {
    http.requestJson.mockResolvedValue({ pinned: false })
    await expect(unpinSpaceThread('space-1', 'thread-1')).resolves.toBeUndefined()
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/space-1/pins/thread-1', { method: 'DELETE' })
  })
})
