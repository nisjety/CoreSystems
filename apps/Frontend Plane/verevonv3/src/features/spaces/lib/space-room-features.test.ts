import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  clearSpaceRoomFeatures,
  contextFeatureOn,
  publishSpaceRoomFeatures,
  roomArchivedAt,
  roomIsArchived,
  spaceRoomFeature,
  spaceRoomFeatureWithDevOverride,
} from './space-room-features'
import { roomArtifactsEnabled } from './room-artifacts'
import { spaceConnectivityBannerEnabled, spaceSendIdempotencyEnabled } from './space-connectivity'
import { spaceThreadPaneEnabled } from './space-thread-deep-link'

afterEach(() => {
  vi.unstubAllEnvs()
  clearSpaceRoomFeatures()
})

describe('the room feature store (cluster 12)', () => {
  it('has every flag off before a context has been read', () => {
    vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', '')
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', '')
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', '')
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', '')
    expect(spaceRoomFeature('thread_pane')).toBe(false)
    expect(spaceThreadPaneEnabled()).toBe(false)
    expect(spaceSendIdempotencyEnabled()).toBe(false)
    expect(spaceConnectivityBannerEnabled()).toBe(false)
    expect(roomArtifactsEnabled()).toBe(false)
  })

  it('the four UI switches read the BFF context features', () => {
    vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', '')
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', '')
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', '')
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', '')
    publishSpaceRoomFeatures({
      thread_pane: true,
      send_idempotency: true,
      connectivity_banner: false,
      room_artifacts: true,
    })
    expect(spaceThreadPaneEnabled()).toBe(true)
    expect(spaceSendIdempotencyEnabled()).toBe(true)
    expect(spaceConnectivityBannerEnabled()).toBe(false)
    expect(roomArtifactsEnabled()).toBe(true)
  })

  it('a read straight after a publish is current, before Solid applies the signal write', () => {
    publishSpaceRoomFeatures({ thread_pane: true })
    expect(spaceRoomFeature('thread_pane')).toBe(true)
    publishSpaceRoomFeatures({ thread_pane: false })
    expect(spaceRoomFeature('thread_pane')).toBe(false)
  })

  it('only the literal true turns a flag on', () => {
    publishSpaceRoomFeatures({ thread_pane: 'true', send_idempotency: 1, room_artifacts: {} })
    expect(spaceRoomFeature('thread_pane')).toBe(false)
    expect(spaceRoomFeature('send_idempotency')).toBe(false)
    expect(spaceRoomFeature('room_artifacts')).toBe(false)
  })

  it('absent features (an older gateway) turn everything off again', () => {
    publishSpaceRoomFeatures({ thread_pane: true })
    publishSpaceRoomFeatures(undefined)
    expect(spaceRoomFeature('thread_pane')).toBe(false)
  })

  it('clearing forgets the flags, and the next publish is applied again', () => {
    publishSpaceRoomFeatures({ thread_pane: true })
    clearSpaceRoomFeatures()
    expect(spaceRoomFeature('thread_pane')).toBe(false)
    publishSpaceRoomFeatures({ thread_pane: true })
    expect(spaceRoomFeature('thread_pane')).toBe(true)
  })

  it('copies what it is given, so a later mutation by the caller changes nothing', () => {
    const features: Record<string, unknown> = { thread_pane: true }
    publishSpaceRoomFeatures(features)
    features.thread_pane = false
    expect(spaceRoomFeature('thread_pane')).toBe(true)
  })
})

describe('the dev override', () => {
  it('"true" and "false" win over the BFF in a dev build', () => {
    vi.stubEnv('DEV', true)
    publishSpaceRoomFeatures({ thread_pane: false, room_artifacts: true })
    expect(spaceRoomFeatureWithDevOverride('thread_pane', 'true')).toBe(true)
    expect(spaceRoomFeatureWithDevOverride('room_artifacts', 'false')).toBe(false)
  })

  it('any other value leaves the BFF in charge', () => {
    vi.stubEnv('DEV', true)
    publishSpaceRoomFeatures({ thread_pane: true })
    expect(spaceRoomFeatureWithDevOverride('thread_pane', '')).toBe(true)
    expect(spaceRoomFeatureWithDevOverride('thread_pane', undefined)).toBe(true)
    expect(spaceRoomFeatureWithDevOverride('thread_pane', 'yes')).toBe(true)
  })

  it('is ignored in a production build: the BFF alone decides', () => {
    vi.stubEnv('DEV', false)
    publishSpaceRoomFeatures({ thread_pane: false, room_artifacts: true })
    expect(spaceRoomFeatureWithDevOverride('thread_pane', 'true')).toBe(false)
    expect(spaceRoomFeatureWithDevOverride('room_artifacts', 'false')).toBe(true)
  })
})

describe('contextFeatureOn', () => {
  it('reads one flag off a context, off when absent', () => {
    expect(contextFeatureOn({ features: { room_archive: true } }, 'room_archive')).toBe(true)
    expect(contextFeatureOn({ features: { room_archive: false } }, 'room_archive')).toBe(false)
    expect(contextFeatureOn({ features: {} }, 'room_archive')).toBe(false)
    expect(contextFeatureOn({}, 'room_archive')).toBe(false)
    expect(contextFeatureOn(undefined, 'room_archive')).toBe(false)
  })
})

describe('roomIsArchived and roomArchivedAt', () => {
  it('is archived when the context or Control says so', () => {
    expect(roomIsArchived({ archived: true })).toBe(true)
    expect(roomIsArchived({ membership: { state: 'archived' } })).toBe(true)
    expect(roomIsArchived({ archived: false, membership: { state: 'active' } })).toBe(false)
  })

  it('is active when an older gateway sends neither', () => {
    expect(roomIsArchived({})).toBe(false)
    expect(roomIsArchived({ membership: {} })).toBe(false)
    expect(roomIsArchived(undefined)).toBe(false)
    expect(roomIsArchived(null)).toBe(false)
  })

  it('names when it was archived only for an archived room', () => {
    expect(roomArchivedAt({ archived: true, membership: { archived_at: ' 2026-10-31T09:30:00Z ' } })).toBe('2026-10-31T09:30:00Z')
    expect(roomArchivedAt({ archived: true, membership: { archived_at: '' } })).toBeNull()
    expect(roomArchivedAt({ archived: false, membership: { archived_at: '2026-10-31T09:30:00Z' } })).toBeNull()
  })
})
