// WS14-S9: the durable room file library, over the gateway routes WS14-S7 added
// (apps/gateway/src/domains/spaces/files.rs). A room file is the extracted TEXT of an uploaded
// document: there are no original bytes, so there is no download, only a text read.
//
// Wire (plan WS14, API paragraph):
//   POST   /api/v1/spaces/{ref}/files                    multipart `files` 1..10, header Idempotency-Key
//          -> { data: { upload_id, deduplicated, files: [{file_name, status}] } }   (202, or 200 replay)
//   GET    /api/v1/spaces/{ref}/files/uploads/{upload_id}  -> { data: { status, total, processed, failed, items } }
//   GET    /api/v1/spaces/{ref}/files?limit&cursor       -> { data: { files, next_cursor, unavailable } }
//   GET    /api/v1/spaces/{ref}/files/{document_id}/text?max_chars  -> { data: { text, truncated, ... } }
//   DELETE /api/v1/spaces/{ref}/files/{document_id}      -> 204 (uploader only)
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as an ApiError.
//
// Presentation only: whether a caller may upload, read or delete is decided by Control, Data and
// Ingestion behind the gateway. Nothing here is written to browser storage.
import { requestForm, requestJson } from './http'

export const SPACE_FILES_PAGE_SIZE = 50

export type SpaceFile = {
  readonly document_id: string
  readonly file_name: string | null
  readonly mime_type: string | null
  readonly size_bytes: number | null
  readonly sha256: string | null
  readonly extension: string | null
  readonly status: string | null
  readonly text_chars: number | null
  readonly uploaded_by: string | null
  readonly uploaded_at: string | null
}

/** A section the gateway could not list, stated rather than dressed as an empty library. */
export type SpaceFilesGap = {
  readonly section: string
  readonly code: string
  readonly reason: string
}

export type SpaceFilesPage = {
  readonly files: readonly SpaceFile[]
  /** Null on the last page. */
  readonly nextCursor: string | null
  readonly unavailable: readonly SpaceFilesGap[]
}

export type SpaceFileUploadAccepted = {
  readonly uploadId: string
  /** True when the same Idempotency-Key was already accepted: nothing new was queued. */
  readonly deduplicated: boolean
  readonly files: readonly { readonly fileName: string; readonly status: string }[]
}

export type SpaceFileUploadItem = {
  readonly sourceName: string | null
  readonly status: string | null
  readonly documentId?: string
  readonly errorCode?: string
}

export type SpaceFileUploadProgress = {
  readonly uploadId: string
  /** queued, running, completed, completed_with_errors or failed. */
  readonly status: string
  readonly total: number
  readonly processed: number
  readonly failed: number
  readonly items: readonly SpaceFileUploadItem[]
}

export type SpaceFileText = {
  readonly documentId: string
  readonly fileName: string | null
  readonly text: string
  readonly textChars: number | null
  readonly truncated: boolean
}

function str(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null
}

function num(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
}

function count(value: unknown): number {
  return num(value) ?? 0
}

function fileRows(value: unknown): SpaceFile[] {
  if (!Array.isArray(value)) return []
  const out: SpaceFile[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    const documentId = str(record.document_id)
    if (!documentId) continue
    out.push({
      document_id: documentId,
      file_name: str(record.file_name),
      mime_type: str(record.mime_type),
      size_bytes: num(record.size_bytes),
      sha256: str(record.sha256),
      extension: str(record.extension),
      status: str(record.status),
      text_chars: num(record.text_chars),
      uploaded_by: str(record.uploaded_by),
      uploaded_at: str(record.uploaded_at),
    })
  }
  return out
}

function gapRows(value: unknown): SpaceFilesGap[] {
  if (!Array.isArray(value)) return []
  const out: SpaceFilesGap[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    out.push({
      section: str(record.section) ?? 'files',
      code: str(record.code) ?? 'files_upstream_unavailable',
      reason: str(record.reason) ?? '',
    })
  }
  return out
}

const filesPath = (spaceRef: string) => `/api/v1/spaces/${encodeURIComponent(spaceRef)}/files`

