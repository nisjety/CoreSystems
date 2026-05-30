import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../chat/_lib/session-store'

const DOCUMENTS_SERVICE_URL =
  process.env.DOCUMENTS_SERVICE_URL ||
  process.env.DOCS_SERVICE_URL ||
  'http://documents-service:8001'

const INTERNAL_API_KEY =
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

export async function GET(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const q = request.nextUrl.searchParams.get('q') ?? ''
    const limit = request.nextUrl.searchParams.get('limit') ?? '5'

    const upstream = await fetch(
      `${DOCUMENTS_SERVICE_URL}/v1/documents/search?org_id=${encodeURIComponent(actor.orgId)}&q=${encodeURIComponent(q)}&limit=${limit}`,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-Api-Key': INTERNAL_API_KEY,
        },
        cache: 'no-store',
      },
    )

    if (!upstream.ok) {
      return NextResponse.json({ suggestions: [] })
    }

    // data-plane returns { results: DocumentSearchItem[], query, total }
    const raw = (await upstream.json()) as {
      results?: Array<{
        document_id: string
        title: string
        type: string
        source: string
      }>
    }

    const suggestions = (raw.results ?? []).map((doc) => ({
      id: doc.document_id,
      title: doc.title,
      type: doc.type,
      source: doc.source,
    }))

    return NextResponse.json({ suggestions })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ suggestions: [] })
  }
}
