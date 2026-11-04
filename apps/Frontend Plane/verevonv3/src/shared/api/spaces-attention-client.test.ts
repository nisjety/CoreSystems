import { beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))

vi.mock('./http', () => ({ requestJson: http.requestJson }))

const {
  attentionFromWire,
  getSpaceNotify,
  getSpacesAttention,
  isNotifyLevel,
  markAllSpacesRead,
  markSpaceUnread,
  setSpaceNotifyLevel,
} = await import('./spaces-attention-client')

/** The JSON body a request was sent with. */
function sentBody(callIndex = 0): Record<string, unknown> {
  const init = http.requestJson.mock.calls[callIndex]?.[1] as RequestInit
  return JSON.parse(String(init.body)) as Record<string, unknown>
}

beforeEach(() => {
  http.requestJson.mockReset()
})

describe('attentionFromWire', () => {
  it('reads each room and the totals as the gateway sent them, in camel case', () => {
    const parsed = attentionFromWire({
      generated_at: 1000,
      mode: 'live',
      rooms: [
        {
          space_ref: ' room-a ',
          unread_threads: 2,
          unread_capped: true,
          mentions: 1,
          needs_you: 1,
          working: 3,
          last_activity_at: 900,
          last_read_at: 800,
          manual_unread: true,
          level: 'all',
          badge: { kind: 'count', n: 2 },
          gaps: ['session_core'],
        },
      ],
      totals: { badge: 2, unread_rooms: 1, other_open: 4 },
      truncated: true,
    })

    expect(parsed.generatedAt).toBe(1000)
    expect(parsed.mode).toBe('live')
    expect(parsed.truncated).toBe(true)
    expect(parsed.totals).toEqual({ badge: 2, unreadRooms: 1, otherOpen: 4 })
    expect(parsed.rooms).toEqual([
      {
        spaceRef: 'room-a',
        unreadThreads: 2,
        unreadCapped: true,
        mentions: 1,
        needsYou: 1,
        working: 3,
        lastActivityAt: 900,
        lastReadAt: 800,
        manualUnread: true,
        level: 'all',
        badge: { kind: 'count', n: 2 },
        gaps: ['session_core'],
      },
    ])
  })

  it('drops a room it cannot name, rather than guessing at one', () => {
    const parsed = attentionFromWire({
      rooms: [{ badge: { kind: 'count', n: 5 } }, { space_ref: '   ' }, 'not a room', null, { space_ref: 'room-b' }],
    })
    expect(parsed.rooms.map((room) => room.spaceRef)).toEqual(['room-b'])
  })

  it('reads an unknown level as the default, and never as zero', () => {
    const [room] = attentionFromWire({ rooms: [{ space_ref: 'room-c', level: 'loudest' }] }).rooms
    expect(room?.level).toBe('mentions')
    expect(room?.badge).toEqual({ kind: 'none', n: 0 })
    expect(room?.unreadThreads).toBe(0)
  })

  it('treats a count of zero, a negative count and an unknown kind as no badge', () => {
    const rooms = attentionFromWire({
      rooms: [
        { space_ref: 'zero', badge: { kind: 'count', n: 0 } },
        { space_ref: 'negative', badge: { kind: 'count', n: -4 } },
        { space_ref: 'unknown', badge: { kind: 'sparkle', n: 9 } },
        { space_ref: 'dot', badge: { kind: 'dot', n: 7 } },
      ],
    }).rooms
    expect(rooms.map((room) => room.badge)).toEqual([
      { kind: 'none', n: 0 },
      { kind: 'none', n: 0 },
      { kind: 'none', n: 0 },
      { kind: 'dot', n: 0 },
    ])
  })

  it('reads the stale sources from the envelope meta, and keeps the zdr_empty mode', () => {
    const parsed = attentionFromWire({ mode: 'zdr_empty', rooms: [], totals: { badge: 0 } }, { stale_sources: ['application'] })
    expect(parsed.mode).toBe('zdr_empty')
    expect(parsed.staleSources).toEqual(['application'])
  })

  it('answers with an empty, live aggregate for a body it does not recognise', () => {
    expect(attentionFromWire(undefined)).toEqual({
      generatedAt: null,
      mode: 'live',
      rooms: [],
      totals: { badge: 0, unreadRooms: 0, otherOpen: 0 },
      truncated: false,
      staleSources: [],
    })
  })
})

