import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import {
  ChatStoreError,
  getKnowledgeDocuments,
  createKnowledgeDocument,
} from '../_lib/knowledge-data'

const textDocSchema = z.object({
  type: z.literal('text'),
  title: z.string().trim().min(1).max(200),
  content: z.string().trim().min(1).max(32_000),
})

export async function GET(request: NextRequest) {
  try {
    const q = request.nextUrl.searchParams.get('q') ?? undefined
    const type = request.nextUrl.searchParams.get('type') ?? undefined
    const limitValue = request.nextUrl.searchParams.get('limit')
    const limit = limitValue ? Number(limitValue) : undefined

    const payload = await getKnowledgeDocuments({
      q,
      type,
      limit: Number.isFinite(limit) ? limit : undefined,
    })

    return NextResponse.json(payload, {
      headers: { 'Cache-Control': 'no-store' },
    })
  } catch (error) {
    return errorResponse(error)
  }
}

/**
 * Wave 11 §2.2: real ingest entry-point. Accepts two shapes:
 *
 *   1. `multipart/form-data` with a `file` field → forwards to
 *      documents-service `/v1/documents` as a multipart upload.
 *   2. `application/json` `{ type: 'text', title, content }` → forwards
 *      as a JSON ingest of a text snippet.
 *
 * documents-service handles chunking + embedding + Qdrant write-through
 * asynchronously; this route returns 201 with the new doc id once the
 * row is committed.
 */
export async function POST(request: NextRequest) {
  try {
    const contentType = request.headers.get('content-type') ?? ''

    if (contentType.includes('multipart/form-data')) {
      const form = await request.formData()
      const file = form.get('file')
      if (!(file instanceof File)) {
        return NextResponse.json(
          { error: 'Missing `file` field in multipart body' },
          { status: 400 },
        )
      }
      const created = await createKnowledgeDocument({ kind: 'file', file })
      return NextResponse.json(created, { status: 201 })
    }

    if (contentType.includes('application/json')) {
      const body = await request.json()
      const parsed = textDocSchema.parse(body)
      const created = await createKnowledgeDocument({
        kind: 'text',
        title: parsed.title,
        content: parsed.content,
      })
      return NextResponse.json(created, { status: 201 })
    }

    return NextResponse.json(
      { error: 'Unsupported Content-Type — expected multipart/form-data or application/json' },
      { status: 415 },
    )
  } catch (error) {
    return errorResponse(error)
  }
}

function errorResponse(error: unknown) {
  if (error instanceof ChatStoreError) {
    return NextResponse.json({ error: error.message }, { status: error.statusCode })
  }
  if (error && typeof error === 'object' && 'issues' in error) {
    const issues = (error as { issues: Array<{ message?: string }> }).issues
    return NextResponse.json(
      { error: issues[0]?.message ?? 'Validation failed' },
      { status: 400 },
    )
  }
  return NextResponse.json(
    { error: error instanceof Error ? error.message : 'Failed to create document' },
    { status: 500 },
  )
}
