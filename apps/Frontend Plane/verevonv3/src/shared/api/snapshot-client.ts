import { z } from 'zod'
import {
  threadGrantSnapshotInput,
  threadRevokeSnapshotInput,
  threadShareSnapshotInput,
} from '@/shared/actions/chat-history'
import { ApiError, requestJson } from '@/shared/api/http'
import {
  readThreadExport,
  THREAD_EXPORT_FORMAT,
  THREAD_EXPORT_VERSION,
  type ThreadExport,
} from '@/features/chat/replay/thread-export-schema'

// Governed conversation snapshots (W4-SH-6, F-t3-046). The browser sends a thread id, a snapshot id and, for a
// grant, the one member to share with. The owner and the organisation are the session's: the gateway sets them
// and never takes either from here. Every call rejects with the server's typed refusal (`ApiError`), among them
// `chat_snapshots_disabled` while the gateway's switch is off, `thread_not_exportable` (with `details.reason`),
// `snapshot_limit_reached`, `grantee_not_in_organization`, `snapshot_not_found` and the zero-retention codes.

/** Whether this build offers conversation snapshots (`VITE_CHAT_SNAPSHOTS`, read with the strict literal `true`). */
export const chatSnapshotsEnabled = () => import.meta.env.VITE_CHAT_SNAPSHOTS === 'true'

/** One snapshot as its owner sees it, without the copy it holds. */
export type ChatSnapshot = {
  id: string
  threadId: string
  title: string
  createdAt: string
  /** Null while the snapshot is live. */
  revokedAt: string | null
}

/** One member a snapshot is shared with. */
export type ChatSnapshotGrant = {
  granteeUserId: string
  /** Null when Control reported no time. */
  grantedAt: string | null
}

/** A snapshot a viewer opened: the snapshot, and the redacted copy as a replayable conversation. */
export type SharedChatSnapshot = {
  snapshot: ChatSnapshot
  conversation: ThreadExport
}

const snapshotWire = z.object({
  id: z.string().min(1),
  thread_id: z.string().default(''),
  title: z.string().default(''),
  created_at: z.string().default(''),
  revoked_at: z.string().nullable().optional(),
})

const grantWire = z.object({
  grantee_user_id: z.string().min(1),
  granted_at: z.string().nullable().optional(),
})

function snapshotFrom(raw: unknown): ChatSnapshot {
  const parsed = snapshotWire.safeParse(raw)
  if (!parsed.success) throw malformed()
  return {
    id: parsed.data.id,
    threadId: parsed.data.thread_id,
    title: parsed.data.title,
    createdAt: parsed.data.created_at,
    revokedAt: parsed.data.revoked_at ?? null,
  }
}

function grantFrom(raw: unknown): ChatSnapshotGrant {
  const parsed = grantWire.safeParse(raw)
  if (!parsed.success) throw malformed()
  return { granteeUserId: parsed.data.grantee_user_id, grantedAt: parsed.data.granted_at ?? null }
}

function malformed(): ApiError {
  return new ApiError('The snapshot answer could not be read.', 502, 'thread_share_failed')
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : {}
}

/** Take a snapshot of one of the caller's own conversations. Nothing is shared by taking it. */
export async function createChatSnapshot(threadId: string): Promise<ChatSnapshot> {
  const request = threadShareSnapshotInput.parse({ threadId })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/threads/${encodeURIComponent(request.threadId)}/snapshots`, {
      method: 'POST',
    }),
  )
  return snapshotFrom(raw.snapshot)
}

/** The caller's live snapshots of one conversation, newest first. */
export async function listChatSnapshots(threadId: string): Promise<ChatSnapshot[]> {
  const request = threadShareSnapshotInput.parse({ threadId })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/threads/${encodeURIComponent(request.threadId)}/snapshots`),
  )
  return Array.isArray(raw.snapshots) ? raw.snapshots.map(snapshotFrom) : []
}

/** Revoke one snapshot: its copy is deleted and so is every member grant on it. */
export async function revokeChatSnapshot(snapshotId: string): Promise<ChatSnapshot> {
  const request = threadRevokeSnapshotInput.parse({ snapshotId })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/snapshots/${encodeURIComponent(request.snapshotId)}`, {
      method: 'DELETE',
    }),
  )
  return snapshotFrom(raw.snapshot)
}

/** The members one live snapshot is shared with. */
export async function listChatSnapshotGrants(snapshotId: string): Promise<ChatSnapshotGrant[]> {
  const request = threadRevokeSnapshotInput.parse({ snapshotId })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/snapshots/${encodeURIComponent(request.snapshotId)}/grants`),
  )
  return Array.isArray(raw.grants) ? raw.grants.map(grantFrom) : []
}

/** Give one member of the organisation a view of a live snapshot. The body names the member and nothing else. */
export async function grantChatSnapshot(snapshotId: string, granteeUserId: string): Promise<ChatSnapshotGrant> {
  const request = threadGrantSnapshotInput.parse({ snapshotId, granteeUserId, change: 'add' })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/snapshots/${encodeURIComponent(request.snapshotId)}/grants`, {
      method: 'POST',
      body: JSON.stringify({ grantee_user_id: request.granteeUserId }),
    }),
  )
  return grantFrom(raw.grant)
}

/** Take a member's view of a snapshot away. */
export async function removeChatSnapshotGrant(snapshotId: string, granteeUserId: string): Promise<void> {
  const request = threadGrantSnapshotInput.parse({ snapshotId, granteeUserId, change: 'remove' })
  await requestJson<unknown>(`/api/v1/chat/snapshots/${encodeURIComponent(request.snapshotId)}/grants`, {
    method: 'DELETE',
    body: JSON.stringify({ grantee_user_id: request.granteeUserId }),
  })
}

/**
 * Read a snapshot the caller owns, or one they were granted a view of. The copy is static text in the shape of a
 * redacted conversation export, so it is checked by the same reader the replay uses and shown by the same view.
 * A snapshot that is revoked, in another organisation, or not shared with the caller reads as `snapshot_not_found`,
 * whichever it is.
 */
export async function readSharedChatSnapshot(snapshotId: string): Promise<SharedChatSnapshot> {
  const request = threadRevokeSnapshotInput.parse({ snapshotId })
  const raw = record(
    await requestJson<unknown>(`/api/v1/chat/shared-snapshots/${encodeURIComponent(request.snapshotId)}`),
  )
  const snapshot = snapshotFrom(raw.snapshot)
  const reading = readThreadExport(
    JSON.stringify({
      ...record(raw.body),
      format: THREAD_EXPORT_FORMAT,
      version: THREAD_EXPORT_VERSION,
      exported_at: snapshot.createdAt,
      profile: 'redacted',
    }),
  )
  if (reading.status !== 'ok') throw malformed()
  return { snapshot, conversation: reading.document }
}
