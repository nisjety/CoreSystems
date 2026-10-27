import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  MAX_ROOM_ARTIFACT_CONTENT_CHARS,
  MAX_ROOM_ARTIFACT_TITLE_CHARS,
  MAX_ROOM_ARTIFACTS_PER_TURN,
  roomArtifactFromEvent,
  roomArtifactsEnabled,
  roomArtifactsFromMetadata,
} from './room-artifacts'

afterEach(() => {
  vi.unstubAllEnvs()
})

const good = (over: Record<string, unknown> = {}) => ({
  id: 'a1',
  kind: 'document',
  title: 'Plan',
  content: 'body',
  version: 1,
  ...over,
})

describe('roomArtifactsFromMetadata (WS07-S11)', () => {
  it('keeps document, code and html with a string id, short title, bounded content and integer version', () => {
    const out = roomArtifactsFromMetadata([
      good({ id: 'd', kind: 'document' }),
      good({ id: 'c', kind: 'code', version: 3 }),
      good({ id: 'h', kind: 'html', version: 2 }),
    ])
    expect(out.map((a) => [a.id, a.kind, a.version])).toEqual([
      ['d', 'document', 1],
      ['c', 'code', 3],
      ['h', 'html', 2],
    ])
  })

  it('drops pdf, image, binary and unknown kinds so they never reach an unsandboxed viewer branch', () => {
    const out = roomArtifactsFromMetadata([
      good({ id: 'p', kind: 'pdf' }),
      good({ id: 'i', kind: 'image' }),
      good({ id: 'b', kind: 'binary' }),
      good({ id: 'x', kind: 'spreadsheet' }),
      good({ id: 'n', kind: 7 }),
      good({ id: 'keep' }),
    ])
    expect(out.map((a) => a.id)).toEqual(['keep'])
  })

  it('drops oversize titles and contents but keeps the exact limits', () => {
    const out = roomArtifactsFromMetadata([
      good({ id: 'title-max', title: 'x'.repeat(MAX_ROOM_ARTIFACT_TITLE_CHARS) }),
      good({ id: 'title-over', title: 'x'.repeat(MAX_ROOM_ARTIFACT_TITLE_CHARS + 1) }),
      good({ id: 'body-max', content: 'y'.repeat(MAX_ROOM_ARTIFACT_CONTENT_CHARS) }),
      good({ id: 'body-over', content: 'y'.repeat(MAX_ROOM_ARTIFACT_CONTENT_CHARS + 1) }),
    ])
    expect(out.map((a) => a.id)).toEqual(['title-max', 'body-max'])
  })

  it('drops non-string id, title and content and an empty id', () => {
    const out = roomArtifactsFromMetadata([
      good({ id: 5 }),
      good({ id: '   ' }),
      good({ id: 'no-title', title: undefined }),
      good({ id: 'num-title', title: 4 }),
      good({ id: 'no-content', content: null }),
      good({ id: 'keep' }),
    ])
    expect(out.map((a) => a.id)).toEqual(['keep'])
  })

  it('drops version 0, negatives, fractions, NaN, Infinity and non-numbers', () => {
    const out = roomArtifactsFromMetadata([
      good({ id: 'v0', version: 0 }),
      good({ id: 'vneg', version: -1 }),
      good({ id: 'vfrac', version: 1.5 }),
      good({ id: 'vnan', version: Number.NaN }),
      good({ id: 'vinf', version: Number.POSITIVE_INFINITY }),
      good({ id: 'vstr', version: '2' }),
      good({ id: 'vnone', version: undefined }),
      good({ id: 'v1', version: 1 }),
    ])
    expect(out.map((a) => a.id)).toEqual(['v1'])
  })

  it('trims the id and title', () => {
    const [a] = roomArtifactsFromMetadata([good({ id: '  a1 ', title: '  Plan  ' })])
    expect(a?.id).toBe('a1')
    expect(a?.title).toBe('Plan')
  })

  it('returns [] for non-arrays and skips non-object entries', () => {
    expect(roomArtifactsFromMetadata(undefined)).toEqual([])
    expect(roomArtifactsFromMetadata(null)).toEqual([])
    expect(roomArtifactsFromMetadata('x')).toEqual([])
    expect(roomArtifactsFromMetadata({ 0: good() })).toEqual([])
    expect(roomArtifactsFromMetadata([null, 3, 'x', good()]).length).toBe(1)
  })

  it('bounds a hostile payload at the per-turn cap', () => {
    const many = Array.from({ length: MAX_ROOM_ARTIFACTS_PER_TURN + 20 }, (_, i) => good({ id: `a${i}` }))
    expect(roomArtifactsFromMetadata(many)).toHaveLength(MAX_ROOM_ARTIFACTS_PER_TURN)
  })

  it('does not copy unknown fields onto the artifact', () => {
    const [a] = roomArtifactsFromMetadata([good({ url: 'https://evil.example', history: [1] })])
    expect(Object.keys(a ?? {}).sort()).toEqual(['content', 'id', 'kind', 'title', 'version'])
  })
})

describe('roomArtifactFromEvent (WS07-S11)', () => {
  it('maps a live event through the same gate', () => {
    expect(roomArtifactFromEvent({ id: 'a', kind: 'code', title: 'T', content: 'c', version: 2 })).toEqual({
      id: 'a',
      kind: 'code',
      title: 'T',
      content: 'c',
      version: 2,
    })
  })

  it('is undefined for pdf, image, missing fields and bad versions', () => {
    expect(roomArtifactFromEvent({ id: 'a', kind: 'pdf', title: 'T', content: 'c', version: 1 })).toBeUndefined()
    expect(roomArtifactFromEvent({ id: 'a', kind: 'image', title: 'T', content: 'c', version: 1 })).toBeUndefined()
    expect(roomArtifactFromEvent({ id: 'a', kind: 'html', title: 'T', content: 'c' })).toBeUndefined()
    expect(roomArtifactFromEvent({ kind: 'html', title: 'T', content: 'c', version: 1 })).toBeUndefined()
    expect(roomArtifactFromEvent({ id: 'a', kind: 'html', title: 'T', content: 'c', version: 0 })).toBeUndefined()
    expect(roomArtifactFromEvent({})).toBeUndefined()
  })
})

describe('roomArtifactsEnabled (WS07-S11)', () => {
  it('is on when the env override is "true", even outside dev', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
    vi.stubEnv('DEV', false)
    expect(roomArtifactsEnabled()).toBe(true)
  })

  it('is off when the env override is "false", even in dev', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'false')
    vi.stubEnv('DEV', true)
    expect(roomArtifactsEnabled()).toBe(false)
  })

  it('follows DEV when there is no override: off in production, on in dev', () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', '')
    vi.stubEnv('DEV', false)
    expect(roomArtifactsEnabled()).toBe(false)
    vi.stubEnv('DEV', true)
    expect(roomArtifactsEnabled()).toBe(true)
  })
})
