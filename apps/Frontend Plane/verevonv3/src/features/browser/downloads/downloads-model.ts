import { z } from 'zod'

import { executeAction } from '@/shared/actions/action-client'
import { browserDownloadSendToKnowledgeOutput } from '@/shared/actions/action-registry'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/api-error'

// The downloads drawer of a browser session (E3.4-7). The session reports the files its browser
// downloaded and Quarry admitted (E3.4-4: admitted downloads are stored as tenant-owned artifacts); the
// drawer lists them with type and size and offers one action per file: `browser_download.send_to_knowledge`,
// which asks the owner (Ingestion Plane, through the gateway) to parse the file and write its text to
// the knowledge base under the caller's org. The file itself never leaves the owner.
//
// The drawer is hidden unless the build flag is on AND the session reports at least one download. A
// session that reports nothing (every session today: no projection carries `downloads` yet) shows nothing.

/** Whether the downloads drawer is on for this build. `VITE_BROWSER_DOWNLOADS_DRAWER` counts only as "1" or "true". */
export function isBrowserDownloadsDrawerEnabled(): boolean {
  const value = import.meta.env.VITE_BROWSER_DOWNLOADS_DRAWER
  return value === '1' || value === 'true'
}

/** The file kinds the owner admits for the knowledge hand-off (the action contract's description). */
export type DownloadKind = 'pdf' | 'text' | 'markdown' | 'csv' | 'other'

export interface BrowserSessionDownload {
  /** The opaque artifact handle the session reported. The owner checks it belongs to this session and org. */
  readonly artifactId: string
  /** Display name only; never sent to the owner. */
  readonly filename: string
  /** The MIME type the session reported, lower-cased without parameters; `''` when unknown. */
  readonly mimeType: string
  /** Size in bytes, or `undefined` when the session did not say. */
  readonly sizeBytes: number | undefined
  readonly kind: DownloadKind
}

// One reported download, as the gateway may spell it (camelCase or Quarry's snake_case). Unknown keys are
// ignored; an entry without an artifact id is dropped, because nothing could be sent for it.
const reportedDownload = z.object({
  artifactId: z.string().trim().min(1).max(64).optional(),
  artifact_id: z.string().trim().min(1).max(64).optional(),
  filename: z.string().optional(),
  file_name: z.string().optional(),
  mimeType: z.string().optional(),
  mime_type: z.string().optional(),
  contentType: z.string().optional(),
  content_type: z.string().optional(),
  sizeBytes: z.number().int().nonnegative().optional(),
  size_bytes: z.number().int().nonnegative().optional(),
})

/** The most entries the drawer lists; a longer report is cut, newest first as reported. */
export const DOWNLOADS_DRAWER_MAX = 50

const KIND_BY_MIME: Readonly<Record<string, DownloadKind>> = {
  'application/pdf': 'pdf',
  'text/plain': 'text',
  'text/markdown': 'markdown',
  'text/x-markdown': 'markdown',
  'text/csv': 'csv',
  'application/csv': 'csv',
}

const KIND_BY_EXTENSION: Readonly<Record<string, DownloadKind>> = {
  pdf: 'pdf',
  txt: 'text',
  md: 'markdown',
  markdown: 'markdown',
  csv: 'csv',
}

/** The file kind from the MIME type, falling back to the file extension when the type is missing or generic. */
export function downloadKind(mimeType: string, filename: string): DownloadKind {
  const byMime = KIND_BY_MIME[mimeType]
  if (byMime) return byMime
  if (mimeType !== '' && mimeType !== 'application/octet-stream') return 'other'
  const dot = filename.lastIndexOf('.')
  if (dot < 0) return 'other'
  return KIND_BY_EXTENSION[filename.slice(dot + 1).toLowerCase()] ?? 'other'
}

function normalizeMime(raw: string | undefined): string {
  return (raw ?? '').split(';')[0]?.trim().toLowerCase() ?? ''
}

/**
 * The downloads a session reported, read from its `downloads` field. Tolerant: anything that is not a
 * list gives `[]`, and an entry that does not parse or has no artifact id is skipped. Duplicate artifact
 * ids keep the first entry.
 */
