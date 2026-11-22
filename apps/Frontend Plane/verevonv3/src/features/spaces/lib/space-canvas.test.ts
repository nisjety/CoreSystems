import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceCanvasVersion, SpaceRosterMember } from '@/shared/api/spaces-client'

import {
  CANVAS_CLEAR_CONTENT,
  canOfferCanvasClear,
  canvasChangeNote,
  canvasCompareOrder,
  canvasLeavesRoom,
  canvasTabEnabled,
  canvasVersionMeta,
  sameCanvasText,
  toggleCompareSelection,
} from './space-canvas'
import { DM_SPACE_FEATURE } from './space-kind'
import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from './space-room-features'

const en = (_noText: string, enText: string) => enText
const no = (noText: string, _enText: string) => noText

const roster: readonly SpaceRosterMember[] = [
  { subject_type: 'user', subject_id: 'subject-kari', role: 'owner', revision: 1, display_name: 'Kari' },
  { subject_type: 'user', subject_id: 'subject-ola', role: 'editor', revision: 1, display_name: 'Ola' },
]

function version(overrides: Partial<SpaceCanvasVersion> = {}): SpaceCanvasVersion {
  return {
    versionId: 'v-12',
    versionNo: 12,
    parentVersionId: 'v-11',
    authorKind: 'user',
    authorId: 'subject-kari',
    onBehalfOf: null,
    runId: null,
    changeKind: 'edit',
    restoredFromVersionId: null,
    summary: null,
    contentBytes: 120,
    contentSha256: 'hash-12',
    createdAt: '2026-11-18T14:02:00.000Z',
    ...overrides,
  }
}

afterEach(() => {
  clearSpaceRoomFeatures()
})

describe('WS19-S5 canvas tab and version line', () => {
  it('canvasTabEnabled is true only for features.canvas exactly true', () => {
    expect(canvasTabEnabled()).toBe(false)
    publishSpaceRoomFeatures({ canvas: true })
    expect(canvasTabEnabled()).toBe(true)

    const notExactlyTrue: readonly unknown[] = ['true', 1, 'yes', {}, false]
    for (const canvas of notExactlyTrue) {
      publishSpaceRoomFeatures({ canvas })
      expect(canvasTabEnabled()).toBe(false)
    }
    publishSpaceRoomFeatures({ other_flag: true })
    expect(canvasTabEnabled()).toBe(false)
  })

  // Control refuses a canvas in a dm or group_dm Space (D-8), so the tab must not be offered where it can only refuse.
  it('canvasTabEnabled is false in a direct message even while features.canvas is on', () => {
    publishSpaceRoomFeatures({ canvas: true, [DM_SPACE_FEATURE]: true })
    expect(canvasTabEnabled()).toBe(false)
    publishSpaceRoomFeatures({ canvas: true, [DM_SPACE_FEATURE]: false })
    expect(canvasTabEnabled()).toBe(true)
    publishSpaceRoomFeatures({ canvas: true })
    expect(canvasTabEnabled()).toBe(true)
  })

  it('canvasVersionMeta names a member, an agent run with its asker, an agent run with no asker, and falls back to a member for an unknown subject; a subject id never appears in the line', () => {
    const member = canvasVersionMeta(version(), roster, '14:02', en)
    expect(member).toBe('Version 12, saved by Kari, 14:02')
    expect(canvasVersionMeta(version(), roster, '14:02', no)).toBe('Versjon 12, lagret av Kari, 14:02')

    const askedBy = canvasVersionMeta(
      version({
        authorKind: 'agent',
        authorId: 'run:1a2b3c4d5e6f7a8b',
        onBehalfOf: 'subject-ola',
        runId: '1a2b3c4d5e6f7a8b',
      }),
      roster,
      '14:02',
      en,
    )
    expect(askedBy).toBe('Version 12, agent run 1a2b3c4d asked by Ola, 14:02')

    const noAsker = canvasVersionMeta(
      version({
        authorKind: 'agent',
        authorId: 'run:1a2b3c4d5e6f7a8b',
        onBehalfOf: null,
        runId: '1a2b3c4d5e6f7a8b',
      }),
      roster,
      '14:02',
      en,
    )
    expect(noAsker).toBe('Version 12, agent run 1a2b3c4d, 14:02')

    const unknownSubject = canvasVersionMeta(version({ authorId: 'subject-gone' }), roster, '14:02', en)
    expect(unknownSubject).toBe('Version 12, saved by a member, 14:02')

    for (const line of [member, askedBy, noAsker, unknownSubject]) {
      expect(line).not.toContain('subject-')
      expect(line).not.toContain('run:')
    }
  })
})

