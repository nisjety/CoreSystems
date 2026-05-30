import { NextResponse } from 'next/server'

import { ChatStoreError, getKnowledgeSources } from '../_lib/knowledge-data'

export async function GET() {
  try {
    const payload = await getKnowledgeSources()
    return NextResponse.json(payload, {
      headers: { 'Cache-Control': 'no-store' },
    })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }

    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Failed to load sources' },
      { status: 500 },
    )
  }
}
