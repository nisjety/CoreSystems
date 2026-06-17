import { NextRequest } from 'next/server'
import { randomUUID } from 'crypto'

import { resolveChatActor } from '../_lib/session-store'

// U2-13 (ui-ux-velion-gap.md §10): chat attachments are now real documents.
//
// Previous behaviour persisted files as base64 data-URLs embedded in the
// chat message — the file "appeared" attached but was never indexed,
// retrievable, or referenceable later. The data-URL fallback also bloated
// every conversation row.
//
// New behaviour: each uploaded file is POSTed to dpv2-documents-api as a
// regular Data Plane document (with `source = chat-upload:<filename>`,
// `type = 'chat-attachment'`). The Data Plane fan-out picks it up:
//
//   1. dpv2-documents-api persists + emits `data.document.created`
//   2. dpv2-retrieval-engine consumes it, embeds, indexes for search
//   3. the chat message keeps a reference: { document_id, name, type, source }
//
// Net result: the user can ask a follow-up question about an attached
// file two hours later in a different conversation and retrieval finds it.

const DOCUMENTS_SERVICE_URL =
  process.env.DOCUMENTS_SERVICE_URL ?? 'http://dpv2-documents-api:8010'
const INTERNAL_KEY =
  process.env.INTERNAL_API_KEY ??
  process.env.INTERNAL_SERVICE_SECRET ??
  ''

const MAX_TEXT_BYTES = 5 * 1024 * 1024 // 5 MB — keeps Data Plane payloads sane
const MAX_INLINE_FALLBACK_BYTES = 256 * 1024 // 256 KB — only kept for tiny images

interface ChatAttachment {
  id: string
  name: string
  url: string
  type: string
  source: 'data-plane' | 'inline'
  document_id?: string
}

interface DocumentsApiResponse {
  document_id?: string
  org_id?: string
  source?: string
  status?: string
}

function isTextLike(file: File): boolean {
  if (file.type.startsWith('text/')) return true
  if (file.type === 'application/json') return true
  if (file.type === 'application/xml') return true
  if (file.type === '' && /\.(md|txt|csv|json|yaml|yml|log)$/i.test(file.name)) return true
  return false
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    if (!actor.orgId) {
      return Response.json(
        { error: 'No active organisation in session' },
        { status: 400 },
      )
    }

    const formData = await request.formData()
    const files = formData.getAll('files').filter((v): v is File => v instanceof File)
    if (files.length === 0) {
      return Response.json({ error: 'No files provided' }, { status: 400 })
    }

    const uploads: ChatAttachment[] = []

    for (const file of files) {
      const buffer = Buffer.from(await file.arrayBuffer())

      if (buffer.byteLength > MAX_TEXT_BYTES) {
        // Refuse oversized files rather than silently truncating.
        return Response.json(
          {
            error: `File "${file.name}" exceeds 5 MB attachment limit`,
            file_size_bytes: buffer.byteLength,
          },
          { status: 413 },
        )
      }

      const idempotencyKey = `chat-upload:${actor.orgId}:${randomUUID()}`
      const sourceTag = `chat-upload:${file.name}`

      // For text-like files we put the content into the Data Plane doc body.
      // For binaries (images, PDFs) we send a metadata-only stub today; the
      // ingestion path through Quarry / Document Intelligence picks it up
      // when we wire U2-13 follow-up (binary ingestion via signed-URL).
      const isText = isTextLike(file)
      const content = isText ? buffer.toString('utf8') : ''

      let documentId: string | undefined
      let persistedViaDataPlane = false

      try {
        const upstreamRes = await fetch(
          `${DOCUMENTS_SERVICE_URL}/v1/documents`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'x-internal-api-key': INTERNAL_KEY,
              'x-org-id': actor.orgId,
            },
            body: JSON.stringify({
              org_id: actor.orgId,
              source: sourceTag,
              type: 'chat-attachment',
              title: file.name,
              content,
              metadata: {
                file_name: file.name,
                mime_type: file.type,
                size_bytes: buffer.byteLength,
                actor_user_id: actor.userId,
                is_text: isText,
              },
              zdr_classification: 'standard',
              created_by: actor.userId ?? '',
              idempotency_key: idempotencyKey,
            }),
            signal: AbortSignal.timeout(30_000),
          },
        )

        if (upstreamRes.ok) {
          const data = (await upstreamRes.json()) as DocumentsApiResponse
          documentId = data.document_id
          persistedViaDataPlane = Boolean(documentId)
        }
      } catch {
        // Fall through to inline fallback. The chat message still gets
        // the file but without the indexable side-effect.
      }

      if (persistedViaDataPlane) {
        uploads.push({
          id: documentId ?? randomUUID(),
          document_id: documentId,
          name: file.name,
          // The data-plane stores documents server-side. The UI references
          // the file by document id; future retrieval surfaces it on a
          // dedicated /knowledge/documents/:id route.
          url: `/api/knowledge/documents/${encodeURIComponent(documentId!)}`,
          type: file.type,
          source: 'data-plane',
        })
        continue
      }

      // Inline fallback — only allowed for small files. Anything else is
      // a hard failure so users don't get a silent "no-index" surprise.
      if (buffer.byteLength > MAX_INLINE_FALLBACK_BYTES) {
        return Response.json(
          {
            error: `Data Plane upload failed for "${file.name}"; inline fallback would exceed 256 KB`,
            hint: 'Check dpv2-documents-api health (curl /readyz on port 8010).',
          },
          { status: 502 },
        )
      }

      const b64 = buffer.toString('base64')
      uploads.push({
        id: randomUUID(),
        name: file.name,
        url: `data:${file.type || 'application/octet-stream'};base64,${b64}`,
        type: file.type,
        source: 'inline',
      })
    }

    return Response.json({ uploads }, { status: 200 })
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Upload failed',
        message: error instanceof Error ? error.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