export async function listSpaceFiles(
  spaceRef: string,
  options: { readonly limit?: number; readonly cursor?: string } = {},
): Promise<SpaceFilesPage> {
  const query = new URLSearchParams()
  query.set('limit', String(options.limit ?? SPACE_FILES_PAGE_SIZE))
  if (options.cursor) query.set('cursor', options.cursor)
  const response = await requestJson<{ files?: unknown; next_cursor?: unknown; unavailable?: unknown }>(
    `${filesPath(spaceRef)}?${query.toString()}`,
  )
  return {
    files: fileRows(response.files),
    nextCursor: str(response.next_cursor),
    unavailable: gapRows(response.unavailable),
  }
}

/** A fresh Idempotency-Key (the gateway wants 8 to 128 characters). */
export function newUploadIdempotencyKey(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return `upload-${crypto.randomUUID()}`
  return `upload-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`
}

/**
 * Upload one to ten files. Pass the same `idempotencyKey` to retry the same batch: the server then
 * answers with the job it already accepted (`deduplicated: true`) instead of queuing another.
 */
export async function uploadSpaceFiles(
  spaceRef: string,
  files: readonly File[],
  idempotencyKey: string = newUploadIdempotencyKey(),
): Promise<SpaceFileUploadAccepted> {
  const form = new FormData()
  for (const file of files) form.append('files', file, file.name)
  const response = await requestForm<{ upload_id?: unknown; deduplicated?: unknown; files?: unknown }>(
    filesPath(spaceRef),
    form,
    { headers: { 'Idempotency-Key': idempotencyKey } },
  )
  const accepted = Array.isArray(response.files) ? response.files : []
  return {
    uploadId: str(response.upload_id) ?? '',
    deduplicated: response.deduplicated === true,
    files: accepted.flatMap((entry) => {
      if (!entry || typeof entry !== 'object') return []
      const record = entry as Record<string, unknown>
      return [{ fileName: str(record.file_name) ?? '', status: str(record.status) ?? 'queued' }]
    }),
  }
}

export async function getSpaceFileUpload(spaceRef: string, uploadId: string): Promise<SpaceFileUploadProgress> {
  const response = await requestJson<{
    upload_id?: unknown
    status?: unknown
    total?: unknown
    processed?: unknown
    failed?: unknown
    items?: unknown
  }>(`${filesPath(spaceRef)}/uploads/${encodeURIComponent(uploadId)}`)
  const items = Array.isArray(response.items) ? response.items : []
  return {
    uploadId: str(response.upload_id) ?? uploadId,
    status: str(response.status) ?? 'queued',
    total: count(response.total),
    processed: count(response.processed),
    failed: count(response.failed),
    items: items.flatMap((entry): SpaceFileUploadItem[] => {
      if (!entry || typeof entry !== 'object') return []
      const record = entry as Record<string, unknown>
      const documentId = str(record.document_id)
      const errorCode = str(record.error_code)
      return [
        {
          sourceName: str(record.source_name),
          status: str(record.status),
          ...(documentId ? { documentId } : {}),
          ...(errorCode ? { errorCode } : {}),
        },
      ]
    }),
  }
}

export async function readSpaceFileText(
  spaceRef: string,
  documentId: string,
  options: { readonly maxChars?: number } = {},
): Promise<SpaceFileText> {
  const query = options.maxChars ? `?max_chars=${encodeURIComponent(String(options.maxChars))}` : ''
  const response = await requestJson<{
    document_id?: unknown
    file_name?: unknown
    text?: unknown
    text_chars?: unknown
    truncated?: unknown
  }>(`${filesPath(spaceRef)}/${encodeURIComponent(documentId)}/text${query}`)
  return {
    documentId: str(response.document_id) ?? documentId,
    fileName: str(response.file_name),
    text: typeof response.text === 'string' ? response.text : '',
    textChars: num(response.text_chars),
    truncated: response.truncated === true,
  }
}

/** Uploader-only. Resolves on 204; a non-uploader or a missing file arrives as a 404 ApiError. */
export async function deleteSpaceFile(spaceRef: string, documentId: string): Promise<void> {
  await requestJson<unknown>(`${filesPath(spaceRef)}/${encodeURIComponent(documentId)}`, { method: 'DELETE' })
}
