import { describe, expect, it } from 'vitest'

import type { ChatArtifact } from '@/features/chat/components/chat-types'
import type { SpaceArtifactSummary } from '@/shared/api/spaces-client'

import { mergeRoomArtifactEntries } from './room-artifact-entries'

const summary = (over: Partial<SpaceArtifactSummary> = {}): SpaceArtifactSummary => ({
  thread_id: 't1',
  message_id: 'm1',
  artifact_id: 'a1',
  kind: 'document',
  title: 'Plan',
  version: 2,
  version_count: 2,
  size_chars: 10,
  ...over,
})

const live = (over: Partial<ChatArtifact> = {}): ChatArtifact => ({
  id: 'a1',
  kind: 'document',
  title: 'Plan',
  content: 'live',
  version: 2,
  ...over,
})

const liveMap = (entries: Array<[string, ChatArtifact[]]>) => new Map(entries)

describe('mergeRoomArtifactEntries (WS07-S12)', () => {
  it('keeps the aggregate order and marks entries as not live', () => {
    const out = mergeRoomArtifactEntries(
      [summary({ artifact_id: 'b' }), summary({ artifact_id: 'a' }), summary({ artifact_id: 'c' })],
      new Map(),
    )
    expect(out.map((e) => e.artifactId)).toEqual(['b', 'a', 'c'])
    expect(out.every((e) => e.live === false)).toBe(true)
  })

  it('maps summary fields and omits absent agent name and created_at', () => {
    const [bare, full] = mergeRoomArtifactEntries(
      [summary(), summary({ artifact_id: 'z', agent_name: 'Hjelpen', created_at: '2026-10-01T10:00:00Z' })],
      new Map(),
    )
    expect(bare).not.toHaveProperty('agentName')
    expect(bare).not.toHaveProperty('createdAt')
    expect(full?.agentName).toBe('Hjelpen')
    expect(full?.createdAt).toBe('2026-10-01T10:00:00Z')
  })

  it('drops a live revision at or below the listed version', () => {
    const same = mergeRoomArtifactEntries([summary({ version: 2 })], liveMap([['t1', [live({ version: 2 })]]]))
    expect(same).toHaveLength(1)
    expect(same[0]?.live).toBe(false)
    const lower = mergeRoomArtifactEntries([summary({ version: 2 })], liveMap([['t1', [live({ version: 1 })]]]))
    expect(lower).toHaveLength(1)
    expect(lower[0]?.live).toBe(false)
    expect(lower[0]?.version).toBe(2)
  })

  it('lets a newer live revision replace the listed entry in place', () => {
    const out = mergeRoomArtifactEntries(
      [summary({ artifact_id: 'first' }), summary({ artifact_id: 'a1', version: 2, version_count: 2 })],
      liveMap([['t1', [live({ version: 4, title: ' Plan v4 ', content: 'twelve chars' })]]]),
    )
    expect(out).toHaveLength(2)
    expect(out[1]).toMatchObject({
      artifactId: 'a1',
      version: 4,
      versionCount: 4,
      title: 'Plan v4',
      sizeChars: 'twelve chars'.length,
      live: true,
    })
    expect(out[0]?.artifactId).toBe('first')
  })

  it('decides on the newest live revision of an artifact, not on emission order', () => {
    const out = mergeRoomArtifactEntries(
      [summary({ version: 2 })],
      liveMap([['t1', [live({ version: 5, content: 'new' }), live({ version: 3, content: 'old' })]]]),
    )
    expect(out[0]?.version).toBe(5)
  })

  it('puts live-only entries first, ahead of the aggregate', () => {
    const out = mergeRoomArtifactEntries(
      [summary({ artifact_id: 'a' }), summary({ artifact_id: 'b' })],
      liveMap([['t9', [live({ id: 'fresh', version: 1 })]]]),
    )
    expect(out.map((e) => e.artifactId)).toEqual(['fresh', 'a', 'b'])
    expect(out[0]).toMatchObject({ threadId: 't9', live: true, version: 1, versionCount: 1 })
  })

  it('keys by thread: the same artifact id in another thread is a separate entry', () => {
    const out = mergeRoomArtifactEntries([summary({ thread_id: 't1' })], liveMap([['t2', [live({ version: 1 })]]]))
    expect(out).toHaveLength(2)
    expect(out[0]).toMatchObject({ threadId: 't2', live: true })
    expect(out[1]).toMatchObject({ threadId: 't1', live: false })
  })

  it('ignores live artifacts of kinds the room does not list', () => {
    const out = mergeRoomArtifactEntries(
      [],
      liveMap([['t1', [live({ id: 'p', kind: 'pdf' }), live({ id: 'i', kind: 'image' }), live({ id: 'k', kind: 'code' })]]]),
    )
    expect(out.map((e) => e.artifactId)).toEqual(['k'])
  })

  it('is [] with nothing in either input', () => {
    expect(mergeRoomArtifactEntries([], new Map())).toEqual([])
  })
})