export function readSessionDownloads(session: unknown): BrowserSessionDownload[] {
  if (session === null || typeof session !== 'object') return []
  const raw = (session as { downloads?: unknown }).downloads
  if (!Array.isArray(raw)) return []
  const seen = new Set<string>()
  const out: BrowserSessionDownload[] = []
  for (const item of raw) {
    if (out.length >= DOWNLOADS_DRAWER_MAX) break
    const parsed = reportedDownload.safeParse(item)
    if (!parsed.success) continue
    const entry = parsed.data
    const artifactId = entry.artifactId ?? entry.artifact_id
    if (!artifactId || seen.has(artifactId)) continue
    seen.add(artifactId)
    const filename = (entry.filename ?? entry.file_name ?? '').trim() || artifactId
    const mimeType = normalizeMime(entry.mimeType ?? entry.mime_type ?? entry.contentType ?? entry.content_type)
    out.push({
      artifactId,
      filename,
      mimeType,
      sizeBytes: entry.sizeBytes ?? entry.size_bytes,
      kind: downloadKind(mimeType, filename),
    })
  }
  return out
}

/** A byte count as the drawer shows it: `512 B`, `12 kB`, `3.4 MB` (nb uses a decimal comma). */
export function formatDownloadSize(bytes: number | undefined, locale: 'no' | 'en'): string {
  if (bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return ''
  const format = (value: number, digits: number): string =>
    new Intl.NumberFormat(locale === 'no' ? 'nb-NO' : 'en-GB', { maximumFractionDigits: digits }).format(value)
  if (bytes < 1000) return `${format(bytes, 0)} B`
  if (bytes < 1_000_000) return `${format(bytes / 1000, 0)} kB`
  if (bytes < 1_000_000_000) return `${format(bytes / 1_000_000, 1)} MB`
  return `${format(bytes / 1_000_000_000, 1)} GB`
}

/** Why a download cannot be sent to knowledge right now; `undefined` when it can. */
export type SendRefusal = 'zdr' | 'unsupported_type' | 'signed_out' | 'not_found' | 'failed'

/** Whether the drawer offers the action for this file, before anything is sent. */
export function sendRefusalBefore(
  download: BrowserSessionDownload,
  zdr: boolean,
): SendRefusal | undefined {
  if (zdr) return 'zdr'
  if (download.kind === 'other') return 'unsupported_type'
  return undefined
}

export type SendOutcome =
  | { readonly outcome: 'sent'; readonly documentId: string; readonly indexStatus: 'pending' | 'indexed' | 'failed' | 'skipped' }
  | { readonly outcome: 'refused'; readonly refusal: SendRefusal }

export interface SendDownloadOptions {
  readonly sessionId: string
  readonly download: BrowserSessionDownload
  readonly zdr: boolean
  readonly actor: ActionActor | undefined
  /** Sends the action. Defaults to the shared action client. */
  readonly execute?: typeof executeAction
}

/** The idempotency key of one hand-off: a retry of the same file in the same session is the same request. */
export function sendToKnowledgeKey(sessionId: string, artifactId: string): string {
  return `browser-download:${sessionId}:${artifactId}`
}

function refusalOf(failure: unknown): SendRefusal {
  if (!(failure instanceof ApiError)) return 'failed'
  if (failure.feature !== null) return 'zdr'
  if (failure.status === 404) return 'not_found'
  if (failure.status === 415 || failure.status === 422) return 'unsupported_type'
  return 'failed'
}

/**
 * Sends one download to the knowledge base through `browser_download.send_to_knowledge`. Refuses before
 * any call under ZDR, for a file kind the owner does not admit and without a signed-in actor. The owner
 * answers with the Data Plane document id and its indexing state; an answer that does not match the
 * contract counts as `failed`.
 */
export async function sendDownloadToKnowledge(options: SendDownloadOptions): Promise<SendOutcome> {
  const before = sendRefusalBefore(options.download, options.zdr)
  if (before) return { outcome: 'refused', refusal: before }
  if (!options.actor) return { outcome: 'refused', refusal: 'signed_out' }
  const execute = options.execute ?? executeAction
  let result: unknown
  try {
    const execution = await execute(
      'browser_download.send_to_knowledge',
      options.actor,
      { sessionId: options.sessionId, artifactId: options.download.artifactId },
      { idempotencyKey: sendToKnowledgeKey(options.sessionId, options.download.artifactId) },
    )
    result = execution.result
  } catch (failure) {
    return { outcome: 'refused', refusal: refusalOf(failure) }
  }
  const answer = browserDownloadSendToKnowledgeOutput.safeParse(result)
  if (!answer.success) return { outcome: 'refused', refusal: 'failed' }
  return { outcome: 'sent', documentId: answer.data.documentId, indexStatus: answer.data.indexStatus }
}
