import { NextRequest, NextResponse } from 'next/server'

import {
  ChatStoreError,
  reindexKnowledgeDocument,
} from '@/app/api/knowledge/_lib/knowledge-data'

interface RouteContext {
  params: Promise<{ id: string }>
}

export async function POST(_request: NextRequest, context: RouteContext) {
  try {
    const { id } = await context.params
    await reindexKnowledgeDocument(id)
    return NextResponse.json({ success: true }, { status: 202 })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Reindex failed' },
      { status: 500 },
    )
  }
}