describe('getSpacesAttention', () => {
  it('asks for every room the caller belongs to when no refs are given', async () => {
    http.requestJson.mockResolvedValue({ mode: 'live', rooms: [], totals: { badge: 0 } })
    await getSpacesAttention()
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/attention')
  })

  it('narrows to the refs given, encoded, and never sends more than 60', async () => {
    http.requestJson.mockResolvedValue({ mode: 'live', rooms: [], totals: { badge: 0 } })
    await getSpacesAttention(['a', 'b c'])
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/attention?refs=a%2Cb%20c')

    const many = Array.from({ length: 61 }, (_, index) => `r${index}`)
    await getSpacesAttention(many)
    const url = String(http.requestJson.mock.calls[1]?.[0])
    expect(decodeURIComponent(url.split('refs=')[1] ?? '').split(',')).toHaveLength(60)
  })

  it('carries the stale sources the gateway mirrors into its data', async () => {
    http.requestJson.mockResolvedValue({ mode: 'live', rooms: [], totals: { badge: 0 }, stale_sources: ['session_core'] })
    const parsed = await getSpacesAttention()
    expect(parsed.staleSources).toEqual(['session_core'])
  })
})

describe('markAllSpacesRead', () => {
  it('posts an empty body to mark every room, and returns what the gateway counted', async () => {
    http.requestJson.mockResolvedValue({ rooms_marked: 3, items_resolved: 2, marked_at: 777 })
    const result = await markAllSpacesRead()
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/attention/read-all', { method: 'POST', body: '{}' })
    expect(result).toEqual({ roomsMarked: 3, itemsResolved: 2, markedAt: 777 })
  })

  it('sends only the rooms and kinds it is given, at most 60 rooms', async () => {
    http.requestJson.mockResolvedValue({})
    await markAllSpacesRead({ spaceRefs: Array.from({ length: 61 }, (_, index) => `r${index}`), kinds: ['mention'] })
    const body = sentBody()
    expect(body.space_refs).toHaveLength(60)
    expect(body.kinds).toEqual(['mention'])
  })

  it('reports zero for a body without counts, rather than inventing them', async () => {
    http.requestJson.mockResolvedValue({})
    await expect(markAllSpacesRead()).resolves.toEqual({ roomsMarked: 0, itemsResolved: 0, markedAt: null })
  })
})

describe('markSpaceUnread', () => {
  it('moves the position to a whole millisecond, with a key for this one request', async () => {
    http.requestJson.mockResolvedValue({ last_read_at: 1234, manual_unread: true })
    const result = await markSpaceUnread('room a/b', 1234.9)
    expect(http.requestJson).toHaveBeenCalledWith(
      '/api/v1/spaces/room%20a%2Fb/unread',
      expect.objectContaining({ method: 'POST' }),
    )
    const body = sentBody()
    expect(body.before_ms).toBe(1234)
    expect(String(body.idempotency_key)).toMatch(/^unread-/)
    expect(result).toEqual({ lastReadAt: 1234, manualUnread: true })
  })

  it('uses a fresh key for each request', async () => {
    http.requestJson.mockResolvedValue({ last_read_at: 1, manual_unread: true })
    await markSpaceUnread('room-1', 10)
    await markSpaceUnread('room-1', 10)
    expect(sentBody(0).idempotency_key).not.toBe(sentBody(1).idempotency_key)
  })
})

describe('room notification level', () => {
  it('recognises the three levels and nothing else', () => {
    expect(['all', 'mentions', 'off'].every(isNotifyLevel)).toBe(true)
    expect(isNotifyLevel('ALL')).toBe(false)
    expect(isNotifyLevel(undefined)).toBe(false)
  })

  it('reads the level and the preview setting, with mentions as the default level', async () => {
    http.requestJson.mockResolvedValue({ level: 'nonsense', hide_preview: true })
    await expect(getSpaceNotify('room-1')).resolves.toEqual({ level: 'mentions', hidePreview: true })
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/room-1/notify')
  })

  it('sends only the level when no preview setting is given', async () => {
    http.requestJson.mockResolvedValue({ level: 'off', hide_preview: false })
    const result = await setSpaceNotifyLevel('room-1', 'off')
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/room-1/notify', { method: 'PUT', body: '{"level":"off"}' })
    expect(result).toEqual({ level: 'off', hidePreview: false })
  })

  it('sends the preview setting when one is given', async () => {
    http.requestJson.mockResolvedValue({ level: 'all', hide_preview: true })
    await setSpaceNotifyLevel('room 1', 'all', true)
    expect(http.requestJson).toHaveBeenCalledWith(
      '/api/v1/spaces/room%201/notify',
      { method: 'PUT', body: '{"level":"all","hide_preview":true}' },
    )
  })

  it('answers with the level it asked for when the gateway does not repeat it', async () => {
    http.requestJson.mockResolvedValue({})
    await expect(setSpaceNotifyLevel('room-1', 'mentions')).resolves.toEqual({ level: 'mentions', hidePreview: false })
  })
})
