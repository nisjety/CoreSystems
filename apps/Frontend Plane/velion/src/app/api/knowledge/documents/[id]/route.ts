import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import {
  ChatStoreError,
  deleteKnowledgeDocument,
  getKnowledgeDocumentDetail,
  updateKnowledgeDocument,
} from '@/app/api/knowledge/_lib/knowledge-data'

interface RouteContext {
  params: Promise<{ id: string }>
}

const patchSchema = z.object({
  title: z.string().trim().min(1).max(200).optional(),
  content: z.string().max(64_000).optional(),
  status: z.enum(['active', 'deprecated']).optional(),
})

export async function GET(_request: NextRequest, context: RouteContext) {
  try {
    const { id } = await context.params
    const detail = await getKnowledgeDocumentDetail(id)
    return NextResponse.json(detail, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    return errorResponse(error)
  }
}

export async function PATCH(request: NextRequest, context: RouteContext) {
  try {
    const { id } = await context.params
    const body = await request.json()
    const parsed = patchSchema.parse(body)
    const updated = await updateKnowledgeDocument(id, parsed)
    return NextResponse.json(updated, { status: 200 })
  } catch (error) {
    return errorResponse(error)
  }
}

export async function DELETE(_request: NextRequest, context: RouteContext) {
  try {
    const { id } = await context.params
    await deleteKnowledgeDocument(id)
    return NextResponse.json({ success: true }, { status: 200 })
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
    { error: error instanceof Error ? error.message : 'Operation failed' },
    { status: 500 },
  )
}
