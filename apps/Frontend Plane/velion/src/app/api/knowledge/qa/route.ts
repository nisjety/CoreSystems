import { NextRequest, NextResponse } from 'next/server'

import {
  listQnAEntries,
  createQnAEntry,
} from '@/components/knowledge/server/qa-store'

export async function GET() {
  try {
    const entries = await listQnAEntries()
    return NextResponse.json({ entries }, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Failed to list Q&A' },
      { status: 500 },
    )
  }
}

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const entry = await createQnAEntry(body)
    return NextResponse.json(entry, { status: 201 })
  } catch (error) {
    if (error && typeof error === 'object' && 'issues' in error) {
      const issues = (error as { issues: Array<{ message?: string }> }).issues
      return NextResponse.json(
        { error: issues[0]?.message ?? 'Validation failed' },
        { status: 400 },
      )
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Create failed' },
      { status: 500 },
    )
  }
}