describe('WS19-S7 canvas leave guard, compare and restore line', () => {
  it('canvasLeavesRoom is false for a hash, a tab or a message link in the same Space (percent-encoded id too) and true for another Space, a path outside the Spaces and a numeric target', () => {
    expect(canvasLeavesRoom('/spaces/room-a#lerret', 'room-a')).toBe(false)
    expect(canvasLeavesRoom('/spaces/room-a#chat', 'room-a')).toBe(false)
    expect(canvasLeavesRoom('/spaces/room-a?thread_id=thread-1&m=msg-9#chat', 'room-a')).toBe(false)
    expect(canvasLeavesRoom('/spaces/Room%20A?thread_id=thread-1#chat', 'Room A')).toBe(false)

    expect(canvasLeavesRoom('/spaces/room-b#lerret', 'room-a')).toBe(true)
    expect(canvasLeavesRoom('/spaces/Room%20B#lerret', 'Room A')).toBe(true)
    expect(canvasLeavesRoom('/chat/thread-1', 'room-a')).toBe(true)
    expect(canvasLeavesRoom('/settings', 'room-a')).toBe(true)
    expect(canvasLeavesRoom(-1, 'room-a')).toBe(true)
  })

  it('sameCanvasText is false for two empty hashes', () => {
    expect(sameCanvasText({ contentSha256: '' }, { contentSha256: '' })).toBe(false)
    expect(sameCanvasText({ contentSha256: 'hash-a' }, { contentSha256: 'hash-a' })).toBe(true)
    expect(sameCanvasText({ contentSha256: 'hash-a' }, { contentSha256: 'hash-b' })).toBe(false)
  })

  it('toggleCompareSelection keeps two, drops the oldest and un-picks', () => {
    const v1 = { versionId: 'v-1' }
    const v2 = { versionId: 'v-2' }
    const v3 = { versionId: 'v-3' }
    const ids = (selected: readonly { readonly versionId: string }[]) => selected.map((item) => item.versionId)

    let selected: readonly { readonly versionId: string }[] = []
    selected = toggleCompareSelection(selected, v1)
    expect(ids(selected)).toEqual(['v-1'])

    selected = toggleCompareSelection(selected, v2)
    expect(ids(selected)).toEqual(['v-1', 'v-2'])

    // A third pick drops the oldest (v-1) and keeps the two newest picks.
    selected = toggleCompareSelection(selected, v3)
    expect(ids(selected)).toEqual(['v-2', 'v-3'])

    // Picking a selected version again takes it back out.
    selected = toggleCompareSelection(selected, v2)
    expect(ids(selected)).toEqual(['v-3'])
  })

  it('canvasCompareOrder puts the lower number first', () => {
    const newer = { versionNo: 7 }
    const older = { versionNo: 4 }
    expect(canvasCompareOrder(newer, older)).toEqual([older, newer])
    expect(canvasCompareOrder(older, newer)).toEqual([older, newer])
  })

  it('canvasChangeNote names a listed source version, falls back to a generic line and is null for an edit', () => {
    const listed = new Map<string, number>([['v-2', 2]])
    const versionNoOf = (versionId: string) => listed.get(versionId)

    const restoredFromListed = version({ changeKind: 'restore', restoredFromVersionId: 'v-2' })
    expect(canvasChangeNote(restoredFromListed, versionNoOf, en)).toBe('Restored from version 2')

    const restoredFromUnlisted = version({ changeKind: 'restore', restoredFromVersionId: 'v-9' })
    expect(canvasChangeNote(restoredFromUnlisted, versionNoOf, en)).toBe('Restored from an earlier version')

    const restoredWithoutSource = version({ changeKind: 'restore', restoredFromVersionId: null })
    expect(canvasChangeNote(restoredWithoutSource, versionNoOf, en)).toBe('Restored from an earlier version')

    const edit = version({ changeKind: 'edit', restoredFromVersionId: null })
    expect(canvasChangeNote(edit, versionNoOf, en)).toBeNull()
  })
})

describe('canOfferCanvasClear (D22aq)', () => {
  it('offers the clear to a room manager or owner only, and clears with one newline', () => {
    expect(canOfferCanvasClear('manager')).toBe(true)
    expect(canOfferCanvasClear('owner')).toBe(true)
    for (const role of ['editor', 'viewer', 'member', '', undefined]) expect(canOfferCanvasClear(role)).toBe(false)
    expect(CANVAS_CLEAR_CONTENT).toBe(String.fromCharCode(10))
  })
})

