/**
 * The artifacts this browser's own stream produced, by thread (WS07-S11/S12).
 *
 * One store per open room, created by `SpacePage` and shared by the timeline
 * (chips) and the Artifacts tab (list). Memory only: the room's record is the
 * transcript, and a live artifact is shown only until a read carries it. Never
 * written to localStorage, sessionStorage, IndexedDB or cookies.
 */
import { createSignal } from 'solid-js'

import type { ChatArtifact } from '@/features/chat/components/chat-types'

export type RoomLiveArtifacts = {
  /** Every held live artifact, by thread id. Reactive. */
  readonly all: () => ReadonlyMap<string, readonly ChatArtifact[]>
  readonly forThread: (threadId: string) => readonly ChatArtifact[]
  /** Hold one artifact from the live stream; the same `id` + `version` replaces itself. */
  readonly note: (artifact: ChatArtifact, threadId: string) => void
}

export function createRoomLiveArtifacts(): RoomLiveArtifacts {
  const [held, setHeld] = createSignal<ReadonlyMap<string, readonly ChatArtifact[]>>(new Map())
  return {
    all: held,
    forThread: (threadId) => held().get(threadId) ?? [],
    note: (artifact, threadId) => {
      setHeld((current) => {
        const next = new Map(current)
        const existing = next.get(threadId) ?? []
        next.set(threadId, [
          ...existing.filter((entry) => !(entry.id === artifact.id && entry.version === artifact.version)),
          artifact,
        ])
        return next
      })
    },
  }
}
