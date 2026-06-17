import { NextRequest, NextResponse } from 'next/server'

export async function POST(request: NextRequest) {
  await request.json().catch(() => null)
  return NextResponse.json(
    {
      error: 'Legacy OAuth callbacks are disabled.',
      detail: 'Integration authorization is now handled by Aqencia integration sessions.',
    },
    { status: 410 }
  )
}
