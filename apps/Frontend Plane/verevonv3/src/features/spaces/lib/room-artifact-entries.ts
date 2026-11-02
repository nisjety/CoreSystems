/**
 * The Artifacts tab's list (WS07-S12): the server aggregate plus whatever this
 * browser's own stream has produced since, held in memory only.
 *
 * A live artifact is shown until the next refresh carries it. It is deduped on
 * `id` + `version` against the aggregate, which lists the LATEST persisted
 * version of each artifact: a live revision at or below that version is already
 * represented and is dropped; a newer one replaces the listed entry; one the
 * aggregate has not seen is added. Pure, no storage.
 */
import type { ChatArtifact } from '@/features/chat/components/chat-types'
import type { SpaceArtifactSummary } from '@/shared/api/spaces-client'
import type { RoomArtifactKind } from './room-artifacts'

export type RoomArtifactEntry = {
  readonly key: string
  readonly threadId: string
  readonly artifactId: string
  readonly kind: RoomArtifactKind
  readonly title: string
  readonly version: number
  readonly versionCount: number
  readonly sizeChars: number
  readonly agentName?: string
  readonly createdAt?: string
  /** True while the entry exists only in this browser's stream, not yet in the aggregate. */
  readonly live: boolean
}

function entryKey(threadId: string, artifactId: string): string {
  return `${threadId}\u0000${artifactId}`
}

function fromSummary(summary: SpaceArtifactSummary): RoomArtifactEntry {
  return {
    key: entryKey(summary.thread_id, summary.artifact_id),
    threadId: summary.thread_id,
    artifactId: summary.artifact_id,
    kind: summary.kind,
    title: summary.title,
    version: summary.version,
    versionCount: summary.version_count,
    sizeChars: summary.size_chars,
    ...(summary.agent_name ? { agentName: summary.agent_name } : {}),
    ...(summary.created_at ? { createdAt: summary.created_at } : {}),
    live: false,
  }
}

/**
 * `live` is the sender's own stream artifacts by thread. Returns the aggregate
 * entries (server order) with any newer live revision swapped in, then live-only
 * entries first, since they are the newest thing in the room.
 */
export function mergeRoomArtifactEntries(
  aggregate: readonly SpaceArtifactSummary[],
  live: ReadonlyMap<string, readonly ChatArtifact[]>,
): RoomArtifactEntry[] {
  const entries = aggregate.map(fromSummary)
  const byKey = new Map(entries.map((entry, index) => [entry.key, index] as const))
  const liveOnly: RoomArtifactEntry[] = []
  for (const [threadId, artifacts] of live) {
    // The newest live revision of each artifact decides; older ones add nothing.
    const newest = new Map<string, ChatArtifact>()
    for (const artifact of artifacts) {
      const held = newest.get(artifact.id)
      if (!held || artifact.version > held.version) newest.set(artifact.id, artifact)
    }
    for (const artifact of newest.values()) {
      if (artifact.kind !== 'document' && artifact.kind !== 'code' && artifact.kind !== 'html') continue
      const key = entryKey(threadId, artifact.id)
      const index = byKey.get(key)
      if (index !== undefined) {
        const listed = entries[index]
        if (!listed || artifact.version <= listed.version) continue
        entries[index] = {
          ...listed,
          kind: artifact.kind,
          title: artifact.title.trim(),
          version: artifact.version,
          versionCount: Math.max(listed.versionCount, artifact.version),
          sizeChars: artifact.content.length,
          live: true,
        }
        continue
      }
      liveOnly.push({
        key,
        threadId,
        artifactId: artifact.id,
        kind: artifact.kind,
        title: artifact.title.trim(),
        version: artifact.version,
        versionCount: artifact.version,
        sizeChars: artifact.content.length,
        live: true,
      })
    }
  }
  return [...liveOnly, ...entries]
}
